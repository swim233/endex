//! Query execution: vector, keyword and hybrid (RRF) search, grouped by kind.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::str::FromStr;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};

use crate::Kind;
use crate::catalog::Catalog;
use crate::config::{Config, parse_enum};
use crate::embed::{self, Device, Embedder, PrefixStyle};
use crate::store::{Store, StoredHit, TableUnavailable};
use crate::terms::{Analyzer, is_cjk};

pub const RRF_K: f32 = 60.0;
/// Chunk candidates fetched per requested file, so that per-file dedup still fills the limit.
const CANDIDATES_PER_HIT: usize = 4;
const MAX_CANDIDATES: usize = 10_000;
const SNIPPET_CHARS: usize = 160;
/// After a table failed to load, later calls fail fast for this long instead of waiting out the
/// retries again (a little over the popup's 2 s Status poll).
const UNAVAILABLE_FAST_FAIL: Duration = Duration::from_secs(3);

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
    /// The chunk shown for the file.
    pub id: String,
    pub path: String,
    pub kind: Kind,
    pub title: String,
    pub location: String,
    pub snippet: String,
    /// RRF score in hybrid mode, cosine similarity in vector mode, BM25 in keyword mode.
    pub score: f32,
    /// Cosine similarity of the file's best chunk, if the vector side ranked the file.
    pub vector_score: Option<f32>,
    /// Set if the keyword side ranked the file.
    pub keyword: Option<KeywordMatch>,
    /// Modification time when indexed, in nanoseconds since the epoch.
    pub mtime: i64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeywordMatch {
    /// BM25 of the file's best chunk.
    pub score: f32,
    /// 1-based rank among the keyword side's files.
    pub rank: usize,
}

/// Documents and images are ranked separately: their scores are not comparable.
#[derive(Debug, Default)]
pub struct Results {
    pub docs: Vec<Hit>,
    pub images: Vec<Hit>,
    /// The analyzed query.
    pub terms: Vec<String>,
}

/// Number of files to return per group.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub docs: usize,
    pub images: usize,
}

impl Limits {
    fn of(self, kind: Kind) -> usize {
        match kind {
            Kind::Doc => self.docs,
            Kind::Image => self.images,
        }
    }
}

/// The error inside `anyhow::Error` when a newer query superseded a search.
#[derive(Debug)]
pub struct Cancelled;

impl fmt::Display for Cancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("superseded by a newer query")
    }
}

impl std::error::Error for Cancelled {}

/// One row and its neighbours among the rows of the same file.
#[derive(Debug)]
pub struct ChunkView {
    pub row: StoredHit,
    /// 0-based position among the file's rows, and their number.
    pub index: usize,
    pub count: usize,
    pub prev: Option<String>,
    pub next: Option<String>,
}

pub struct Searcher {
    config: Config,
    /// `None` until an index run creates the table.
    store: Option<Store>,
    analyzer: Analyzer,
    embedder: Option<Box<dyn Embedder>>,
    /// The prefix style of the indexed vectors, which queries must use.
    query_style: PrefixStyle,
    /// When the table last failed to load.
    unavailable_at: Option<Instant>,
    /// Never writes into the index directory (see [`Searcher::untouched`]).
    untouched: bool,
}

impl Searcher {
    /// Opens the index lazily: every call first follows changes made by `endex index`.
    pub fn new(config: &Config) -> Self {
        Self {
            config: config.clone(),
            store: None,
            analyzer: Analyzer::new(),
            embedder: None,
            query_style: config.embedding.prefix_style,
            unavailable_at: None,
            untouched: false,
        }
    }

    /// For indexes that must stay as they are, such as the public eval sets: not even SQLite's
    /// shared memory is written.
    pub fn untouched(config: &Config) -> Self {
        Self {
            untouched: true,
            ..Self::new(config)
        }
    }

    /// A changed config (endexd reloads it); the query model is reloaded if it changed.
    pub fn set_config(&mut self, config: &Config) {
        if config.embedding != self.config.embedding
            || config.devices.query != self.config.devices.query
        {
            self.embedder = None;
        }
        self.config = config.clone();
    }

    /// Follows index writers (`endex index`, endexd's indexer): new table versions, a table
    /// created since, a rebuilt one, or a model change.
    pub async fn refresh(&mut self) -> Result<()> {
        let catalog = self.config.catalog_path();
        if catalog.exists() {
            let catalog = match self.untouched {
                true => Catalog::open_untouched(&catalog)?,
                false => Catalog::open_read_only(&catalog)?,
            };
            // Vectors of another model are not comparable; another prefix style only means
            // that queries keep the index's style until it is rebuilt.
            let model = embed::model_identity(&self.config)?;
            if let Some(stored) = catalog.meta("model")?.filter(|m| *m != model) {
                bail!(
                    "the index was built with model `{stored}` but the config names `{model}`; \
                     rebuild the index"
                );
            }
            let style = catalog
                .meta("prefix_style")?
                .and_then(|s| parse_enum(&s).ok())
                .unwrap_or(self.config.embedding.prefix_style);
            if style != self.query_style {
                self.query_style = style;
                self.embedder = None;
            }
        }
        if let Some(store) = &self.store
            && store.checkout_latest().await.is_ok()
        {
            return Ok(());
        }
        let retry = self
            .unavailable_at
            .is_none_or(|at| at.elapsed() > UNAVAILABLE_FAST_FAIL);
        let opened = Store::open_existing(&self.config.index_dir(), retry).await;
        self.unavailable_at = match &opened {
            Err(e) if e.is::<TableUnavailable>() => Some(Instant::now()),
            _ => None,
        };
        self.store = opened?;
        Ok(())
    }

