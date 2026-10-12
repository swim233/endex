//! One file through parse → chunk → OCR → embed, with models loaded on first use.

use std::fs;
use std::io::{Cursor, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result, ensure};
use image::ImageFormat;

use super::{Batch, Event, Report, kind_of, mtime_ns, unix_now};
use crate::Kind;
use crate::catalog::{Change, FileRecord, STATUS_ERROR, STATUS_OK, detect_change};
use crate::chunk::{MAX_TOKENS, TokenCounter, chunk_section};
use crate::config::Config;
use crate::embed::{self, Device, Embedder};
use crate::ocr::{Ocr, PaddleOcr};
use crate::parse::{self, DocFormat};
use crate::store::Row;
use crate::terms::Analyzer;

pub(super) struct Pipeline {
    pub(super) config: Config,
    /// Open files through symlinks (manifests may list links; endexd never follows them).
    pub(super) follow_links: bool,
    pub(super) device: Device,
    pub(super) threads: Option<usize>,
    analyzer: Analyzer,
    /// Loaded on first use: unchanged runs skip the 30 MB tokenizer.
    pub(super) counter: Option<TokenCounter>,
    pub(super) embedder: Option<Box<dyn Embedder>>,
    pub(super) ocr: Option<Box<dyn Ocr>>,
}

impl Pipeline {
    pub(super) fn new(config: &Config) -> Self {
        Self {
            config: config.clone(),
            follow_links: true,
            device: Device::Cpu,
            threads: None,
            analyzer: Analyzer::new(),
            counter: None,
            embedder: None,
            ocr: None,
        }
    }

    pub(super) fn visit(
        &mut self,
        path_str: &str,
        owner: &str,
        prev: Option<FileRecord>,
        batch: &mut Batch,
        report: &mut Report,
        on: &mut dyn FnMut(Event),
    ) -> Result<()> {
        let path = Path::new(path_str);
        let Some(kind) = kind_of(path) else {
            report.unsupported += 1;
            return Ok(());
        };
        let mut record = FileRecord {
            path: path_str.to_owned(),
            kind,
            size: 0,
            mtime: 0,
            hash: String::new(),
            status: STATUS_OK.into(),
            error: None,
            indexed_at: unix_now(),
            owner: owner.to_owned(),
        };
        let max_bytes = self.config.max_document_bytes();
        let mut bytes = None;
        let follow = self.follow_links;
        let stat = if follow {
            fs::metadata(path)
        } else {
            fs::symlink_metadata(path)
        };
        let change = stat.context("reading metadata").and_then(|meta| {
            ensure!(meta.is_file(), "not a regular file");
            (record.size, record.mtime) = (meta.len(), mtime_ns(&meta));
            ensure!(
                kind != Kind::Doc || record.size <= max_bytes,
                "{:.1} MB is over index.max_document_mb ({})",
                record.size as f64 / f64::from(1 << 20),
                self.config.index.max_document_mb
            );
            let limit = (kind == Kind::Doc).then_some(max_bytes);
            detect_change(prev.as_ref(), record.size, record.mtime, || {
                let content = read_same_file(path, &meta, follow, limit)?;
                let hash = blake3::hash(&content).to_hex().to_string();
                bytes = Some(content);
                Ok(hash)
            })
        });
        let had_rows = prev.as_ref().is_some_and(|p| p.status == STATUS_OK);
        let rows = match change {
            Ok(Change::Unchanged) => {
                report.unchanged += 1;
                let prev = prev.expect("unchanged implies a previous record");
                if prev.owner != owner {
                    batch.touch(path_str);
                    batch.records.push(FileRecord {
                        owner: owner.to_owned(),
                        ..prev
                    });
                }
                return Ok(());
            }
            Ok(Change::Touched { hash }) => {
                report.touched += 1;
                let prev = prev.expect("touched implies a previous record");
                (record.hash, record.status, record.error) = (hash, prev.status, prev.error);
                batch.touch(path_str);
                batch.records.push(record);
                return Ok(());
            }
            Ok(Change::New { hash } | Change::Modified { hash }) => {
                if prev.is_some() {
                    report.modified += 1;
                } else {
                    report.new += 1;
                }
                record.hash = hash;
                self.load_models(kind, report, on)?;
                let bytes = bytes.take().expect("hashing read the file");
                self.process(path_str, kind, record.mtime, &bytes, report)
            }
            Err(error) => Err(error),
        };
        batch.dirty |= had_rows || rows.as_ref().is_ok_and(|rows| !rows.is_empty());
        match rows {
            Ok(rows) => batch.rows.extend(rows),
            Err(error) => {
                on(Event::Failed {
                    path: path_str,
                    error: &error,
                });
                report.failed += 1;
                record.status = STATUS_ERROR.into();
                record.error = Some(format!("{error:#}"));
            }
        }
        batch.touch(path_str);
        batch.paths.push(path_str.to_owned());
        batch.records.push(record);
        Ok(())
    }

