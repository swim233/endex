//! Timers of the indexer: releasing idle models, index upkeep and periodic scans.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use endex_core::embed::Device;
use endex_core::exclude::Filter;
use endex_core::index::unix_now;

use super::{
    CPU_IDLE_RELEASE, FALLBACK_SCAN, MAINTENANCE_DELAY, META_LAST_SCAN, Run, UNAVAILABLE_POLL, log,
};
use crate::priority;

impl Run {
    pub(super) fn timers(&mut self) -> Result<()> {
        let now = Instant::now();
        let gpu_minutes = self.config.resources.gpu_idle_release_min;
        if self.indexer.loaded() == Some(Device::Gpu)
            && gpu_minutes > 0
            && self
                .last_gpu
                .is_none_or(|t| now >= t + minutes(gpu_minutes))
        {
            self.indexer.release(true);
            priority::trim_heap();
            log("released the GPU engine");
        }
        if self.indexer.cpu_models_loaded() && now >= self.last_work + CPU_IDLE_RELEASE {
            self.indexer.release(false);
            priority::trim_heap();
        }
        if self.can_work()
            && self
                .last_write
                .is_some_and(|t| now >= t + MAINTENANCE_DELAY)
        {
            if self.indexer.is_dirty()? {
                self.runtime.clone().block_on(self.indexer.maintain())?;
            }
            self.last_write = None;
        }
        if self.next_scan.is_some_and(|t| now >= t) {
            self.queue_all();
        }
        if self.next_poll.is_some_and(|t| now >= t) {
            self.poll_unavailable();
        }
        self.publish();
        Ok(())
    }

    /// Only deadlines that lead to work, so that an idle indexer sleeps.
    pub(super) fn next_timer(&self) -> Instant {
        let mut next = Instant::now() + Duration::from_secs(3600);
        let gpu_minutes = self.config.resources.gpu_idle_release_min;
        if self.indexer.loaded() == Some(Device::Gpu) && gpu_minutes > 0 {
            next = next.min(self.last_gpu.unwrap_or(self.last_work) + minutes(gpu_minutes));
        }
        if self.indexer.cpu_models_loaded() {
            next = next.min(self.last_work + CPU_IDLE_RELEASE);
        }
        if let Some(t) = self.last_write
            && self.can_work()
        {
            next = next.min(t + MAINTENANCE_DELAY);
        }
        for t in [self.next_scan, self.next_poll].into_iter().flatten() {
            next = next.min(t);
        }
        next
    }

    /// Rescans unavailable source folders that came back (resolving symlinked ones again).
    fn poll_unavailable(&mut self) {
        self.next_poll = None;
        if self.unavailable.is_empty() {
            return;
        }
        if let Ok(filter) = Filter::new(&self.config) {
            self.filter = filter;
        }
        let back: Vec<PathBuf> = self
            .filter
            .sources()
            .iter()
            .filter(|s| self.unavailable.contains(&s.owner))
            .filter(|s| self.root_available(&s.root, &s.owner).unwrap_or(false))
            .map(|s| s.root.clone())
            .collect();
        if self.mismatch.is_none() {
            self.trees.extend(back);
        }
        self.next_poll = Some(Instant::now() + UNAVAILABLE_POLL);
    }

    /// The next periodic scan: the configured interval, and at least hourly while the inotify
    /// watch limit stops watching.
    pub(super) fn schedule_scan(&mut self) {
        let update = &self.config.update;
        let mut period = if update.scan {
            update.scan_interval.period()
        } else {
            None
        };
        if self.watch_failed {
            period = Some(period.map_or(FALLBACK_SCAN, |p| p.min(FALLBACK_SCAN)));
        }
        let last = self
            .indexer
            .catalog()
            .meta(META_LAST_SCAN)
            .ok()
            .flatten()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);
        self.next_scan = period.map(|p| {
            let since = Duration::from_secs((unix_now() - last).max(0) as u64);
            Instant::now() + p.saturating_sub(since)
        });
    }
}

fn minutes(n: u32) -> Duration {
    Duration::from_secs(u64::from(n) * 60)
}
