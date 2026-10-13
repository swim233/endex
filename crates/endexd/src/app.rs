//! The daemon's parts, how a new config reaches them, and the config file: loading it (with a
//! last good copy to fall back on), following edits by hand, keeping that copy current.

use std::fs;
use std::io::{self, Write as _};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use endex_core::config::{Config, expand_home};
use futures_util::StreamExt;
use inotify::{Inotify, WatchMask};

use crate::indexer::{Command, IndexerHandle};
use crate::settings;
use crate::state::{Shared, lock};
use crate::watch::WatcherHandle;
use crate::worker::Worker;

/// The last config that loaded, in the data dir: endexd starts from it when config.toml is
/// broken.
const LAST_GOOD: &str = "config.last-good.toml";

pub struct App {
    pub shared: Arc<Shared>,
    pub worker: Worker,
    /// `None` in read-only mode.
    pub indexer: Option<IndexerHandle>,
    pub watcher: Mutex<Option<WatcherHandle>>,
    pub config_path: PathBuf,
    /// `data_dir` and the LiteRT library in effect; changing them needs a restart.
    pub running: (PathBuf, PathBuf),
    /// The same two as the config file has them now.
    pub requested: Mutex<(PathBuf, PathBuf)>,
}

impl App {
    /// A change that only takes effect after a restart is pending: `data_dir`, the library, or
    /// turning low priority off.
    pub fn restart_required(&self) -> bool {
        let config = self.shared.config();
        *lock(&self.requested) != self.running
            || (self.shared.lowered.load(Ordering::Relaxed) && !config.resources.low_priority)
    }

    /// Re-reads the config file; a file that does not load or does not pass the checks keeps
    /// the last good config running.
    pub fn reload(&self) {
        match load_checked(&self.config_path, Some(&self.shared.config())) {
            Ok(Loaded { config, text }) => {
                *lock(&self.shared.config_error) = None;
                self.remember(&text);
                self.set_config(config);
            }
            Err(error) => {
                let message = format!("{error:#}");
                log(&format!("keeping the last good config: {message}"));
                *lock(&self.shared.config_error) = Some(message);
            }
        }
    }

    /// Hands a new config to the search thread, the indexer and the watcher.
    pub fn set_config(&self, mut config: Config) {
        let current = self.shared.config();
        config.index_root = current.index_root.clone();
        *lock(&self.requested) = (config.data_dir.clone(), config.embedding.library.clone());
        config.data_dir = self.running.0.clone();
        config.embedding.library = self.running.1.clone();
        if config == *current {
            return;
        }
        // The look is endex-ui's business: the search thread, indexer and watcher keep going.
        let look_only = Config {
            appearance: current.appearance.clone(),
            ..config.clone()
        } == *current;
        let config = Arc::new(config);
        self.shared.set_config(Arc::clone(&config));
        if look_only {
            return;
        }
        self.worker.configure((*config).clone());
        if let Some(indexer) = &self.indexer {
            indexer.send(Command::Configure(Arc::clone(&config)));
        }
        if let Some(watcher) = lock(&self.watcher).as_ref() {
            watcher.configure(config);
        }
    }

    /// Keeps `text`, the config file as it just loaded, as the last good copy: in the running
    /// data dir only (never one a new `data_dir` names), and in writer mode only.
    pub fn remember(&self, text: &str) {
        if !self.shared.writer || !self.running.0.is_dir() {
            return;
        }
        let result = (|| {
            let copy = self.running.0.join(LAST_GOOD);
            let tmp = copy.with_extension("toml.tmp");
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)?;
            writeln!(file, "{}", copy_header(&self.config_path))?;
            file.write_all(text.as_bytes())?;
            fs::rename(tmp, copy)
        })();
        if let Err(error) = result {
            log(&format!("cannot keep a copy of the config: {error}"));
        }
    }

    /// Follows edits of the config file (also replacements by editors that write a new file).
    /// If following fails, it is set up again a little later.
    pub fn follow_config_file(self: &Arc<Self>) {
        let app = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                if let Err(error) = app.follow_once().await {
                    log(&format!("following the config file: {error:#}; retrying"));
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
    }

    async fn follow_once(&self) -> Result<()> {
        let path = fs::canonicalize(&self.config_path)?;
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            return Err(anyhow!("{} has no folder", path.display()));
        };
        let name = name.to_owned();
        let inotify = Inotify::init()?;
        inotify.watches().add(
            dir,
            WatchMask::CLOSE_WRITE
                | WatchMask::MOVED_TO
                | WatchMask::CREATE
                | WatchMask::DELETE
                | WatchMask::MOVED_FROM
                | WatchMask::DELETE_SELF
                | WatchMask::MOVE_SELF,
        )?;
        let mut events = inotify.into_event_stream(vec![0u8; 4096])?;
        while let Some(event) = events.next().await {
            let event = event?;
            if event.name.is_none() {
                return Err(anyhow!("the config folder went away"));
            }
            if event.name.as_deref() != Some(name.as_os_str()) {
                continue;
            }
            // Let the write finish: editors save in several steps.
            while let Ok(Some(_)) =
                tokio::time::timeout(Duration::from_millis(300), events.next()).await
            {}
            if self.config_path.exists() {
                self.reload();
            }
        }
        Err(anyhow!("the event stream ended"))
    }
}

