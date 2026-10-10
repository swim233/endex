//! Retrieval quality on a labelled query set: Recall@k and MRR per mode and per query kind.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::config::expand_home;
use crate::search::{Mode, Searcher};

#[derive(Debug, Deserialize)]
struct QueryFile {
    query: Vec<Query>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub text: String,
    pub expect: Vec<String>,
    /// Free-form label such as "visual" or "text"; metrics are also reported per kind.
    pub kind: Option<String>,
}

pub fn load(path: &Path) -> Result<Vec<Query>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut file: QueryFile =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    for q in &mut file.query {
        for e in &mut q.expect {
            *e = expand_home(Path::new(e.as_str()))
                .to_string_lossy()
                .into_owned();
        }
    }
    Ok(file.query)
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Metrics {
    pub queries: usize,
    recall_sum: f64,
    rr_sum: f64,
}

impl Metrics {
    fn add(&mut self, recall: f64, rr: f64) {
        self.queries += 1;
        self.recall_sum += recall;
        self.rr_sum += rr;
    }

    pub fn recall(&self) -> f64 {
        self.recall_sum / self.queries.max(1) as f64
    }

    pub fn mrr(&self) -> f64 {
        self.rr_sum / self.queries.max(1) as f64
    }
}

#[derive(Debug)]
pub struct Miss {
    pub query: String,
    /// Expected paths not in the top k.
    pub missing: Vec<String>,
    /// Best rank of any expected path, when one was found.
    pub best_rank: Option<usize>,
}

#[derive(Debug)]
pub struct ModeReport {
    pub mode: Mode,
    pub overall: Metrics,
    pub by_kind: BTreeMap<String, Metrics>,
    pub misses: Vec<Miss>,
}

/// An expected path's rank is its 1-based position in the result group (documents or images)
/// that contains it.
pub async fn run(
    searcher: &mut Searcher,
    queries: &[Query],
    limit: usize,
) -> Result<Vec<ModeReport>> {
    let mut reports = Vec::new();
    for mode in Mode::ALL {
        let mut report = ModeReport {
            mode,
            overall: Metrics::default(),
            by_kind: BTreeMap::new(),
            misses: Vec::new(),
        };
        for q in queries {
            let results = searcher.search(&q.text, mode, limit).await?;
            let rank_of = |path: &str| {
                [&results.docs, &results.images]
                    .into_iter()
                    .find_map(|hits| hits.iter().position(|h| h.path == path).map(|i| i + 1))
            };
            let ranks: Vec<Option<usize>> = q.expect.iter().map(|e| rank_of(e)).collect();
            let (recall, rr) = recall_and_rr(&ranks);
            report.overall.add(recall, rr);
            if let Some(kind) = &q.kind {
                report
                    .by_kind
                    .entry(kind.clone())
                    .or_default()
                    .add(recall, rr);
            }
            if ranks.contains(&None) {
                report.misses.push(Miss {
                    query: q.text.clone(),
                    missing: q
                        .expect
                        .iter()
                        .zip(&ranks)
                        .filter(|(_, r)| r.is_none())
                        .map(|(e, _)| e.clone())
                        .collect(),
                    best_rank: ranks.iter().flatten().min().copied(),
                });
            }
        }
        reports.push(report);
    }
    Ok(reports)
}

/// Recall (share of expected files found) and reciprocal rank of the best-ranked one, given
/// the rank of each expected file (`None` = not in the results).
pub fn recall_and_rr(ranks: &[Option<usize>]) -> (f64, f64) {
    let found = ranks.iter().flatten().count();
    let recall = found as f64 / ranks.len().max(1) as f64;
    let rr = ranks
        .iter()
        .flatten()
        .min()
        .map_or(0.0, |&best| 1.0 / best as f64);
    (recall, rr)
}

#[cfg(test)]
mod tests {
    use super::recall_and_rr;

    #[test]
    fn recall_and_reciprocal_rank() {
        assert_eq!(recall_and_rr(&[None, Some(2)]), (0.5, 0.5));
        assert_eq!(recall_and_rr(&[None]), (0.0, 0.0));
    }
}
