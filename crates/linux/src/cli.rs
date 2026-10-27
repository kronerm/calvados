//! `calvados watch` and `calvados set …`: clients of the running daemon's
//! D-Bus object, for scripts and shells that can run a command but can't
//! speak D-Bus (e.g. Noctalia plugins).

use std::io::Write as _;

use futures_util::{StreamExt as _, stream};
use protocol::aacp::NoiseControlMode;
use protocol::display;
use serde_json::{Value, json};
use zbus::fdo::DBusProxy;
use zbus::proxy::CacheProperties;

use crate::dbus::{self, WireComponent};

const USAGE: &str = "\
usage: calvados                      run the daemon
       calvados watch                print the state as JSON, one line per change
       calvados set auto-switch|auto-connect on|off|toggle
       calvados set noise anc|transparency|adaptive|next";

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

/// A line now, then one whenever a property changes or the daemon starts or
/// stops.
async fn watch(bus: &zbus::Connection, proxy: &CalvadosProxy<'_>) -> anyhow::Result<()> {
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
    let mut events = stream::select(changes, owners);
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
            ),
        ))
    };
    let Ok((connected, name, phase, buds, noise, auto_switch, auto_connect, firmware)) = read.await
    else {
        return json!({ "running": false });
    };
    let (firmware, latest_firmware, firmware_update) = firmware;
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
