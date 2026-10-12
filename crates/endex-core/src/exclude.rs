//! Which files under the configured sources are indexed. Scans, the watcher and check-path all
//! ask [`Filter`], so they agree: hidden files, the builtin and sensitive lists, extra rules
//! and `.ignore`/`.gitignore` files. Excluded folders are never entered, and files matched by
//! a rule are never opened.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::Kind;
use crate::config::{Config, SourceKind};
use crate::index::kind_of;

/// Build output, caches, lock files, and the code of saved web pages.
pub const BUILTIN: &[&str] = &[
    ".git/",
    ".hg/",
    ".svn/",
    "node_modules/",
    "target/",
    "__pycache__/",
    ".venv/",
    "build/",
    "dist/",
    ".cache/",
    ".tox/",
    ".mypy_cache/",
    ".pytest_cache/",
    ".gradle/",
    "*.o",
    "*.pyc",
    "*.class",
    "*.min.js",
    "*.min.css",
    "package-lock.json",
    "pnpm-lock.yaml",
    SAVED_PAGE_CODE,
];

/// Scripts, styles and frames anywhere below a folder that a browser's "Save page as,
/// complete" writes next to the page (`<page>_files/`); its images stay. Matched in any case
/// (`Page_Files/APP.JS`), unlike the other builtin patterns.
pub const SAVED_PAGE_CODE: &str = "**/*_files/**/*.{js,css,html,htm}";

/// Keys, credentials, password stores, keyrings and browser profiles.
pub const SENSITIVE: &[&str] = &[
    ".env",
    ".env.*",
    "*.pem",
    "*.key",
    "*.p12",
    "*.pfx",
    "*.jks",
    "*.keystore",
    "id_rsa*",
    "id_dsa*",
    "id_ecdsa*",
    "id_ed25519*",
    "*.kdbx",
    "*.kdb",
    "*.gpg",
    "*.asc",
    "*.age",
    ".ssh/",
    ".gnupg/",
    ".password-store/",
    ".netrc",
    ".pgpass",
    ".git-credentials",
    "credentials.json",
    ".aws/",
    ".kube/",
    ".docker/",
    "keyrings/",
    "kwalletd/",
    "*.kwl",
    ".mozilla/",
    ".thunderbird/",
    "google-chrome/",
    "chromium/",
    "BraveSoftware/",
    "vivaldi/",
    "microsoft-edge/",
];

/// Why a path is not indexed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    OutsideSources,
    /// Endex's own data directory.
    Internal,
    Symlink,
    /// Not a regular file or folder (socket, FIFO, device).
    Special,
    Hidden,
    Sensitive(String),
    Builtin(String),
    Rule(String),
    Ignored {
        file: PathBuf,
        pattern: String,
    },
    Unsupported,
    /// A document in an images-only source or the other way round.
    Kind(SourceKind),
    /// A file name that is not UTF-8, which the index cannot hold.
    Encoding,
}

impl Reason {
    /// Stable machine-readable name, e.g. for D-Bus.
    pub fn code(&self) -> &'static str {
        match self {
            Reason::OutsideSources => "outside_sources",
            Reason::Internal => "internal",
            Reason::Symlink => "symlink",
            Reason::Special => "special",
            Reason::Hidden => "hidden",
            Reason::Sensitive(_) => "sensitive",
            Reason::Builtin(_) => "builtin",
            Reason::Rule(_) => "rule",
            Reason::Ignored { .. } => "gitignore",
            Reason::Unsupported => "unsupported",
            Reason::Kind(_) => "kind",
            Reason::Encoding => "encoding",
        }
    }

    /// The matching pattern, if a rule excluded the path.
    pub fn pattern(&self) -> Option<&str> {
        match self {
            Reason::Sensitive(p) | Reason::Builtin(p) | Reason::Rule(p) => Some(p),
            Reason::Ignored { pattern, .. } => Some(pattern),
            _ => None,
        }
    }
}

/// The source a file belongs to and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Included {
    /// [`crate::config::Source::owner`] of the source.
    pub owner: PathBuf,
    pub kind: Kind,
}

/// `.ignore` and `.gitignore` of one folder.
#[derive(Default)]
pub struct DirIgnores {
    ignore: Option<Gitignore>,
    gitignore: Option<Gitignore>,
}

impl DirIgnores {
    fn load(dir: &Path) -> Self {
        let read = |name| {
            let path = dir.join(name);
            path.is_file().then(|| Gitignore::new(&path).0)
        };
        Self {
            ignore: read(".ignore"),
            gitignore: read(".gitignore"),
        }
    }

    fn is_empty(&self) -> bool {
        self.ignore.is_none() && self.gitignore.is_none()
    }
}

