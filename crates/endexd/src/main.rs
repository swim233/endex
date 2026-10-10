//! endexd: serves searches over D-Bus (`org.endex.Daemon1`) from the index that `endex index`
//! builds. Read-only; picks up new index runs without a restart.

mod service;
mod worker;

use std::collections::HashMap;
use std::io::{self, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Result, anyhow, bail};
use clap::Parser;
use endex_core::config::{self, Config};
use futures_util::StreamExt;
use tokio::runtime::Handle;
use tokio::signal::unix::{SignalKind, signal};

use crate::service::{Daemon, NAME, PATH};
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
    /// Serve the index and catalog in this directory instead of the configured data_dir
    #[arg(long, env = "ENDEX_DATA_DIR")]
    data_dir: Option<PathBuf>,
}

/// Startup failures a restart cannot fix: config, model, or the bus name already taken.
/// The systemd unit does not restart on this code (`RestartPreventExitStatus=`).
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
    match serve(running).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(io::stderr(), "endexd: {error:#}");
            ExitCode::FAILURE
        }
    }
}

struct Running {
    connection: zbus::Connection,
    clients: Arc<Mutex<HashMap<String, u32>>>,
}

async fn start(args: Args) -> Result<Running> {
    let (config, config_path) = match args.config {
        Some(path) => (Config::from_file(&path)?, path),
        None => (Config::load()?, config::default_path()?),
    };
    let config = config.with_index_root(args.data_dir);
    let started = Instant::now();
    let index_path = config.index_root().to_owned();
    let worker = Worker::start(config, Handle::current()).await?;
    let daemon = Daemon::new(worker, index_path.clone(), config_path);
    let clients = daemon.clients();
    // Never take the name from a running endexd, and never hand it to a newer one.
    let connection = zbus::connection::Builder::session()?
        .allow_name_replacements(false)
        .replace_existing_names(false)
        .name(NAME)?
        .serve_at(PATH, daemon)?
        .build()
        .await
        .map_err(|error| match error {
            zbus::Error::NameTaken => anyhow!(
                "{NAME} is already taken: another endexd is running \
                 (if it is the service: systemctl --user status endexd)"
            ),
            error => error.into(),
        })?;
    let _ = writeln!(
        io::stderr(),
        "endexd: serving {} in {:.2}s",
        index_path.display(),
        started.elapsed().as_secs_f64()
    );
    Ok(Running {
        connection,
        clients,
    })
}

async fn serve(running: Running) -> Result<()> {
    let bus = zbus::fdo::DBusProxy::new(&running.connection).await?;
    let mut lost = bus.receive_name_lost().await?;
    let mut owners = bus.receive_name_owner_changed().await?;
    let mut terminate = signal(SignalKind::terminate())?;
    loop {
        tokio::select! {
            _ = terminate.recv() => return Ok(()),
            _ = tokio::signal::ctrl_c() => return Ok(()),
            Some(signal) = lost.next() => {
                if signal.args().is_ok_and(|args| args.name() == NAME) {
                    bail!("lost the bus name {NAME}");
                }
            }
            // A client that left the bus no longer needs its newest query id.
            Some(signal) = owners.next() => {
                if let Ok(args) = signal.args()
                    && args.new_owner().is_none()
                {
                    running
                        .clients
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(args.name().as_str());
                }
            }
        }
    }
}
