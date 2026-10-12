//! Folder scans, file reconciles, the device choice and the write cadence.

use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use anyhow::Result;
use endex_core::Kind;
use endex_core::embed::Device;
use endex_core::index::Event;
use endex_core::scan;

use super::queue::is_image;
use super::{
    BULK_CPU_SECONDS, BULK_FLUSH_INTERVAL, BULK_FLUSHES_PER_COMPACTION, FLUSH_INTERVAL, MAX_STAGED,
    Pending, Run, UNAVAILABLE_POLL, UNINDEXED_ROWS, log,
};
use crate::state::lock;

impl Run {
    /// Compares a folder with the catalog and queues what changed. Rows below an unavailable
    /// source folder stay.
    pub(super) fn scan_tree(&mut self, tree: PathBuf) -> Result<()> {
        // Outside every source (a removed source, a folder moved out): those rows went with the
        // source, file by file, or stay with an unavailable one.
        let Some(source) = self.filter.source_of(&tree) else {
            return Ok(());
        };
        let (root, owner) = (source.root.clone(), source.owner.clone());
        if !self.root_available(&root, &owner)? {
            self.set_available(&owner, false);
            return Ok(());
        }
        self.scanning = true;
        self.publish();
        // Ignore files may have changed (the watcher asks for a rescan when they do).
        self.filter.forget(Some(&tree));
        let interrupt = Arc::clone(&self.interrupt);
        let stop = move || interrupt.load(Ordering::Relaxed);
        let source = self.filter.source_of(&tree).expect("checked above");
        let walk = scan::walk(&self.filter, source, &tree, true, &stop);
        self.scanning = false;
        if walk.stopped {
            self.trees.insert(tree);
            return Ok(());
        }
        if walk.unreadable.contains(&root) {
            self.set_available(&owner, false);
            return Ok(());
        }
        self.set_available(&owner, true);
        let catalog = self.indexer.catalog();
        let mut removed = Vec::new();
        if tree == root {
            if !walk.files.is_empty()
                && let Ok(meta) = fs::metadata(&root)
            {
                catalog.set_meta(
                    &meta_key("source_dev", &owner),
                    Some(&meta.dev().to_string()),
                )?;
            }
            // A symlinked source that now points elsewhere: rows from its old target go.
            let root_key = meta_key("source_root", &owner);
            if catalog
                .meta(&root_key)?
                .is_some_and(|old| Path::new(&old) != root)
            {
                removed = catalog
                    .paths_of_owner(&owner.to_string_lossy())?
                    .into_iter()
                    .filter(|path| !Path::new(path).starts_with(&root))
                    .collect();
            }
            catalog.set_meta(&root_key, Some(&root.to_string_lossy()))?;
        }
        let known = catalog.under(&tree)?;
        let diff = scan::diff(&walk.files, &known, &walk.unreadable);
        removed.extend(diff.removed);
        self.runtime
            .clone()
            .block_on(self.indexer.remove(&removed))?;
        for path in diff.changed {
            let size = walk.files.get(&path).map(|s| s.size);
            let owner = Some(owner.clone());
            self.paths.insert(path, Pending { owner, size });
        }
        self.maybe_flush()
    }

    /// A source folder is unavailable when it is missing or not a folder, and when it is
    /// empty but has indexed files while it is a mount point or on another device than when it
    /// last had files: the mount point of a drive that is not mounted.
    pub(super) fn root_available(&self, root: &Path, owner: &Path) -> Result<bool> {
        let Ok(meta) = fs::symlink_metadata(root) else {
            return Ok(false);
        };
        if !meta.is_dir() {
            return Ok(false);
        }
        let empty = fs::read_dir(root).is_ok_and(|mut entries| entries.next().is_none());
        if !empty {
            return Ok(true);
        }
        let catalog = self.indexer.catalog();
        let recorded = catalog
            .meta(&meta_key("source_dev", owner))?
            .and_then(|dev| dev.parse::<u64>().ok());
        let parent = root
            .parent()
            .and_then(|p| fs::metadata(p).ok())
            .map(|m| m.dev());
        let elsewhere = recorded.is_some_and(|dev| dev != meta.dev())
            || parent.is_some_and(|dev| dev != meta.dev());
        Ok(!(elsewhere && catalog.has_files_under(root)?))
    }

    pub(super) fn set_available(&mut self, owner: &Path, available: bool) {
        let changed = if available {
            self.unavailable.remove(owner)
        } else {
            self.unavailable.insert(owner.to_owned())
        };
        if changed {
            *lock(&self.shared.unavailable) = self.unavailable.clone();
            self.shared.availability_changed.notify_one();
            log(&format!(
                "source folder {} is {}",
                owner.display(),
                if available {
                    "back"
                } else {
                    "unavailable; its files stay indexed"
                }
            ));
        }
        if !self.unavailable.is_empty() && self.next_poll.is_none() {
            self.next_poll = Some(Instant::now() + UNAVAILABLE_POLL);
        }
    }

    /// One queued file, with the bookkeeping for the ETA and idle release.
    pub(super) fn process(&mut self, path: &str, pending: Pending) -> Result<()> {
        let started = Instant::now();
        self.reconcile(path, pending.owner)?;
        self.last_work = Instant::now();
        if self.indexer.loaded() == Some(Device::Gpu) {
            self.last_gpu = Some(self.last_work);
        }
        let work = self.work.as_mut().expect("begun by step");
        work.done += 1;
        let kind = if is_image(path) {
            &mut work.images
        } else {
            &mut work.docs
        };
        kind.0 += started.elapsed();
        kind.1 += 1;
        Ok(())
    }

