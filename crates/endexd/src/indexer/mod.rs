//! The indexer thread, endexd's only writer. It keeps a queue of folders to rescan and files
//! to reconcile with the catalog, works through it one file at a time at low priority, and
//! publishes its progress. Models load on first use and are released when idle.

use std::collections::BTreeSet;
use std::io::{self, Write as _};
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use endex_core::catalog::Mismatch;
use endex_core::config::Config;
use endex_core::embed::Device;
use endex_core::exclude::Filter;
use endex_core::index::{Indexer, unix_now};
use tokio::runtime::Handle;
use tokio::sync::oneshot;

use crate::state::{Phase, Shared, lock};

mod files;
mod queue;
mod reconfigure;
mod timers;

use queue::{Files, Pending, Work};

/// CPU embedding seconds a change set must be expected to take before it counts as bulk work
/// and goes to the bulk device (GPU). Loading the GPU engine takes about 4 s and it then
/// embeds 5× faster, so from 20 s of CPU work the GPU finishes clearly sooner; below that a
/// few files are not worth waking the GPU for.
const BULK_CPU_SECONDS: f64 = 20.0;
/// Staged changes are written at least this often, so search sees them while work goes on.
const FLUSH_INTERVAL: Duration = Duration::from_secs(1);
/// Bulk work writes less often: every write adds a table fragment, and many small fragments
/// slow searches down.
const BULK_FLUSH_INTERVAL: Duration = Duration::from_secs(10);
/// Staged rows, files and removals written at once at most.
const MAX_STAGED: usize = 4096;
/// Bulk writes between compactions (which also fold new rows into the keyword index).
const BULK_FLUSHES_PER_COMPACTION: usize = 30;
/// Rows searched without the keyword index before they are folded into it.
const UNINDEXED_ROWS: usize = 2000;
/// Idle time after which OCR, the tokenizer and a CPU embedder are released.
const CPU_IDLE_RELEASE: Duration = Duration::from_secs(60);
/// Idle time after the last write before the index is compacted and its keyword index updated.
const MAINTENANCE_DELAY: Duration = Duration::from_secs(120);
/// With the inotify watch limit reached, scans run at least this often instead.
const FALLBACK_SCAN: Duration = Duration::from_secs(3600);
/// How often unavailable source folders are checked for coming back.
const UNAVAILABLE_POLL: Duration = Duration::from_secs(30);

const META_SCAN_PENDING: &str = "scan_pending";
const META_REBUILD: &str = "rebuild";
const META_LAST_SCAN: &str = "last_scan";

pub enum Command {
    Configure(Arc<Config>),
    /// Rescan all sources.
    ScanAll,
    /// Rescan these folders (created, deleted or moved).
    Trees(Vec<PathBuf>),
    /// Reconcile these files.
    Paths(Vec<PathBuf>),
    /// Reprocess these failed files; all of them if empty.
    Retry(Vec<String>),
    Rebuild,
    /// Pause (true) or resume (false) on the user's request.
    Pause(bool),
    /// UPower's OnBattery; `None` if unknown.
    Power(Option<bool>),
    /// Index on battery until the power state changes.
    ContinueOnBattery,
    /// Watching failed (true) or works (false): scan periodically instead.
    WatchFailed(bool),
    Shutdown(mpsc::Sender<()>),
}

impl Command {
    /// Commands that stop a folder walk in progress.
    fn interrupts(&self) -> bool {
        matches!(
            self,
            Command::Configure(_)
                | Command::Rebuild
                | Command::Pause(true)
                | Command::Power(Some(true))
                | Command::Shutdown(_)
        )
    }
}

#[derive(Clone)]
pub struct IndexerHandle {
    tx: mpsc::Sender<Command>,
    interrupt: Arc<AtomicBool>,
}

impl IndexerHandle {
    pub fn send(&self, command: Command) {
        if command.interrupts() {
            self.interrupt.store(true, Ordering::Relaxed);
        }
        let _ = self.tx.send(command);
    }

    /// Asks the thread to write what it staged and stop; waits at most `timeout`.
    pub fn shutdown(&self, timeout: Duration) {
        let (ack, done) = mpsc::channel();
        self.send(Command::Shutdown(ack));
        let _ = done.recv_timeout(timeout);
    }
}