    fn process(
        &mut self,
        path: &str,
        kind: Kind,
        mtime: i64,
        bytes: &[u8],
        report: &mut Report,
    ) -> Result<Vec<Row>> {
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
            Kind::Doc => self.document(file, bytes, base, report),
            Kind::Image => self.image(file, bytes, base, report).map(|row| vec![row]),
        }
    }

    fn document(
        &mut self,
        file: &Path,
        bytes: &[u8],
        base: Row,
        report: &mut Report,
    ) -> Result<Vec<Row>> {
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
        report.parse.add(t, rows.len());

        let t = Instant::now();
        let embedder = self.embedder.as_deref_mut().expect("loaded by visit");
        for row in &mut rows {
            row.vector = embedder.embed_document(&row.text)?;
        }
        report.embed_text.add(t, rows.len());
        Ok(rows)
    }

    fn image(&mut self, file: &Path, bytes: &[u8], base: Row, report: &mut Report) -> Result<Row> {
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
        let text = match self.ocr.as_deref_mut() {
            Some(ocr) => ocr.recognize(decoded.into_rgb8())?,
            None => String::new(),
        };
        report.ocr.add(t, 1);

        let t = Instant::now();
        let vector = self
            .embedder
            .as_deref_mut()
            .expect("loaded by visit")
            .embed_image(encoded.as_deref().unwrap_or(bytes))?;
        report.embed_image.add(t, 1);
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
    fn load_models(
        &mut self,
        kind: Kind,
        report: &mut Report,
        on: &mut dyn FnMut(Event),
    ) -> Result<()> {
        if self.embedder.is_none() {
            let t = Instant::now();
            let embedder = match embed::open(&self.config, self.device, self.threads) {
                Ok(e) => e,
                Err(gpu_error) if self.device == Device::Gpu => {
                    // Only a GPU-specific failure is worth a fallback notice; if the CPU fails
                    // too (missing library or model), that error is the one to report.
                    let cpu = embed::open(&self.config, Device::Cpu, self.threads)?;
                    on(Event::GpuFallback(&gpu_error));
                    cpu
                }
                Err(error) => return Err(error),
            };
            report.device = Some(embedder.device());
            report.load += t.elapsed();
            on(Event::Loaded {
                what: "embedding model",
                took: t.elapsed(),
            });
            self.embedder = Some(embedder);
        }
        if kind == Kind::Doc && self.counter.is_none() {
            self.counter = Some(TokenCounter::from_file(&self.config.embedding.tokenizer)?);
        }
        if kind == Kind::Image && self.config.ocr.enabled && self.ocr.is_none() {
            let t = Instant::now();
            self.ocr = Some(Box::new(PaddleOcr::new(
                &self.config.ocr_model_dir(),
                self.threads,
            )?));
            report.load += t.elapsed();
            on(Event::Loaded {
                what: "OCR models",
                took: t.elapsed(),
            });
        }
        Ok(())
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

/// Reads the file `meta` describes, at most `limit` bytes. Never blocks on a FIFO, and with
/// `follow` false never goes through a symlink: a file swapped since `meta` was taken is not
/// read. (`O_NOFOLLOW` covers the last component only; a parent folder swapped for a symlink
/// in between would need `openat2(RESOLVE_NO_SYMLINKS)` from the source root to catch.)
fn read_same_file(
    path: &Path,
    meta: &fs::Metadata,
    follow: bool,
    limit: Option<u64>,
) -> Result<Vec<u8>> {
    let mut flags = libc::O_NONBLOCK;
    if !follow {
        flags |= libc::O_NOFOLLOW;
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags)
        .open(path)
        .context("opening the file")?;
    let opened = file.metadata()?;
    ensure!(
        opened.is_file() && opened.dev() == meta.dev() && opened.ino() == meta.ino(),
        "the file was replaced while it was read"
    );
    let mut content = Vec::with_capacity(opened.len() as usize);
    let limit = limit.unwrap_or(u64::MAX);
    file.take(limit.saturating_add(1))
        .read_to_end(&mut content)?;
    ensure!(
        content.len() as u64 <= limit,
        "the file grew over index.max_document_mb while it was read"
    );
    Ok(content)
}
