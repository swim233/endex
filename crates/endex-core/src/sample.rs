//! Reproducible test-set manifests (paths only; file contents are never read).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use walkdir::WalkDir;

use crate::index::IMAGE_EXTENSIONS;

pub const SCREENSHOTS: usize = 85;
pub const OTHER_IMAGES: usize = 15;
pub const MARKDOWN: usize = 30;
pub const DOCX: usize = 10;

pub struct Spec {
    pub pictures: PathBuf,
    pub documents: PathBuf,
    pub out_dir: PathBuf,
    pub seed: u64,
}

#[derive(Debug)]
pub struct Report {
    pub screenshots: usize,
    pub screenshot_months: usize,
    pub other_images: usize,
    pub other_groups: usize,
    pub markdown: usize,
    pub docx: usize,
    pub images_manifest: PathBuf,
    pub texts_manifest: PathBuf,
}

pub fn run(spec: &Spec) -> Result<Report> {
    let mut rng = SplitMix64(spec.seed);
    let screenshots_dir = spec.pictures.join("Screenshots");

    let mut months: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for path in files(&screenshots_dir)? {
        if let Some(month) = screenshot_month(&path) {
            months.entry(month).or_default().push(path);
        }
    }
    let mut others: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for path in files(&spec.pictures)? {
        if path.starts_with(&screenshots_dir) || !has_extension(&path, IMAGE_EXTENSIONS) {
            continue;
        }
        let rel = path
            .strip_prefix(&spec.pictures)
            .expect("walked below pictures");
        let group = match rel.components().count() {
            1 => "(top-level)".to_owned(),
            _ => rel
                .components()
                .next()
                .expect("non-empty")
                .as_os_str()
                .to_string_lossy()
                .into_owned(),
        };
        others.entry(group).or_default().push(path);
    }
    let screenshots = stratified(&months, SCREENSHOTS, &mut rng);
    let other_images = stratified(&others, OTHER_IMAGES, &mut rng);

    let (mut markdown, mut docx) = (Vec::new(), Vec::new());
    for path in files(&spec.documents)? {
        let lock_file = path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("~$"));
        if has_extension(&path, &["md", "markdown"]) {
            markdown.push(path);
        } else if has_extension(&path, &["docx"]) && !lock_file {
            docx.push(path);
        }
    }
    let markdown = pick(markdown, MARKDOWN, &mut rng);
    let docx = pick(docx, DOCX, &mut rng);

    fs::create_dir_all(&spec.out_dir)?;
    let images_manifest = spec.out_dir.join("images.txt");
    let texts_manifest = spec.out_dir.join("texts.txt");
    write_manifest(&images_manifest, screenshots.iter().chain(&other_images))?;
    write_manifest(&texts_manifest, markdown.iter().chain(&docx))?;
    Ok(Report {
        screenshots: screenshots.len(),
        screenshot_months: months.len(),
        other_images: other_images.len(),
        other_groups: others.len(),
        markdown: markdown.len(),
        docx: docx.len(),
        images_manifest,
        texts_manifest,
    })
}

/// `屏幕截图_20250812_101500.png` → `202508`.
fn screenshot_month(path: &Path) -> Option<String> {
    if !has_extension(path, &["png"]) {
        return None;
    }
    let name = path.file_name()?.to_str()?;
    let date = name.strip_prefix("屏幕截图_")?.get(..8)?;
    date.bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| date[..6].to_owned())
}

/// Spreads `n` picks as evenly as possible over the groups; groups with too few files give
/// their share to the others. Deterministic for a given seed and file set.
pub fn stratified(
    groups: &BTreeMap<String, Vec<PathBuf>>,
    n: usize,
    rng: &mut SplitMix64,
) -> Vec<PathBuf> {
    let mut keys: Vec<&String> = groups.keys().collect();
    rng.shuffle(&mut keys);
    let mut quota: BTreeMap<&String, usize> = keys.iter().map(|k| (*k, 0)).collect();
    let mut remaining = n;
    while remaining > 0 {
        let mut progressed = false;
        for key in &keys {
            if remaining > 0 && quota[key] < groups[*key].len() {
                *quota.get_mut(key).expect("every key has a quota") += 1;
                remaining -= 1;
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
    let mut picked = Vec::new();
    for (key, files) in groups {
        picked.extend(pick(files.clone(), quota[key], rng));
    }
    picked.sort();
    picked
}

fn pick(mut files: Vec<PathBuf>, n: usize, rng: &mut SplitMix64) -> Vec<PathBuf> {
    files.sort();
    rng.shuffle(&mut files);
    files.truncate(n);
    files.sort();
    files
}

/// Regular files below `dir`, skipping hidden entries.
fn files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let walker = WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'));
    for entry in walker {
        let entry = entry.with_context(|| format!("listing {}", dir.display()))?;
        if entry.file_type().is_file() {
            out.push(entry.into_path());
        }
    }
    Ok(out)
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| extensions.contains(&e.to_ascii_lowercase().as_str()))
}

fn write_manifest<'a>(path: &Path, files: impl Iterator<Item = &'a PathBuf>) -> Result<()> {
    let text: String = files.map(|p| format!("{}\n", p.display())).collect();
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

/// Small, stable PRNG so a seed reproduces the same sample across dependency upgrades.
pub struct SplitMix64(pub u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = (self.next() % (i as u64 + 1)) as usize;
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn groups(sizes: &[(&str, usize)]) -> BTreeMap<String, Vec<PathBuf>> {
        sizes
            .iter()
            .map(|(k, n)| {
                (
                    k.to_string(),
                    (0..*n)
                        .map(|i| PathBuf::from(format!("/{k}/{i}")))
                        .collect(),
                )
            })
            .collect()
    }

    #[test]
    fn stratified_is_even_reproducible_and_redistributes() {
        let g = groups(&[
            ("202508", 40),
            ("202509", 3),
            ("202510", 40),
            ("202511", 40),
        ]);
        let picked = stratified(&g, 20, &mut SplitMix64(7));
        let count = |k: &str| {
            picked
                .iter()
                .filter(|p| p.starts_with(format!("/{k}")))
                .count()
        };
        assert_eq!(picked.len(), 20);
        assert_eq!(count("202509"), 3);
        for k in ["202508", "202510", "202511"] {
            assert!((5..=6).contains(&count(k)), "{k}: {}", count(k));
        }
        assert_eq!(picked, stratified(&g, 20, &mut SplitMix64(7)));
        assert_eq!(stratified(&g, 500, &mut SplitMix64(7)).len(), 123);
    }

    #[test]
    fn screenshot_month_from_name() {
        let month = |n: &str| screenshot_month(Path::new(n));
        assert_eq!(
            month("/p/屏幕截图_20250812_101500.png").as_deref(),
            Some("202508")
        );
        assert_eq!(month("/p/屏幕截图_2025.png"), None);
        assert_eq!(month("/p/other_20250812.png"), None);
    }
}
