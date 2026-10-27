//! `calvados watch`, `calvados set …` and `calvados firmware …`: clients of
//! the running daemon's D-Bus object, for scripts and shells that can run a
//! command but can't speak D-Bus (e.g. Noctalia plugins), and for people.

use std::io::Write as _;

use futures_util::{Stream, StreamExt as _, stream};
use protocol::aacp::NoiseControlMode;
use protocol::display;
use serde_json::{Value, json};
use zbus::fdo::DBusProxy;
use zbus::proxy::CacheProperties;

use crate::dbus::{self, WireComponent, WirePeerUpdate, WireUpdate};

const USAGE: &str = "\
usage: calvados                      run the daemon
       calvados watch                print the state as JSON, one line per change
       calvados set auto-switch|auto-connect on|off|toggle
       calvados set noise anc|transparency|adaptive|next
       calvados firmware status     the firmware, the latest release, an update
       calvados firmware update     update the AirPods to the latest release
       calvados firmware cancel     stop an update";

#[zbus::proxy(
    interface = "dev.kroner.Calvados1",
    default_service = "dev.kroner.Calvados",
    default_path = "/dev/kroner/Calvados"
)]
trait Calvados {
    #[zbus(property)]
    fn connected(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn name(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn ownership_phase(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn buds(&self) -> zbus::Result<Vec<WireComponent>>;
    #[zbus(property)]
    fn noise_mode(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn set_noise_mode(&self, name: &str) -> zbus::Result<()>;
    #[zbus(property)]
    fn firmware(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn latest_firmware(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn firmware_update(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn update(&self) -> zbus::Result<WireUpdate>;
    #[zbus(property)]
    fn peer_update(&self) -> zbus::Result<WirePeerUpdate>;
    fn start_update(&self) -> zbus::Result<()>;
    fn cancel_update(&self) -> zbus::Result<()>;
    #[zbus(property)]
    fn auto_switch(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn set_auto_switch(&self, enabled: bool) -> zbus::Result<()>;
    #[zbus(property)]
    fn auto_connect(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn set_auto_connect(&self, enabled: bool) -> zbus::Result<()>;
}

pub async fn run(args: &[String]) -> anyhow::Result<()> {
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let bus = zbus::Connection::session().await?;
    // Uncached: every read asks the daemon, so nothing lags a signal.
    let proxy = CalvadosProxy::builder(&bus)
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    match args[..] {
        ["watch"] => watch(&bus, &proxy).await,
        ["set", "auto-switch", value] => {
            let enabled = flag(value, proxy.auto_switch()).await?;
            Ok(proxy.set_auto_switch(enabled).await?)
        }
        ["set", "auto-connect", value] => {
            let enabled = flag(value, proxy.auto_connect()).await?;
            Ok(proxy.set_auto_connect(enabled).await?)
        }
        ["set", "noise", "next"] => {
            let current = display::parse_mode(&proxy.noise_mode().await?);
            let next = NoiseControlMode::next(current);
            Ok(proxy.set_noise_mode(display::mode_name(Some(next))).await?)
        }
        ["set", "noise", name] => Ok(proxy.set_noise_mode(name).await?),
        ["firmware", "status"] => firmware_status(&proxy).await,
        ["firmware", "update"] => firmware_update(&bus, &proxy).await,
        ["firmware", "cancel"] => proxy.cancel_update().await.map_err(failure),
        _ => anyhow::bail!("{USAGE}"),
    }
}

/// `on`, `off`, or `toggle` (`current` is only awaited for that).
async fn flag(
    value: &str,
    current: impl Future<Output = zbus::Result<bool>>,
) -> anyhow::Result<bool> {
    match value {
        "on" => Ok(true),
        "off" => Ok(false),
        "toggle" => Ok(!current.await?),
        _ => anyhow::bail!("{USAGE}"),
    }
}

/// Whenever a property changes or the daemon starts or stops.
async fn events(bus: &zbus::Connection) -> anyhow::Result<impl Stream<Item = ()> + Unpin> {
    let changes = zbus::fdo::PropertiesProxy::builder(bus)
        .destination(dbus::NAME)?
        .path(dbus::PATH)?
        .build()
        .await?
        .receive_properties_changed()
        .await?
        .map(|_| ());
    let owners = DBusProxy::new(bus)
        .await?
        .receive_name_owner_changed_with_args(&[(0, dbus::NAME)])
        .await?
        .map(|_| ());
    Ok(stream::select(changes, owners))
}

/// The daemon's reason, as it gave it.
fn failure(e: zbus::Error) -> anyhow::Error {
    match e {
        zbus::Error::MethodError(_, Some(reason), _) => anyhow::anyhow!(reason),
        zbus::Error::FDO(e) => match *e {
            zbus::fdo::Error::Failed(reason) => anyhow::anyhow!(reason),
            e => e.into(),
        },
        e => e.into(),
    }
}

/// An update's state in words; `None` without one.
fn describe((state, percent, detail): &WireUpdate) -> Option<String> {
    Some(match state.as_str() {
        "downloading" => "downloading the firmware…".to_owned(),
        "offered" => "offering it to the AirPods…".to_owned(),
        "deferred" => "waiting for the AirPods to accept it…".to_owned(),
        "transferring" => format!("transferring, {percent} %"),
        "interrupted" => {
            format!("interrupted at {percent} %; it resumes when the AirPods are back")
        }
        "staged" => "staged: close the lid".to_owned(),
        "done" => format!("updated to {detail}"),
        "failed" => format!("failed: {detail}"),
        _ => return None,
    })
}

async fn firmware_status(proxy: &CalvadosProxy<'_>) -> anyhow::Result<()> {
    let firmware = proxy.firmware().await?;
    let latest = proxy.latest_firmware().await?;
    let known = |value: String, otherwise: &str| match value.is_empty() {
        true => otherwise.to_owned(),
        false => value,
    };
    println!(
        "Firmware: {}",
        known(firmware, "not known (AirPods connected?)")
    );
    let newer = match proxy.firmware_update().await? {
        true => ", newer",
        false => "",
    };
    println!("Latest:   {}{newer}", known(latest, "not checked yet"));
    if let Some(update) = describe(&proxy.update().await?) {
        println!("Update:   {update}");
    }
    let (host, state, percent, detail) = proxy.peer_update().await?;
    if let Some(update) = describe(&(state, percent, detail)) {
        println!("Update from {host}: {update}");
    }
    Ok(())
}

const STAGED: &str = "\
The firmware is staged on the AirPods. Now close the lid and leave the case
closed and charging for about 30 minutes: the case copies the firmware to the
other bud and installs it on both. Then open it; once the AirPods are
connected, `calvados firmware status` shows the new version.";

/// Start an update and follow it until it's staged, or fails.
async fn firmware_update(bus: &zbus::Connection, proxy: &CalvadosProxy<'_>) -> anyhow::Result<()> {
    let mut events = events(bus).await?;
    proxy.start_update().await.map_err(failure)?;
    println!(
        "Updating the AirPods' firmware: keep both AirPods in the case, the lid open\n\
         and the case charging, for about 8 minutes. Stopping this command doesn't\n\
         stop the update (`calvados firmware cancel` does)."
    );
    let mut last = None;
    loop {
        let update = proxy.update().await.map_err(failure)?;
        // Progress in steps of 5 %.
        let shown = (update.0.clone(), update.1 / 5);
        if last.as_ref() != Some(&shown) {
            if let Some(line) = describe(&update) {
                println!("{line}");
            }
            last = Some(shown);
        }
        match update.0.as_str() {
            "staged" => {
                println!("{STAGED}");
                return Ok(());
            }
            "done" => return Ok(()),
            "failed" => anyhow::bail!("the update failed: {}", update.2),
            _ => {}
        }
        if events.next().await.is_none() {
            anyhow::bail!("lost the daemon");
        }
    }
}

/// A line now, then one whenever a property changes or the daemon starts or
/// stops.
async fn watch(bus: &zbus::Connection, proxy: &CalvadosProxy<'_>) -> anyhow::Result<()> {
    let mut events = events(bus).await?;
    let mut last = None;
    loop {
        let state = snapshot(proxy).await;
        if last.as_ref() != Some(&state) {
            // The reader went away (`| head -1`): done.
            if writeln!(std::io::stdout(), "{state}").is_err() {
                return Ok(());
            }
            last = Some(state);
        }
        if events.next().await.is_none() {
            return Ok(());
        }
    }
}

/// The daemon's state; `{"running": false}` without a daemon.
async fn snapshot(proxy: &CalvadosProxy<'_>) -> Value {
    let read = async {
        zbus::Result::Ok((
            proxy.connected().await?,
            proxy.name().await?,
            proxy.ownership_phase().await?,
            proxy.buds().await?,
            proxy.noise_mode().await?,
            proxy.auto_switch().await?,
            proxy.auto_connect().await?,
            (
                proxy.firmware().await?,
                proxy.latest_firmware().await?,
                proxy.firmware_update().await?,
                proxy.update().await?,
                proxy.peer_update().await?,
            ),
        ))
    };
    let Ok((connected, name, phase, buds, noise, auto_switch, auto_connect, firmware)) = read.await
    else {
        return json!({ "running": false });
    };
    let (firmware, latest_firmware, firmware_update, update, peer_update) = firmware;
    let (state, progress, detail) = update;
    let (host, peer_state, peer_progress, peer_detail) = peer_update;
    json!({
        "running": true,
        "connected": connected,
        "name": name,
        "ownership": phase,
        "noise_mode": noise,
        "auto_switch": auto_switch,
        "auto_connect": auto_connect,
        "firmware": firmware,
        "latest_firmware": latest_firmware,
        "firmware_update": firmware_update,
        "update": { "state": state, "progress": progress, "detail": detail },
        "peer_update": { "host": host, "state": peer_state, "progress": peer_progress, "detail": peer_detail },
        "buds": buds.iter().map(|(component, level, charging, last_known, place, primary)| json!({
            "component": component,
            "level": level,
            "charging": charging,
            "last_known": last_known,
            "place": place,
            "primary": primary,
        })).collect::<Vec<_>>(),
    })
}