/// Opens the index on a new thread and returns once it is open.
pub async fn start(
    config: Arc<Config>,
    shared: Arc<Shared>,
    runtime: Handle,
) -> Result<IndexerHandle> {
    let (tx, rx) = mpsc::channel();
    let interrupt = Arc::new(AtomicBool::new(false));
    let handle = IndexerHandle {
        tx,
        interrupt: Arc::clone(&interrupt),
    };
    let (ready, opened) = oneshot::channel();
    thread::Builder::new()
        .name("indexer".into())
        .spawn(move || {
            let run = Run::open(config, shared, runtime, rx, interrupt);
            let run = match run {
                Ok(run) => {
                    let _ = ready.send(Ok(()));
                    run
                }
                Err(error) => {
                    let _ = ready.send(Err(error));
                    return;
                }
            };
            match panic::catch_unwind(AssertUnwindSafe(|| run.run())) {
                // Acknowledged once the models are released (`run` consumed them).
                Ok(Some(ack)) => {
                    let _ = ack.send(());
                }
                Ok(None) => {}
                Err(_) => {
                    log("the indexer thread panicked; exiting");
                    // As in `main`: the model runtimes' destructors can hang.
                    // SAFETY: ends the process; everything written is committed.
                    unsafe { libc::_exit(1) }
                }
            }
        })?;
    opened.await.context("indexer thread exited")??;
    Ok(handle)
}

struct Run {
    config: Arc<Config>,
    filter: Filter,
    indexer: Indexer,
    runtime: Handle,
    shared: Arc<Shared>,
    rx: mpsc::Receiver<Command>,
    interrupt: Arc<AtomicBool>,
    trees: BTreeSet<PathBuf>,
    paths: Files,
    work: Option<Work>,
    scanning: bool,
    /// The queued work includes a scan of all sources.
    full_scan: bool,
    user_paused: bool,
    on_battery: bool,
    battery_override: bool,
    mismatch: Option<Mismatch>,
    error: Option<String>,
    rebuilding: bool,
    /// `Rebuild` was asked for while indexing was paused; the index stays as it is until then.
    rebuild_pending: bool,
    lowered: bool,
    watch_failed: bool,
    last_gpu: Option<Instant>,
    last_work: Instant,
    last_write: Option<Instant>,
    next_scan: Option<Instant>,
    /// Sources (by owner) whose folder is missing or not mounted.
    unavailable: BTreeSet<PathBuf>,
    next_poll: Option<Instant>,
}

impl Run {
    fn open(
        config: Arc<Config>,
        shared: Arc<Shared>,
        runtime: Handle,
        rx: mpsc::Receiver<Command>,
        interrupt: Arc<AtomicBool>,
    ) -> Result<Self> {
        let mut indexer = runtime.block_on(Indexer::open(&config, false))?;
        // endexd keeps its own write cadence and never follows symlinks.
        indexer.set_auto_flush(false);
        indexer.set_follow_links(false);
        let filter = Filter::new(&config)?;
        let mut run = Self {
            config,
            filter,
            indexer,
            runtime,
            shared,
            rx,
            interrupt,
            trees: BTreeSet::new(),
            paths: Files::default(),
            work: None,
            scanning: false,
            full_scan: false,
            user_paused: false,
            on_battery: false,
            battery_override: false,
            mismatch: None,
            error: None,
            rebuilding: false,
            rebuild_pending: false,
            lowered: false,
            watch_failed: false,
            last_gpu: None,
            last_work: Instant::now(),
            last_write: None,
            next_scan: None,
            unavailable: BTreeSet::new(),
            next_poll: None,
        };
        run.apply_resources();
        run.check_identity();
        let catalog = run.indexer.catalog();
        run.rebuilding = catalog.meta(META_REBUILD)?.is_some();
        // An interrupted run (or rebuild) is finished by scanning everything again.
        if run.rebuilding || catalog.meta(META_SCAN_PENDING)?.is_some() {
            run.queue_all();
        }
        run.schedule_scan();
        Ok(run)
    }

    /// Work left by an earlier run, done on this thread so that startup stays quick.
    fn recover(&mut self) -> Result<()> {
        if self.indexer.is_dirty()? || self.has_queue() {
            self.begin_work()?;
            self.publish();
        }
        self.drop_foreign_files()?;
        // Completes compaction and keyword indexing an interrupted run left behind.
        self.runtime.clone().block_on(self.indexer.finish())
    }

