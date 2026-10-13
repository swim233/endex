//! Dictionaries of the D-Bus replies: hits, previews and the index state.

use std::fs;

use endex_core::Kind;
use endex_core::search::{ChunkView, Hit, highlights};
use zbus::zvariant::Value;

use super::{Dict, Error};
use crate::bus::{self, put};
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

/// Highlights are found in the text as sent.
pub(super) fn preview_dict(view: ChunkView, terms: &[String]) -> Dict {
    let row = view.row;
    let text = bus::text(row.text);
    let mut d = Dict::new();
    put(&mut d, "highlights", spans(&text, terms));
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
            put(&mut d, "ocr", !text.trim().is_empty());
            put_dimensions(&mut d, &row.path);
        }
    }
    put(&mut d, "text", text);
    put(&mut d, "path", row.path);
    d
}

/// Highlights are found in the snippet as sent.
pub(super) fn hit_dict(hit: Hit, terms: &[String]) -> Dict {
    let snippet = bus::text(hit.snippet);
    let mut d = Dict::new();
    put(&mut d, "highlights", spans(&snippet, terms));
    put(&mut d, "size", file_size(&hit.path));
    if hit.kind == Kind::Image {
        put(&mut d, "ocr", !snippet.is_empty());
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
    put(&mut d, "snippet", snippet);
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use zbus::DBusError;
    use zbus::message::Message;

    use super::*;

    fn strings(value: &Value<'_>, out: &mut Vec<String>) {
        match value {
            Value::Str(s) => out.push(s.to_string()),
            Value::Value(inner) => strings(inner, out),
            Value::Array(a) => a.iter().for_each(|v| strings(v, out)),
            Value::Dict(d) => d.iter().for_each(|(k, v)| {
                strings(k, out);
                strings(v, out);
            }),
            Value::Structure(s) => s.fields().iter().for_each(|v| strings(v, out)),
            _ => {}
        }
    }

    #[test]
    fn no_nul_reaches_the_bus_and_highlights_fit_the_sent_text() {
        let hit = Hit {
            id: "/n.txt#0".into(),
            path: "/n.txt".into(),
            kind: Kind::Doc,
            title: "n\0.txt".into(),
            location: "lines 1-2".into(),
            snippet: "\0\0zip\0 kwzipzq part\0".into(),
            score: 1.0,
            vector_score: None,
            keyword: None,
            mtime: 0,
        };
        let mut d = hit_dict(hit, &["kwzipzq".into()]);
        let snippet: String = d["snippet"].downcast_ref::<&str>().unwrap().into();
        assert_eq!(snippet, "\u{FFFD}\u{FFFD}zip\u{FFFD} kwzipzq part\u{FFFD}");
        let spans: Vec<(u32, u32)> = d["highlights"].try_clone().unwrap().try_into().unwrap();
        let chars: Vec<char> = snippet.chars().collect();
        let (start, len) = (spans[0].0 as usize, spans[0].1 as usize);
        assert_eq!(
            chars[start..start + len].iter().collect::<String>(),
            "kwzipzq"
        );

        put(&mut d, "list", vec!["c\0", "d"]);
        put(&mut d, "errors", HashMap::from([("k\0", "v\0")]));
        let mut inner = Dict::new();
        put(&mut inner, "path", "e\0");
        put(&mut d, "nested", vec![inner]);
        let mut all = Vec::new();
        strings(&Value::from(d), &mut all);
        assert!(all.contains(&"n\u{FFFD}.txt".into()) && all.contains(&"v\u{FFFD}".into()));
        assert!(all.iter().all(|s| !s.contains('\0')), "{all:?}");

        let call = Message::method_call("/org/endex/Daemon", "Search")
            .unwrap()
            .build(&())
            .unwrap();
        let reply = Error::Failed("bad\0rule".into())
            .create_reply(&call.header())
            .unwrap();
        let text: String = reply.body().deserialize().unwrap();
        assert_eq!(text, "bad\u{FFFD}rule");
    }
}