    /// Indexes, updates or removes one file, whatever happened to it. A file that is gone is
    /// removed only while its source folder is available.
    fn reconcile(&mut self, path: &str, owner: Option<PathBuf>) -> Result<()> {
        // Seen again before its last changes were written (a rescan, the watcher): write them
        // here, with the usual upkeep, so that the catalog is current.
        if self.indexer.is_staged(path) {
            self.flush()?;
        }
        let meta = fs::symlink_metadata(path);
        // Only these mean the file is gone; any other error (a folder made unreadable, a disk
        // error) leaves the file as it is.
        let gone = match &meta {
            Ok(_) => false,
            Err(e) if e.kind() == io::ErrorKind::NotFound => true,
            Err(e) if e.raw_os_error() == Some(libc::ENOTDIR) => true,
            Err(_) => return Ok(()),
        };
        let regular = meta.as_ref().is_ok_and(|m| m.is_file());
        let owner = match owner {
            // A file from a scan with the current rules; anything but a regular file (a
            // symlink put in its place, say) is judged again.
            Some(owner) if regular => Some(owner),
            _ if gone => None,
            _ => match self.filter.check(Path::new(path)) {
                Ok(Some(included)) if regular => Some(included.owner),
                Ok(None) => {
                    self.trees.insert(path.into());
                    return Ok(());
                }
                _ => None,
            },
        };
        let Some(owner) = owner else {
            if gone && let Some(source) = self.filter.source_of(Path::new(path)) {
                let (root, owner) = (source.root.clone(), source.owner.clone());
                if !self.root_available(&root, &owner)? {
                    self.set_available(&owner, false);
                    return Ok(());
                }
            }
            if self.indexer.catalog().get(path)?.is_some() {
                self.runtime
                    .clone()
                    .block_on(self.indexer.remove(&[path.to_owned()]))?;
            }
            return Ok(());
        };
        let owner = owner.to_string_lossy();
        let mut on = |event: Event| match event {
            Event::Failed { path, error } => log(&format!("failed: {path}: {error:#}")),
            Event::GpuFallback(error) => log(&format!(
                "GPU unavailable ({error:#}); embedding on CPU instead"
            )),
            Event::Loaded { what, took } => {
                log(&format!("loaded {what} in {:.2}s", took.as_secs_f64()))
            }
            _ => {}
        };
        let indexer = &mut self.indexer;
        let runtime = &self.runtime;
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            runtime.block_on(indexer.update(path, &owner, &mut on))
        }));
        match result {
            Ok(result) => result,
            Err(_) => {
                // Models may be in a bad state after a panic: start over with fresh ones.
                self.indexer.release_models();
                log(&format!("failed: {path}: internal error (panic)"));
                self.indexer
                    .record_failure(path, &owner, "内部错误：处理这个文件时程序出错");
                Ok(())
            }
        }
    }

    /// Picks the embedding device at the start of a change set and when it grows.
    pub(super) fn choose_device(&mut self) {
        let Some(work) = &self.work else {
            return;
        };
        if work.device_chosen {
            return;
        }
        let devices = &self.config.devices;
        let bulk = work.bulk || self.paths.cpu_seconds() >= BULK_CPU_SECONDS;
        // A warm bulk engine is used for small changes too until it is released.
        let device = if bulk || self.indexer.loaded() == Some(devices.bulk) {
            devices.bulk
        } else {
            devices.incremental
        };
        self.indexer.set_device(device);
        let work = self.work.as_mut().expect("checked above");
        work.bulk = bulk;
        work.device = Some(device);
        work.device_chosen = true;
    }

    /// Writes staged changes once they are old enough (1 s, or 10 s in bulk work) or many;
    /// bulk work is compacted every few writes.
    pub(super) fn maybe_flush(&mut self) -> Result<()> {
        let bulk = self.work.as_ref().is_some_and(|w| w.bulk);
        let interval = if bulk {
            BULK_FLUSH_INTERVAL
        } else {
            FLUSH_INTERVAL
        };
        let due = self
            .indexer
            .staged_since()
            .is_some_and(|t| t.elapsed() >= interval)
            || self.indexer.staged_len() >= MAX_STAGED;
        if !due {
            return Ok(());
        }
        self.flush()?;
        let Some(work) = &mut self.work else {
            return Ok(());
        };
        if !work.bulk {
            // A change set that grew while running on the incremental device is re-judged.
            work.device_chosen = false;
            return Ok(());
        }
        work.flushes += 1;
        if work.flushes >= BULK_FLUSHES_PER_COMPACTION {
            work.flushes = 0;
            self.runtime.clone().block_on(async {
                let store = self.indexer.store();
                store.compact().await?;
                store.update_keyword_index().await
            })?;
        }
        Ok(())
    }

    /// Writes staged changes. A new index gets its keyword index right away; later rows are
    /// searched without it until there are enough to fold in.
    pub(super) fn flush(&mut self) -> Result<()> {
        if self.indexer.staged_since().is_none() {
            return Ok(());
        }
        self.runtime.clone().block_on(async {
            self.indexer.flush().await?;
            let store = self.indexer.store();
            if !store.has_keyword_index().await? {
                if store.count(Kind::Doc).await? + store.count(Kind::Image).await? > 0 {
                    store.build_keyword_index().await?;
                }
            } else if store.unindexed_rows().await? >= UNINDEXED_ROWS {
                store.update_keyword_index().await?;
            }
            anyhow::Ok(())
        })?;
        self.last_write = Some(Instant::now());
        Ok(())
    }
}

/// Catalog meta keys per source: `source_dev` (the device its folder was on when it last had
/// files) and `source_root` (the folder it resolved to at its last scan).
fn meta_key(what: &str, owner: &Path) -> String {
    format!("{what}:{}", owner.display())
}