/// Ignore files that apply to entries of one folder, outermost first.
pub type Chain = Vec<Arc<DirIgnores>>;

pub struct SourceRules {
    /// The resolved folder ([`crate::config::Source::root`]); paths below it are judged.
    pub root: PathBuf,
    /// The configured folder ([`crate::config::Source::owner`]).
    pub owner: PathBuf,
    pub kind: SourceKind,
    sensitive: Option<Gitignore>,
    builtin: Option<Gitignore>,
    builtin_any_case: Option<Gitignore>,
    extra: Gitignore,
    /// Ignore files above the root, up to the enclosing git repository's top folder.
    parents: Chain,
}

pub struct Filter {
    sources: Vec<SourceRules>,
    internal: Vec<PathBuf>,
    hidden: bool,
    gitignore: bool,
    cache: Mutex<HashMap<PathBuf, Arc<DirIgnores>>>,
}

impl Filter {
    /// Fails on an extra rule that is not valid gitignore syntax.
    pub fn new(config: &Config) -> Result<Self> {
        let e = &config.exclude;
        let internal = vec![config.data_dir.clone(), config.index_root().to_owned()];
        let mut filter = Self {
            sources: Vec::new(),
            internal,
            hidden: e.hidden,
            gitignore: e.gitignore,
            cache: Mutex::default(),
        };
        for source in &config.index.sources {
            let root = source.root();
            let list = |enabled: bool, patterns: &[&str], any_case| -> Result<Option<Gitignore>> {
                enabled
                    .then(|| matcher(&root, patterns, any_case))
                    .transpose()
            };
            let exact: Vec<&str> = BUILTIN
                .iter()
                .copied()
                .filter(|p| *p != SAVED_PAGE_CODE)
                .collect();
            let rules = SourceRules {
                sensitive: list(e.sensitive, SENSITIVE, false)?,
                builtin: list(e.builtin, &exact, false)?,
                builtin_any_case: list(e.builtin, &[SAVED_PAGE_CODE], true)?,
                extra: matcher(&root, &e.rules, false)?,
                parents: if e.gitignore {
                    filter.parent_chain(&root)
                } else {
                    Vec::new()
                },
                kind: source.kind,
                owner: source.owner(),
                root,
            };
            filter.sources.push(rules);
        }
        Ok(filter)
    }

    pub fn sources(&self) -> &[SourceRules] {
        &self.sources
    }

    /// The source folder containing `path` (the innermost, should sources nest).
    pub fn source_of(&self, path: &Path) -> Option<&SourceRules> {
        self.sources
            .iter()
            .filter(|s| path.starts_with(&s.root))
            .max_by_key(|s| s.root.as_os_str().len())
    }

