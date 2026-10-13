//! The D-Bus interface `org.endex.Daemon1` and its dictionaries.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use endex_core::Kind;
use endex_core::catalog::{Catalog, Identity};
use endex_core::config::{Config, tilde};
use endex_core::exclude::{Filter, Reason};
use endex_core::search::{Hit, Limits, Mode, Results};
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;

use crate::app::App;
use crate::bus::{self, put};
use crate::indexer::Command;
use crate::settings;
use crate::state::lock;
use crate::usage::{Entry, UsageLog};
use crate::worker::Preview;

mod dicts;
mod error;

pub use error::Error;

use dicts::{hit_dict, index_state_dict, opt_u32, preview_dict, truncated};

pub const NAME: &str = "org.endex.Daemon1";
pub const PATH: &str = "/org/endex/Daemon";
pub const INTERFACE_VERSION: u32 = 2;

/// Enough for client-side paging: 8 documents and 9 images per page.
const DEFAULT_LIMIT_DOCS: u32 = 64;
const DEFAULT_LIMIT_IMAGES: u32 = 72;
const MAX_LIMIT: u32 = 1000;
/// Longer queries are cut: a huge paste would hold the single search thread for seconds.
pub const MAX_QUERY_CHARS: usize = 512;

type Dict = HashMap<String, OwnedValue>;

pub struct Daemon {
    app: Arc<App>,
    /// Newest `query_id` per client (unique bus name); older queries of that client are dropped.
    latest: Arc<Mutex<HashMap<String, u32>>>,
    /// Size of the index on disk, and when it was measured.
    disk: Mutex<Option<(Instant, u64)>>,
    /// One `ApplySettings` at a time: each reads, edits and writes the file.
    applying: tokio::sync::Mutex<()>,
}

impl Daemon {
    pub fn new(app: Arc<App>) -> Self {
        Self {
            app,
            latest: Arc::default(),
            disk: Mutex::default(),
            applying: tokio::sync::Mutex::new(()),
        }
    }

    /// The newest `query_id` per client, for pruning when a client leaves the bus.
    pub fn clients(&self) -> Arc<Mutex<HashMap<String, u32>>> {
        Arc::clone(&self.latest)
    }

    fn config(&self) -> Arc<Config> {
        self.app.shared.config()
    }

    /// The indexer, or `ReadOnly` in read-only mode.
    fn writer(&self) -> Result<&crate::indexer::IndexerHandle, Error> {
        self.app
            .indexer
            .as_ref()
            .ok_or_else(|| Error::ReadOnly("endexd serves this index read-only".into()))
    }

    fn enabled_usage_log(&self) -> Option<UsageLog> {
        let config = self.config();
        (self.app.shared.writer && config.log.usage)
            .then(|| UsageLog::new(&config.usage_log_path()))
    }

    /// The table and the catalog, not the rest of the data dir (models, caches, eval sets).
    fn disk_size(&self, config: &Config) -> u64 {
        let mut cached = lock(&self.disk);
        if let Some((at, size)) = *cached
            && at.elapsed() < Duration::from_secs(10)
        {
            return size;
        }
        let table: u64 = walkdir::WalkDir::new(config.index_dir())
            .into_iter()
            .flatten()
            .filter_map(|e| e.metadata().ok())
            .filter(|m| m.is_file())
            .map(|m| m.len())
            .sum();
        let catalog = config.catalog_path();
        let catalog: u64 = ["", "-wal", "-shm"]
            .iter()
            .filter_map(|suffix| fs::metadata(format!("{}{suffix}", catalog.display())).ok())
            .map(|m| m.len())
            .sum();
        let size = table + catalog;
        *cached = Some((Instant::now(), size));
        size
    }
}

