//! The D-Bus interface `org.endex.Daemon1` and its dictionaries.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use endex_core::Kind;
use endex_core::search::{Cancelled, ChunkView, Hit, Limits, Mode, Results, highlights};
use zbus::message::Header;
use zbus::zvariant::{OwnedValue, Value};

use crate::worker::{Preview, Worker};

pub const NAME: &str = "org.endex.Daemon1";
pub const PATH: &str = "/org/endex/Daemon";
pub const INTERFACE_VERSION: u32 = 1;

/// Enough for client-side paging: 8 documents and 9 images per page.
const DEFAULT_LIMIT_DOCS: u32 = 64;
const DEFAULT_LIMIT_IMAGES: u32 = 72;
const MAX_LIMIT: u32 = 1000;
/// Longer queries are cut: a huge paste would hold the single search thread for seconds.
pub const MAX_QUERY_CHARS: usize = 512;

type Dict = HashMap<String, OwnedValue>;

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.endex.Daemon1.Error")]
pub enum Error {
    #[zbus(error)]
    ZBus(zbus::Error),
    /// A newer query from the same client superseded this one.
    Cancelled(String),
    InvalidArgs(String),
    NotFound(String),
    Failed(String),
}

impl From<anyhow::Error> for Error {
    fn from(error: anyhow::Error) -> Self {
        if error.downcast_ref::<Cancelled>().is_some() {
            Error::Cancelled(error.to_string())
        } else {
            Error::Failed(format!("{error:#}"))
        }
    }
}

pub struct Daemon {
    worker: Worker,
    /// Newest `query_id` per client (unique bus name); older queries of that client are dropped.
    latest: Arc<Mutex<HashMap<String, u32>>>,
    index_path: PathBuf,
    config_path: PathBuf,
}

impl Daemon {
    pub fn new(worker: Worker, index_path: PathBuf, config_path: PathBuf) -> Self {
        Self {
            worker,
            latest: Arc::default(),
            index_path,
            config_path,
        }
    }

    /// The newest `query_id` per client, for pruning when a client leaves the bus.
    pub fn clients(&self) -> Arc<Mutex<HashMap<String, u32>>> {
        Arc::clone(&self.latest)
    }
}

#[zbus::interface(name = "org.endex.Daemon1")]
impl Daemon {
    /// Documents and images, ranked separately. `opts`: `limit_docs`, `limit_images`,
    /// `query_id` (echoed back; a newer id from the same client cancels this query).
    async fn search(
        &self,
        #[zbus(header)] header: Header<'_>,
        query: String,
        mode: String,
        opts: Dict,
    ) -> Result<(u32, Vec<Dict>, Vec<Dict>), Error> {
        let mode: Mode = mode
            .parse()
            .map_err(|e: anyhow::Error| Error::InvalidArgs(e.to_string()))?;
        let limit = |key, default| -> Result<usize, Error> {
            Ok(opt_u32(&opts, key)?.unwrap_or(default).min(MAX_LIMIT) as usize)
        };
        let limits = Limits {
            docs: limit("limit_docs", DEFAULT_LIMIT_DOCS)?,
            images: limit("limit_images", DEFAULT_LIMIT_IMAGES)?,
        };
        let query_id = opt_u32(&opts, "query_id")?.unwrap_or(0);
        let query = truncated(query, MAX_QUERY_CHARS);
        if query.trim().is_empty() {
            return Ok((query_id, Vec::new(), Vec::new()));
        }
        let client = header.sender().map(|s| s.to_string()).unwrap_or_default();
        if query_id > 0 {
            let mut latest = self.latest.lock().unwrap_or_else(|e| e.into_inner());
            let newest = latest.entry(client.clone()).or_default();
            *newest = (*newest).max(query_id);
        }
        let latest = Arc::clone(&self.latest);
        let cancelled = move || {
            query_id > 0
                && latest
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&client)
                    .is_some_and(|&newest| newest > query_id)
        };
        let Results {
            docs,
            images,
            terms,
        } = self
            .worker
            .search(query, mode, limits, Box::new(cancelled))
            .await?;
        // stat() and image headers: keep blocking file I/O off the async executor.
        let (docs, images) = tokio::task::spawn_blocking(move || {
            let dicts = |hits: Vec<Hit>| hits.into_iter().map(|h| hit_dict(h, &terms)).collect();
            (dicts(docs), dicts(images))
        })
        .await
        .map_err(|e| Error::Failed(e.to_string()))?;
        Ok((query_id, docs, images))
    }

    /// One result chunk in full, with highlights for `query`. Documents also get their
    /// position among the file's chunks and the ids of the neighbouring chunks.
    async fn preview(&self, id: String, query: String) -> Result<Dict, Error> {
        let query = truncated(query, MAX_QUERY_CHARS);
        let Some(Preview { view, terms }) = self.worker.preview(id.clone(), query).await? else {
            return Err(Error::NotFound(id));
        };
        tokio::task::spawn_blocking(move || preview_dict(view, &terms))
            .await
            .map_err(|e| Error::Failed(e.to_string()))
    }

    /// Index counts and locations. `state` is `ready`, `empty` (nothing indexed yet),
    /// `rebuilding` (another process is replacing the index; try again shortly) or `error`
    /// (see `error`).
    async fn status(&self) -> Result<Dict, Error> {
        let status = self.worker.status().await?;
        let mut d = Dict::new();
        put(&mut d, "version", env!("CARGO_PKG_VERSION"));
        put(&mut d, "model", status.model);
        put(&mut d, "query_device", "cpu");
        put(&mut d, "index_path", self.index_path.display().to_string());
        put(
            &mut d,
            "config_path",
            self.config_path.display().to_string(),
        );
        let counts = match status.index {
            None => {
                put(&mut d, "state", "empty");
                Default::default()
            }
            Some(Ok(counts)) => {
                put(&mut d, "state", "ready");
                counts
            }
            Some(Err(error)) => {
                put(
                    &mut d,
                    "state",
                    if status.rebuilding {
                        "rebuilding"
                    } else {
                        "error"
                    },
                );
                put(&mut d, "error", format!("{error:#}"));
                Default::default()
            }
        };
        put(&mut d, "doc_files", counts.doc_files as u32);
        put(&mut d, "image_files", counts.image_files as u32);
        put(&mut d, "doc_chunks", counts.doc_chunks as u32);
        put(&mut d, "image_chunks", counts.image_chunks as u32);
        put(&mut d, "errors", counts.errors as u32);
        put(&mut d, "last_indexed", counts.last_indexed.unwrap_or(0));
        Ok(d)
    }

    #[zbus(property)]
    fn version(&self) -> u32 {
        INTERFACE_VERSION
    }
}