    /// Whether `path` is indexed, judged from its source root down; reads file types but never
    /// content. `Ok(None)` is an included folder; a path that does not exist is judged as a
    /// file.
    pub fn check(&self, path: &Path) -> Result<Option<Included>, Reason> {
        let source = self.source_of(path).ok_or(Reason::OutsideSources)?;
        if path == source.root {
            return Ok(None);
        }
        let chain = self.descend(source, path)?;
        let is_dir = match fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(Reason::Symlink),
            Ok(meta) if meta.is_dir() => true,
            Ok(meta) if !meta.is_file() => return Err(Reason::Special),
            _ => false,
        };
        self.entry(source, path, is_dir, &chain)?;
        if is_dir {
            return Ok(None);
        }
        let included = file_kind(source, path)?;
        if path.to_str().is_none() {
            return Err(Reason::Encoding);
        }
        Ok(Some(included))
    }

    /// The ignore files that apply to the entries of `dir`, if `dir` itself is included.
    pub fn dir_chain(&self, source: &SourceRules, dir: &Path) -> Result<Chain, Reason> {
        let mut chain = if dir == source.root {
            source.parents.clone()
        } else {
            let chain = self.descend(source, dir)?;
            self.entry(source, dir, true, &chain)?;
            chain
        };
        self.push_ignores(&mut chain, dir);
        Ok(chain)
    }

    /// Checks the folders between the source root and `path` and returns the ignore files
    /// that apply to `path`.
    fn descend(&self, source: &SourceRules, path: &Path) -> Result<Chain, Reason> {
        let rel = path
            .strip_prefix(&source.root)
            .map_err(|_| Reason::OutsideSources)?;
        let parts: Vec<_> = rel.components().collect();
        let mut chain = source.parents.clone();
        let mut current = source.root.clone();
        self.push_ignores(&mut chain, &current);
        for part in &parts[..parts.len().saturating_sub(1)] {
            current.push(part);
            if fs::symlink_metadata(&current).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(Reason::Symlink);
            }
            self.entry(source, &current, true, &chain)?;
            self.push_ignores(&mut chain, &current);
        }
        Ok(chain)
    }

    /// Adds the ignore files of `dir` to `chain`, if any apply.
    pub fn push_ignores(&self, chain: &mut Chain, dir: &Path) {
        if self.gitignore {
            let ignores = self.dir_ignores(dir);
            if !ignores.is_empty() {
                chain.push(ignores);
            }
        }
    }

    /// The verdict for one directory entry whose parent folders are known to be included;
    /// `chain` holds the ignore files of those folders (see [`Filter::dir_ignores`]).
    pub fn entry(
        &self,
        source: &SourceRules,
        path: &Path,
        is_dir: bool,
        chain: &[Arc<DirIgnores>],
    ) -> Result<(), Reason> {
        if self.internal.iter().any(|dir| path == dir) {
            return Err(Reason::Internal);
        }
        let hit = |m: &Option<Gitignore>| {
            m.as_ref().and_then(|m| match m.matched(path, is_dir) {
                Match::Ignore(glob) => Some(glob.original().to_owned()),
                _ => None,
            })
        };
        if let Some(p) = hit(&source.sensitive) {
            return Err(Reason::Sensitive(p));
        }
        if let Some(p) = hit(&source.builtin).or_else(|| hit(&source.builtin_any_case)) {
            return Err(Reason::Builtin(p));
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if !self.hidden && name.starts_with('.') {
            return Err(Reason::Hidden);
        }
        if let Match::Ignore(glob) = source.extra.matched(path, is_dir) {
            return Err(Reason::Rule(glob.original().to_owned()));
        }
        if self.gitignore {
            // As in ripgrep: any `.ignore` match outranks `.gitignore`; within each, the
            // innermost folder decides.
            for dot_ignore in [true, false] {
                for ignores in chain.iter().rev() {
                    let file = match dot_ignore {
                        true => ignores.ignore.as_ref(),
                        false => ignores.gitignore.as_ref(),
                    };
                    match file.map(|m| m.matched(path, is_dir)) {
                        Some(Match::Ignore(glob)) => {
                            return Err(Reason::Ignored {
                                file: glob.from().map(Path::to_owned).unwrap_or_default(),
                                pattern: glob.original().to_owned(),
                            });
                        }
                        Some(Match::Whitelist(_)) => return Ok(()),
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }

    /// The ignore files of `dir`, cached until [`Filter::forget`].
    pub fn dir_ignores(&self, dir: &Path) -> Arc<DirIgnores> {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        Arc::clone(
            cache
                .entry(dir.to_owned())
                .or_insert_with(|| Arc::new(DirIgnores::load(dir))),
        )
    }

    /// Drops cached ignore files of `dir` and below (all of them for `None`).
    pub fn forget(&self, dir: Option<&Path>) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        match dir {
            Some(dir) => cache.retain(|d, _| !d.starts_with(dir)),
            None => cache.clear(),
        }
    }

    pub fn uses_gitignore(&self) -> bool {
        self.gitignore
    }

    fn parent_chain(&self, root: &Path) -> Chain {
        // Never from the home folder or above: a dot-file repository there often ignores `*`.
        let home = dirs::home_dir();
        let ancestors: Vec<&Path> = root
            .ancestors()
            .skip(1)
            .take_while(|dir| home.as_deref().is_none_or(|home| !home.starts_with(dir)))
            .collect();
        // Above a repository's top folder its .gitignore files no longer apply; outside any
        // repository, only the source's own ignore files count.
        let Some(top) = ancestors.iter().position(|dir| dir.join(".git").exists()) else {
            return Vec::new();
        };
        ancestors[..=top]
            .iter()
            .rev()
            .map(|dir| self.dir_ignores(dir))
            .filter(|d| !d.is_empty())
            .collect()
    }
}

/// What a file under `source` is, if the source takes that kind.
pub fn file_kind(source: &SourceRules, path: &Path) -> Result<Included, Reason> {
    let kind = kind_of(path).ok_or(Reason::Unsupported)?;
    let wanted = match source.kind {
        SourceKind::All => true,
        SourceKind::Docs => kind == Kind::Doc,
        SourceKind::Images => kind == Kind::Image,
    };
    if !wanted {
        return Err(Reason::Kind(source.kind));
    }
    Ok(Included {
        owner: source.owner.clone(),
        kind,
    })
}

/// Whether `rule` is valid gitignore syntax.
pub fn check_rule(rule: &str) -> Result<()> {
    matcher(Path::new("/"), &[rule], false).map(drop)
}

fn matcher<S: AsRef<str>>(root: &Path, patterns: &[S], any_case: bool) -> Result<Gitignore> {
    let mut builder = GitignoreBuilder::new(root);
    builder.case_insensitive(any_case)?;
    for pattern in patterns {
        let pattern = pattern.as_ref();
        builder
            .add_line(None, pattern)
            .map_err(|e| anyhow!("{e}"))
            .with_context(|| format!("invalid rule {pattern:?}"))?;
    }
    Ok(builder.build()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Source;

    fn tree(files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "endex-exclude-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        for (path, content) in files {
            let path = dir.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        dir
    }

    #[test]
    fn rules_in_order_and_ignore_files() {
        let dir = tree(&[
            ("repo/.git/HEAD", ""),
            ("repo/.gitignore", "draft*.md\nnotes/private/\n"),
            ("repo/notes/.ignore", "!draft-keep.md\n"),
            ("repo/notes/a.md", ""),
            ("repo/notes/draft-keep.md", ""),
            ("repo/notes/draft1.md", ""),
            ("repo/notes/private/b.md", ""),
            ("repo/notes/node_modules/c.md", ""),
            ("repo/notes/.env", ""),
            ("repo/notes/.hidden.md", ""),
            ("repo/notes/sub/backup/d.md", ""),
            ("repo/notes/shot.png", ""),
            ("repo/notes/data.bin", ""),
            ("repo/notes/lib.js", ""),
            ("repo/notes/Saved Page_files/app.js", ""),
            ("repo/notes/Saved Page_files/frames/f.htm", ""),
            ("repo/notes/Saved Page_files/notes.md", ""),
            ("repo/notes/Saved Page_files/logo.png", ""),
            ("repo/notes/sub/x_files/s.css", ""),
            ("repo/notes/P_Files/APP.JS", ""),
        ]);
        let notes = dir.join("repo/notes");
        let mut config = Config::parse(
            "[embedding]\nlibrary = \"/l\"\nmodel = \"/m\"\ntokenizer = \"/t\"\n\
             [exclude]\nrules = [\"backup/\"]\n",
        )
        .unwrap();
        config.index.sources = vec![Source {
            path: notes.clone(),
            kind: SourceKind::Docs,
        }];
        let filter = Filter::new(&config).unwrap();
        let check = |p: &str| filter.check(&notes.join(p));
        assert_eq!(check("a.md").unwrap().unwrap().kind, Kind::Doc);
        // The .gitignore above the source applies (inside the repository); `.ignore` whitelists.
        assert!(
            matches!(check("draft1.md"), Err(Reason::Ignored { pattern, .. }) if pattern == "draft*.md")
        );
        assert!(check("draft-keep.md").is_ok());
        assert!(matches!(check("private/b.md"), Err(Reason::Ignored { .. })));
        assert_eq!(
            check("node_modules/c.md"),
            Err(Reason::Builtin("node_modules/".into()))
        );
        assert_eq!(check(".env"), Err(Reason::Sensitive(".env".into())));
        assert_eq!(check(".hidden.md"), Err(Reason::Hidden));
        assert_eq!(
            check("sub/backup/d.md"),
            Err(Reason::Rule("backup/".into()))
        );
        assert_eq!(check("shot.png"), Err(Reason::Kind(SourceKind::Docs)));
        let saved_page = Err(Reason::Builtin(SAVED_PAGE_CODE.into()));
        assert_eq!(check("Saved Page_files/app.js"), saved_page);
        assert_eq!(check("Saved Page_files/frames/f.htm"), saved_page);
        assert_eq!(check("sub/x_files/s.css"), saved_page);
        assert_eq!(check("P_Files/APP.JS"), saved_page);
        assert!(check("Saved Page_files/notes.md").is_ok());
        assert!(check("lib.js").is_ok());
        // Not excluded as builtin: only the source's kind (documents) leaves the image out.
        assert_eq!(
            check("Saved Page_files/logo.png"),
            Err(Reason::Kind(SourceKind::Docs))
        );
        assert_eq!(check("data.bin"), Err(Reason::Unsupported));
        assert_eq!(
            filter.check(&dir.join("repo/other.md")),
            Err(Reason::OutsideSources)
        );

        config.exclude.gitignore = false;
        config.exclude.builtin = false;
        let filter = Filter::new(&config).unwrap();
        assert!(filter.check(&notes.join("draft1.md")).is_ok());
        assert!(filter.check(&notes.join("private/b.md")).is_ok());
        assert!(filter.check(&notes.join("node_modules/c.md")).is_ok());
        fs::remove_dir_all(dir).unwrap();
    }
}
