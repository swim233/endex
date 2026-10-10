//! Keyword analysis shared by indexing and querying: text becomes a whitespace-joined list of
//! lowercase terms that LanceDB indexes with its plain whitespace tokenizer.
//!
//! - CJK runs are segmented with jieba in search mode (emits both words and their sub-words);
//!   Chinese function words are dropped.
//! - Latin/digit runs are kept whole and also split into identifier parts:
//!   `load_user_settings` → `load_user_settings load user settings`,
//!   `RetryingHttpClient` → `retryinghttpclient retrying http client`, `E0382` → `e0382 e 0382`.
//! - File names additionally get prefixes of every term ([`Analyzer::name_terms`]) in a separate
//!   column, which gives partial-name matching without the noise of infix n-grams.

use jieba_rs::Jieba;

pub struct Analyzer {
    jieba: Jieba,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    pub fn new() -> Self {
        Self {
            jieba: Jieba::new(),
        }
    }

    pub fn terms(&self, text: &str) -> Vec<String> {
        let mut terms = Vec::new();
        let mut rest = text;
        while let Some(c) = rest.chars().next() {
            let end = if is_cjk(c) {
                run_end(rest, is_cjk)
            } else if is_word_char(c) {
                run_end(rest, is_word_char)
            } else {
                rest = &rest[c.len_utf8()..];
                continue;
            };
            let (run, tail) = rest.split_at(end);
            if is_cjk(c) {
                terms.extend(
                    self.jieba
                        .cut_for_search(run, true)
                        .into_iter()
                        .filter(|t| !CJK_STOP_WORDS.contains(&t.word))
                        .map(|t| t.word.to_lowercase()),
                );
            } else {
                identifier_terms(run, &mut terms);
            }
            rest = tail;
        }
        terms
    }

    pub fn joined(&self, text: &str) -> String {
        self.terms(text).join(" ")
    }

    /// Terms of a file name plus their prefixes (edge n-grams), so that the query terms of
    /// `router_man` or `屏幕截` match `router_manual.docx` or `屏幕截图_….png`.
    pub fn name_terms(&self, name: &str) -> String {
        let mut out = Vec::new();
        for term in self.terms(name) {
            let chars: Vec<char> = term.chars().collect();
            if chars.len() < MIN_PREFIX {
                continue; // single letters/digits from ids like `a9341af9` only add noise
            }
            for len in MIN_PREFIX..chars.len().min(MAX_PREFIX + 1) {
                out.push(chars[..len].iter().collect());
            }
            out.push(term);
        }
        out.join(" ")
    }
}

const MIN_PREFIX: usize = 2;
const MAX_PREFIX: usize = 24;

/// Chinese function words; they match almost any text and drown real keyword hits.
/// (English stop words are removed by LanceDB's analyzer.)
const CJK_STOP_WORDS: &[&str] = &[
    "的", "了", "在", "是", "和", "与", "及", "或", "也", "都", "而", "就", "着", "过", "被", "把",
    "将", "对", "从", "向", "于", "为", "以", "之", "其", "这", "那", "这个", "那个", "这些",
    "那些", "个", "一", "一个", "一些", "有", "我", "你", "他", "她", "它", "我们", "你们", "他们",
    "她们", "它们", "吗", "呢", "吧", "啊", "呀", "很", "又", "还", "但", "并", "等", "里", "让",
    "给", "得", "地", "所", "如果", "因为", "所以", "但是", "而且", "然后", "可以", "什么", "怎么",
    "哪", "哪里", "哪些", "谁", "么", "还是", "就是", "是否",
];

fn run_end(s: &str, pred: fn(char) -> bool) -> usize {
    s.char_indices()
        .find(|&(_, c)| !pred(c))
        .map_or(s.len(), |(i, _)| i)
}

fn is_word_char(c: char) -> bool {
    (c.is_alphanumeric() || c == '_') && !is_cjk(c)
}

pub(crate) fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{3040}'..='\u{30ff}' // kana
        | '\u{3400}'..='\u{4dbf}'
        | '\u{4e00}'..='\u{9fff}'
        | '\u{ac00}'..='\u{d7af}' // hangul
        | '\u{f900}'..='\u{faff}'
        | '\u{20000}'..='\u{2fa1f}')
}

fn identifier_terms(word: &str, out: &mut Vec<String>) {
    let whole = word.trim_matches('_');
    if whole.is_empty() {
        return;
    }
    let parts: Vec<&str> = whole
        .split('_')
        .filter(|p| !p.is_empty())
        .flat_map(split_case_and_digits)
        .collect();
    out.push(whole.to_lowercase());
    if parts.len() > 1 {
        out.extend(parts.iter().map(|p| p.to_lowercase()));
    }
}

/// Splits `parseHTTPResponse2` into `parse`, `HTTP`, `Response`, `2`.
fn split_case_and_digits(s: &str) -> Vec<&str> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut parts = Vec::new();
    let mut start = 0;
    for i in 1..chars.len() {
        let (prev, (at, cur)) = (chars[i - 1].1, chars[i]);
        let next_lower = chars.get(i + 1).is_some_and(|&(_, n)| n.is_lowercase());
        let boundary = (prev.is_lowercase() && cur.is_uppercase())
            || (prev.is_uppercase() && cur.is_uppercase() && next_lower)
            || (prev.is_numeric() != cur.is_numeric());
        if boundary {
            parts.push(&s[start..at]);
            start = at;
        }
    }
    parts.push(&s[start..]);
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(text: &str) -> Vec<String> {
        Analyzer::new().terms(text)
    }

    #[test]
    fn splits_identifiers() {
        assert_eq!(
            terms("load_user_settings"),
            ["load_user_settings", "load", "user", "settings"]
        );
        assert_eq!(
            terms("RetryingHttpClient"),
            ["retryinghttpclient", "retrying", "http", "client"]
        );
        assert_eq!(
            terms("parseHTTPResponse2"),
            ["parsehttpresponse2", "parse", "http", "response", "2"]
        );
        assert_eq!(terms("error[E0382]"), ["error", "e0382", "e", "0382"]);
        assert_eq!(terms("plain words"), ["plain", "words"]);
    }

    #[test]
    fn name_terms_include_prefixes() {
        let name = Analyzer::new().name_terms("manuals/router_manual.docx");
        let name: Vec<&str> = name.split(' ').collect();
        for expected in ["router_manual", "router", "rou", "man", "manual", "docx"] {
            assert!(name.contains(&expected), "{expected} missing in {name:?}");
        }
        assert!(!name.contains(&"m") && !name.contains(&"ual"));
    }

    #[test]
    fn segments_chinese_and_mixed_text() {
        let t = terms("红烧肉的做法：五花肉切块");
        for expected in ["红烧", "红烧肉", "做法", "五花肉", "切块"] {
            assert!(
                t.iter().any(|x| x == expected),
                "{expected} missing in {t:?}"
            );
        }
        assert!(t.iter().all(|x| !x.contains('：') && x != "的"));
        let t = terms("使用 fcitx5 输入法");
        assert!(t.contains(&"fcitx5".to_string()) && t.contains(&"输入法".to_string()));
    }
}
