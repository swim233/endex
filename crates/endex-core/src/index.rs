//! Incremental indexing of manifest files: parse → chunk → OCR → embed → write.

use std::collections::HashSet;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail, ensure};
use image::ImageFormat;

use crate::Kind;
use crate::catalog::{Catalog, Change, FileRecord, STATUS_ERROR, STATUS_OK, detect_change};
use crate::chunk::{MAX_TOKENS, TokenCounter, chunk_section};
use crate::config::{Config, expand_home};
use crate::embed::{self, Device, Embedder};
use crate::ocr::{Ocr, PaddleOcr};
use crate::parse::{self, DocFormat};
use crate::store::{Row, Store};
use crate::terms::Analyzer;

pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];

const FLUSH_ROWS: usize = 256;
const FLUSH_FILES: usize = 64;
/// Catalog flag set before the store changes and cleared once compaction, keyword index
/// rebuild and pruning are done.
const DIRTY: &str = "dirty";

pub struct Options {
    pub manifests: Vec<PathBuf>,
    pub device: Device,
    pub rebuild: bool,
}

pub enum Event<'a> {
    GpuFallback(&'a anyhow::Error),
    Loaded {
        what: &'static str,
        took: Duration,
    },
    Failed {
        path: &'a str,
        error: &'a anyhow::Error,
    },
    Progress {
        done: usize,
        total: usize,
    },
}

#[derive(Debug, Default)]
pub struct Stage {
    pub time: Duration,
    pub items: usize,
}

impl Stage {
    fn add(&mut self, since: Instant, items: usize) {
        self.time += since.elapsed();
        self.items += items;
    }
}

#[derive(Debug, Default)]
pub struct Report {
    pub new: usize,
    pub modified: usize,
    pub touched: usize,
    pub unchanged: usize,
    pub removed: usize,
    pub failed: usize,
    pub unsupported: usize,
    pub device: Option<Device>,
    pub load: Duration,
    /// Reading, parsing, chunking and keyword analysis; items = chunks.
    pub parse: Stage,
    /// Image decoding and OCR; items = images.
    pub ocr: Stage,
    pub embed_text: Stage,
    pub embed_image: Stage,
    /// LanceDB + catalog writes and the final compaction/index build.
    pub write: Stage,
    pub total: Duration,
}

struct Entry {
    path: String,
    manifest: String,
}

pub async fn run(config: &Config, options: &Options, on: &mut dyn FnMut(Event)) -> Result<Report> {
    let started = Instant::now();
    fs::create_dir_all(config.index_root())?;
    let mut catalog = Catalog::open(&config.catalog_path())?;
    let store = if options.rebuild {
        catalog.clear()?;
        Store::reset(&config.index_dir()).await?
    } else {
        Store::open(&config.index_dir()).await?
    };
    catalog.ensure_embedding_identity(
        &embed::model_identity(config)?,
        config.embedding.prefix_style.as_str(),
    )?;
    let mut pipeline = Pipeline {
        config,
        device: options.device,
        analyzer: Analyzer::new(),
        counter: None,
        embedder: None,
        ocr: None,
        report: Report::default(),
    };

    let entries = read_manifests(&options.manifests)?;
    let removed = removed_paths(&catalog, &options.manifests, &entries)?;
    if !removed.is_empty() {
        let t = Instant::now();
        catalog.set_meta(DIRTY, Some("1"))?;
        store.replace(&removed, &[]).await?;
        catalog.remove(&removed)?;
        pipeline.report.write.add(t, 0);
        pipeline.report.removed = removed.len();
    }

    let mut batch = Batch::default();
    for (i, entry) in entries.iter().enumerate() {
        on(Event::Progress {
            done: i,
            total: entries.len(),
        });
        pipeline.visit(entry, &catalog, &mut batch, on)?;
        if batch.rows.len() >= FLUSH_ROWS || batch.records.len() >= FLUSH_FILES {
            batch
                .flush(&store, &mut catalog, &mut pipeline.report)
                .await?;
        }
    }
    batch
        .flush(&store, &mut catalog, &mut pipeline.report)
        .await?;
    on(Event::Progress {
        done: entries.len(),
        total: entries.len(),
    });

    // Release the GPU before the (CPU-only) index build.
    pipeline.embedder = None;
    pipeline.ocr = None;
    // The flag survives interrupted runs, so an unfinished index build is always completed.
    if catalog.meta(DIRTY)?.is_some() {
        let t = Instant::now();
        store.finish().await?;
        catalog.set_meta(DIRTY, None)?;
        pipeline.report.write.add(t, 0);
    }
    pipeline.report.total = started.elapsed();
    Ok(pipeline.report)
}

