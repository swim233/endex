//! State shared by the D-Bus service, the indexer, the watcher and the power monitor.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

use endex_core::config::Config;
use endex_core::embed::Device;
use tokio::sync::Notify;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Scanning,
    Indexing,
    Paused,
    NeedsRebuild,
    Error,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Scanning => "scanning",
            Phase::Indexing => "indexing",
            Phase::Paused => "paused",
            Phase::NeedsRebuild => "needs_rebuild",
            Phase::Error => "error",
        }
    }
}

/// What the indexer is doing; published as the `IndexState` property.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IndexState {
    pub phase: Phase,
    /// `user` or `battery` when paused, the changed setting when a rebuild is needed, the
    /// message on errors.
    pub reason: String,
    pub done: usize,
    pub total: usize,
    /// Images among the files still queued (OCR makes them the slow part).
    pub images: usize,
    /// The embedding device of the current run.
    pub device: Option<Device>,
    /// The current run is bulk work (first build, rebuild, many changes).
    pub bulk: bool,
    /// The index was cleared and is being refilled: results are partial.
    pub rebuilding: bool,
    pub eta_secs: Option<u64>,
    /// The GPU engine is loaded.
    pub gpu_loaded: bool,
    /// A requested rebuild waits for the pause (`reason`) to end; the index is untouched.
    pub rebuild_pending: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WatchState {
    /// `off`, `on`, `limit` (inotify watch limit reached; periodic scans instead) or `error`.
    pub state: &'static str,
    pub watches: usize,
    pub max_watches: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PowerState {
    /// `None` when UPower cannot be reached.
    pub on_battery: Option<bool>,
    pub percent: Option<f64>,
    /// 「本次继续」: indexing goes on despite the battery until the power state changes.
    pub battery_override: bool,
}

pub struct Shared {
    /// Writer mode: endexd owns and updates the index. Otherwise read-only, as in M2.
    pub writer: bool,
    config: RwLock<Arc<Config>>,
    /// Why the config file could not be loaded; the last good config stays in effect.
    pub config_error: Mutex<Option<String>>,
    pub index: Mutex<IndexState>,
    pub watch: Mutex<WatchState>,
    pub power: Mutex<PowerState>,
    /// Sources (by [`endex_core::config::Source::owner`]) whose folder is missing or whose
    /// drive is not mounted; their files stay indexed.
    pub unavailable: Mutex<BTreeSet<PathBuf>>,
    /// Signalled when `unavailable` changes (the watcher re-watches those folders).
    pub availability_changed: Notify,
    /// The indexer thread runs at low priority (which cannot be undone without a restart).
    pub lowered: AtomicBool,
    /// Signalled when `index` changes.
    pub index_changed: Notify,
}

impl Shared {
    pub fn new(config: Config, writer: bool) -> Arc<Self> {
        Arc::new(Self {
            writer,
            config: RwLock::new(Arc::new(config)),
            config_error: Mutex::default(),
            index: Mutex::default(),
            watch: Mutex::new(WatchState {
                state: "off",
                ..Default::default()
            }),
            power: Mutex::default(),
            unavailable: Mutex::default(),
            availability_changed: Notify::new(),
            lowered: AtomicBool::new(false),
            index_changed: Notify::new(),
        })
    }

    pub fn config(&self) -> Arc<Config> {
        Arc::clone(&self.config.read().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn set_config(&self, config: Arc<Config>) {
        *self.config.write().unwrap_or_else(|e| e.into_inner()) = config;
    }

    /// Changes the index state and wakes the property emitter if it differs.
    pub fn update_index(&self, change: impl FnOnce(&mut IndexState)) {
        let mut state = lock(&self.index);
        let before = state.clone();
        change(&mut state);
        if *state != before {
            drop(state);
            self.index_changed.notify_one();
        }
    }
}

/// Locks, ignoring poisoning: the state stays usable after a panic elsewhere.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}
