//! Incremental indexing: parse → chunk → OCR → embed → write. An [`Indexer`] stages changed
//! and removed files and writes them in batches; [`run`] drives it from manifests
//! (`endex index`), endexd from its source folders.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::Kind;
use crate::catalog::{Catalog, FileRecord, Identity, STATUS_ERROR};
use crate::config::Config;
use crate::embed::Device;
use crate::parse::DocFormat;
use crate::store::{Row, Store};

mod manifest;
mod pipeline;

pub use manifest::{Options, run};
use pipeline::Pipeline;

pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];

/// Version of the document text pipeline (parsing, the packed-data filter, chunking); bump it
/// when documents already indexed should go through a change. A new or emptied index records
/// it; endexd processes the documents of an index with another version again, `endex index`
/// does not (`--rebuild` does). 1: M1–M3; 2: packed data dropped.
pub const TEXT_PIPELINE: u32 = 2;

const FLUSH_ROWS: usize = 256;
const FLUSH_FILES: usize = 64;
const FLUSH_REMOVALS: usize = 512;
/// Catalog flag set before the store changes and cleared once compaction, keyword index
/// rebuild and pruning are done.
const DIRTY: &str = "dirty";
/// Catalog flag set while [`Indexer::clear`] runs; an interrupted clear is redone on open.
const CLEARING: &str = "clearing";

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
    /// A manifest line that is not an absolute UTF-8 path; the run goes on without it but
    /// removes nothing.
    Skipped {
        manifest: &'a Path,
        line: usize,
        reason: &'static str,
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
    /// Manifest lines that are not absolute UTF-8 paths. If there are any, the run removes
    /// nothing from the index.
    pub skipped: usize,
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

/// One writer's view of the index: catalog, table, models loaded on first use, and the batch
/// not written yet.
pub struct Indexer {
    catalog: Catalog,
    store: Store,
    pipeline: Pipeline,
    batch: Batch,
    auto_flush: bool,
    pub report: Report,
}

impl Indexer {
    /// Opens the index, creating it if needed; `rebuild` drops the table and the catalog
    /// first. The caller checks the identity.
    pub async fn open(config: &Config, rebuild: bool) -> Result<Self> {
        fs::create_dir_all(config.index_root())?;
        let mut catalog = Catalog::open(&config.catalog_path())?;
        let store = if rebuild {
            catalog.clear()?;
            Store::reset(&config.index_dir()).await?
        } else if catalog.meta(CLEARING)?.is_some() {
            let store = Store::reset(&config.index_dir()).await?;
            catalog.clear_files()?;
            catalog.set_meta(DIRTY, None)?;
            catalog.set_meta(CLEARING, None)?;
            store
        } else {
            Store::open(&config.index_dir()).await?
        };
        if catalog.file_count()? == 0 {
            catalog.set_text_pipeline(TEXT_PIPELINE)?;
        }
        Ok(Self {
            catalog,
            store,
            pipeline: Pipeline::new(config),
            batch: Batch::default(),
            auto_flush: true,
            report: Report::default(),
        })
    }

    /// Off: the caller decides when to [`Indexer::flush`] (endexd keeps its own write cadence)
    /// instead of a flush whenever a batch is full.
    pub fn set_auto_flush(&mut self, on: bool) {
        self.auto_flush = on;
    }

    /// Off: files are opened without following symlinks (endexd). The CLI follows them, as
    /// manifests may list links.
    pub fn set_follow_links(&mut self, on: bool) {
        self.pipeline.follow_links = on;
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Takes effect for files staged from now on; models already loaded stay (see
    /// [`Indexer::release_models`]).
    pub fn set_config(&mut self, config: &Config) {
        self.pipeline.config = config.clone();
    }

    /// The embedding device for files staged from now on; a model loaded on another device
    /// is released.
    pub fn set_device(&mut self, device: Device) {
        if self.pipeline.device != device {
            self.pipeline.embedder = None;
        }
        self.pipeline.device = device;
    }

    /// Caps the threads of models loaded from now on (CPU embedding and OCR).
    pub fn set_threads(&mut self, threads: Option<usize>) {
        self.pipeline.threads = threads;
    }

    /// The device of the loaded embedding model.
    pub fn loaded(&self) -> Option<Device> {
        self.pipeline.embedder.as_ref().map(|e| e.device())
    }

    pub fn models_loaded(&self) -> bool {
        let p = &self.pipeline;
        p.embedder.is_some() || p.ocr.is_some() || p.counter.is_some()
    }

    /// OCR, the tokenizer or a CPU embedder is loaded (what [`Indexer::release`] frees
    /// without `gpu`).
    pub fn cpu_models_loaded(&self) -> bool {
        let p = &self.pipeline;
        p.ocr.is_some() || p.counter.is_some() || self.loaded() == Some(Device::Cpu)
    }

    pub fn release_models(&mut self) {
        self.release(true);
    }

    /// Releases OCR, the tokenizer and a CPU embedder; a GPU embedder only if `gpu`.
    pub fn release(&mut self, gpu: bool) {
        let p = &mut self.pipeline;
        if gpu
            || p.embedder
                .as_ref()
                .is_some_and(|e| e.device() == Device::Cpu)
        {
            p.embedder = None;
        }
        p.ocr = None;
        p.counter = None;
    }

    /// Detects what changed for one file and stages the outcome; flushes a full batch. File
    /// failures are recorded in the catalog (and drop stale rows); failing to load a model
    /// is an error. A file with changes staged already (seen again by a rescan or the watcher
    /// before the write) is written first, so that it is compared with them, not staged twice.
    pub async fn update(
        &mut self,
        path: &str,
        owner: &str,
        on: &mut dyn FnMut(Event),
    ) -> Result<()> {
        if self.is_staged(path) {
            self.flush().await?;
        }
        let prev = self.catalog.get(path)?;
        self.pipeline
            .visit(path, owner, prev, &mut self.batch, &mut self.report, on)?;
        if self.auto_flush && self.batch.is_full() {
            self.flush().await?;
        }
        Ok(())
    }

    /// Stages the removal of indexed files; flushes a full batch. Changes staged for one of
    /// them are written first.
    pub async fn remove(&mut self, paths: &[String]) -> Result<()> {
        for path in paths {
            if self.is_staged(path) {
                self.flush().await?;
            }
            self.batch.touch(path);
            self.batch.paths.push(path.clone());
            self.batch.removed.push(path.clone());
            self.batch.dirty = true;
            self.report.removed += 1;
            if self.auto_flush && self.batch.is_full() {
                self.flush().await?;
            }
        }
        Ok(())
    }

    /// Whether `path` has changes waiting for [`Indexer::flush`].
    pub fn is_staged(&self, path: &str) -> bool {
        self.batch.staged.contains(path)
    }

    /// Whether staged changes wait for [`Indexer::flush`], and since when.
    pub fn staged_since(&self) -> Option<Instant> {
        self.batch.since
    }

    /// Rows, files and removals staged.
    pub fn staged_len(&self) -> usize {
        let b = &self.batch;
        b.rows.len() + b.records.len() + b.removed.len()
    }

    pub async fn flush(&mut self) -> Result<()> {
        let b = &mut self.batch;
        if b.paths.is_empty() && b.records.is_empty() {
            return Ok(());
        }
        let t = Instant::now();
        if b.dirty {
            self.catalog.set_meta(DIRTY, Some("1"))?;
        }
        self.store.replace(&b.paths, &b.rows).await?;
        self.catalog.remove(&b.removed)?;
        self.catalog.upsert(&b.records)?;
        self.report.write.add(t, b.rows.len());
        *b = Batch::default();
        Ok(())
    }

    /// The store changed since the last [`Indexer::finish`] (also after an interrupted run).
    pub fn is_dirty(&self) -> Result<bool> {
        Ok(self.catalog.meta(DIRTY)?.is_some())
    }

    /// Compacts, rebuilds the keyword indexes and prunes old versions if the store changed.
    /// The flag survives interrupted runs, so an unfinished index build is always completed.
    pub async fn finish(&mut self) -> Result<()> {
        if self.is_dirty()? {
            let t = Instant::now();
            self.store.finish().await?;
            self.catalog.set_meta(DIRTY, None)?;
            self.report.write.add(t, 0);
        }
        Ok(())
    }

    /// Cheaper upkeep after small writes: folds new rows into the keyword indexes, compacts
    /// and prunes old versions. Leaves a clean index like [`Indexer::finish`].
    pub async fn maintain(&mut self) -> Result<()> {
        if self.is_dirty()? {
            if self.store.has_keyword_index().await? {
                self.store.update_keyword_index().await?;
                self.store.compact().await?;
                self.store.prune().await?;
            } else {
                self.store.finish().await?;
            }
            self.catalog.set_meta(DIRTY, None)?;
        }
        Ok(())
    }

    /// Empties the index (table, then catalog) and records `identity` and the text pipeline
    /// for what follows. Other meta flags stay.
    pub async fn clear(&mut self, identity: &Identity) -> Result<()> {
        self.batch = Batch::default();
        self.catalog.set_meta(CLEARING, Some("1"))?;
        self.store = Store::reset(&self.pipeline.config.index_dir()).await?;
        self.catalog.clear_files()?;
        self.catalog.set_identity(identity)?;
        self.catalog.set_text_pipeline(TEXT_PIPELINE)?;
        self.catalog.set_meta(DIRTY, None)?;
        self.catalog.set_meta(CLEARING, None)
    }

    /// Records a file that could not be processed at all (e.g. a panic in a parser).
    pub fn record_failure(&mut self, path: &str, owner: &str, error: &str) {
        self.batch.touch(path);
        self.batch.records.push(FileRecord {
            path: path.to_owned(),
            kind: kind_of(Path::new(path)).unwrap_or(Kind::Doc),
            size: 0,
            mtime: 0,
            hash: String::new(),
            status: STATUS_ERROR.into(),
            error: Some(error.to_owned()),
            indexed_at: unix_now(),
            owner: owner.to_owned(),
        });
        self.report.failed += 1;
    }
}

#[derive(Default)]
struct Batch {
    /// Files whose rows are deleted before `rows` are added.
    paths: Vec<String>,
    rows: Vec<Row>,
    records: Vec<FileRecord>,
    /// Files that leave the catalog.
    removed: Vec<String>,
    /// Rows are added or removed. Files that keep failing only repeat a no-op delete and must
    /// not trigger the post-run compaction and index build.
    dirty: bool,
    since: Option<Instant>,
    /// Every path above.
    staged: HashSet<String>,
}

impl Batch {
    fn is_full(&self) -> bool {
        self.rows.len() >= FLUSH_ROWS
            || self.records.len() >= FLUSH_FILES
            || self.removed.len() >= FLUSH_REMOVALS
    }

    fn touch(&mut self, path: &str) {
        self.since.get_or_insert_with(Instant::now);
        self.staged.insert(path.to_owned());
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

pub fn mtime_ns(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos() as i64)
}

pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::{DIM, Embedder};

    struct Constant;

    impl Embedder for Constant {
        fn device(&self) -> Device {
            Device::Cpu
        }
        fn embed_query(&mut self, _: &str) -> Result<Vec<f32>> {
            Ok(vec![0.5; DIM])
        }
        fn embed_document(&mut self, _: &str) -> Result<Vec<f32>> {
            Ok(vec![0.5; DIM])
        }
        fn embed_image(&mut self, _: &[u8]) -> Result<Vec<f32>> {
            Ok(vec![0.5; DIM])
        }
    }

    #[tokio::test]
    async fn a_file_seen_twice_before_a_write_has_one_set_of_rows() {
        let dir = std::env::temp_dir().join(format!("endex-index-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let image = dir.join("a.png");
        image::RgbImage::new(4, 4).save(&image).unwrap();
        let mut config = Config::parse(
            "[embedding]\nlibrary = \"/l\"\nmodel = \"/m\"\ntokenizer = \"/t\"\n\
             [ocr]\nenabled = false\n",
        )
        .unwrap();
        config.data_dir = dir.join("data");
        let mut indexer = Indexer::open(&config, false).await.unwrap();
        indexer.set_auto_flush(false);
        indexer.pipeline.embedder = Some(Box::new(Constant));
        let path = image.to_str().unwrap();
        // As when a rescan queues a file again whose rows are not written yet.
        for _ in 0..2 {
            indexer.update(path, "src", &mut |_| {}).await.unwrap();
        }
        indexer.flush().await.unwrap();
        assert_eq!(indexer.store().count(Kind::Image).await.unwrap(), 1);
        assert_eq!(indexer.report.new, 1);
        fs::remove_dir_all(&dir).unwrap();
    }
}
