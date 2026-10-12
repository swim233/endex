//! Real-time watching of the source folders (inotify). Only included folders get a watch.
//! Events are coalesced per path until writes settle, then handed to the indexer, which
//! reconciles each path with the catalog: that covers create, modify, move and delete alike.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write as _;
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use endex_core::config::{Config, ExcludeConfig, Source};
use endex_core::exclude::{Filter, Reason};
use endex_core::scan;
use futures_util::StreamExt;
use inotify::{EventMask, EventOwned, EventStream, Inotify, WatchDescriptor, WatchMask, Watches};
use tokio::sync::mpsc;

use crate::indexer::{Command, IndexerHandle};
use crate::state::{Shared, WatchState, lock};

mod coalescer;

pub use coalescer::{Change, Coalescer};

/// Quiet time after the last event of a path before it is indexed.
const SETTLE: Duration = Duration::from_millis(500);
/// A path that never settles (a growing log) is indexed this often.
const MAX_WAIT: Duration = Duration::from_secs(60);
/// How often source folders that went away are checked for coming back.
const MISSING_POLL: Duration = Duration::from_secs(30);

const MASK: WatchMask = WatchMask::CREATE
    .union(WatchMask::DELETE)
    .union(WatchMask::MODIFY)
    .union(WatchMask::CLOSE_WRITE)
    .union(WatchMask::ATTRIB)
    .union(WatchMask::MOVED_FROM)
    .union(WatchMask::MOVED_TO)
    .union(WatchMask::DELETE_SELF)
    .union(WatchMask::MOVE_SELF)
    .union(WatchMask::ONLYDIR)
    .union(WatchMask::DONT_FOLLOW)
    .union(WatchMask::EXCL_UNLINK);

pub enum Control {
    Configure(Arc<Config>),
}

#[derive(Clone)]
pub struct WatcherHandle {
    tx: mpsc::UnboundedSender<Control>,
}

impl WatcherHandle {
    pub fn configure(&self, config: Arc<Config>) {
        let _ = self.tx.send(Control::Configure(config));
    }
}

/// Sets up the watches (when enabled) and returns once they are in place, so that a scan
/// started afterwards misses no change.
pub async fn start(
    config: &Config,
    indexer: IndexerHandle,
    shared: Arc<Shared>,
) -> Result<WatcherHandle> {
    let stream = Inotify::init()?.into_event_stream(vec![0u8; 64 * 1024])?;
    // ENDEX_DEBUG_MAX_WATCHES lowers the limit for tests (the real one is a system setting).
    let debug_limit = std::env::var("ENDEX_DEBUG_MAX_WATCHES")
        .ok()
        .and_then(|s| s.parse().ok());
    let max_watches = debug_limit
        .or_else(|| {
            std::fs::read_to_string("/proc/sys/fs/inotify/max_user_watches")
                .ok()
                .and_then(|s| s.trim().parse().ok())
        })
        .unwrap_or(0);
    let mut watcher = Watcher {
        watches: stream.watches(),
        dirs: HashMap::new(),
        wds: BTreeMap::new(),
        filter: None,
        state: "off",
        overflow: false,
        coalescer: Coalescer::new(SETTLE, MAX_WAIT),
        indexer,
        shared,
        max_watches,
        debug_limit,
        applied: None,
        config: None,
        missing: BTreeSet::new(),
        parents: HashMap::new(),
        next_poll: None,
    };
    tokio::task::block_in_place(|| watcher.configure(config));
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(watcher.run(stream, rx));
    Ok(WatcherHandle { tx })
}

struct Watcher {
    watches: Watches,
    dirs: HashMap<WatchDescriptor, PathBuf>,
    wds: BTreeMap<PathBuf, WatchDescriptor>,
    /// `None` while watching is off.
    filter: Option<Filter>,
    state: &'static str,
    overflow: bool,
    coalescer: Coalescer,
    indexer: IndexerHandle,
    shared: Arc<Shared>,
    max_watches: usize,
    debug_limit: Option<usize>,
    /// The settings the watches were set up for.
    applied: Option<(Vec<Source>, ExcludeConfig, bool)>,
    config: Option<Config>,
    /// Sources (by owner) whose folder went away; checked for coming back.
    missing: BTreeSet<PathBuf>,
    /// Watches on the parents of missing source folders, to notice them coming back.
    parents: HashMap<WatchDescriptor, PathBuf>,
    next_poll: Option<tokio::time::Instant>,
}