#[derive(Default)]
struct Batch {
    paths: Vec<String>,
    rows: Vec<Row>,
    records: Vec<FileRecord>,
    /// Rows are added or removed. Files that keep failing only repeat a no-op delete and must
    /// not trigger the post-run compaction and index build.
    dirty: bool,
}

impl Batch {
    async fn flush(
        &mut self,
        store: &Store,
        catalog: &mut Catalog,
        report: &mut Report,
    ) -> Result<()> {
        let t = Instant::now();
        if self.dirty {
            catalog.set_meta(DIRTY, Some("1"))?;
        }
        store.replace(&self.paths, &self.rows).await?;
        catalog.upsert(&self.records)?;
        report.write.add(t, self.rows.len());
        self.dirty = false;
        self.paths.clear();
        self.rows.clear();
        self.records.clear();
        Ok(())
    }
}

struct Pipeline<'a> {
    config: &'a Config,
    device: Device,
    analyzer: Analyzer,
    /// Loaded on first use: unchanged runs skip the 30 MB tokenizer.
    counter: Option<TokenCounter>,
    embedder: Option<Box<dyn Embedder>>,
    ocr: Option<Box<dyn Ocr>>,
    report: Report,
}

impl Pipeline<'_> {
    /// Detects what changed for one manifest entry and stages the outcome in `batch`.
    /// File failures are recorded in the catalog (and drop stale rows); failing to load a
    /// model aborts the run.
    fn visit(
        &mut self,
        entry: &Entry,
        catalog: &Catalog,
        batch: &mut Batch,
        on: &mut dyn FnMut(Event),
    ) -> Result<()> {
        let path = Path::new(&entry.path);
        let Some(kind) = kind_of(path) else {
            self.report.unsupported += 1;
            return Ok(());
        };
        let prev = catalog.get(&entry.path)?;
        let mut record = FileRecord {
            path: entry.path.clone(),
            kind,
            size: 0,
            mtime: 0,
            hash: String::new(),
            status: STATUS_OK.into(),
            error: None,
            indexed_at: unix_now(),
            manifest: entry.manifest.clone(),
        };
        let max_bytes = self.config.index.max_document_mb.saturating_mul(1 << 20);
        let mut bytes = None;
        let change = fs::metadata(path)
            .context("reading metadata")
            .and_then(|meta| {
                (record.size, record.mtime) = (meta.len(), mtime_ns(&meta));
                ensure!(
                    kind != Kind::Doc || record.size <= max_bytes,
                    "{:.1} MB is over index.max_document_mb ({})",
                    record.size as f64 / f64::from(1 << 20),
                    self.config.index.max_document_mb
                );
                detect_change(prev.as_ref(), record.size, record.mtime, || {
                    let content = fs::read(path)?;
                    let hash = blake3::hash(&content).to_hex().to_string();
                    bytes = Some(content);
                    Ok(hash)
                })
            });
        let had_rows = prev.as_ref().is_some_and(|p| p.status == STATUS_OK);
        let rows = match change {
            Ok(Change::Unchanged) => {
                self.report.unchanged += 1;
                let prev = prev.expect("unchanged implies a previous record");
                if prev.manifest != entry.manifest {
                    batch.records.push(FileRecord {
                        manifest: entry.manifest.clone(),
                        ..prev
                    });
                }
                return Ok(());
            }
            Ok(Change::Touched { hash }) => {
                self.report.touched += 1;
                let prev = prev.expect("touched implies a previous record");
                (record.hash, record.status, record.error) = (hash, prev.status, prev.error);
                batch.records.push(record);
                return Ok(());
            }
            Ok(Change::New { hash } | Change::Modified { hash }) => {
                if prev.is_some() {
                    self.report.modified += 1;
                } else {
                    self.report.new += 1;
                }
                record.hash = hash;
                self.load_models(kind, on)?;
                let bytes = bytes.take().expect("hashing read the file");
                self.process(&entry.path, kind, record.mtime, &bytes)
            }
            Err(error) => Err(error),
        };
        batch.dirty |= had_rows || rows.as_ref().is_ok_and(|rows| !rows.is_empty());
        match rows {
            Ok(rows) => batch.rows.extend(rows),
            Err(error) => {
                on(Event::Failed {
                    path: &entry.path,
                    error: &error,
                });
                self.report.failed += 1;
                record.status = STATUS_ERROR.into();
                record.error = Some(format!("{error:#}"));
            }
        }
        batch.paths.push(entry.path.clone());
        batch.records.push(record);
        Ok(())
    }

    fn process(&mut self, path: &str, kind: Kind, mtime: i64, bytes: &[u8]) -> Result<Vec<Row>> {
        let file = Path::new(path);
        let name = self.analyzer.name_terms(&display_name(file));
        let base = Row {
            id: String::new(),
            path: path.to_owned(),
            kind,
            title: String::new(),
            location: String::new(),
            text: String::new(),
            terms: String::new(),
            name,
            vector: Vec::new(),
            mtime,
        };
        match kind {
            Kind::Doc => self.document(file, bytes, base),
            Kind::Image => self.image(file, bytes, base).map(|row| vec![row]),
        }
    }

    fn document(&mut self, file: &Path, bytes: &[u8], base: Row) -> Result<Vec<Row>> {
        let t = Instant::now();
        let format = DocFormat::from_path(file).context("unsupported document type")?;
        let parsed = parse::parse(format, file, bytes)?;
        let counter = self.counter.as_ref().expect("loaded by visit");
        let chunks: Vec<_> = parsed
            .sections
            .iter()
            .flat_map(|section| chunk_section(section, MAX_TOKENS, counter))
            .collect();
        let mut rows: Vec<Row> = chunks
            .into_iter()
            .enumerate()
            .map(|(i, chunk)| Row {
                id: format!("{}#{i}", base.path),
                title: parsed.title.clone(),
                location: chunk.location,
                terms: self.analyzer.joined(&chunk.text),
                text: chunk.text,
                ..base.clone()
            })
            .collect();
        self.report.parse.add(t, rows.len());

        let t = Instant::now();
        let embedder = self.embedder.as_deref_mut().expect("loaded by visit");
        for row in &mut rows {
            row.vector = embedder.embed_document(&row.text)?;
        }
        self.report.embed_text.add(t, rows.len());
        Ok(rows)
    }

    fn image(&mut self, file: &Path, bytes: &[u8], base: Row) -> Result<Row> {
        let t = Instant::now();
        let format = image::guess_format(bytes).context("unrecognized image format")?;
        let decoded =
            image::load_from_memory_with_format(bytes, format).context("decoding image")?;
        // LiteRT decodes PNG and JPEG itself; anything else is handed over as PNG.
        let encoded = match format {
            ImageFormat::Png | ImageFormat::Jpeg => None,
            _ => {
                let mut png = Cursor::new(Vec::new());
                decoded.write_to(&mut png, ImageFormat::Png)?;
                Some(png.into_inner())
            }
        };
        let ocr = self.ocr.as_deref_mut().expect("loaded by visit");
        let text = ocr.recognize(decoded.into_rgb8())?;
        self.report.ocr.add(t, 1);

        let t = Instant::now();
        let vector = self
            .embedder
            .as_deref_mut()
            .expect("loaded by visit")
            .embed_image(encoded.as_deref().unwrap_or(bytes))?;
        self.report.embed_image.add(t, 1);
        Ok(Row {
            id: format!("{}#0", base.path),
            title: file
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            terms: self.analyzer.joined(&text),
            text,
            vector,
            ..base
        })
    }

    /// Loads what `kind` needs on first use. Errors here are not about one file, so they
    /// abort the run instead of being recorded against every file.
    fn load_models(&mut self, kind: Kind, on: &mut dyn FnMut(Event)) -> Result<()> {
        if self.embedder.is_none() {
            let t = Instant::now();
            let embedder = match embed::open(self.config, self.device) {
                Ok(e) => e,
                Err(gpu_error) if self.device == Device::Gpu => {
                    // Only a GPU-specific failure is worth a fallback notice; if the CPU fails
                    // too (missing library or model), that error is the one to report.
                    let cpu = embed::open(self.config, Device::Cpu)?;
                    on(Event::GpuFallback(&gpu_error));
                    cpu
                }
                Err(error) => return Err(error),
            };
            self.report.device = Some(embedder.device());
            self.report.load += t.elapsed();
            on(Event::Loaded {
                what: "embedding model",
                took: t.elapsed(),
            });
            self.embedder = Some(embedder);
        }
        if kind == Kind::Doc && self.counter.is_none() {
            self.counter = Some(TokenCounter::from_file(&self.config.embedding.tokenizer)?);
        }
        if kind == Kind::Image && self.ocr.is_none() {
            let t = Instant::now();
            self.ocr = Some(Box::new(PaddleOcr::new(&self.config.ocr_model_dir())?));
            self.report.load += t.elapsed();
            on(Event::Loaded {
                what: "OCR models",
                took: t.elapsed(),
            });
        }
        Ok(())
    }
}

