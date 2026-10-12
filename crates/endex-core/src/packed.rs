//! Packed binary data inside text and code files: lookup tables, fonts and images that
//! bundlers and editors inline as string literals. The code points of a packed table land in
//! letter ranges, so the analyzer finds words in it (`cute` between CJK characters) and
//! previews show garbage; base64 turns into many chunks of noise. Documents go through
//! [`clean`] line by line before chunking.

use unicode_blocks::{find_unicode_block, is_cjk_block};

/// Shorter lines (whitespace aside) are never dropped.
const MIN_CHARS: usize = 200;
/// A packed table mixes at least this many scripts...
const MIN_SCRIPTS: usize = 6;
/// ...and at least one letter in this many is directly followed by a letter of another script.
const SWITCH_EVERY: usize = 8;
/// Base64 runs at least this long are cut out.
const MIN_BASE64: usize = 256;

/// Blanks a line that is a packed table and replaces long base64 runs with `…`. The line
/// itself stays, so line numbers do not move.
pub fn clean(line: &mut String) {
    if is_packed_table(line) {
        line.clear();
    } else if let Some(cut) = cut_base64(line) {
        *line = cut;
    }
}

/// Many scripts alternating every few letters. Real text switches between two or three
/// scripts (Chinese with English or pinyin, Greek in formulas), and text in many scripts
/// (city names, language lists) switches at word boundaries.
fn is_packed_table(line: &str) -> bool {
    if line.len() < MIN_CHARS || line.is_ascii() {
        return false;
    }
    let mut seen: Vec<&str> = Vec::new();
    let (mut chars, mut letters, mut switches) = (0, 0, 0);
    let mut prev = None;
    for c in line.chars() {
        if c.is_whitespace() {
            prev = None;
            continue;
        }
        chars += 1;
        let Some(script) = script(c) else {
            prev = None;
            continue;
        };
        letters += 1;
        if prev.is_some_and(|p| p != script) {
            switches += 1;
        }
        prev = Some(script);
        if seen.len() < MIN_SCRIPTS && !seen.contains(&script) {
            seen.push(script);
        }
    }
    chars >= MIN_CHARS && seen.len() >= MIN_SCRIPTS && switches * SWITCH_EVERY >= letters
}

/// A letter's script, by block family: Han with kana, Hangul, all Latin blocks (with IPA,
/// modifier, letterlike, mathematical, number-form and super- or subscript letters, which
/// technical text mixes into any language: 第Ⅱ章, ℝⁿ, xₖ), else the first word of the block
/// name ("Greek Extended" is Greek). Controls (but tab, CR, FF, backspace and escape),
/// noncharacters and code points outside every block count as one more script; other
/// characters, private use included (icon fonts, Office bullets), as none.
fn script(c: char) -> Option<&'static str> {
    let block = find_unicode_block(c);
    let u = c as u32;
    if (c.is_control() && !matches!(c, '\t' | '\r' | '\x08' | '\x0c' | '\x1b'))
        || (0xfdd0..=0xfdef).contains(&u)
        || u & 0xfffe == 0xfffe
        || block.is_none()
    {
        return Some("odd");
    }
    let block = block.filter(|_| c.is_alphabetic())?;
    let name = block.name();
    if name.starts_with("Hangul") {
        return Some("Hangul");
    }
    if is_cjk_block(block) {
        return Some("CJK");
    }
    let first = name.split([' ', '-']).next().unwrap_or(name);
    Some(match first {
        "Basic" | "Latin" | "IPA" | "Spacing" | "Phonetic" | "Letterlike" | "Mathematical"
        | "Alphabetic" | "Enclosed" | "Number" | "Superscripts" | "Combining" => "Latin",
        _ => first,
    })
}

