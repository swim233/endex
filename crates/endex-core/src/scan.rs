//! Walking a source folder under the exclusion rules, and comparing what is on disk with the
//! catalog.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::catalog::Known;
use crate::exclude::{Filter, SourceRules, file_kind};
use crate::index::mtime_ns;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stat {
    pub size: u64,
    pub mtime: i64,
}

#[derive(Debug, Default)]
pub struct Walk {
    /// Included files by path.
    pub files: BTreeMap<String, Stat>,
    /// Included folders, the start folder first.
    pub dirs: Vec<PathBuf>,
    /// Folders and files that could not be read (or a source folder that is missing); what the
    /// catalog has at or below them is kept.
    pub unreadable: Vec<PathBuf>,
    /// Paths that are not UTF-8 and cannot be indexed.
    pub skipped: usize,
    /// `stop` returned true before the walk was complete.
    pub stopped: bool,
}

/// Walks `dir` (the source root or a folder below it), entering only included folders and
/// never following symlinks. With `files` false only folders are collected (for watches).
pub fn walk(
    filter: &Filter,
    source: &SourceRules,
    dir: &Path,
    files: bool,
    stop: &dyn Fn() -> bool,
) -> Walk {
    let mut out = Walk::default();
    let Ok(chain) = filter.dir_chain(source, dir) else {
        return out;
    };
    match fs::symlink_metadata(dir) {
        Ok(meta) if meta.is_dir() => {}
        // A missing source folder is unavailable (an unmounted drive, a folder renamed for a
        // while), not empty: its files stay indexed.
        _ if dir == source.root => {
            out.unreadable.push(dir.to_owned());
            return out;
        }
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            out.unreadable.push(dir.to_owned());
            return out;
        }
        // A folder below the source that is gone or no longer a folder.
        _ => return out,
    }
    let mut stack = vec![(dir.to_owned(), chain)];
    while let Some((dir, chain)) = stack.pop() {
        if stop() {
            out.stopped = true;
            break;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => {
                out.unreadable.push(dir);
                continue;
            }
        };
        for entry in entries {
            // A read error mid-listing (a flaky disk) must not look like deleted files.
            let Ok(entry) = entry else {
                out.unreadable.push(dir.clone());
                break;
            };
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    keep_unless_gone(&mut out, path, &error);
                    continue;
                }
            };
            if file_type.is_dir() {
                if filter.entry(source, &path, true, &chain).is_ok() {
                    let mut chain = chain.clone();
                    filter.push_ignores(&mut chain, &path);
                    stack.push((path, chain));
                }
            } else if files && file_type.is_file() {
                if filter.entry(source, &path, false, &chain).is_err()
                    || file_kind(source, &path).is_err()
                {
                    continue;
                }
                let Some(name) = path.to_str() else {
                    out.skipped += 1;
                    continue;
                };
                let meta = match entry.metadata() {
                    Ok(meta) => meta,
                    Err(error) => {
                        keep_unless_gone(&mut out, path, &error);
                        continue;
                    }
                };
                let stat = Stat {
                    size: meta.len(),
                    mtime: mtime_ns(&meta),
                };
                out.files.insert(name.to_owned(), stat);
            }
        }
        out.dirs.push(dir);
    }
    out
}

/// An entry that vanished since it was listed is gone; one that cannot be read is kept.
fn keep_unless_gone(out: &mut Walk, path: PathBuf, error: &io::Error) {
    if error.kind() != io::ErrorKind::NotFound {
        out.unreadable.push(path);
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Diff {
    /// New files, files whose size or mtime differ, and files whose last attempt failed.
    pub changed: Vec<String>,
    /// Catalog entries no longer on disk or no longer included.
    pub removed: Vec<String>,
    pub unchanged: usize,
}

/// Compares a complete walk with the catalog entries below the same folder.
pub fn diff(
    walked: &BTreeMap<String, Stat>,
    known: &BTreeMap<String, Known>,
    unreadable: &[PathBuf],
) -> Diff {
    let mut out = Diff::default();
    for (path, stat) in walked {
        match known.get(path) {
            Some(k) if k.ok && k.size == stat.size && k.mtime == stat.mtime => out.unchanged += 1,
            _ => out.changed.push(path.clone()),
        }
    }
    out.removed = known
        .keys()
        .filter(|path| !walked.contains_key(*path))
        .filter(|path| {
            !unreadable
                .iter()
                .any(|dir| Path::new(path).starts_with(dir))
        })
        .cloned()
        .collect();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Source, SourceKind};

    #[test]
    fn diff_finds_new_changed_failed_and_removed() {
        let stat = |size, mtime| Stat { size, mtime };
        let known = |size, mtime, ok| Known { size, mtime, ok };
        let walked = BTreeMap::from([
            ("/s/new.md".to_owned(), stat(1, 1)),
            ("/s/same.md".to_owned(), stat(2, 2)),
            ("/s/edited.md".to_owned(), stat(3, 9)),
            ("/s/failed.md".to_owned(), stat(4, 4)),
        ]);
        let catalog = BTreeMap::from([
            ("/s/same.md".to_owned(), known(2, 2, true)),
            ("/s/edited.md".to_owned(), known(3, 3, true)),
            ("/s/failed.md".to_owned(), known(4, 4, false)),
            ("/s/gone.md".to_owned(), known(5, 5, true)),
            ("/s/locked/kept.md".to_owned(), known(6, 6, true)),
        ]);
        let d = diff(&walked, &catalog, &[PathBuf::from("/s/locked")]);
        assert_eq!(d.changed, ["/s/edited.md", "/s/failed.md", "/s/new.md"]);
        assert_eq!(d.removed, ["/s/gone.md"]);
        assert_eq!(d.unchanged, 1);
    }

    #[test]
    fn a_missing_source_keeps_its_rows_a_missing_subfolder_does_not() {
        let dir = std::env::temp_dir().join(format!("endex-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let root = dir.join("src");
        fs::create_dir_all(root.join("sub")).unwrap();
        let mut config =
            Config::parse("[embedding]\nlibrary = \"/l\"\nmodel = \"/m\"\ntokenizer = \"/t\"\n")
                .unwrap();
        config.index.sources = vec![Source {
            path: root.clone(),
            kind: SourceKind::All,
        }];
        let known = |path: &str| {
            let k = Known {
                size: 1,
                mtime: 1,
                ok: true,
            };
            BTreeMap::from([(root.join(path).to_string_lossy().into_owned(), k)])
        };
        let filter = Filter::new(&config).unwrap();
        let source = &filter.sources()[0];
        let sub = walk(&filter, source, &root.join("sub/gone"), true, &|| false);
        assert!(sub.unreadable.is_empty());
        let d = diff(&sub.files, &known("sub/gone/a.md"), &sub.unreadable);
        assert_eq!(d.removed.len(), 1);

        fs::remove_dir_all(&root).unwrap();
        let missing = walk(&filter, source, &root, true, &|| false);
        assert_eq!(missing.unreadable, std::slice::from_ref(&root));
        let d = diff(&missing.files, &known("sub/a.md"), &missing.unreadable);
        assert!(d.removed.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }
}
