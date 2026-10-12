//! How the indexer follows config changes, model identity and rebuilds.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use anyhow::Result;
use endex_core::catalog::{Identity, Mismatch};
use endex_core::config::Config;
use endex_core::exclude::Filter;

use super::{META_REBUILD, META_SCAN_PENDING, Run, log};
use crate::priority;
use crate::state::lock;

impl Run {
    pub(super) fn configure(&mut self, config: Arc<Config>) {
        let old = std::mem::replace(&mut self.config, Arc::clone(&config));
        self.indexer.set_config(&config);
        self.error = None;
        let rules_changed = old.exclude != config.exclude
            || old.index.max_document_mb != config.index.max_document_mb;
        if rules_changed || old.index.sources != config.index.sources {
            match Filter::new(&config) {
                Ok(filter) => self.filter = filter,
                Err(error) => return self.fail(error),
            }
            // Staged rows may belong to a source that was just removed: write them first, so
            // that dropping its rows below catches them.
            if let Err(error) = self.flush() {
                return self.fail(error);
            }
            // Files already queued were judged by the old rules: judge them again, and forget
            // folders no source contains any more.
            self.paths.forget_owners();
            let filter = &self.filter;
            self.trees.retain(|tree| filter.source_of(tree).is_some());
            let owners: Vec<_> = config.index.sources.iter().map(|s| s.owner()).collect();
            self.unavailable.retain(|owner| owners.contains(owner));
            *lock(&self.shared.unavailable) = self.unavailable.clone();
            if let Err(error) = self.drop_foreign_files() {
                return self.fail(error);
            }
            if rules_changed {
                self.queue_all();
            } else {
                let added = config
                    .index
                    .sources
                    .iter()
                    .filter(|s| !old.index.sources.contains(s))
                    .map(|s| s.root());
                self.trees.extend(added);
            }
        }
        if old.embedding != config.embedding || old.ocr != config.ocr {
            self.indexer.release_models();
            if self.rebuilding || self.rebuild_pending {
                // Rows of the old identity must not mix with the new: start over.
                self.request_rebuild();
            } else {
                self.check_identity();
            }
        }
        if old.resources.thread_count() != config.resources.thread_count()
            || old.devices != config.devices
        {
            self.indexer.release_models();
        }
        self.apply_resources();
        self.schedule_scan();
    }

    pub(super) fn apply_resources(&mut self) {
        let resources = &self.config.resources;
        self.indexer.set_threads(Some(resources.thread_count()));
        if resources.low_priority && !self.lowered {
            match priority::lower_current_thread() {
                Ok(()) => {
                    self.lowered = true;
                    self.shared.lowered.store(true, Ordering::Relaxed);
                }
                Err(error) => log(&format!("cannot lower the indexer's priority: {error}")),
            }
        }
    }

    /// Rows of folders that are no longer sources (and of `endex index` manifests) go.
    pub(super) fn drop_foreign_files(&mut self) -> Result<()> {
        // By the configured folder, which stays the same while a source is unavailable.
        let owners: Vec<String> = self
            .config
            .index
            .sources
            .iter()
            .map(|s| s.owner().to_string_lossy().into_owned())
            .collect();
        let foreign = self.indexer.catalog().paths_not_owned_by(&owners)?;
        if !foreign.is_empty() {
            self.runtime.clone().block_on(async {
                self.indexer.remove(&foreign).await?;
                self.indexer.flush().await
            })?;
            self.last_write = Some(Instant::now());
            let foreign: std::collections::HashSet<String> = foreign.into_iter().collect();
            self.paths.retain(|p, _| !foreign.contains(p));
        }
        Ok(())
    }

    /// A changed model, prefix style or OCR setting stops indexing until a rebuild; an empty
    /// index just adopts the new identity.
    pub(super) fn check_identity(&mut self) {
        let result = (|| -> Result<Option<Mismatch>> {
            let identity = Identity::of(&self.config)?;
            let catalog = self.indexer.catalog();
            match catalog.identity_mismatch(&identity)? {
                Some(m) if catalog.file_count()? > 0 => Ok(Some(m)),
                _ => {
                    catalog.set_identity(&identity)?;
                    Ok(None)
                }
            }
        })();
        match result {
            Ok(mismatch) => {
                if mismatch.is_some() {
                    self.trees.clear();
                    self.paths.clear();
                } else if self.mismatch.is_some() {
                    // Changes were ignored while a rebuild was pending.
                    self.mismatch = None;
                    self.queue_all();
                }
                self.mismatch = mismatch;
            }
            Err(error) => self.fail(error),
        }
    }

    /// Rebuilds now, or once a pause (user, battery) ends; until then the index stays.
    pub(super) fn request_rebuild(&mut self) {
        if self.can_start() {
            if let Err(error) = self.rebuild() {
                self.fail(error);
            }
        } else {
            self.rebuild_pending = true;
        }
    }

    pub(super) fn rebuild(&mut self) -> Result<()> {
        self.rebuild_pending = false;
        self.trees.clear();
        self.paths.clear();
        self.work = None;
        let identity = Identity::of(&self.config)?;
        self.indexer.release_models();
        // Flags first: a rebuild interrupted at any point is finished on the next start.
        let catalog = self.indexer.catalog();
        catalog.set_meta(META_REBUILD, Some("1"))?;
        catalog.set_meta(META_SCAN_PENDING, Some("1"))?;
        self.runtime
            .clone()
            .block_on(self.indexer.clear(&identity))?;
        self.mismatch = None;
        self.error = None;
        self.rebuilding = true;
        self.last_write = None;
        self.queue_all();
        log("rebuilding the index");
        Ok(())
    }
}