pub fn kind_of(path: &Path) -> Option<Kind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        Some(Kind::Image)
    } else {
        DocFormat::from_path(path).map(|_| Kind::Doc)
    }
}

/// `parent/file.ext`, the text behind partial-name matches.
fn display_name(path: &Path) -> String {
    let file = path.file_name().unwrap_or_default().to_string_lossy();
    match path.parent().and_then(|p| p.file_name()) {
        Some(parent) => format!("{}/{file}", parent.to_string_lossy()),
        None => file.into_owned(),
    }
}

fn read_manifests(manifests: &[PathBuf]) -> Result<Vec<Entry>> {
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for manifest in manifests {
        let key = manifest_key(manifest)?;
        let text = fs::read_to_string(manifest)
            .with_context(|| format!("reading manifest {}", manifest.display()))?;
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let path = expand_home(Path::new(line));
            if !path.is_absolute() {
                bail!(
                    "{}: manifest paths must be absolute: {line}",
                    manifest.display()
                );
            }
            let path = path.to_string_lossy().into_owned();
            if seen.insert(path.clone()) {
                entries.push(Entry {
                    path,
                    manifest: key.clone(),
                });
            }
        }
    }
    Ok(entries)
}

/// Catalog entries of the given manifests that the manifests no longer list.
fn removed_paths(
    catalog: &Catalog,
    manifests: &[PathBuf],
    entries: &[Entry],
) -> Result<Vec<String>> {
    let listed: HashSet<&str> = entries.iter().map(|e| e.path.as_str()).collect();
    let mut removed = Vec::new();
    for manifest in manifests {
        for path in catalog.paths_of_manifest(&manifest_key(manifest)?)? {
            if !listed.contains(path.as_str()) {
                removed.push(path);
            }
        }
    }
    Ok(removed)
}

fn manifest_key(manifest: &Path) -> Result<String> {
    Ok(fs::canonicalize(manifest)
        .with_context(|| format!("manifest {}", manifest.display()))?
        .to_string_lossy()
        .into_owned())
}

fn mtime_ns(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos() as i64)
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}