impl Watcher {
    async fn run(
        mut self,
        mut stream: EventStream<Vec<u8>>,
        mut control: mpsc::UnboundedReceiver<Control>,
    ) {
        let shared = Arc::clone(&self.shared);
        let availability = &shared.availability_changed;
        loop {
            let deadline = self
                .coalescer
                .deadline()
                .map(tokio::time::Instant::from_std);
            tokio::select! {
                event = stream.next() => match event {
                    Some(Ok(event)) => tokio::task::block_in_place(|| self.handle(event, Instant::now())),
                    other => {
                        let _ = writeln!(
                            std::io::stderr(),
                            "endexd: watching stopped ({:?}); changes are found by scans only",
                            other.and_then(|r| r.err())
                        );
                        self.remove_all();
                        self.state = "error";
                        self.publish();
                        return;
                    }
                },
                message = control.recv() => match message {
                    Some(Control::Configure(config)) => {
                        tokio::task::block_in_place(|| self.configure(&config));
                    }
                    None => return,
                },
                _ = tokio::time::sleep_until(deadline.unwrap_or_else(tokio::time::Instant::now)), if deadline.is_some() => {
                    self.flush_due(Instant::now());
                }
                _ = tokio::time::sleep_until(self.next_poll.unwrap_or_else(tokio::time::Instant::now)), if self.next_poll.is_some() => {
                    tokio::task::block_in_place(|| self.poll_missing(Instant::now()));
                }
                _ = availability.notified() => {
                    tokio::task::block_in_place(|| self.follow_unavailable(Instant::now()));
                }
            }
            if self.overflow {
                // Events were lost: watch everything again and let a scan catch up.
                let _ = writeln!(
                    std::io::stderr(),
                    "endexd: the inotify queue overflowed; rescanning"
                );
                self.overflow = false;
                self.coalescer.clear();
                self.rewatch();
                self.indexer.send(Command::ScanAll);
            }
            self.publish();
        }
    }

    fn configure(&mut self, config: &Config) {
        let relevant = (
            config.index.sources.clone(),
            config.exclude.clone(),
            config.update.watch,
        );
        if self.applied.as_ref() == Some(&relevant) {
            return;
        }
        self.applied = Some(relevant);
        self.config = Some(config.clone());
        self.remove_all();
        self.coalescer.clear();
        self.filter = None;
        self.state = "off";
        if config.update.watch {
            match Filter::new(config) {
                Ok(filter) => {
                    self.filter = Some(filter);
                    self.state = "on";
                    self.indexer.send(Command::WatchFailed(false));
                    self.rewatch();
                }
                Err(_) => self.state = "error",
            }
        }
        self.publish();
    }

    fn rewatch(&mut self) {
        self.remove_all();
        let Some(filter) = &self.filter else {
            return;
        };
        filter.forget(None);
        let roots: Vec<(PathBuf, PathBuf)> = filter
            .sources()
            .iter()
            .map(|s| (s.root.clone(), s.owner.clone()))
            .collect();
        self.missing.clear();
        for (root, owner) in roots {
            if !root.is_dir() {
                self.miss(&root, owner);
            } else if !self.add_tree(&root) {
                return;
            }
        }
    }

    /// Notes a missing source folder: its parent is watched for it to come back, and it is
    /// checked from time to time (the parent may be missing too).
    fn miss(&mut self, root: &Path, owner: PathBuf) {
        self.missing.insert(owner);
        if let Some(parent) = root.parent()
            && let Ok(wd) = self.watches.add(
                parent,
                WatchMask::CREATE | WatchMask::MOVED_TO | WatchMask::ONLYDIR,
            )
        {
            self.parents.insert(wd, parent.to_owned());
        }
        self.next_poll
            .get_or_insert_with(|| tokio::time::Instant::now() + MISSING_POLL);
    }

