//! The indexer's queue of files and the bookkeeping of the current stretch of work.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use endex_core::Kind;
use endex_core::embed::Device;
use endex_core::index::kind_of;

/// CPU embedding cost per document chunk and per image (measured in M1).
const CPU_SECONDS_PER_CHUNK: f64 = 0.15;
const CPU_SECONDS_PER_IMAGE: f64 = 0.72;
/// About 480 tokens of text.
const BYTES_PER_CHUNK: u64 = 2000;

/// A file waiting to be reconciled.
pub(super) struct Pending {
    /// The source's owner, when a scan with the current rules found the file included.
    pub(super) owner: Option<PathBuf>,
    /// Size from the scan, for the bulk estimate.
    pub(super) size: Option<u64>,
}

impl Pending {
    /// A file to judge with the filter first (from the watcher, a retry, a changed rule).
    pub(super) fn unchecked() -> Self {
        Self {
            owner: None,
            size: None,
        }
    }
}

/// Files waiting to be reconciled; counts images apart, for the ETA.
#[derive(Default)]
pub(super) struct Files {
    map: BTreeMap<String, Pending>,
    pub(super) images: usize,
}

impl Files {
    pub(super) fn insert(&mut self, path: String, pending: Pending) {
        let image = is_image(&path);
        if self.map.insert(path, pending).is_none() && image {
            self.images += 1;
        }
    }

    pub(super) fn pop_first(&mut self) -> Option<(String, Pending)> {
        let (path, pending) = self.map.pop_first()?;
        if is_image(&path) {
            self.images -= 1;
        }
        Some((path, pending))
    }

    pub(super) fn retain(&mut self, keep: impl FnMut(&String, &mut Pending) -> bool) {
        self.map.retain(keep);
        self.images = self.map.keys().filter(|p| is_image(p)).count();
    }

    /// Makes every queued file pass the filter again (after a rule or source change).
    pub(super) fn forget_owners(&mut self) {
        for pending in self.map.values_mut() {
            pending.owner = None;
        }
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(super) fn len(&self) -> usize {
        self.map.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Expected CPU embedding seconds for the queued files (the first 20,000).
    pub(super) fn cpu_seconds(&self) -> f64 {
        self.map
            .iter()
            .take(20_000)
            .map(|(path, pending)| match kind_of(Path::new(path)) {
                Some(Kind::Image) => CPU_SECONDS_PER_IMAGE,
                Some(Kind::Doc) => {
                    let size = pending
                        .size
                        .or_else(|| fs::metadata(path).ok().map(|m| m.len()))
                        .unwrap_or(0);
                    (size / BYTES_PER_CHUNK + 1) as f64 * CPU_SECONDS_PER_CHUNK
                }
                None => 0.0,
            })
            .sum()
    }
}

pub(super) fn is_image(path: &str) -> bool {
    kind_of(Path::new(path)) == Some(Kind::Image)
}

/// The current stretch of work, from the first queued item until the queue is empty.
pub(super) struct Work {
    pub(super) started: Instant,
    pub(super) done: usize,
    pub(super) bulk: bool,
    /// The embedding device chosen for this work (shown before a model is loaded).
    pub(super) device: Option<Device>,
    pub(super) device_chosen: bool,
    /// Time spent and files done, for documents and images.
    pub(super) docs: (Duration, usize),
    pub(super) images: (Duration, usize),
    /// Bulk writes since the last compaction.
    pub(super) flushes: usize,
}

impl Work {
    /// Seconds left, from the time per document and per image so far.
    pub(super) fn eta(&self, docs_left: usize, images_left: usize) -> Option<u64> {
        let (time, n) = (self.docs.0 + self.images.0, self.docs.1 + self.images.1);
        let overall = (n >= 3).then(|| time.as_secs_f64() / n as f64);
        let rate = |(time, n): (Duration, usize)| {
            (n >= 3).then(|| time.as_secs_f64() / n as f64).or(overall)
        };
        let doc_rate = if docs_left > 0 { rate(self.docs)? } else { 0.0 };
        let image_rate = if images_left > 0 {
            rate(self.images)?
        } else {
            0.0
        };
        Some((docs_left as f64 * doc_rate + images_left as f64 * image_rate) as u64)
    }
}
