//! Dictionaries of the D-Bus replies: hits, previews and the index state.

use std::fs;

use endex_core::Kind;
use endex_core::search::{ChunkView, Hit, highlights};
use zbus::zvariant::{OwnedValue, Value};

use super::{Dict, Error};
use crate::state::IndexState;

pub(super) fn index_state_dict(s: &IndexState) -> Dict {
    let mut d = Dict::new();
    put(&mut d, "state", s.phase.as_str());
    put(&mut d, "reason", s.reason.clone());
    put(&mut d, "done", s.done as u32);
    put(&mut d, "total", s.total as u32);
    put(&mut d, "images", s.images as u32);
    put(&mut d, "device", s.device.map_or("", |d| d.as_str()));
    put(&mut d, "bulk", s.bulk);
    put(&mut d, "rebuilding", s.rebuilding);
    put(&mut d, "eta", s.eta_secs.map_or(-1, |s| s as i64));
    put(&mut d, "gpu_loaded", s.gpu_loaded);
    put(&mut d, "rebuild_pending", s.rebuild_pending);
    d
}

pub(super) fn preview_dict(view: ChunkView, terms: &[String]) -> Dict {
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

pub(super) fn hit_dict(hit: Hit, terms: &[String]) -> Dict {
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

pub(super) fn truncated(mut text: String, max_chars: usize) -> String {
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

pub(super) fn put<'a>(d: &mut Dict, key: &str, value: impl Into<Value<'a>>) {
    let value = OwnedValue::try_from(value.into()).expect("no file descriptors");
    d.insert(key.to_owned(), value);
}

/// Accepts any integer type: Qt sends `int` as `i`, busctl as whatever it is told.
pub(super) fn opt_u32(opts: &Dict, key: &str) -> Result<Option<u32>, Error> {
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