    /// A source folder that is gone (deleted, moved, unmounted): unwatch what was below it
    /// (moved watches would report the wrong paths), let the indexer note it unavailable, and
    /// look for it again from time to time.
    fn lose_root(&mut self, root: PathBuf, now: Instant) {
        let owner = self
            .filter
            .as_ref()
            .and_then(|f| f.sources().iter().find(|s| s.root == root))
            .map(|s| s.owner.clone());
        let Some(owner) = owner else {
            return;
        };
        self.remove_tree(&root);
        self.miss(&root, owner);
        self.coalescer.add(root, true, now);
    }

    /// Sources the indexer found unavailable although their folder exists (the empty mount
    /// point of a drive that is not mounted): watches there would miss the mounted drive, so
    /// they are treated as missing until they have content again.
    fn follow_unavailable(&mut self, now: Instant) {
        if self.filter.is_none() {
            return;
        }
        let unavailable = lock(&self.shared.unavailable).clone();
        // The indexer found one back: watch it now rather than at the next poll.
        if self
            .missing
            .iter()
            .any(|owner| !unavailable.contains(owner))
        {
            self.poll_missing(now);
        }
        let Some(filter) = &self.filter else {
            return;
        };
        let lost: Vec<(PathBuf, PathBuf)> = filter
            .sources()
            .iter()
            .filter(|s| unavailable.contains(&s.owner) && !self.missing.contains(&s.owner))
            .map(|s| (s.root.clone(), s.owner.clone()))
            .collect();
        for (root, owner) in lost {
            self.remove_tree(&root);
            self.miss(&root, owner);
        }
    }

    fn poll_missing(&mut self, now: Instant) {
        self.next_poll = None;
        // Resolve symlinked sources again: a link's target may have come back.
        if let (Some(config), Some(_)) = (&self.config, &self.filter)
            && let Ok(filter) = Filter::new(config)
        {
            self.filter = Some(filter);
        }
        let Some(filter) = &self.filter else {
            return;
        };
        let unavailable = lock(&self.shared.unavailable).clone();
        let back: Vec<(PathBuf, PathBuf)> = filter
            .sources()
            .iter()
            .filter(|s| self.missing.contains(&s.owner) && s.root.is_dir())
            // Still the empty mount point the indexer found.
            .filter(|s| !(unavailable.contains(&s.owner) && is_empty(&s.root)))
            .map(|s| (s.root.clone(), s.owner.clone()))
            .collect();
        for (root, owner) in back {
            if !self.add_tree(&root) {
                return;
            }
            self.missing.remove(&owner);
            self.coalescer.add(root, true, now);
        }
        if self.missing.is_empty() {
            for (wd, _) in self.parents.drain() {
                let _ = self.watches.remove(wd);
            }
        } else {
            self.next_poll = Some(tokio::time::Instant::now() + MISSING_POLL);
        }
    }

    /// Watches `dir` and the included folders below it; false once the watch limit is hit.
    fn add_tree(&mut self, dir: &Path) -> bool {
        let Some(filter) = &self.filter else {
            return true;
        };
        let Some(source) = filter.source_of(dir) else {
            return true;
        };
        let walk = scan::walk(filter, source, dir, false, &|| false);
        for dir in walk.dirs {
            let added = match self.debug_limit {
                Some(limit) if self.dirs.len() >= limit => {
                    Err(std::io::Error::from_raw_os_error(libc::ENOSPC))
                }
                _ => self.watches.add(&dir, MASK),
            };
            match added {
                Ok(wd) => {
                    self.dirs.insert(wd.clone(), dir.clone());
                    self.wds.insert(dir, wd);
                }
                Err(e) if e.raw_os_error() == Some(libc::ENOSPC) => {
                    self.remove_all();
                    self.state = "limit";
                    self.filter = None;
                    self.missing.clear();
                    self.next_poll = None;
                    // What is pending still goes to the indexer (see `flush_due`); nothing
                    // new is added.
                    self.indexer.send(Command::WatchFailed(true));
                    return false;
                }
                // Gone or unreadable meanwhile.
                Err(_) => {}
            }
        }
        true
    }

