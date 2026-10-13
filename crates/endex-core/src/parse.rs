//! Document parsing into sections of numbered units (lines or paragraphs).

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use quick_xml::XmlVersion;
use quick_xml::events::Event;

use crate::packed;

/// A run of consecutive units under one heading path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Heading path such as `Title > Chapter`; empty for unstructured text.
    pub heading: String,
    /// "lines" or "paragraphs".
    pub unit: &'static str,
    /// 1-based number of the first unit.
    pub start: usize,
    pub units: Vec<String>,
}

#[derive(Debug)]
pub struct Parsed {
    pub title: String,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocFormat {
    Markdown,
    Plain,
    Docx,
}

const PLAIN_EXTENSIONS: &[&str] = &[
    "txt", "text", "log", "csv", "rs", "py", "java", "kt", "js", "ts", "tsx", "jsx", "c", "h",
    "cc", "cpp", "hpp", "go", "rb", "php", "sh", "zsh", "bash", "fish", "lua", "sql", "toml",
    "yaml", "yml", "json", "ini", "conf", "cfg", "xml", "html", "css", "qml", "tex", "org", "rst",
];

/// Plain-text window size, in lines.
const WINDOW_LINES: usize = 60;

/// A NUL in this many first bytes marks a file as binary (git's test).
const BINARY_PROBE: usize = 8000;

impl DocFormat {
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "md" | "markdown" => Some(Self::Markdown),
            "docx" => Some(Self::Docx),
            e if PLAIN_EXTENSIONS.contains(&e) => Some(Self::Plain),
            _ => None,
        }
    }
}

/// Parses a document; plain and Markdown files of binary data fail (see [`decode`]). NUL is
/// removed (a damaged region of a log, a raw NUL in a docx), lines (or paragraphs) of packed
/// binary data come out blank and long base64 runs are cut, in headings and the title too (see
/// [`packed`]).
pub fn parse(format: DocFormat, path: &Path, bytes: &[u8]) -> Result<Parsed> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut parsed = match format {
        DocFormat::Markdown => markdown(&decode(bytes)?, &name),
        DocFormat::Plain => plain(&decode(bytes)?, &name),
        DocFormat::Docx => docx(bytes, &name)?,
    };
    for section in &mut parsed.sections {
        clean(&mut section.heading);
        section.units.iter_mut().for_each(clean);
    }
    clean(&mut parsed.title);
    if parsed.title.trim().is_empty() {
        parsed.title = name;
    }
    Ok(parsed)
}

fn clean(text: &mut String) {
    text.retain(|c| c != '\0');
    packed::clean(text);
}

/// Plain or Markdown bytes as text: UTF-16 with a byte order mark is decoded, anything else
/// read as UTF-8 (without its byte order mark). Binary data fails.
fn decode(bytes: &[u8]) -> Result<String> {
    let utf16 = match bytes {
        [0xFF, 0xFE, rest @ ..] => Some(utf16(rest, u16::from_le_bytes)),
        [0xFE, 0xFF, rest @ ..] => Some(utf16(rest, u16::from_be_bytes)),
        _ => None,
    };
    let utf8 = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let probe = utf16.as_ref().map_or(utf8, |text| text.as_bytes());
    ensure!(
        !probe[..probe.len().min(BINARY_PROBE)].contains(&0),
        "binary data, not text (a NUL byte in the first {BINARY_PROBE} bytes)"
    );
    Ok(utf16.unwrap_or_else(|| String::from_utf8_lossy(utf8).into_owned()))
}

fn utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    let units = bytes.as_chunks().0.iter().map(|&pair| unit(pair));
    char::decode_utf16(units)
        .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

pub fn markdown(text: &str, fallback_title: &str) -> Parsed {
    let mut title = None;
    let mut path: Vec<(usize, String)> = Vec::new();
    let mut sections = Vec::new();
    let mut current = new_section(String::new(), "lines", 1);
    let mut fence: Option<&str> = None;
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let marker = ["```", "~~~"].into_iter().find(|m| trimmed.starts_with(m));
        match (fence, marker) {
            (None, Some(m)) => fence = Some(m),
            (Some(open), Some(m)) if open == m => fence = None,
            _ => {}
        }
        if let (None, Some((level, heading))) = (fence, atx_heading(line)) {
            push_section(&mut sections, current);
            if level == 1 && title.is_none() {
                title = Some(heading.clone());
            }
            path.retain(|(l, _)| *l < level);
            path.push((level, heading));
            let joined = path.iter().map(|(_, h)| h.as_str()).collect::<Vec<_>>();
            current = new_section(joined.join(" > "), "lines", i + 2);
            continue;
        }
        current.units.push(line.to_owned());
    }
    push_section(&mut sections, current);
    Parsed {
        title: title.unwrap_or_else(|| fallback_title.to_owned()),
        sections,
    }
}

