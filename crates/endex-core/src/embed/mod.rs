//! Text and image embeddings into one shared vector space.

pub mod litert;

use std::fmt;
use std::fs;
use std::str::FromStr;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::config::Config;

pub const DIM: usize = 768;

/// Produces L2-normalized `DIM`-dimensional vectors; implementations own their prefixes.
pub trait Embedder {
    fn device(&self) -> Device;
    fn embed_query(&mut self, text: &str) -> Result<Vec<f32>>;
    fn embed_document(&mut self, text: &str) -> Result<Vec<f32>>;
    fn embed_image(&mut self, bytes: &[u8]) -> Result<Vec<f32>>;
}

/// The configured embedder on `device`. The only place that picks the implementation.
pub fn open(config: &Config, device: Device) -> Result<Box<dyn Embedder>> {
    let cache_dir = config.litert_cache_dir();
    fs::create_dir_all(&cache_dir)?;
    let engine = litert::EngineConfig {
        library: &config.embedding.library,
        model: &config.embedding.model,
        cache_dir: &cache_dir,
        device,
    };
    Ok(Box::new(litert::LiteRtEmbedder::new(
        &engine,
        config.embedding.prefix_style,
    )?))
}

/// Identifies the vector space: vectors from different identities must not be mixed.
pub fn model_identity(config: &Config) -> Result<String> {
    let model = &config.embedding.model;
    let size = fs::metadata(model)
        .with_context(|| format!("embedding model {}", model.display()))?
        .len();
    let name = model.file_name().unwrap_or_default().to_string_lossy();
    Ok(format!("{name}:{size}:{DIM}"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Cpu,
    Gpu,
}

impl Device {
    pub fn as_str(self) -> &'static str {
        match self {
            Device::Cpu => "cpu",
            Device::Gpu => "gpu",
        }
    }
}

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Device {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "cpu" => Ok(Device::Cpu),
            "gpu" => Ok(Device::Gpu),
            _ => bail!("unknown device {s:?} (expected cpu or gpu)"),
        }
    }
}

/// Task prefixes for EmbeddingGemma. Changing the style invalidates the index.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrefixStyle {
    /// As used by the LiteRT demo and Sift.
    #[default]
    Litert,
    /// As documented on the google/embeddinggemma-2 model card.
    ModelCard,
}

impl PrefixStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            PrefixStyle::Litert => "litert",
            PrefixStyle::ModelCard => "model_card",
        }
    }

    pub fn query(self, text: &str) -> String {
        match self {
            PrefixStyle::Litert => format!("task: search query | text: {}", text.trim()),
            PrefixStyle::ModelCard => format!("task: search result | query: {}", text.trim()),
        }
    }

    pub fn document(self, text: &str) -> String {
        match self {
            PrefixStyle::Litert => format!("task: search result | text: {}", text.trim()),
            PrefixStyle::ModelCard => format!("title: none | text: {}", text.trim()),
        }
    }
}

pub fn normalize(vector: &mut [f32]) -> Result<()> {
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    ensure!(
        norm.is_finite() && norm > 0.0,
        "embedding has an invalid norm ({norm})"
    );
    vector.iter_mut().for_each(|v| *v /= norm);
    Ok(())
}
