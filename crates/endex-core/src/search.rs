//! Query execution: vector, keyword and hybrid (RRF) search, grouped by kind.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::str::FromStr;

use anyhow::{Result, bail};

use crate::Kind;
use crate::catalog::Catalog;
use crate::config::Config;
use crate::embed::{self, Device, Embedder};
use crate::store::{Store, StoredHit};
use crate::terms::Analyzer;

pub const RRF_K: f32 = 60.0;
/// Chunk candidates fetched per requested file, so that per-file dedup still fills the limit.
const CANDIDATES_PER_HIT: usize = 4;
const MAX_CANDIDATES: usize = 10_000;
const SNIPPET_CHARS: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Hybrid,
    Vector,
    Keyword,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Vector, Mode::Keyword, Mode::Hybrid];

    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Hybrid => "hybrid",
            Mode::Vector => "vector",
            Mode::Keyword => "keyword",
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Mode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "hybrid" => Ok(Mode::Hybrid),
            "vector" => Ok(Mode::Vector),
            "keyword" => Ok(Mode::Keyword),
            _ => bail!("unknown mode {s:?} (expected hybrid, vector or keyword)"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub path: String,
    pub kind: Kind,
    pub title: String,
    pub location: String,
    pub snippet: String,
    pub score: f32,
}

/// Documents and images are ranked separately: their scores are not comparable.
#[derive(Debug, Default)]
pub struct Results {
    pub docs: Vec<Hit>,
    pub images: Vec<Hit>,
}

pub struct Searcher<'a> {
    config: &'a Config,
    store: Store,
    analyzer: Analyzer,
    embedder: Option<Box<dyn Embedder>>,
}

impl<'a> Searcher<'a> {
    pub async fn open(config: &'a Config) -> Result<Self> {
        if config.catalog_path().exists() {
            Catalog::open(&config.catalog_path())?.check_embedding_identity(
                &embed::model_identity(config)?,
                config.embedding.prefix_style.as_str(),
            )?;
        }
        Ok(Self {
            config,
            store: Store::open(&config.index_dir()).await?,
            analyzer: Analyzer::new(),
            embedder: None,
        })
    }

    pub async fn search(&mut self, query: &str, mode: Mode, limit: usize) -> Result<Results> {
        let vector = match mode {
            Mode::Keyword => None,
            Mode::Vector | Mode::Hybrid => Some(self.embed_query(query)?),
        };
        let terms = self.analyzer.terms(query);
        let joined = terms.join(" ");
        let store = &self.store;
        let mut results = Results::default();
        for kind in [Kind::Doc, Kind::Image] {
            let by_vector = match &vector {
                Some(v) => {
                    distinct_files(limit, async |n| store.vector_search(v, kind, n).await).await?
                }
                None => Vec::new(),
            };
            let by_keyword = match mode {
                Mode::Vector => Vec::new(),
                Mode::Keyword | Mode::Hybrid => {
                    distinct_files(limit, async |n| {
                        store.keyword_search(&joined, kind, n).await
                    })
                    .await?
                }
            };
            let ranked = match mode {
                Mode::Vector => by_vector,
                Mode::Keyword => by_keyword,
                Mode::Hybrid => fuse(by_vector, by_keyword),
            };
            let hits = ranked
                .into_iter()
                .take(limit)
                .map(|h| Hit {
                    snippet: snippet(&h.text, &terms, SNIPPET_CHARS),
                    path: h.path,
                    kind: h.kind,
                    title: h.title,
                    location: h.location,
                    score: h.score,
                })
                .collect();
            match kind {
                Kind::Doc => results.docs = hits,
                Kind::Image => results.images = hits,
            }
        }
        Ok(results)
    }

    /// Queries always embed on the CPU: low latency, no GPU wake-up.
    fn embed_query(&mut self, query: &str) -> Result<Vec<f32>> {
        if self.embedder.is_none() {
            self.embedder = Some(embed::open(self.config, Device::Cpu)?);
        }
        self.embedder
            .as_mut()
            .expect("just created")
            .embed_query(query)
    }
}

