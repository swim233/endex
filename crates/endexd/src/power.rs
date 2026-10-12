//! Power state from UPower on the system bus (read-only: property reads and change signals),
//! forwarded to the indexer. `ENDEX_DEBUG_BATTERY=ac|battery` fakes it for tests instead and
//! enables `org.endex.DaemonDebug1.SetOnBattery`.

use std::io::{self, Write as _};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use futures_util::StreamExt;

use crate::indexer::{Command, IndexerHandle};
use crate::state::{Shared, lock};

pub const DEBUG_ENV: &str = "ENDEX_DEBUG_BATTERY";

/// The faked state, if the debug override is `ac` or `battery` (any other value is ignored).
pub fn fake() -> Option<bool> {
    match std::env::var(DEBUG_ENV).ok()?.as_str() {
        "battery" => Some(true),
        "ac" => Some(false),
        _ => None,
    }
}

pub async fn monitor(indexer: IndexerHandle, shared: Arc<Shared>) {
    if let Some(on_battery) = fake() {
        set(&indexer, &shared, Some(on_battery), Some(62.0));
        return;
    }
    // UPower may be missing, start late or restart: follow it again after a pause, keeping the
    // last known state meanwhile (as if on AC power if there never was one).
    let mut failing = false;
    loop {
        match follow(&indexer, &shared).await {
            Ok(()) => log("UPower went away; following it again"),
            Err(error) if !failing => log(&format!(
                "no power state from UPower ({error:#}); retrying every minute"
            )),
            Err(_) => {}
        }
        failing = true;
        if lock(&shared.power).on_battery.is_none() {
            set(&indexer, &shared, None, None);
        }
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

async fn follow(indexer: &IndexerHandle, shared: &Shared) -> Result<()> {
    let bus = zbus::Connection::system().await?;
    let service = "org.freedesktop.UPower";
    let upower = zbus::Proxy::new(&bus, service, "/org/freedesktop/UPower", service).await?;
    let display = zbus::Proxy::new(
        &bus,
        service,
        "/org/freedesktop/UPower/devices/DisplayDevice",
        "org.freedesktop.UPower.Device",
    )
    .await?;
    let mut on_battery_changed = upower.receive_property_changed::<bool>("OnBattery").await;
    let mut percent_changed = display.receive_property_changed::<f64>("Percentage").await;
    let mut on_battery = upower.get_property::<bool>("OnBattery").await?;
    let mut percent = display.get_property::<f64>("Percentage").await.ok();
    log("following the power state (UPower)");
    loop {
        set(indexer, shared, Some(on_battery), percent);
        tokio::select! {
            Some(change) = on_battery_changed.next() => on_battery = change.get().await?,
            Some(change) = percent_changed.next() => percent = change.get().await.ok(),
            else => return Ok(()),
        }
    }
}

pub fn set(
    indexer: &IndexerHandle,
    shared: &Shared,
    on_battery: Option<bool>,
    percent: Option<f64>,
) {
    let mut power = lock(&shared.power);
    let changed = power.on_battery != on_battery;
    power.on_battery = on_battery;
    power.percent = percent;
    drop(power);
    if changed {
        indexer.send(Command::Power(on_battery));
    }
}

fn log(message: &str) {
    let _ = writeln!(io::stderr(), "endexd: {message}");
}