    fn remove_tree(&mut self, dir: &Path) {
        let below: Vec<(PathBuf, WatchDescriptor)> = self
            .wds
            .range::<Path, _>((Bound::Included(dir), Bound::Unbounded))
            .take_while(|(p, _)| p.starts_with(dir))
            .map(|(p, wd)| (p.clone(), wd.clone()))
            .collect();
        for (path, wd) in below {
            let _ = self.watches.remove(wd.clone());
            self.dirs.remove(&wd);
            self.wds.remove(&path);
        }
    }

    fn remove_all(&mut self) {
        for wd in self.dirs.keys().chain(self.parents.keys()) {
            let _ = self.watches.remove(wd.clone());
        }
        self.dirs.clear();
        self.wds.clear();
        self.parents.clear();
    }

    fn handle(&mut self, event: EventOwned, now: Instant) {
        let mask = event.mask;
        if mask.contains(EventMask::Q_OVERFLOW) {
            self.overflow = true;
            return;
        }
        if self.parents.contains_key(&event.wd) {
            if mask.contains(EventMask::IGNORED) {
                self.parents.remove(&event.wd);
            } else {
                // A folder appeared next to where a missing source was: is it back?
                self.poll_missing(now);
            }
            return;
        }
        let Some(dir) = self.dirs.get(&event.wd).cloned() else {
            return;
        };
        let Some(filter) = &self.filter else {
            return;
        };
        let is_root = filter.sources().iter().any(|s| s.root == dir);
        if mask.contains(EventMask::IGNORED) {
            self.dirs.remove(&event.wd);
            if self.wds.get(&dir) == Some(&event.wd) {
                self.wds.remove(&dir);
            }
            if is_root {
                self.lose_root(dir, now);
            }
            return;
        }
        let Some(name) = event.name else {
            // The watched folder itself went away; its parent reports that, except for roots.
            if is_root
                && mask
                    .intersects(EventMask::DELETE_SELF | EventMask::MOVE_SELF | EventMask::UNMOUNT)
            {
                self.lose_root(dir, now);
            }
            return;
        };
        let path = dir.join(&name);
        if name == ".gitignore" || name == ".ignore" {
            if filter.uses_gitignore() {
                filter.forget(Some(&dir));
                self.remove_tree(&dir);
                if self.add_tree(&dir) {
                    self.coalescer.add(dir, true, now);
                }
            }
            return;
        }
        if mask.contains(EventMask::ISDIR) {
            if mask.intersects(EventMask::CREATE | EventMask::MOVED_TO) {
                if filter.check(&path).is_ok() && self.add_tree(&path) {
                    self.coalescer.add(path, true, now);
                }
            } else if mask.intersects(EventMask::DELETE | EventMask::MOVED_FROM) {
                self.remove_tree(&path);
                self.coalescer.add(path, true, now);
            }
            return;
        }
        self.coalescer.add(path, false, now);
    }

    /// Hands settled changes to the indexer, which judges each again. Without a filter (the
    /// watch limit was hit) they all go.
    fn flush_due(&mut self, now: Instant) {
        let (mut paths, mut trees) = (Vec::new(), Vec::new());
        for change in self.coalescer.take_due(now) {
            match change {
                Change::Tree(dir) => trees.push(dir),
                Change::Path(path) => {
                    // Excluded and unsupported files never reach the index; a file that became
                    // a symlink or a special file goes, so that its rows are removed.
                    let wanted = match self.filter.as_ref().map(|f| f.check(&path)) {
                        None | Some(Ok(_)) => true,
                        Some(Err(reason)) => matches!(reason, Reason::Symlink | Reason::Special),
                    };
                    if wanted {
                        paths.push(path);
                    }
                }
            }
        }
        if !trees.is_empty() {
            self.indexer.send(Command::Trees(trees));
        }
        if !paths.is_empty() {
            self.indexer.send(Command::Paths(paths));
        }
    }

    fn publish(&self) {
        let state = WatchState {
            state: self.state,
            watches: self.dirs.len(),
            max_watches: self.max_watches,
        };
        let mut shared = lock(&self.shared.watch);
        if *shared != state {
            *shared = state;
        }
    }
}

fn is_empty(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_none())
}
