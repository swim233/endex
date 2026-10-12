//! Endex core: parsing, OCR, embeddings, storage and search for local documents and images.

pub mod catalog;
pub mod chunk;
pub mod config;
pub mod embed;
pub mod eval;
pub mod exclude;
pub mod index;
pub mod lock;
pub mod ocr;
pub mod packed;
pub mod parse;
pub mod sample;
pub mod scan;
pub mod search;
pub mod store;
pub mod terms;

use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Doc,
    Image,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Doc => "doc",
            Kind::Image => "image",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Kind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "doc" => Ok(Kind::Doc),
            "image" => Ok(Kind::Image),
            _ => anyhow::bail!("unknown kind {s:?}"),
        }
    }
}