    /// Returns the acknowledgement channel of a shutdown request.
    fn run(mut self) -> Option<mpsc::Sender<()>> {
        if let Err(error) = self.recover() {
            self.fail(error);
        }
        loop {
            while let Ok(command) = self.rx.try_recv() {
                if let Some(ack) = self.handle(command) {
                    return Some(ack);
                }
            }
            self.interrupt.store(false, Ordering::Relaxed);
            if self.rebuild_pending
                && self.can_start()
                && let Err(error) = self.rebuild()
            {
                self.fail(error);
            }
            if self.can_work() && self.has_queue() {
                if let Err(error) = self.step() {
                    self.fail(error);
                }
                continue;
            }
            if self.work.is_some() && !self.has_queue() {
                if let Err(error) = self.end_work() {
                    self.fail(error);
                }
            } else if self.indexer.staged_since().is_some()
                && let Err(error) = self.flush()
            {
                self.fail(error);
            }
            self.publish();
            let wait = self.next_timer().saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(wait.max(Duration::from_millis(10))) {
                Ok(command) => {
                    if let Some(ack) = self.handle(command) {
                        return Some(ack);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Err(error) = self.timers() {
                        self.fail(error);
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return None,
            }
        }
    }

    /// Returns the acknowledgement channel on shutdown.
    fn handle(&mut self, command: Command) -> Option<mpsc::Sender<()>> {
        match command {
            Command::Configure(config) => self.configure(config),
            Command::ScanAll => self.queue_all(),
            Command::Trees(dirs) => {
                if self.mismatch.is_none() {
                    self.trees.extend(dirs);
                }
            }
            Command::Paths(paths) => {
                if self.mismatch.is_none() {
                    for path in paths.iter().filter_map(|p| p.to_str()) {
                        self.paths.insert(path.to_owned(), Pending::unchecked());
                    }
                }
            }
            Command::Retry(paths) => {
                self.error = None;
                let paths = if paths.is_empty() {
                    let failed = self.indexer.catalog().failed(usize::MAX);
                    failed.map(|f| f.into_iter().map(|r| r.path).collect())
                } else {
                    Ok(paths)
                };
                match paths {
                    Ok(paths) => {
                        for path in paths {
                            self.paths.insert(path, Pending::unchecked());
                        }
                    }
                    Err(error) => self.fail(error),
                }
            }
            Command::Rebuild => self.request_rebuild(),
            Command::Pause(paused) => {
                self.user_paused = paused;
                if !paused {
                    self.error = None;
                }
            }
            Command::Power(on_battery) => {
                let on_battery = on_battery.unwrap_or(false);
                if on_battery != self.on_battery {
                    self.on_battery = on_battery;
                    self.battery_override = false;
                }
            }
            Command::ContinueOnBattery => self.battery_override = true,
            Command::WatchFailed(failed) => {
                // Changes since the watches went away are only found by a scan: one now, then
                // periodic ones.
                if failed && !self.watch_failed {
                    self.queue_all();
                }
                self.watch_failed = failed;
                self.schedule_scan();
            }
            Command::Shutdown(ack) => {
                if let Err(error) = self.flush() {
                    log(&format!("writing staged changes: {error:#}"));
                }
                return Some(ack);
            }
        }
        lock(&self.shared.power).battery_override = self.battery_override;
        self.publish();
        None
    }

    fn has_queue(&self) -> bool {
        !self.trees.is_empty() || !self.paths.is_empty()
    }

    fn battery_paused(&self) -> bool {
        self.config.resources.pause_on_battery && self.on_battery && !self.battery_override
    }

    /// Nothing holds indexing back but possibly a pending rebuild.
    fn can_start(&self) -> bool {
        self.error.is_none() && !self.user_paused && !self.battery_paused()
    }

    fn can_work(&self) -> bool {
        self.can_start() && self.mismatch.is_none() && !self.rebuild_pending
    }

    fn begin_work(&mut self) -> Result<()> {
        self.indexer
            .catalog()
            .set_meta(META_SCAN_PENDING, Some("1"))?;
        self.work = Some(Work {
            started: Instant::now(),
            done: 0,
            bulk: self.rebuilding,
            device: None,
            device_chosen: false,
            docs: Default::default(),
            images: Default::default(),
            flushes: 0,
        });
        Ok(())
    }

    /// One folder walk or one file.
    fn step(&mut self) -> Result<()> {
        if self.work.is_none() {
            self.begin_work()?;
        }
        if let Some(tree) = self.trees.pop_first() {
            return self.scan_tree(tree);
        }
        // Judged with the file in hand still queued, and shown as indexing while it runs.
        self.choose_device();
        self.publish();
        let Some((path, pending)) = self.paths.pop_first() else {
            return Ok(());
        };
        self.process(&path, pending)?;
        self.maybe_flush()?;
        self.publish();
        Ok(())
    }

    fn end_work(&mut self) -> Result<()> {
        self.flush()?;
        let work = self.work.take().expect("checked by the caller");
        if work.bulk {
            self.runtime.clone().block_on(self.indexer.finish())?;
            // Clean now: no upkeep due.
            self.last_write = None;
        }
        let r = std::mem::take(&mut self.indexer.report);
        if r.new + r.modified + r.removed + r.failed > 0 {
            let secs = |stage: &endex_core::index::Stage| stage.time.as_secs_f64();
            log(&format!(
                "{} new, {} modified, {} removed, {} failed in {:.1}s{}: \
                 load {:.1}s, parse {:.1}s, ocr {:.1}s, embed {:.1}s + {:.1}s, write {:.1}s",
                r.new,
                r.modified,
                r.removed,
                r.failed,
                work.started.elapsed().as_secs_f64(),
                r.device
                    .or(self.indexer.loaded())
                    .map_or(String::new(), |d| format!(" on {d}")),
                r.load.as_secs_f64(),
                secs(&r.parse),
                secs(&r.ocr),
                secs(&r.embed_text),
                secs(&r.embed_image),
                secs(&r.write),
            ));
        }
        let catalog = self.indexer.catalog();
        catalog.set_meta(META_SCAN_PENDING, None)?;
        catalog.set_meta(META_REBUILD, None)?;
        if self.full_scan {
            catalog.set_meta(META_LAST_SCAN, Some(&unix_now().to_string()))?;
            self.full_scan = false;
            self.schedule_scan();
        }
        self.rebuilding = false;
        Ok(())
    }

    fn queue_all(&mut self) {
        // A pending rebuild scans everything anyway.
        self.next_scan = None;
        if self.mismatch.is_some() {
            return;
        }
        self.filter.forget(None);
        self.trees
            .extend(self.filter.sources().iter().map(|s| s.root.clone()));
        self.full_scan = true;
    }

    fn fail(&mut self, error: anyhow::Error) {
        log(&format!("indexing stopped: {error:#}"));
        self.error = Some(format!("{error:#}"));
        self.publish();
    }

    fn publish(&self) {
        let pause = if self.user_paused {
            Some("user")
        } else if self.battery_paused() {
            Some("battery")
        } else {
            None
        };
        let (phase, reason) = if let Some(error) = &self.error {
            (Phase::Error, error.clone())
        } else if let (true, Some(pause)) = (self.rebuild_pending, pause) {
            (Phase::Paused, pause.to_owned())
        } else if let Some(m) = &self.mismatch {
            (Phase::NeedsRebuild, m.key.to_owned())
        } else if let Some(pause) = pause {
            (Phase::Paused, pause.to_owned())
        } else if self.scanning || (self.paths.is_empty() && !self.trees.is_empty()) {
            (Phase::Scanning, String::new())
        } else if self.work.is_some() {
            // Includes the final write and index upkeep after the last file.
            (Phase::Indexing, String::new())
        } else {
            (Phase::Idle, String::new())
        };
        let (done, bulk, eta) = match &self.work {
            Some(work) => {
                let images = self.paths.images;
                let eta = work.eta(self.paths.len() - images, images);
                (work.done, work.bulk, eta)
            }
            None => (0, false, None),
        };
        let device = self
            .work
            .as_ref()
            .and_then(|w| self.indexer.loaded().or(w.device));
        self.shared.update_index(|s| {
            s.phase = phase;
            s.reason = reason;
            s.done = done;
            s.total = done + self.paths.len();
            s.images = self.paths.images;
            s.device = device;
            s.bulk = bulk;
            s.rebuilding = self.rebuilding;
            s.eta_secs = eta;
            s.gpu_loaded = self.indexer.loaded() == Some(Device::Gpu);
            s.rebuild_pending = self.rebuild_pending;
        });
    }
}

fn log(message: &str) {
    let _ = writeln!(io::stderr(), "endexd: {message}");
}