/// Reply strings go through `bus::put` (dictionaries) or `bus::text` (bare strings), and error
/// text through [`Error`]: a NUL in any of them gets endexd disconnected from the bus.
#[zbus::interface(name = "org.endex.Daemon1")]
impl Daemon {
    /// Documents and images, ranked separately. `opts`: `limit_docs`, `limit_images`,
    /// `query_id` (echoed back; a newer id from the same client cancels this query).
    async fn search(
        &self,
        #[zbus(header)] header: Header<'_>,
        query: String,
        mode: String,
        opts: Dict,
    ) -> Result<(u32, Vec<Dict>, Vec<Dict>), Error> {
        let mode: Mode = mode
            .parse()
            .map_err(|e: anyhow::Error| Error::InvalidArgs(e.to_string()))?;
        let limit = |key, default| -> Result<usize, Error> {
            Ok(opt_u32(&opts, key)?.unwrap_or(default).min(MAX_LIMIT) as usize)
        };
        let limits = Limits {
            docs: limit("limit_docs", DEFAULT_LIMIT_DOCS)?,
            images: limit("limit_images", DEFAULT_LIMIT_IMAGES)?,
        };
        let query_id = opt_u32(&opts, "query_id")?.unwrap_or(0);
        let query = truncated(query, MAX_QUERY_CHARS);
        if query.trim().is_empty() {
            return Ok((query_id, Vec::new(), Vec::new()));
        }
        let client = header.sender().map(|s| s.to_string()).unwrap_or_default();
        if query_id > 0 {
            let mut latest = self.latest.lock().unwrap_or_else(|e| e.into_inner());
            let newest = latest.entry(client.clone()).or_default();
            *newest = (*newest).max(query_id);
        }
        let latest = Arc::clone(&self.latest);
        let cancelled = move || {
            query_id > 0
                && latest
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&client)
                    .is_some_and(|&newest| newest > query_id)
        };
        let Results {
            docs,
            images,
            terms,
        } = self
            .app
            .worker
            .search(query, mode, limits, Box::new(cancelled))
            .await?;
        // stat() and image headers: keep blocking file I/O off the async executor.
        let (docs, images) = tokio::task::spawn_blocking(move || {
            let dicts = |hits: Vec<Hit>| hits.into_iter().map(|h| hit_dict(h, &terms)).collect();
            (dicts(docs), dicts(images))
        })
        .await
        .map_err(|e| Error::Failed(e.to_string()))?;
        Ok((query_id, docs, images))
    }

    /// One result chunk in full, with highlights for `query`. Documents also get their
    /// position among the file's chunks and the ids of the neighbouring chunks.
    async fn preview(&self, id: String, query: String) -> Result<Dict, Error> {
        let query = truncated(query, MAX_QUERY_CHARS);
        let Some(Preview { view, terms }) = self.app.worker.preview(id.clone(), query).await?
        else {
            return Err(Error::NotFound(id));
        };
        tokio::task::spawn_blocking(move || preview_dict(view, &terms))
            .await
            .map_err(|e| Error::Failed(e.to_string()))
    }

    /// Index counts, locations and the daemon's state; see the README for all keys.
    async fn status(&self) -> Result<Dict, Error> {
        let status = self.app.worker.status().await?;
        let config = self.config();
        let shared = &self.app.shared;
        let mut d = Dict::new();
        put(&mut d, "version", env!("CARGO_PKG_VERSION"));
        put(&mut d, "model", status.model);
        put(&mut d, "query_device", config.devices.query.as_str());
        put(
            &mut d,
            "index_path",
            config.index_root().display().to_string(),
        );
        put(
            &mut d,
            "config_path",
            self.app.config_path.display().to_string(),
        );
        put(&mut d, "data_dir", config.data_dir.display().to_string());
        let counts = match status.index {
            None => {
                put(&mut d, "state", "empty");
                Default::default()
            }
            Some(Ok(counts)) => {
                put(&mut d, "state", "ready");
                counts
            }
            Some(Err(error)) => {
                put(
                    &mut d,
                    "state",
                    if status.rebuilding {
                        "rebuilding"
                    } else {
                        "error"
                    },
                );
                put(&mut d, "error", format!("{error:#}"));
                Default::default()
            }
        };
        put(&mut d, "doc_files", counts.doc_files as u32);
        put(&mut d, "image_files", counts.image_files as u32);
        put(&mut d, "doc_chunks", counts.doc_chunks as u32);
        put(&mut d, "image_chunks", counts.image_chunks as u32);
        put(&mut d, "errors", counts.errors as u32);
        put(&mut d, "last_indexed", counts.last_indexed.unwrap_or(0));

        put(
            &mut d,
            "mode",
            if shared.writer { "writer" } else { "read_only" },
        );
        put(
            &mut d,
            "index_state",
            index_state_dict(&lock(&shared.index)),
        );
        let config_error = lock(&shared.config_error).clone().unwrap_or_default();
        put(&mut d, "config_error", config_error);
        put(&mut d, "restart_required", self.app.restart_required());
        let watch = lock(&shared.watch).clone();
        put(&mut d, "watch", watch.state);
        put(&mut d, "watches", watch.watches as u32);
        put(&mut d, "max_watches", watch.max_watches as u32);
        let power = lock(&shared.power).clone();
        put(&mut d, "on_battery", power.on_battery.unwrap_or(false));
        put(&mut d, "battery_known", power.on_battery.is_some());
        put(&mut d, "battery_percent", power.percent.unwrap_or(-1.0));
        put(&mut d, "battery_override", power.battery_override);
        put(
            &mut d,
            "pause_on_battery",
            config.resources.pause_on_battery,
        );
        put(&mut d, "usage_log", config.log.usage && shared.writer);

        let catalog_path = config.catalog_path();
        let sources = config.index.sources.clone();
        let (owners, formats) = tokio::task::spawn_blocking(move || {
            let catalog = Catalog::open_read_only(&catalog_path).ok();
            let owners = catalog.as_ref().and_then(|c| c.owner_counts().ok());
            let formats = catalog.as_ref().and_then(|c| c.formats().ok());
            (owners.unwrap_or_default(), formats.unwrap_or_default())
        })
        .await
        .map_err(|e| Error::Failed(e.to_string()))?;
        let unavailable = lock(&shared.unavailable).clone();
        let sources: Vec<Dict> = sources
            .iter()
            .map(|source| {
                let owner = source.owner();
                let available = source.root().is_dir() && !unavailable.contains(&owner);
                let owner = owner.to_string_lossy();
                let counts = owners.iter().find(|o| o.owner == owner);
                let mut s = Dict::new();
                put(&mut s, "path", tilde(&source.path));
                put(&mut s, "kind", source.kind.to_string());
                put(&mut s, "files", counts.map_or(0, |c| c.files) as u32);
                put(&mut s, "errors", counts.map_or(0, |c| c.errors) as u32);
                put(&mut s, "exists", available);
                s
            })
            .collect();
        put(&mut d, "sources", sources);
        let formats: HashMap<String, u32> = formats
            .into_iter()
            .map(|(ext, n)| (ext, n as u32))
            .collect();
        put(&mut d, "formats", formats);
        let disk = tokio::task::block_in_place(|| self.disk_size(&config));
        put(&mut d, "disk_bytes", disk);
        Ok(d)
    }

    /// The settings as flat keys (see the README).
    async fn get_settings(&self) -> Dict {
        settings::get(&self.config(), &self.app.config_path)
    }

    /// Validates `changes` (flat keys as from `GetSettings`) and writes them to config.toml,
    /// keeping its comments. Reply: `applied` (b), `errors` (a{ss}: key → message, `""` for the
    /// file), `needs_rebuild` (b), `restart_required` (b). Nothing is written on errors.
    async fn apply_settings(&self, changes: Dict) -> Result<Dict, Error> {
        self.writer()?;
        let _applying = self.applying.lock().await;
        let path = self.app.config_path.clone();
        let text = fs::read_to_string(&path).map_err(|e| Error::Failed(e.to_string()))?;
        let mut d = Dict::new();
        let (text, config) = match settings::apply(&text, &changes, &self.config()) {
            Ok(applied) => applied,
            Err(errors) => {
                put(&mut d, "applied", false);
                put(
                    &mut d,
                    "errors",
                    errors.into_iter().collect::<HashMap<_, _>>(),
                );
                put(&mut d, "needs_rebuild", false);
                put(&mut d, "restart_required", false);
                return Ok(d);
            }
        };
        write_atomically(&path, &text).map_err(|e| Error::Failed(format!("{e:#}")))?;
        *lock(&self.app.shared.config_error) = None;
        self.app.remember(&text);
        self.app.set_config(config.clone());
        let catalog_path = self.config().catalog_path();
        let needs_rebuild = tokio::task::spawn_blocking(move || {
            let catalog = Catalog::open_read_only(&catalog_path).ok()?;
            let mismatch = catalog
                .identity_mismatch(&Identity::of(&config).ok()?)
                .ok()?;
            Some(mismatch.is_some() && catalog.file_count().ok()? > 0)
        })
        .await
        .ok()
        .flatten()
        .unwrap_or(false);
        put(&mut d, "applied", true);
        put(&mut d, "errors", HashMap::<String, String>::new());
        put(&mut d, "needs_rebuild", needs_rebuild);
        put(&mut d, "restart_required", self.app.restart_required());
        Ok(d)
    }

    /// Whether `path` would be indexed: `indexed` (b), `reason` (why not: outside_sources,
    /// internal, symlink, special, hidden, sensitive, builtin, rule, gitignore, unsupported,
    /// kind, too_large), `rule` (the matching pattern), `file` (the ignore file), `source`,
    /// `kind` (doc, image or folder). Never reads file contents.
    async fn check_path(&self, path: String) -> Result<Dict, Error> {
        let config = self.config();
        tokio::task::spawn_blocking(move || {
            let filter = Filter::new(&config).map_err(|e| Error::Failed(format!("{e:#}")))?;
            let path = resolve(
                &filter,
                &endex_core::config::expand_home(Path::new(path.trim())),
            );
            let mut d = Dict::new();
            match filter.check(&path) {
                Ok(included) => {
                    let too_large = included.as_ref().is_some_and(|i| {
                        i.kind == Kind::Doc
                            && fs::metadata(&path)
                                .is_ok_and(|m| m.len() > config.max_document_bytes())
                    });
                    put(&mut d, "indexed", !too_large);
                    put(&mut d, "reason", if too_large { "too_large" } else { "" });
                    if let Some(source) = filter.source_of(&path) {
                        put(&mut d, "source", tilde(&source.owner));
                    }
                    let kind = included.map_or("folder", |i| i.kind.as_str());
                    put(&mut d, "kind", kind);
                }
                Err(reason) => {
                    put(&mut d, "indexed", false);
                    put(&mut d, "reason", reason.code());
                    put(&mut d, "rule", reason.pattern().unwrap_or_default());
                    if let Reason::Ignored { file, .. } = &reason {
                        put(&mut d, "file", tilde(file));
                    }
                    if let Some(source) = filter.source_of(&path) {
                        put(&mut d, "source", tilde(&source.owner));
                    }
                }
            }
            Ok(d)
        })
        .await
        .map_err(|e| Error::Failed(e.to_string()))?
    }

    /// Files whose last attempt failed, the newest first: `path`, `error`, `time` (unix).
    async fn failed_files(&self, limit: u32) -> Result<Vec<Dict>, Error> {
        let catalog_path = self.config().catalog_path();
        tokio::task::spawn_blocking(move || {
            if !catalog_path.exists() {
                return Ok(Vec::new());
            }
            let catalog = Catalog::open_read_only(&catalog_path)?;
            let failed = catalog.failed(limit.min(10_000) as usize)?;
            Ok(failed
                .into_iter()
                .map(|r| {
                    let mut d = Dict::new();
                    put(&mut d, "path", r.path);
                    put(&mut d, "error", r.error.unwrap_or_default());
                    put(&mut d, "time", r.indexed_at);
                    d
                })
                .collect())
        })
        .await
        .map_err(|e| Error::Failed(e.to_string()))?
        .map_err(|e: anyhow::Error| Error::Failed(format!("{e:#}")))
    }

    /// Processes failed files again; all of them if `paths` is empty.
    async fn retry(&self, paths: Vec<String>) -> Result<(), Error> {
        self.writer()?.send(Command::Retry(paths));
        Ok(())
    }

    async fn pause(&self) -> Result<(), Error> {
        self.writer()?.send(Command::Pause(true));
        Ok(())
    }

    async fn resume(&self) -> Result<(), Error> {
        self.writer()?.send(Command::Pause(false));
        Ok(())
    }

    /// Index on battery anyway, until the power state changes or endexd restarts.
    async fn continue_on_battery(&self) -> Result<(), Error> {
        self.writer()?.send(Command::ContinueOnBattery);
        Ok(())
    }

    /// Compares all sources with the index now.
    async fn rescan(&self) -> Result<(), Error> {
        self.writer()?.send(Command::ScanAll);
        Ok(())
    }

    /// Clears the index and indexes all sources again. Search answers from what is indexed
    /// so far until it is done (`IndexState.rebuilding`).
    async fn rebuild(&self) -> Result<(), Error> {
        self.writer()?.send(Command::Rebuild);
        Ok(())
    }

    /// Records one popup session in the usage log (if enabled): `query` (s), `mode` (s),
    /// `results` (u), `latency_ms` (u), `picked` (s, empty if nothing was opened).
    async fn report_usage(&self, entry: Dict) -> Result<(), Error> {
        let Some(log) = self.enabled_usage_log() else {
            return Ok(());
        };
        let text = |key: &str| {
            entry
                .get(key)
                .and_then(|v| v.downcast_ref::<&str>().ok().map(str::to_owned))
                .unwrap_or_default()
        };
        let query = truncated(text("query"), MAX_QUERY_CHARS);
        if query.trim().is_empty() {
            return Err(Error::InvalidArgs("query is empty".into()));
        }
        let picked = text("picked");
        let entry = Entry {
            t: String::new(),
            q: query,
            mode: text("mode"),
            n: opt_u32(&entry, "results")?.unwrap_or(0),
            ms: opt_u32(&entry, "latency_ms")?.unwrap_or(0),
            pick: (!picked.is_empty()).then_some(picked),
        };
        tokio::task::spawn_blocking(move || log.append(entry))
            .await
            .map_err(|e| Error::Failed(e.to_string()))?
            .map_err(|e| Error::Failed(format!("{e:#}")))
    }

    /// Distinct recent queries from the usage log, the newest first.
    async fn recent_queries(&self, limit: u32) -> Vec<String> {
        let Some(log) = self.enabled_usage_log() else {
            return Vec::new();
        };
        let queries = tokio::task::spawn_blocking(move || log.recent_queries(limit as usize));
        let queries = queries.await.unwrap_or_default();
        queries.into_iter().map(bus::text).collect()
    }

    /// The last `limit` usage-log lines (JSON), oldest first.
    async fn usage_log(&self, limit: u32) -> Vec<String> {
        let config = self.config();
        if !self.app.shared.writer {
            return Vec::new();
        }
        let log = UsageLog::new(&config.usage_log_path());
        let lines = tokio::task::spawn_blocking(move || log.tail(limit as usize));
        let lines = lines.await.unwrap_or_default();
        lines.into_iter().map(bus::text).collect()
    }

    async fn clear_usage_log(&self) -> Result<(), Error> {
        self.writer()?;
        let log = UsageLog::new(&self.config().usage_log_path());
        log.clear().map_err(|e| Error::Failed(format!("{e:#}")))
    }

    #[zbus(property)]
    fn version(&self) -> u32 {
        INTERFACE_VERSION
    }

    /// What the indexer does: `state` (idle, scanning, indexing, paused, needs_rebuild,
    /// error), `reason`, `done`, `total`, `images` (queued), `device`, `bulk`, `rebuilding`,
    /// `eta` (seconds, -1 unknown), `gpu_loaded`, `rebuild_pending`. Change signals come at
    /// most about twice a second.
    #[zbus(property)]
    fn index_state(&self) -> Dict {
        index_state_dict(&lock(&self.app.shared.index))
    }
}