fn atx_heading(line: &str) -> Option<(usize, String)> {
    let level = line.chars().take_while(|&c| c == '#').count();
    let rest = &line[level..];
    if !(1..=6).contains(&level) || !(rest.is_empty() || rest.starts_with([' ', '\t'])) {
        return None;
    }
    let text = rest.trim().trim_end_matches('#').trim();
    Some((level, text.to_owned()))
}

pub fn plain(text: &str, title: &str) -> Parsed {
    let lines: Vec<&str> = text.lines().collect();
    let mut sections = Vec::new();
    for (i, window) in lines.chunks(WINDOW_LINES).enumerate() {
        let mut section = new_section(String::new(), "lines", i * WINDOW_LINES + 1);
        section.units = window.iter().map(|l| l.to_string()).collect();
        push_section(&mut sections, section);
    }
    Parsed {
        title: title.to_owned(),
        sections,
    }
}

fn docx(bytes: &[u8], title: &str) -> Result<Parsed> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).context("not a zip")?;
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .context("missing word/document.xml")?
        .read_to_string(&mut xml)?;
    let mut styles = String::new();
    let styles = match archive.by_name("word/styles.xml") {
        Ok(mut file) => {
            file.read_to_string(&mut styles)?;
            docx_style_names(&styles)?
        }
        Err(_) => HashMap::new(),
    };
    let paragraphs = docx_paragraphs(&xml, &styles)?;

    let mut path: Vec<(usize, String)> = Vec::new();
    let mut sections = Vec::new();
    let mut current = new_section(String::new(), "paragraphs", 1);
    let mut first_heading = None;
    for (i, (level, text)) in paragraphs.into_iter().enumerate() {
        match level {
            Some(level) if !text.trim().is_empty() => {
                push_section(&mut sections, current);
                first_heading.get_or_insert_with(|| text.clone());
                path.retain(|(l, _)| *l < level);
                path.push((level, text));
                let joined = path.iter().map(|(_, h)| h.as_str()).collect::<Vec<_>>();
                current = new_section(joined.join(" > "), "paragraphs", i + 2);
            }
            _ => current.units.push(text),
        }
    }
    push_section(&mut sections, current);
    Ok(Parsed {
        title: first_heading.unwrap_or_else(|| title.to_owned()),
        sections,
    })
}

