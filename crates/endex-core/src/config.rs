//! `~/.config/endex/config.toml`, the single source of truth for paths, sources and policies.
//! Every key except `[embedding]` has a default, so older files keep loading.

pub mod edit;

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::embed::{Device, PrefixStyle};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Index, catalog, OCR models, caches, usage log and test-set manifests live here.
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    pub embedding: EmbeddingConfig,
    #[serde(default)]
    pub ocr: OcrConfig,
    #[serde(default)]
    pub index: IndexConfig,
    #[serde(default)]
    pub exclude: ExcludeConfig,
    #[serde(default)]
    pub update: UpdateConfig,
    #[serde(default)]
    pub devices: DeviceConfig,
    #[serde(default)]
    pub resources: ResourceConfig,
    #[serde(default)]
    pub log: LogConfig,
    #[serde(default)]
    pub sample: SampleConfig,
    /// Keeps the index and catalog elsewhere (e.g. one isolated index per eval set) while
    /// models and caches stay shared in `data_dir`.
    #[serde(skip)]
    pub index_root: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingConfig {
    /// `liblitert-lm.so` exporting the LiteRT-LM C API.
    pub library: PathBuf,
    /// EmbeddingGemma `.litertlm` model file.
    pub model: PathBuf,
    /// HF `tokenizer.json` of the same model, used to size chunks.
    pub tokenizer: PathBuf,
    #[serde(default)]
    pub prefix_style: PrefixStyle,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OcrConfig {
    /// Recognize text in images; off leaves only the image vector.
    pub enabled: bool,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IndexConfig {
    /// Larger documents are skipped and recorded as errors (images are not limited).
    pub max_document_mb: u64,
    /// Folders endexd keeps indexed.
    pub sources: Vec<Source>,
    /// Suggested folders the user chose not to index.
    pub declined: Vec<PathBuf>,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            max_document_mb: 10,
            sources: Vec::new(),
            declined: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// As written in the file; may start with `~/`. See [`Source::root`].
    pub path: PathBuf,
    #[serde(default)]
    pub kind: SourceKind,
}

impl Source {
    /// The folder as configured (`~` expanded, no trailing slash): the source's identity in the
    /// catalog, which stays the same while the folder is unavailable.
    pub fn owner(&self) -> PathBuf {
        expand_home(&self.path).components().collect()
    }

    /// The real folder, with symlinks resolved, so that a source that links to another disk is
    /// walked and watched where its files are. The configured path while it does not resolve.
    pub fn root(&self) -> PathBuf {
        let owner = self.owner();
        fs::canonicalize(&owner).unwrap_or(owner)
    }
}

/// Which files of a source are indexed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    #[default]
    All,
    Docs,
    Images,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExcludeConfig {
    /// Respect `.gitignore` and `.ignore` files, including those in parent folders.
    pub gitignore: bool,
    /// Skip build output and caches ([`crate::exclude::BUILTIN`]).
    pub builtin: bool,
    /// Never open likely secrets ([`crate::exclude::SENSITIVE`]).
    pub sensitive: bool,
    /// Index files and folders whose name starts with a dot.
    pub hidden: bool,
    /// Extra rules in gitignore syntax, relative to each source folder.
    pub rules: Vec<String>,
}

impl Default for ExcludeConfig {
    fn default() -> Self {
        Self {
            gitignore: true,
            builtin: true,
            sensitive: true,
            hidden: false,
            rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UpdateConfig {
    /// Compare the sources with the index and process what changed.
    pub scan: bool,
    pub scan_interval: ScanInterval,
    /// Follow changes in real time (inotify).
    pub watch: bool,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            scan: true,
            scan_interval: ScanInterval::Startup,
            watch: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanInterval {
    /// When endexd starts.
    #[default]
    Startup,
    Hourly,
    Daily,
}

impl ScanInterval {
    /// `None` for a scan at startup only.
    pub fn period(self) -> Option<std::time::Duration> {
        match self {
            ScanInterval::Startup => None,
            ScanInterval::Hourly => Some(std::time::Duration::from_secs(3600)),
            ScanInterval::Daily => Some(std::time::Duration::from_secs(86400)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DeviceConfig {
    /// Query embeddings.
    pub query: Device,
    /// First builds, rebuilds and large change sets.
    pub bulk: Device,
    /// A few changed files.
    pub incremental: Device,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            query: Device::Cpu,
            bulk: Device::Gpu,
            incremental: Device::Cpu,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResourceConfig {
    pub pause_on_battery: bool,
    /// Index at nice 19 and idle IO priority.
    pub low_priority: bool,
    pub profile: Profile,
    pub threads: Threads,
    /// Minutes without GPU work before the GPU engine is released; 0 keeps it loaded.
    pub gpu_idle_release_min: u32,
}

impl Default for ResourceConfig {
    fn default() -> Self {
        Self {
            pause_on_battery: true,
            low_priority: true,
            profile: Profile::Mid,
            threads: Threads::default(),
            gpu_idle_release_min: 2,
        }
    }
}

impl ResourceConfig {
    /// Thread cap for parsing, OCR and CPU embedding.
    pub fn thread_count(&self) -> usize {
        let t = &self.threads;
        match self.profile {
            Profile::Light => t.light,
            Profile::Mid => t.mid,
            Profile::Heavy => t.heavy,
        }
        .max(1) as usize
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    Light,
    #[default]
    Mid,
    Heavy,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Threads {
    pub light: u32,
    pub mid: u32,
    pub heavy: u32,
}

impl Default for Threads {
    fn default() -> Self {
        Self {
            light: 2,
            mid: 4,
            heavy: 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LogConfig {
    /// Record searches and picked results in `<data_dir>/usage.jsonl`.
    pub usage: bool,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self { usage: true }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SampleConfig {
    pub seed: u64,
}

impl Default for SampleConfig {
    fn default() -> Self {
        Self { seed: 20261010 }
    }
}

impl Config {
    /// Loads the config, writing a commented default on first run.
    pub fn load() -> Result<Self> {
        let path = default_path()?;
        if !path.exists() {
            fs::create_dir_all(path.parent().expect("config path has a parent"))?;
            fs::write(&path, default_toml())
                .with_context(|| format!("writing default config {}", path.display()))?;
            let _ = writeln!(io::stderr(), "created default config {}", path.display());
        }
        Self::from_file(&path)
    }

    pub fn from_file(path: &Path) -> Result<Self> {
        let text =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let mut config: Config = toml::from_str(text)?;
        config.data_dir = expand_home(&config.data_dir);
        let e = &mut config.embedding;
        e.library = expand_home(&e.library);
        e.model = expand_home(&e.model);
        e.tokenizer = expand_home(&e.tokenizer);
        Ok(config)
    }

    pub fn with_index_root(mut self, dir: Option<PathBuf>) -> Self {
        self.index_root = dir.map(|d| expand_home(&d));
        self
    }

    /// Directory holding the index and the catalog.
    pub fn index_root(&self) -> &Path {
        self.index_root.as_deref().unwrap_or(&self.data_dir)
    }

    pub fn index_dir(&self) -> PathBuf {
        self.index_root().join("index")
    }

    pub fn catalog_path(&self) -> PathBuf {
        self.index_root().join("catalog.sqlite")
    }

    /// Held (flock) by the one process allowed to write the index.
    pub fn lock_path(&self) -> PathBuf {
        self.index_root().join("endex.lock")
    }

    pub fn ocr_model_dir(&self) -> PathBuf {
        self.data_dir.join("models/ocr")
    }

    pub fn litert_cache_dir(&self) -> PathBuf {
        self.data_dir.join("cache/litert")
    }

    pub fn testset_dir(&self) -> PathBuf {
        self.data_dir.join("testset")
    }

    pub fn usage_log_path(&self) -> PathBuf {
        self.data_dir.join("usage.jsonl")
    }

    pub fn max_document_bytes(&self) -> u64 {
        self.index.max_document_mb.saturating_mul(1 << 20)
    }
}

pub fn default_path() -> Result<PathBuf> {
    Ok(dirs::config_dir()
        .context("no config directory")?
        .join("endex/config.toml"))
}

/// `$XDG_DATA_HOME/endex`, i.e. `~/.local/share/endex` by default.
fn default_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| expand_home(Path::new("~/.local/share")))
        .join("endex")
}

/// The commented default config, with the LiteRT-LM library located if possible.
pub fn default_toml() -> String {
    let library = find_litert_library()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| {
            "~/.venvs/litert-lm/lib/python3/site-packages/litert_lm/liblitert-lm.so".into()
        });
    format!(
        r#"# Endex configuration. Paths may start with "~/". endexd edits this file when you apply
# settings in the settings window and keeps your comments; edits by hand are picked up too.

# Index (LanceDB), catalog (SQLite), OCR models, caches and the usage log.
# Default: $XDG_DATA_HOME/endex, i.e. ~/.local/share/endex.
# data_dir = "~/.local/share/endex"

[embedding]
# LiteRT-LM C library (ships inside the litert-lm Python wheel).
library = "{library}"
model = "~/models/embeddinggemma-2-litert/embeddinggemma-2-740m.litertlm"
# Tokenizer of the same model; only used to size chunks.
tokenizer = "~/models/embeddinggemma-2/tokenizer.json"
# "litert" (task: search query | text: ...) or "model_card" (task: search result | query: ...).
# Changing it, the model or OCR requires a rebuild of the index (settings window).
prefix_style = "litert"

[ocr]
# Recognize text in screenshots and photos (PP-OCRv6).
enabled = true

[index]
# Folders that endexd keeps indexed; kind is "all", "docs" or "images". Nothing else is read.
# sources = [
#   {{ path = "~/Notes", kind = "all" }},
# ]
sources = []
# Larger documents are skipped and reported as errors.
max_document_mb = 10

[exclude]
# Respect .gitignore and .ignore files (also those in parent folders).
gitignore = true
# Skip build output and caches such as node_modules/, target/ and .git/.
builtin = true
# Never open likely secrets such as .env, *.pem, id_rsa*, *.kdbx, .ssh/ and browser profiles.
sensitive = true
# Index files and folders whose name starts with a dot.
hidden = false
# Extra rules in .gitignore syntax, relative to each source folder.
rules = []

[update]
# Compare the folders with the index: at "startup", "hourly" or "daily".
scan = true
scan_interval = "startup"
# Follow changes in real time (inotify).
watch = true

[devices]
# "cpu" or "gpu". Bulk work (first build, rebuild, many changes) and a few changed files.
query = "cpu"
bulk = "gpu"
incremental = "cpu"

[resources]
pause_on_battery = true
# Index at the lowest CPU and IO priority.
low_priority = true
# Threads for parsing, OCR and CPU embedding: "light", "mid" or "heavy".
profile = "mid"
threads = {{ light = 2, mid = 4, heavy = 8 }}
# Release the GPU after this many idle minutes (0: keep it loaded).
gpu_idle_release_min = 2

[log]
# Record searches and opened results in <data_dir>/usage.jsonl (never leaves this machine).
usage = true

[sample]
# Seed for `endex sample`; the same seed reproduces the same test set.
seed = 20261010
"#
    )
}

/// The wheel's site-packages path contains the Python version, so look it up once.
fn find_litert_library() -> Option<PathBuf> {
    let lib = dirs::home_dir()?.join(".venvs/litert-lm/lib");
    fs::read_dir(lib)
        .ok()?
        .flatten()
        .map(|entry| entry.path().join("site-packages/litert_lm/liblitert-lm.so"))
        .find(|p| p.is_file())
}

pub fn expand_home(path: &Path) -> PathBuf {
    match (path.strip_prefix("~"), dirs::home_dir()) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => path.to_owned(),
    }
}

/// `~/rest` for paths inside the home directory, for display and for writing back.
pub fn tilde(path: &Path) -> String {
    match dirs::home_dir().and_then(|home| path.strip_prefix(home).ok().map(Path::to_owned)) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// Parses a snake_case config value such as `"hourly"` with the enum's serde names.
pub fn parse_enum<T: serde::de::DeserializeOwned>(value: &str) -> Result<T> {
    T::deserialize(serde::de::value::StrDeserializer::<serde::de::value::Error>::new(value))
        .map_err(|e| anyhow::anyhow!("{e}"))
}

/// The serde name of a snake_case config enum value.
pub fn enum_name<T: Serialize>(value: &T) -> String {
    toml::Value::try_from(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

impl fmt::Display for SourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&enum_name(self))
    }
}

impl FromStr for SourceKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        parse_enum(s).or_else(|_| bail!("unknown source kind {s:?} (expected all, docs or images)"))
    }
}