    pub async fn search(&mut self, query: &str, mode: Mode, limit: usize) -> Result<Results> {
        let limits = Limits {
            docs: limit,
            images: limit,
        };
        self.search_until(query, mode, limits, &|| false).await
    }

    /// Searches with a limit per group; gives up with [`Cancelled`] between stages once
    /// `cancelled` returns true.
    pub async fn search_until(
        &mut self,
        query: &str,
        mode: Mode,
        limits: Limits,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Results> {
        let result = self.search_once(query, mode, limits, cancelled).await;
        if self.reopen_after(&result) {
            return self.search_once(query, mode, limits, cancelled).await;
        }
        result
    }

    async fn search_once(
        &mut self,
        query: &str,
        mode: Mode,
        limits: Limits,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Results> {
        let check = || {
            if cancelled() {
                Err(anyhow::Error::new(Cancelled))
            } else {
                Ok(())
            }
        };
        check()?;
        self.refresh().await?;
        let terms = self.analyzer.terms(query);
        let joined = terms.join(" ");
        let mut results = Results::default();
        if self.store.is_some() {
            let vector = match mode {
                Mode::Keyword => None,
                Mode::Vector | Mode::Hybrid => Some(self.embed_query(query)?),
            };
            let store = self.store.as_ref().expect("checked above");
            for kind in [Kind::Doc, Kind::Image] {
                check()?;
                let limit = limits.of(kind);
                let by_vector = match &vector {
                    Some(v) => {
                        distinct_files(limit, async |n| store.vector_search(v, kind, n).await)
                            .await?
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
                let hits = rank(mode, by_vector, by_keyword)
                    .into_iter()
                    .take(limit)
                    .map(|r| Hit {
                        snippet: snippet(&r.hit.text, &terms, SNIPPET_CHARS),
                        id: r.hit.id,
                        path: r.hit.path,
                        kind: r.hit.kind,
                        title: r.hit.title,
                        location: r.hit.location,
                        score: r.score,
                        vector_score: r.vector_score,
                        keyword: r.keyword,
                        mtime: r.hit.mtime,
                    })
                    .collect();
                match kind {
                    Kind::Doc => results.docs = hits,
                    Kind::Image => results.images = hits,
                }
            }
        }
        results.terms = terms;
        Ok(results)
    }

    /// A row by id, with its position in its file.
    pub async fn chunk(&mut self, id: &str) -> Result<Option<ChunkView>> {
        let result = self.chunk_once(id).await;
        if self.reopen_after(&result) {
            return self.chunk_once(id).await;
        }
        result
    }

    async fn chunk_once(&mut self, id: &str) -> Result<Option<ChunkView>> {
        self.refresh().await?;
        let Some(store) = &self.store else {
            return Ok(None);
        };
        let Some(row) = store.row(id).await? else {
            return Ok(None);
        };
        let mut ids = store.ids_of(&row.path).await?;
        ids.sort_by_key(|id| chunk_number(id));
        let index = ids.iter().position(|i| i == id).unwrap_or(0);
        Ok(Some(ChunkView {
            prev: index.checked_sub(1).map(|i| ids[i].clone()),
            next: ids.get(index + 1).cloned(),
            index,
            count: ids.len(),
            row,
        }))
    }

    /// Indexed rows of `kind`; 0 before the first index run.
    pub async fn count(&mut self, kind: Kind) -> Result<usize> {
        let result = self.count_once(kind).await;
        if self.reopen_after(&result) {
            return self.count_once(kind).await;
        }
        result
    }

    async fn count_once(&mut self, kind: Kind) -> Result<usize> {
        self.refresh().await?;
        match &self.store {
            Some(store) => store.count(kind).await,
            None => Ok(0),
        }
    }

    /// After a failure, drops the table so that one retry reopens it: another process may have
    /// rebuilt the table or pruned the version this reader was on.
    fn reopen_after<T>(&mut self, result: &Result<T>) -> bool {
        let retry = matches!(result, Err(e) if e.downcast_ref::<Cancelled>().is_none())
            && self.store.is_some();
        if retry {
            self.store = None;
        }
        retry
    }

    pub fn terms(&self, text: &str) -> Vec<String> {
        self.analyzer.terms(text)
    }

    /// Loads the query embedder now instead of on the first query.
    pub fn warm_up(&mut self) -> Result<()> {
        self.embed_query("warm-up").map(drop)
    }

    /// Queries embed on `devices.query`, the CPU by default: low latency, no GPU wake-up.
    fn embed_query(&mut self, query: &str) -> Result<Vec<f32>> {
        if self.embedder.is_none() {
            let mut config = self.config.clone();
            config.embedding.prefix_style = self.query_style;
            let device = config.devices.query;
            let embedder = embed::open(&config, device, None).or_else(|error| match device {
                Device::Gpu => embed::open(&config, Device::Cpu, None),
                Device::Cpu => Err(error),
            })?;
            self.embedder = Some(embedder);
        }
        self.embedder
            .as_mut()
            .expect("just created")
            .embed_query(query)
    }
}

/// Row ids are `path#n`, n counting the file's chunks from 0.
fn chunk_number(id: &str) -> usize {
    id.rsplit_once('#')
        .and_then(|(_, n)| n.parse().ok())
        .unwrap_or(0)
}

/// A file's place in the final order, with what each side contributed.
struct Ranked {
    /// The chunk shown for the file.
    hit: StoredHit,
    score: f32,
    vector_score: Option<f32>,
    keyword: Option<KeywordMatch>,
}

fn rank(mode: Mode, by_vector: Vec<StoredHit>, by_keyword: Vec<StoredHit>) -> Vec<Ranked> {
    let vector_scores: HashMap<String, f32> = by_vector
        .iter()
        .map(|h| (h.path.clone(), h.score))
        .collect();
    let keyword_matches: HashMap<String, KeywordMatch> = by_keyword
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let m = KeywordMatch {
                score: h.score,
                rank: i + 1,
            };
            (h.path.clone(), m)
        })
        .collect();
    let ordered = match mode {
        Mode::Vector => by_vector,
        Mode::Keyword => by_keyword,
        Mode::Hybrid => fuse(by_vector, by_keyword),
    };
    ordered
        .into_iter()
        .map(|hit| Ranked {
            score: hit.score,
            vector_score: vector_scores.get(&hit.path).copied(),
            keyword: keyword_matches.get(&hit.path).copied(),
            hit,
        })
        .collect()
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

/// Character ranges `(start, len)` of the query terms in `text`, case-insensitive, merged and
/// in order. CJK terms match anywhere (a single character can be a word). Other terms match at
/// word starts (after a non-alphanumeric or a CJK character): short ones as whole words, longer
/// ones also as a prefix, which then covers the rest of the word (`alter` → `altered`); English
/// stop words are not highlighted.
pub fn highlights(text: &str, terms: &[String]) -> Vec<(usize, usize)> {
    let lower: Vec<char> = text
        .chars()
        .map(|c| c.to_lowercase().next().unwrap_or(c))
        .collect();
    let word_end = |mut i: usize| {
        while i < lower.len() && lower[i].is_alphanumeric() && !is_cjk(lower[i]) {
            i += 1;
        }
        i
    };
    let mut spans = Vec::new();
    for term in terms {
        let needle: Vec<char> = term.chars().collect();
        let cjk = needle.iter().all(|&c| is_cjk(c));
        if needle.is_empty()
            || needle.len() > lower.len()
            || (!cjk && (needle.len() < 2 || EN_STOP_WORDS.contains(&term.as_str())))
        {
            continue;
        }
        for start in 0..=lower.len() - needle.len() {
            if lower[start..start + needle.len()] != needle[..] {
                continue;
            }
            let end = start + needle.len();
            if cjk {
                spans.push((start, end));
            } else if start == 0 || !lower[start - 1].is_alphanumeric() || is_cjk(lower[start - 1])
            {
                let word = word_end(end);
                if word == end || needle.len() >= MIN_PREFIX_HIGHLIGHT {
                    spans.push((start, word));
                }
            }
        }
    }
    spans.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in spans {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
        .into_iter()
        .map(|(start, end)| (start, end - start))
        .collect()
}

const MIN_PREFIX_HIGHLIGHT: usize = 4;

/// Words that match almost anywhere; LanceDB drops them from the keyword index too.
const EN_STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "but", "by", "for", "from", "has", "have", "if",
    "in", "into", "is", "it", "its", "no", "not", "of", "on", "or", "such", "that", "the", "their",
    "then", "there", "these", "they", "this", "to", "was", "were", "will", "with",
];

#[cfg(test)]
mod tests {
    use super::{highlights, rrf};

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

    #[test]
    fn highlights_are_case_insensitive_merged_character_ranges() {
        let terms = ["守护进程", "进程", "dbus", "a", "猫"].map(String::from);
        let text = "启动守护进程 via D-Bus or DBus, a 猫";
        assert_eq!(highlights(text, &terms), [(2, 4), (20, 4), (28, 1)]);
        // Word starts only; short terms whole, longer ones as prefixes; no stop words.
        let terms = ["the", "alter", "gut", "store", "rust"].map(String::from);
        let text = "The gut, altered to restore guts 用Rust写";
        assert_eq!(highlights(text, &terms), [(4, 3), (9, 7), (34, 4)]);
    }
}
