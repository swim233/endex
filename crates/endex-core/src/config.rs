//! `~/.config/endex/config.toml`, the single source of truth for paths and model settings.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::embed::PrefixStyle;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Index, catalog, OCR models, caches and test-set manifests live here.
    pub data_dir: PathBuf,
    pub embedding: EmbeddingConfig,
    #[serde(default)]
    pub index: IndexConfig,
    #[serde(default)]
    pub sample: SampleConfig,
    /// Keeps the index and catalog elsewhere (e.g. one isolated index per eval set) while
    /// models and caches stay shared in `data_dir`.
    #[serde(skip)]
    pub index_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexConfig {
    /// Larger documents are skipped and recorded as errors (images are not limited).
    pub max_document_mb: u64,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            max_document_mb: 10,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
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
        let mut config: Config =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
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

    pub fn ocr_model_dir(&self) -> PathBuf {
        self.data_dir.join("models/ocr")
    }

    pub fn litert_cache_dir(&self) -> PathBuf {
        self.data_dir.join("cache/litert")
    }

    pub fn testset_dir(&self) -> PathBuf {
        self.data_dir.join("testset")
    }
}

pub fn default_path() -> Result<PathBuf> {
    Ok(dirs::config_dir()
        .context("no config directory")?
        .join("endex/config.toml"))
}

fn default_toml() -> String {
    let library = find_litert_library()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| {
            "~/.venvs/litert-lm/lib/python3/site-packages/litert_lm/liblitert-lm.so".into()
        });
    format!(
        r#"# Endex configuration. Paths may start with "~/".

# Index (LanceDB), catalog (SQLite), OCR models, caches and test-set manifests.
data_dir = "~/.local/share/endex"

[embedding]
# LiteRT-LM C library (ships inside the litert-lm Python wheel).
library = "{library}"
model = "~/models/embeddinggemma-2-litert/embeddinggemma-2-740m.litertlm"
# Tokenizer of the same model; only used to size chunks.
tokenizer = "~/models/embeddinggemma-2/tokenizer.json"
# "litert" (task: search query | text: ...) or "model_card" (task: search result | query: ...).
# Changing it requires `endex index --rebuild`.
prefix_style = "litert"

[index]
# Larger documents are skipped and reported as errors.
max_document_mb = 10

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