/// Style id → style name. Localized Word writes ids like `1` or `a3`; the names stay
/// `heading 1`, `Title`.
fn docx_style_names(xml: &str) -> Result<HashMap<String, String>> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut names = HashMap::new();
    let mut id = None;
    loop {
        match reader.read_event()? {
            Event::Start(e) if e.name().as_ref() == "w:style" => {
                id = e
                    .try_get_attribute("w:styleId")?
                    .map(|v| v.normalized_value(XmlVersion::Implicit1_0))
                    .transpose()?
                    .map(|v| v.into_owned());
            }
            Event::Empty(e) if e.name().as_ref() == "w:name" => {
                if let (Some(id), Some(name)) = (&id, e.try_get_attribute("w:val")?) {
                    let name = name.normalized_value(XmlVersion::Implicit1_0)?;
                    names.insert(id.clone(), name.into_owned());
                }
            }
            Event::End(e) if e.name().as_ref() == "w:style" => id = None,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(names)
}

/// Paragraph texts with their heading level (from `heading N` / `Title` styles).
fn docx_paragraphs(
    xml: &str,
    styles: &HashMap<String, String>,
) -> Result<Vec<(Option<usize>, String)>> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut paragraphs = Vec::new();
    let mut text = String::new();
    let mut level = None;
    let mut in_text = false;
    loop {
        match reader.read_event()? {
            Event::Start(e) if e.name().as_ref() == "w:t" => in_text = true,
            Event::End(e) if e.name().as_ref() == "w:t" => in_text = false,
            Event::Text(t) if in_text => text.push_str(&t.xml10_content()),
            Event::GeneralRef(r) if in_text => {
                if let Some(c) = r.resolve_char_ref()? {
                    text.push(c);
                } else {
                    text.push_str(quick_xml::escape::resolve_xml_entity(&r).unwrap_or(""));
                }
            }
            Event::Empty(e) => match e.name().as_ref() {
                "w:tab" => text.push('\t'),
                "w:br" | "w:cr" => text.push(' '),
                "w:pStyle" => {
                    if let Some(val) = e.try_get_attribute("w:val")? {
                        let id = val.normalized_value(XmlVersion::Implicit1_0)?;
                        let name = styles.get(id.as_ref()).map_or(id.as_ref(), String::as_str);
                        level = heading_level(name);
                    }
                }
                _ => {}
            },
            Event::End(e) if e.name().as_ref() == "w:p" => {
                paragraphs.push((level.take(), std::mem::take(&mut text)));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(paragraphs)
}

/// `Title` → 1, `heading 2` / `Heading2` → 2.
fn heading_level(style: &str) -> Option<usize> {
    if style.eq_ignore_ascii_case("title") {
        return Some(1);
    }
    let digits = style
        .get(.."heading".len())
        .filter(|prefix| prefix.eq_ignore_ascii_case("heading"))
        .map(|_| style["heading".len()..].trim())?;
    digits.parse().ok().filter(|l| (1..=9).contains(l))
}

fn new_section(heading: String, unit: &'static str, start: usize) -> Section {
    Section {
        heading,
        unit,
        start,
        units: Vec::new(),
    }
}

fn push_section(sections: &mut Vec<Section>, section: Section) {
    if section.units.iter().any(|u| !u.trim().is_empty()) {
        sections.push(section);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_sections_keep_heading_path_and_line_numbers() {
        let text =
            "intro\n# Title\n\n## A\nalpha\n```\n# not a heading\n```\n### A1\ndeep\n## B\nbeta\n";
        let parsed = markdown(text, "file.md");
        assert_eq!(parsed.title, "Title");
        let got: Vec<(&str, usize)> = parsed
            .sections
            .iter()
            .map(|s| (s.heading.as_str(), s.start))
            .collect();
        assert_eq!(
            got,
            [
                ("", 1),
                ("Title > A", 5),
                ("Title > A > A1", 10),
                ("Title > B", 12)
            ]
        );
        assert_eq!(parsed.sections[1].units[1], "```");
        assert_eq!(parsed.sections[1].units[2], "# not a heading");
    }

    #[test]
    fn binary_text_fails_late_nul_is_dropped_byte_order_marks_are_read() {
        let path = Path::new("a.txt");
        let lines = |p: Parsed| {
            p.sections
                .into_iter()
                .flat_map(|s| s.units)
                .collect::<Vec<_>>()
        };
        let error = parse(DocFormat::Plain, path, b"PK\x03\x04\x14\0\0\0 kwzipzq").unwrap_err();
        assert_eq!(
            error.to_string(),
            "binary data, not text (a NUL byte in the first 8000 bytes)"
        );
        let mut late = "log line\n".repeat(900).into_bytes();
        late.extend(b"bad \0\0region\n");
        let parsed = parse(DocFormat::Markdown, path, &late).unwrap();
        assert_eq!(lines(parsed).last().unwrap(), "bad region");
        let text = "笔记 kw\r\n第二行\r\n";
        let le: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let be: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
        for (bom, units) in [([0xFF, 0xFE], le), ([0xFE, 0xFF], be)] {
            let bytes = [&bom[..], &units].concat();
            let parsed = parse(DocFormat::Plain, path, &bytes).unwrap();
            assert_eq!(lines(parsed), ["笔记 kw", "第二行"]);
        }
        let bom = "\u{FEFF}# 标题\n正文".as_bytes();
        assert_eq!(parse(DocFormat::Markdown, path, bom).unwrap().title, "标题");
    }

    #[test]
    fn docx_paragraphs_and_headings() {
        let xml = r#"<w:document xmlns:w="w"><w:body>
            <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Report</w:t></w:r></w:p>
            <w:p><w:r><w:t>Sales &amp; growth</w:t></w:r><w:r><w:t xml:space="preserve"> up 12%</w:t></w:r></w:p>
        </w:body></w:document>"#;
        let paragraphs = docx_paragraphs(xml, &HashMap::new()).unwrap();
        assert_eq!(
            paragraphs,
            [
                (Some(1), "Report".to_owned()),
                (None, "Sales & growth up 12%".to_owned())
            ]
        );
    }
}