/// `line` with each base64 run of at least [`MIN_BASE64`] characters that mixes upper and
/// lower case letters and digits, as encoded binary does (hex and long words do not), replaced
/// by `…`; `None` if it has none.
fn cut_base64(line: &str) -> Option<String> {
    if line.len() < MIN_BASE64 {
        return None;
    }
    let bytes = line.as_bytes();
    let base64 = |b: u8| b.is_ascii_alphanumeric() || b == b'+' || b == b'/';
    let mut out = String::new();
    let (mut copied, mut i) = (0, 0);
    while i < bytes.len() {
        if !base64(bytes[i]) {
            i += 1;
            continue;
        }
        let start = i;
        let (mut upper, mut lower, mut digit) = (false, false, false);
        while i < bytes.len() && base64(bytes[i]) {
            upper |= bytes[i].is_ascii_uppercase();
            lower |= bytes[i].is_ascii_lowercase();
            digit |= bytes[i].is_ascii_digit();
            i += 1;
        }
        while i < bytes.len() && bytes[i] == b'=' {
            i += 1;
        }
        if i - start >= MIN_BASE64 && upper && lower && digit {
            out.push_str(&line[copied..start]);
            out.push('…');
            copied = i;
        }
    }
    (copied > 0).then(|| out + &line[copied..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{DocFormat, parse};

    /// Shaped like the HTML-entity decode trie that bundlers inline as one string: entity-name
    /// fragments between characters that only encode numbers (a value with its flags, branch
    /// offsets, the value itself), so CJK, Hangul, Latin-1, C1 controls and letters of many
    /// other scripts alternate every few characters.
    fn packed_table() -> String {
        let names = [
            "cute", "reve;", "rave", "pha;", "acr;", "ogon;", "ring;", "ilde", "lig",
        ];
        let mut seed = 1u32;
        let mut next = move |n: u32| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (seed >> 16) % n
        };
        let mut line = String::from("var t = new Uint16Array(\"");
        for _ in 0..800 {
            line.push_str(names[next(names.len() as u32) as usize]);
            for _ in 0..=next(3) {
                let unit = match next(4) {
                    0 => 0x4000 | (0xc0 + next(0x40)),
                    1 => 0x8000 | (0x21 + next(0x5e)),
                    2 => 0x80 + next(0x3a00),
                    _ => 0xc0 + next(0x40),
                };
                line.extend(char::from_u32(unit));
            }
        }
        line + "\".split(\"\").map(c => c.charCodeAt(0)));"
    }

    fn cleaned(line: &str) -> String {
        let mut line = line.to_owned();
        clean(&mut line);
        line
    }

    #[test]
    fn drops_packed_tables_keeps_text_and_cuts_base64() {
        let table = packed_table();
        assert!(table.matches("cute").count() > 50);
        assert_eq!(cleaned(&table), "");
        // The line stays, so the lines after it keep their numbers.
        let file = format!("const a = 1;\n{table}\nconst b = 2;\n");
        let parsed = parse(DocFormat::Plain, "lib.js".as_ref(), file.as_bytes()).unwrap();
        assert_eq!(parsed.sections[0].start, 1);
        assert_eq!(
            parsed.sections[0].units,
            ["const a = 1;", "", "const b = 2;"]
        );

        let text = [
            "索引会在后台持续更新，修改过的文件大约一秒之后就能搜到；电池供电时默认暂停，接上电源后自动继续。",
            "このプログラムはファイルを監視して、変更があれば自動的にインデックスを更新します。設定はGUIから。",
            "이 프로그램은 文書와 이미지를 색인합니다. 설정 창에서 Rust 데몬을 다시 시작할 수 있습니다. ",
            "在Rust中用tokio::spawn启动async任务，再用JoinHandle等待结果；CPU占用高时调低threads。",
            "zhe个地铁对yu远城区的意义肯定是利大于弊，带来方bian快捷安全的同shi，也确实会带走yi部分的xiao费。",
            "设α为x的函数，若∀ε>0∃δ>0使|f(x)−f(a)|<ε，则f在a处连续；∑ᵢxᵢ²≤∫₀¹g(t)dt，x∈ℝ。",
            "┌──────┐ 📁 Documents ✅ 已索引 │ 🖼️ Pictures ⏸️ 暂停 └──────┘ ★☆♥ ±×÷ ≤≥≠ ∞ → ⇒ ",
            "Привет, мир! Γειά σου κόσμε! مرحبا بالعالم! שלום עולם! สวัสดีชาวโลก! नमस्ते दुनिया! ",
            "English · Deutsch · Français · 日本語 · 한국어 · 中文 · Русский · العربية · עברית · हिन्दी · ไทย · Ελληνικά · ",
            "{en:\"Settings\",zh:\"设置\",ja:\"設定\",ko:\"설정\",ru:\"Настройки\",el:\"Ρυθμίσεις\",ar:\"الإعدادات\",he:\"הגדרות\",th:\"การตั้งค่า\",hi:\"सेटिंग्स\"},",
            "function(e,t){var n=t.length;for(var r=0;r<n;r++){e[r]=t[r]*2+e[r-1]||0}return e.map(function(x){return x.toFixed(2)})};",
            "ΑA,ΒB,ΨC,ΔD,ΕE,ΦF,ΓG,ΗH,ΙI,ΞJ,ΚK,ΛL,ΜM,ΝN,ΟO,ΠP,QQ,ΡR,ΣS,ΤT,ΘU,ΩV,WW,ΧX,ΥY,ΖZ,",
            "\u{e5ff} src \u{e0a0} main \u{f00c} cargo build --release \u{f017} 0.12s \u{f1d3}git\u{e725}push ",
            "\x1b[32mINFO\x1b[0m indexer: 12 new, 3 modified in 0.8s on cpu \x1b[2m(load 0.4s)\x1b[0m ",
            "Tiếng Việt có nhiều dấu thanh: sắc, huyền, hỏi, ngã, nặng; chữ ơ, ư, đ cũng thường gặp. ",
            // Technical text with number forms, super- and subscripts and private-use glyphs
            // (an Office bullet, Symbol-font Greek from a PDF) next to letters.
            "\u{f0b7} 第Ⅱ章 优化：设g:ℝᵐ→ℝ可微且∇g满足L-Lipschitz条件，迭代xₖ₊₁=xₖ−η∇g(xₖ)在0<η≤1/L时单调下降；若g为μ-强凸，则以1−μ/L线性收敛（见第Ⅲ章，Wolfe条件c₁=10⁻⁴）。",
            "제Ⅲ장 電磁氣學: 도체 내부의 전기장E는 0이고 전위φ는 일정하다. 유전율ε인 매질에서 ∇·D=ρ, 단위 C·m⁻². Rⁿ의 발산정리는 제Ⅳ장, 경계조건 Eₜ₁=Eₜ₂. ",
            "第Ⅳ節ではロシア語のкошкаとギリシア語のγάταを比べ、λₘₐₓとⅰ・ⅱの対応をGrimmの法則で説明する。",
            "已知函数f(x)=sin(\u{f077}x+\u{f06a})（\u{f077}>0），第Ⅱ卷第17题：求\u{f06a}的取值范围，并证明xₙ→\u{f070}/2。",
        ];
        for sentence in text {
            let line = sentence.repeat(6);
            assert_eq!(cleaned(&line), line, "{sentence}");
        }

        let base64: String = (0..400)
            .map(|i| {
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"[i * 37 % 64]
                    as char
            })
            .collect();
        let image = format!("![门牌照片](data:image/png;base64,{base64}==) 拍于 2026 年");
        assert_eq!(
            cleaned(&image),
            "![门牌照片](data:image/png;base64,…) 拍于 2026 年"
        );
        let note = format!("# ![logo](data:image/png;base64,{base64})\ntext\n");
        let parsed = parse(DocFormat::Markdown, "n.md".as_ref(), note.as_bytes()).unwrap();
        assert_eq!(parsed.title, "![logo](data:image/png;base64,…)");
        assert_eq!(parsed.sections[0].heading, parsed.title);
        let hex = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".repeat(5);
        assert_eq!(cleaned(&hex), hex);
    }
}