/// Emits `IndexState` changes, at most every 500 ms and always the latest.
pub async fn emit_index_state(
    emitter: SignalEmitter<'static>,
    daemon: zbus::object_server::InterfaceRef<Daemon>,
) {
    let shared = Arc::clone(&daemon.get().await.app.shared);
    loop {
        shared.index_changed.notified().await;
        let _ = daemon.get().await.index_state_changed(&emitter).await;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// `org.endex.DaemonDebug1`, only with `ENDEX_DEBUG_BATTERY` set: fakes the power state.
pub struct Debug {
    pub app: Arc<App>,
}

#[zbus::interface(name = "org.endex.DaemonDebug1")]
impl Debug {
    async fn set_on_battery(&self, on_battery: bool) {
        if let Some(indexer) = &self.app.indexer {
            crate::power::set(indexer, &self.app.shared, Some(on_battery), Some(62.0));
        }
    }
}

/// `path` as the filter sees files: `..` removed, a configured source folder replaced by
/// where it resolves to. Symlinks below a source stay, so that they are reported as such; a
/// path outside every source has its folder part resolved.
fn resolve(filter: &Filter, path: &Path) -> std::path::PathBuf {
    let mut normal = std::path::PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                normal.pop();
            }
            std::path::Component::CurDir => {}
            part => normal.push(part),
        }
    }
    for source in filter.sources() {
        if let Ok(rest) = normal.strip_prefix(&source.owner) {
            return source.root.join(rest);
        }
    }
    if filter.source_of(&normal).is_some() {
        return normal;
    }
    let (Some(parent), Some(name)) = (normal.parent(), normal.file_name()) else {
        return normal;
    };
    fs::canonicalize(parent).map_or_else(|_| normal.clone(), |parent| parent.join(name))
}

/// Replaces the file (or a symlink's target) in one step, keeping its permissions.
fn write_atomically(path: &Path, text: &str) -> anyhow::Result<()> {
    let target = fs::canonicalize(path)?;
    let tmp = target.with_extension("toml.endex-tmp");
    fs::write(&tmp, text)?;
    if let Ok(meta) = fs::metadata(&target) {
        fs::set_permissions(&tmp, meta.permissions())?;
    }
    fs::rename(&tmp, &target)?;
    Ok(())
}