/// The best-ranked chunk of each file, at least `limit` files when the index has them:
/// fetches deeper while several chunks of one file leave the list short.
async fn distinct_files(
    limit: usize,
    fetch: impl AsyncFn(usize) -> Result<Vec<StoredHit>>,
) -> Result<Vec<StoredHit>> {
    let mut candidates = limit * CANDIDATES_PER_HIT;
    while candidates > 0 {
        let hits = fetch(candidates).await?;
        let exhausted = hits.len() < candidates || candidates >= MAX_CANDIDATES;
        let mut seen = HashSet::new();
        let files: Vec<StoredHit> = hits
            .into_iter()
            .filter(|h| seen.insert(h.path.clone()))
            .collect();
        if files.len() >= limit || exhausted {
            return Ok(files);
        }
        candidates = (candidates * 4).min(MAX_CANDIDATES);
    }
    Ok(Vec::new())
}

/// Vector ranks go first so that ties (one list each) favour the vector hit.
fn fuse(vector: Vec<StoredHit>, keyword: Vec<StoredHit>) -> Vec<StoredHit> {
    let lists = [
        vector.iter().map(|h| h.path.as_str()).collect::<Vec<_>>(),
        keyword.iter().map(|h| h.path.as_str()).collect::<Vec<_>>(),
    ];
    let fused = rrf(&lists, RRF_K);
    let mut details: HashMap<&str, &StoredHit> = HashMap::new();
    // Prefer the keyword chunk for display: it contains the query words.
    for h in vector.iter().chain(keyword.iter()) {
        details.insert(h.path.as_str(), h);
    }
    fused
        .into_iter()
        .map(|(path, score)| StoredHit {
            score,
            ..details[path].clone()
        })
        .collect()
}

/// Reciprocal rank fusion: score(d) = Σ 1 / (k + rank(d)), ranks starting at 1.
/// Ties keep first-seen order.
pub fn rrf<'a>(lists: &[Vec<&'a str>], k: f32) -> Vec<(&'a str, f32)> {
    let mut scores: Vec<(&str, f32)> = Vec::new();
    let mut index: HashMap<&str, usize> = HashMap::new();
    for list in lists {
        for (rank, &key) in list.iter().enumerate() {
            let add = 1.0 / (k + rank as f32 + 1.0);
            match index.get(key) {
                Some(&i) => scores[i].1 += add,
                None => {
                    index.insert(key, scores.len());
                    scores.push((key, add));
                }
            }
        }
    }
    scores.sort_by(|a, b| b.1.total_cmp(&a.1));
    scores
}

/// A single-line excerpt around the first query term found in `text`.
fn snippet(text: &str, terms: &[String], max_chars: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower = flat.to_lowercase();
    let start_byte = terms
        .iter()
        .filter(|t| t.chars().count() >= 2)
        .filter_map(|t| lower.find(t.as_str()))
        .min()
        .unwrap_or(0);
    // Lowercasing can shift byte offsets for some scripts; fall back to the start then.
    let start_byte = if flat.is_char_boundary(start_byte) && lower.len() == flat.len() {
        start_byte
    } else {
        0
    };
    let start_char = flat[..start_byte]
        .chars()
        .count()
        .saturating_sub(max_chars / 4);
    let excerpt: String = flat.chars().skip(start_char).take(max_chars).collect();
    let mut out = String::new();
    if start_char > 0 {
        out.push('…');
    }
    out.push_str(&excerpt);
    if start_char + max_chars < flat.chars().count() {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::rrf;

    #[test]
    fn rrf_rewards_agreement_and_keeps_order_on_ties() {
        let fused = rrf(&[vec!["a", "b", "c"], vec!["b", "c", "d"]], 60.0);
        let order: Vec<&str> = fused.iter().map(|(k, _)| *k).collect();
        assert_eq!(order, ["b", "c", "a", "d"]);
        assert!((fused[0].1 - (1.0 / 62.0 + 1.0 / 61.0)).abs() < 1e-6);
        let fused = rrf(&[vec!["x"], vec!["y"]], 60.0);
        assert_eq!(
            fused.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
            ["x", "y"]
        );
    }
}
