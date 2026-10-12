//! endexd: serves searches over D-Bus (`org.endex.Daemon1`) and, as the only writer of its
//! index, keeps the configured folders indexed. Given `--data-dir` (or `ENDEX_DATA_DIR`) it
//! serves that index read-only instead and picks up `endex index` runs without a restart.

mod app;
mod indexer;
mod power;
mod priority;
mod service;
mod settings;
mod state;
mod usage;
mod watch;
mod worker;

use std::collections::HashMap;
use std::io::{self, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use clap::Parser;
use endex_core::config::{self, Config, ScanInterval};
use endex_core::lock::WriterLock;
use futures_util::StreamExt;
use tokio::runtime::Handle;
use tokio::signal::unix::{SignalKind, signal};

use crate::app::App;
use crate::indexer::Command;
use crate::service::{Daemon, NAME, PATH};
use crate::state::{Shared, lock};
use crate::worker::Worker;

#[derive(Parser)]
#[command(
    name = "endexd",
    version,
    about = "Endex search daemon (D-Bus org.endex.Daemon1)"
)]
struct Args {
    /// Config file [default: ~/.config/endex/config.toml, created on first run]
    #[arg(long)]
    config: Option<PathBuf>,
    /// Serve the index and catalog in this directory read-only instead of indexing the
    /// configured folders
    #[arg(long, env = "ENDEX_DATA_DIR")]
    data_dir: Option<PathBuf>,
}

/// Startup failures a restart cannot fix: config, model, the index locked by another
/// process, or the bus name already taken. The systemd unit does not restart on this code
/// (`RestartPreventExitStatus=`).
const EXIT_STARTUP: u8 = 2;

#[tokio::main]
async fn main() -> ExitCode {
    let running = match start(Args::parse()).await {
        Ok(running) => running,
        Err(error) => {
            let _ = writeln!(io::stderr(), "endexd: {error:#}");
            return ExitCode::from(EXIT_STARTUP);
        }
    };
    let code = match serve(running).await {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(io::stderr(), "endexd: {error:#}");
            1
        }
    };
    // The model runtimes (LiteRT, ONNX Runtime) do not tear down while the indexer thread may
    // still be in the middle of a file; skip their destructors. Everything written is
    // committed, and an interrupted file is redone on the next start.
    // SAFETY: ends the process without running atexit handlers; nothing is left to flush.
    unsafe { libc::_exit(code) }
}

struct Running {
    connection: zbus::Connection,
    clients: Arc<Mutex<HashMap<String, u32>>>,
    app: Arc<App>,
    _lock: Option<WriterLock>,
}

async fn start(args: Args) -> Result<Running> {
    let writer = args.data_dir.is_none();
    let explicit = args.config.is_some();
    let config_path = match args.config {
        Some(path) => path,
        None => {
            let path = config::default_path()?;
            if !path.exists() {
                Config::load()?; // writes the commented default
            }
            path
        }
    };
    // A writer survives a broken default config.toml with its last good copy.
    let (config, good_text, config_error) = if writer {
        let (loaded, error) = app::load_writer_config(&config_path, explicit)?;
        let text = error.is_none().then_some(loaded.text);
        (loaded.config, text, error)
    } else {
        (Config::from_file(&config_path)?, None, None)
    };
    let config = config.with_index_root(args.data_dir);
    let started = Instant::now();
    // Taken before anything else touches the index: a second writer must not start at all.
    let writer_lock = writer.then(|| WriterLock::acquire(&config)).transpose()?;
    let shared = Shared::new(config.clone(), writer);
    *lock(&shared.config_error) = config_error;
    let worker = Worker::start(config.clone(), Handle::current()).await?;
    let indexer = match writer {
        true => Some(
            indexer::start(
                Arc::new(config.clone()),
                Arc::clone(&shared),
                Handle::current(),
            )
            .await?,
        ),
        false => None,
    };
    let running = (config.data_dir.clone(), config.embedding.library.clone());
    let app = Arc::new(App {
        shared,
        worker,
        indexer,
        watcher: Mutex::new(None),
        config_path,
        requested: Mutex::new(running.clone()),
        running,
    });
    if let Some(text) = good_text {
        app.remember(&text);
    }
    let daemon = Daemon::new(Arc::clone(&app));
    let clients = daemon.clients();
    // Never take the name from a running endexd, and never hand it to a newer one.
    let mut builder = zbus::connection::Builder::session()?
        .allow_name_replacements(false)
        .replace_existing_names(false)
        .name(NAME)?
        .serve_at(PATH, daemon)?;
    if writer && power::fake().is_some() {
        builder = builder.serve_at(
            PATH,
            service::Debug {
                app: Arc::clone(&app),
            },
        )?;
    }
    let connection = builder.build().await.map_err(|error| match error {
        zbus::Error::NameTaken => anyhow!(
            "{NAME} is already taken: another endexd is running \
             (if it is the service: systemctl --user status endexd)"
        ),
        error => error.into(),
    })?;
    let iface = connection
        .object_server()
        .interface::<_, Daemon>(PATH)
        .await?;
    tokio::spawn(service::emit_index_state(
        iface.signal_emitter().clone(),
        iface,
    ));
    if let Some(indexer) = &app.indexer {
        tokio::spawn(power::monitor(indexer.clone(), Arc::clone(&app.shared)));
        // Watches first, so that the startup scan misses no change.
        let watcher = watch::start(&config, indexer.clone(), Arc::clone(&app.shared)).await?;
        *lock(&app.watcher) = Some(watcher);
        app.follow_config_file();
        // Hourly and daily scans are scheduled by the indexer from the last scan's time.
        if config.update.scan && config.update.scan_interval == ScanInterval::Startup {
            indexer.send(Command::ScanAll);
        }
    }
    let _ = writeln!(
        io::stderr(),
        "endexd: serving {} ({}) in {:.2}s",
        config.index_root().display(),
        if writer { "writer" } else { "read-only" },
        started.elapsed().as_secs_f64()
    );
    Ok(Running {
        connection,
        clients,
        app,
        _lock: writer_lock,
    })
}

async fn serve(running: Running) -> Result<()> {
    let bus = zbus::fdo::DBusProxy::new(&running.connection).await?;
    let mut lost = bus.receive_name_lost().await?;
    let mut owners = bus.receive_name_owner_changed().await?;
    let mut terminate = signal(SignalKind::terminate())?;
    let result = loop {
        tokio::select! {
            _ = terminate.recv() => break Ok(()),
            _ = tokio::signal::ctrl_c() => break Ok(()),
            signal = lost.next() => match signal {
                Some(signal) if signal.args().is_ok_and(|args| args.name() == NAME) => {
                    break Err(anyhow!("lost the bus name {NAME}"));
                }
                Some(_) => {}
                // The session bus is gone: nobody can reach this daemon, and it holds the
                // index lock. Exit, so that a new one can start.
                None => break Err(anyhow!("lost the connection to the session bus")),
            },
            // A client that left the bus no longer needs its newest query id.
            signal = owners.next() => match signal {
                Some(signal) => {
                    if let Ok(args) = signal.args()
                        && args.new_owner().is_none()
                    {
                        lock(&running.clients).remove(args.name().as_str());
                    }
                }
                None => break Err(anyhow!("lost the connection to the session bus")),
            },
        }
    };
    if let Some(indexer) = &running.app.indexer {
        // Writes what is staged; whatever is left is picked up by the next start.
        let indexer = indexer.clone();
        tokio::task::spawn_blocking(move || indexer.shutdown(Duration::from_secs(5))).await?;
    }
    if let Err(error) = result {
        bail!(error);
    }
    Ok(())
}