fn preview_dict(view: ChunkView, terms: &[String]) -> Dict {
    let row = view.row;
    let mut d = Dict::new();
    put(&mut d, "highlights", spans(&row.text, terms));
    put(&mut d, "id", row.id);
    put(&mut d, "kind", row.kind.as_str());
    put(&mut d, "title", row.title);
    put(&mut d, "location", row.location);
    put(&mut d, "mtime", row.mtime / 1_000_000_000);
    put(&mut d, "size", file_size(&row.path));
    match row.kind {
        Kind::Doc => {
            put(&mut d, "index", view.index as u32);
            put(&mut d, "count", view.count as u32);
            put(&mut d, "prev", view.prev.unwrap_or_default());
            put(&mut d, "next", view.next.unwrap_or_default());
        }
        Kind::Image => {
            put(&mut d, "ocr", !row.text.trim().is_empty());
            put_dimensions(&mut d, &row.path);
        }
    }
    put(&mut d, "text", row.text);
    put(&mut d, "path", row.path);
    d
}

fn hit_dict(hit: Hit, terms: &[String]) -> Dict {
    let mut d = Dict::new();
    put(&mut d, "highlights", spans(&hit.snippet, terms));
    put(&mut d, "size", file_size(&hit.path));
    if hit.kind == Kind::Image {
        put(&mut d, "ocr", !hit.snippet.is_empty());
        put_dimensions(&mut d, &hit.path);
    }
    if let Some(score) = hit.vector_score {
        put(&mut d, "vec_score", f64::from(score));
    }
    if let Some(keyword) = hit.keyword {
        put(&mut d, "kw_score", f64::from(keyword.score));
        put(&mut d, "kw_rank", keyword.rank as u32);
    }
    put(&mut d, "id", hit.id);
    put(&mut d, "kind", hit.kind.as_str());
    put(&mut d, "path", hit.path);
    put(&mut d, "title", hit.title);
    put(&mut d, "location", hit.location);
    put(&mut d, "snippet", hit.snippet);
    put(&mut d, "mtime", hit.mtime / 1_000_000_000);
    put(&mut d, "score", f64::from(hit.score));
    d
}

fn truncated(mut text: String, max_chars: usize) -> String {
    if let Some((cut, _)) = text.char_indices().nth(max_chars) {
        text.truncate(cut);
    }
    text
}

/// Highlight ranges as `(start, length)` in Unicode characters.
fn spans(text: &str, terms: &[String]) -> Vec<(u32, u32)> {
    highlights(text, terms)
        .into_iter()
        .map(|(start, len)| (start as u32, len as u32))
        .collect()
}

fn file_size(path: &str) -> u64 {
    fs::metadata(path).map_or(0, |m| m.len())
}

/// Reads only the image header; omitted when the file is gone or unreadable.
fn put_dimensions(d: &mut Dict, path: &str) {
    let dimensions = image::ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .ok()
        .and_then(|r| r.into_dimensions().ok());
    if let Some((width, height)) = dimensions {
        put(d, "width", width);
        put(d, "height", height);
    }
}

fn put<'a>(d: &mut Dict, key: &str, value: impl Into<Value<'a>>) {
    let value = OwnedValue::try_from(value.into()).expect("no file descriptors");
    d.insert(key.to_owned(), value);
}

/// Accepts any integer type: Qt sends `int` as `i`, busctl as whatever it is told.
fn opt_u32(opts: &Dict, key: &str) -> Result<Option<u32>, Error> {
    let Some(value) = opts.get(key) else {
        return Ok(None);
    };
    let n: i64 = match &**value {
        Value::U8(n) => (*n).into(),
        Value::U16(n) => (*n).into(),
        Value::I16(n) => (*n).into(),
        Value::U32(n) => (*n).into(),
        Value::I32(n) => (*n).into(),
        Value::I64(n) => *n,
        Value::U64(n) => i64::try_from(*n).unwrap_or(i64::MAX),
        _ => return Err(Error::InvalidArgs(format!("{key} must be an integer"))),
    };
    u32::try_from(n)
        .map(Some)
        .map_err(|_| Error::InvalidArgs(format!("{key} out of range: {n}")))
}
