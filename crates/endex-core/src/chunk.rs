//! Packs parsed sections into embedding-sized chunks.

use std::path::Path;

use anyhow::{Result, anyhow};

use crate::parse::Section;

/// Leaves room for the task prefix inside the model's 512-token signature.
pub const MAX_TOKENS: usize = 480;

const BREAKS: [char; 10] = ['。', '！', '？', '.', '!', '?', ';', '；', ' ', '\t'];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub location: String,
    pub text: String,
}

/// Token boundaries as the embedding model sees them.
pub trait Tokens {
    fn count(&self, text: &str) -> usize;
    /// Byte offset at which each token of `text` ends.
    fn ends(&self, text: &str) -> Vec<usize>;
}

/// The embedding model's own tokenizer.
pub struct TokenCounter(tokenizers::Tokenizer);

impl TokenCounter {
    pub fn from_file(path: &Path) -> Result<Self> {
        tokenizers::Tokenizer::from_file(path)
            .map(Self)
            .map_err(|e| anyhow!("loading tokenizer {}: {e}", path.display()))
    }
}

impl Tokens for TokenCounter {
    fn count(&self, text: &str) -> usize {
        self.0
            .encode_fast(text, false)
            .map_or_else(|_| text.chars().count(), |encoding| encoding.len())
    }

    fn ends(&self, text: &str) -> Vec<usize> {
        match self.0.encode(text, false) {
            Ok(encoding) => encoding.get_offsets().iter().map(|&(_, end)| end).collect(),
            Err(_) => text.char_indices().map(|(i, c)| i + c.len_utf8()).collect(),
        }
    }
}

/// Greedily packs consecutive units into chunks of at most `budget` tokens, heading line
/// included, splitting single units that are too long on their own. Long heading paths are
/// shortened inside the chunk text (to a quarter of the budget) but kept whole in `location`.
pub fn chunk_section(section: &Section, budget: usize, tokens: &dyn Tokens) -> Vec<Chunk> {
    let heading = truncate(&section.heading, budget / 4, tokens);
    let header_tokens = if heading.is_empty() {
        0
    } else {
        tokens.count(heading) + 1
    };
    let avail = budget - header_tokens;
    let mut chunks = Vec::new();
    let mut piece: Vec<&str> = Vec::new();
    let mut piece_start = 0;
    let mut piece_tokens = 0;
    for (i, unit) in section.units.iter().enumerate() {
        let unit_tokens = tokens.count(unit) + 1;
        if !piece.is_empty() && piece_tokens + unit_tokens > avail {
            push_chunk(
                &mut chunks,
                section,
                heading,
                (piece_start, i - 1),
                &piece.join("\n"),
            );
            piece.clear();
            piece_tokens = 0;
        }
        if unit_tokens > avail {
            for part in split_long(unit, avail, tokens) {
                push_chunk(&mut chunks, section, heading, (i, i), part);
            }
            continue;
        }
        if piece.is_empty() {
            piece_start = i;
        }
        piece.push(unit);
        piece_tokens += unit_tokens;
    }
    if !piece.is_empty() {
        let end = piece_start + piece.len() - 1;
        push_chunk(
            &mut chunks,
            section,
            heading,
            (piece_start, end),
            &piece.join("\n"),
        );
    }
    // Counting lines separately is off by a token now and then (merges at trimmed edges);
    // the rare chunk that ends up over budget is split once more.
    chunks
        .into_iter()
        .flat_map(|chunk| {
            if tokens.count(&chunk.text) <= budget {
                return vec![chunk];
            }
            split_long(&chunk.text, budget, tokens)
                .into_iter()
                .map(|text| Chunk {
                    location: chunk.location.clone(),
                    text: text.to_owned(),
                })
                .collect()
        })
        .collect()
}

fn push_chunk(
    chunks: &mut Vec<Chunk>,
    section: &Section,
    heading: &str,
    (first, last): (usize, usize),
    body: &str,
) {
    let body = body.trim();
    if body.is_empty() {
        return;
    }
    let (a, b) = (section.start + first, section.start + last);
    let range = if a == b {
        format!("{} {a}", section.unit)
    } else {
        format!("{} {a}-{b}", section.unit)
    };
    let (location, text) = if section.heading.is_empty() {
        (range, body.to_owned())
    } else {
        (
            format!("{} · {range}", section.heading),
            format!("{heading}\n{body}"),
        )
    };
    chunks.push(Chunk { location, text });
}