/// A config and the text it was parsed from.
pub struct Loaded {
    pub config: Config,
    pub text: String,
}

/// Reads, parses and checks a config file once (`previous`: the config it replaces, if any).
fn load_checked(path: &Path, previous: Option<&Config>) -> Result<Loaded> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let config = Config::parse(&text).with_context(|| format!("parsing {}", path.display()))?;
    let previous = previous.unwrap_or(&config);
    if let Some((key, message)) = settings::validate(&config, previous).into_iter().next() {
        return Err(anyhow!("{key}: {message}"));
    }
    Ok(Loaded { config, text })
}

/// Loads the config for a writer endexd. When the default config file (no `--config`) does
/// not load, the last good copy of that same file, in the data dir it names, is used and the
/// problem returned for `Status.config_error`. A config named with `--config` is never
/// replaced by another file: it loads or endexd does not start.
pub fn load_writer_config(path: &Path, explicit: bool) -> Result<(Loaded, Option<String>)> {
    let error = match load_checked(path, None) {
        Ok(loaded) => return Ok((loaded, None)),
        Err(error) => error,
    };
    if explicit {
        return Err(error);
    }
    let Some(loaded) = last_good_copy(path) else {
        return Err(error);
    };
    let message = format!("{error:#}");
    log(&format!(
        "config.toml does not load ({message}); starting from its last good copy"
    ));
    Ok((loaded, Some(message)))
}

/// The last good copy of `path`: in the data dir the file names (the default one if it names
/// none), recorded for this very file, and naming that same data dir.
fn last_good_copy(path: &Path) -> Option<Loaded> {
    let named = fs::read_to_string(path)
        .ok()
        .and_then(|text| named_data_dir(&text))
        .map(|dir| expand_home(&dir));
    let dir = named.or_else(|| dirs::data_dir().map(|dir| dir.join("endex")))?;
    let text = fs::read_to_string(dir.join(LAST_GOOD)).ok()?;
    let (header, body) = text.split_once('\n')?;
    if header != copy_header(path) {
        return None;
    }
    let config = Config::parse(body).ok()?;
    let same_dir = |a: &Path, b: &Path| fs::canonicalize(a).ok() == fs::canonicalize(b).ok();
    same_dir(&config.data_dir, &dir).then(|| Loaded {
        config,
        text: body.to_owned(),
    })
}

/// First line of a last good copy: which config file it is a copy of.
fn copy_header(path: &Path) -> String {
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    format!("# endexd: last good copy of {}", path.display())
}

/// `data_dir` of a config file that may not parse: from TOML if it does, else from its
/// top-level `data_dir = "…"` line.
fn named_data_dir(text: &str) -> Option<PathBuf> {
    if let Ok(doc) = text.parse::<toml_edit::DocumentMut>() {
        return doc.get("data_dir")?.as_str().map(PathBuf::from);
    }
    text.lines()
        .map(str::trim)
        .take_while(|line| !line.starts_with('['))
        .filter_map(|line| line.strip_prefix("data_dir"))
        .filter_map(|rest| rest.trim_start().strip_prefix('='))
        .find_map(|value| {
            let value = value.trim().strip_prefix('"')?;
            Some(PathBuf::from(&value[..value.find('"')?]))
        })
}

fn log(message: &str) {
    let _ = writeln!(io::stderr(), "endexd: {message}");
}
