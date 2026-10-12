//! Text extraction from images (PP-OCRv6 via oar-ocr / ONNX Runtime).

use std::path::Path;

use anyhow::{Context, Result};
use oar_ocr::core::config::OrtSessionConfig;
use oar_ocr::domain::tasks::TextDetectionConfig;
use oar_ocr::oarocr::{OAROCR, OAROCRBuilder};
use oar_ocr::processors::LimitType;

use crate::terms::is_cjk;

pub const DET_MODEL: &str = "pp-ocrv6_small_det.onnx";
pub const REC_MODEL: &str = "pp-ocrv6_small_rec.onnx";
pub const DICT: &str = "ppocrv6_dict.txt";

pub trait Ocr {
    /// Recognized text, one line per detected region; empty when nothing language-like is found.
    fn recognize(&mut self, image: image::RgbImage) -> Result<String>;
}

pub struct PaddleOcr {
    pipeline: OAROCR,
}

impl PaddleOcr {
    /// `threads` caps ONNX Runtime's intra-op threads (its default: one per core) and lets idle
    /// workers sleep instead of spinning, so that a background pipeline leaves the cores free.
    pub fn new(model_dir: &Path, threads: Option<usize>) -> Result<Self> {
        for name in [DET_MODEL, REC_MODEL, DICT] {
            let path = model_dir.join(name);
            anyhow::ensure!(
                path.is_file(),
                "OCR model {} is missing (run `just models`)",
                path.display()
            );
        }
        // Screenshots carry small UI text: keep native resolution (PaddleX "min 64" policy)
        // instead of the library's default downscale to 960 px.
        let detection = TextDetectionConfig {
            limit_side_len: Some(64),
            limit_type: Some(LimitType::Min),
            max_side_len: Some(4000),
            ..Default::default()
        };
        let mut builder = OAROCRBuilder::new(
            model_dir.join(DET_MODEL),
            model_dir.join(REC_MODEL),
            model_dir.join(DICT),
        )
        .text_detection_config(detection);
        if let Some(threads) = threads {
            let mut session = OrtSessionConfig::new()
                .with_intra_threads(threads)
                .with_inter_threads(1);
            session.session_config_entries =
                Some([("session.intra_op.allow_spinning".into(), "0".into())].into());
            builder = builder.ort_session(session);
        }
        let pipeline = builder.build().context("building the OCR pipeline")?;
        Ok(Self { pipeline })
    }
}

impl Ocr for PaddleOcr {
    fn recognize(&mut self, image: image::RgbImage) -> Result<String> {
        let results = self.pipeline.predict(vec![image])?;
        let lines: Vec<&str> = results
            .iter()
            .flat_map(|r| r.text_regions.iter())
            .filter_map(|region| region.text_with_confidence())
            .map(|(text, _)| text.trim())
            .filter(|text| !text.is_empty())
            .collect();
        let text = lines.join("\n");
        Ok(if looks_like_text(&text) {
            text
        } else {
            String::new()
        })
    }
}

const MIN_WORDS: usize = 2;
const MIN_WORD_CHARS: usize = 8;
const MIN_CJK_RUN: usize = 4;
const MIN_WORD_RATIO: f64 = 0.6;

/// Rejects OCR noise (icons, textures, stray glyphs), ported from Sift: the output must contain
/// at least two words covering at least 8 characters, or one CJK run of 4+ characters (a
/// single line such as 北京南站欢迎您), and words must cover 60% of the non-space, non-digit
/// characters. A word is a run of 3+ letters, or 2+ letters for CJK scripts where
/// two-character words are the norm. Digits are neutral so timestamps and prices don't count
/// against real text.
pub fn looks_like_text(text: &str) -> bool {
    let total = text
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_numeric())
        .count();
    let (mut words, mut in_words, mut longest_cjk) = (0, 0, 0);
    let (mut run, mut run_cjk) = (0, false);
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_alphabetic() {
            run += 1;
            run_cjk |= is_cjk(c);
        } else {
            if run >= 3 || (run == 2 && run_cjk) {
                words += 1;
                in_words += run;
            }
            if run_cjk {
                longest_cjk = longest_cjk.max(run);
            }
            (run, run_cjk) = (0, false);
        }
    }
    let enough = (words >= MIN_WORDS && in_words >= MIN_WORD_CHARS) || longest_cjk >= MIN_CJK_RUN;
    enough && in_words as f64 / total as f64 >= MIN_WORD_RATIO
}

#[cfg(test)]
mod tests {
    use super::looks_like_text;

    #[test]
    fn keeps_language_and_drops_noise() {
        assert!(looks_like_text("Failed to mount volume\npermission denied"));
        assert!(looks_like_text("明天下午三点开会\n记得带上报表"));
        assert!(looks_like_text("北京南站欢迎您"));
        assert!(looks_like_text(
            "火车票订单已支付\n车次 G1234 北京南→上海虹桥\n发车时间 2026年5月1日08:00\n票价 ¥553.0"
        ));
        assert!(!looks_like_text(""));
        assert!(!looks_like_text("|| -- ## 12:30 ~~ ::"));
        assert!(!looks_like_text("abc"));
        assert!(!looks_like_text("口口 一 1 2 Il"));
        assert!(!looks_like_text(
            "ok 1 2 3 4 5 6 7 8 9 0 % $ # @ ! ^ & * ( ) [ ] { }"
        ));
    }
}