/// The longest prefix of `text` with at most `max` tokens.
fn truncate<'a>(text: &'a str, max: usize, tokens: &dyn Tokens) -> &'a str {
    let ends = tokens.ends(text);
    if ends.len() <= max {
        return text;
    }
    match max.checked_sub(1) {
        Some(last) => text[..text.floor_char_boundary(ends[last])].trim_end(),
        None => "",
    }
}

/// Splits `text` into pieces of at most `budget` tokens, preferring sentence or word breaks.
/// Tokenizes once; cuts fall on token ends.
fn split_long<'a>(text: &'a str, budget: usize, tokens: &dyn Tokens) -> Vec<&'a str> {
    let ends = tokens.ends(text);
    let mut parts = Vec::new();
    let (mut start, mut next) = (0, 0);
    while start < text.len() {
        let rest = text[start..].trim();
        if next >= ends.len() || (ends.len() - next <= budget && tokens.count(rest) <= budget) {
            parts.push(rest);
            break;
        }
        // As many tokens as fit; fewer if the piece, tokenized on its own, comes out longer
        // (merges at its edges can differ from the whole text's).
        let mut last = (next + budget).min(ends.len()) - 1;
        let cut = loop {
            let cut = cut_before(text, start, ends[last]);
            if last == next || tokens.count(text[start..cut].trim()) <= budget {
                break cut;
            }
            last -= 1;
        };
        parts.push(text[start..cut].trim());
        start = cut;
        next = ends.partition_point(|&end| end <= cut);
    }
    parts.retain(|p| !p.is_empty());
    parts
}

/// A cut at byte `end` (or the next char boundary after `start`), moved back to a sentence or
/// word break within the last fifth of the piece unless `end` already sits on one.
fn cut_before(text: &str, start: usize, end: usize) -> usize {
    let hard = text.ceil_char_boundary(text.floor_char_boundary(end).max(start + 1));
    if hard >= text.len() || text[hard..].starts_with(char::is_whitespace) {
        return hard;
    }
    let floor = text.floor_char_boundary(start + (hard - start) * 4 / 5);
    match text[floor..hard].rfind(BREAKS) {
        Some(pos) => {
            let c = text[floor + pos..].chars().next().expect("found a char");
            floor + pos + c.len_utf8()
        }
        None => hard,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whitespace-separated words as tokens.
    struct Words;

    impl Tokens for Words {
        fn count(&self, text: &str) -> usize {
            text.split_whitespace().count()
        }

        fn ends(&self, text: &str) -> Vec<usize> {
            text.split_whitespace()
                .map(|w| w.as_ptr().addr() - text.as_ptr().addr() + w.len())
                .collect()
        }
    }

    fn section(heading: &str, units: &[&str]) -> Section {
        Section {
            heading: heading.into(),
            unit: "lines",
            start: 10,
            units: units.iter().map(|u| u.to_string()).collect(),
        }
    }

    #[test]
    fn packs_units_within_budget() {
        let s = section("Doc > Part", &["a b c", "d e", "", "f g h i", "j"]);
        let chunks = chunk_section(&s, 12, &Words);
        assert_eq!(
            chunks,
            [
                Chunk {
                    location: "Doc > Part · lines 10-12".into(),
                    text: "Doc > Part\na b c\nd e".into()
                },
                Chunk {
                    location: "Doc > Part · lines 13-14".into(),
                    text: "Doc > Part\nf g h i\nj".into()
                },
            ]
        );
    }

    #[test]
    fn splits_oversized_unit_at_word_breaks() {
        let long = (0..25)
            .map(|i| format!("w{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let chunks = chunk_section(&section("", &["short", &long]), 10, &Words);
        assert_eq!(chunks[0].text, "short");
        assert!(
            chunks[1..]
                .iter()
                .all(|c| Words.count(&c.text) <= 10 && c.location == "lines 11")
        );
        let rejoined = chunks[1..]
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(rejoined.join(" "), long);
    }
}
