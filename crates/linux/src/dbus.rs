//! The `dev.kroner.Calvados1` session-bus object: status properties, which
//! signal `PropertiesChanged` (no polling needed), the two remembered
//! settings, and the firmware update's methods.

use std::sync::Arc;
use std::time::Duration;

use protocol::display::{self, Buds, Place, mode_name, parse_mode};
use protocol::engine::Engine;
use protocol::firmware::Release;
use protocol::settings::Setting;
use protocol::update::UpdateStatus;
use tokio::sync::{oneshot, watch};
use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::object_server::InterfaceRef;

use crate::{Daemon, Input};

pub const NAME: &str = "dev.kroner.Calvados";
pub const PATH: &str = "/dev/kroner/Calvados";

/// A `Buds` entry: component (`left`, `right`, `case`), level, charging,
/// last known, place (`in-ear`, `in-case`, or empty), primary.
pub type WireComponent = (String, u8, bool, bool, String, bool);

fn wire_buds(buds: Buds) -> Vec<WireComponent> {
    [
        ("left", buds.left),
        ("right", buds.right),
        ("case", buds.case),
    ]
    .into_iter()
    .filter_map(|(name, component)| {
        let component = component?;
        let place = match component.place {
            Some(Place::InEar) => "in-ear",
            Some(Place::InCase) => "in-case",
            None => "",
        };
        Some((
            name.to_owned(),
            component.level,
            component.charging,
            component.last_known,
            place.to_owned(),
            component.primary,
        ))
    })
    .collect()
}

/// `Update`: state (empty without one, else `downloading`, `offered`,
/// `deferred`, `transferring`, `interrupted`, `staged`, `done`, `failed`),
/// percent of the transfer, detail (the failure's reason, or the version
/// done).
pub type WireUpdate = (String, u8, String);

/// `PeerUpdate`: the updating host's address, then as [`WireUpdate`].
pub type WirePeerUpdate = (String, String, u8, String);

fn wire_update(status: Option<UpdateStatus>) -> WireUpdate {
    status.map_or_else(WireUpdate::default, |status| {
        (status.name().to_owned(), status.percent(), status.detail())
    })
}

/// What the D-Bus properties expose.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Status {
    connected: bool,
    ownership: String,
    peers: Vec<String>,
    summary: String,
    name: String,
    noise_mode: String,
    buds: Vec<WireComponent>,
    firmware: String,
    latest_firmware: String,
    firmware_update: bool,
    update: WireUpdate,
    peer_update: WirePeerUpdate,
    auto_switch: bool,
    auto_connect: bool,
}

impl Status {
    /// `latest`: the catalog's release for the pods, once checked.
    pub fn of(engine: &Engine, latest: Option<&Release>) -> Self {
        let settings = engine.settings();
        let session = match engine.is_connected() {
            true => Self::connected(engine, latest),
            false => Self {
                ownership: "not-owning".into(),
                summary: "disconnected".into(),
                ..Self::default()
            },
        };
        Self {
            // Also while the pods are away: an interrupted update waits.
            update: wire_update(engine.update_status()),
            auto_switch: settings.auto_switch,
            auto_connect: settings.auto_connect,
            ..session
        }
    }

    fn connected(engine: &Engine, latest: Option<&Release>) -> Self {
        let state = engine.state();
        let ownership = engine.ownership().to_string();
        let peers = engine
            .local_mac()
            .map(|local| {
                state
                    .peers_except(local)
                    .map(|p| p.to_hex_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let summary = std::iter::once("connected".to_string())
            .chain(state.name.clone().filter(|n| !n.is_empty()))
            .chain((!state.battery.is_empty()).then(|| state.battery.to_string()))
            .chain(state.ears_summary().map(|ears| format!("ears {ears}")))
            .chain([format!("audio: {ownership}")])
            .chain((!peers.is_empty()).then(|| format!("peers: {}", peers.join(","))))
            .collect::<Vec<_>>()
            .join(" | ");
        Self {
            connected: true,
            ownership,
            peers,
            summary,
            name: state.name.clone().unwrap_or_default(),
            noise_mode: mode_name(state.noise_mode).to_owned(),
            buds: wire_buds(Buds::of(state)),
            firmware: display::firmware(&state.firmware).unwrap_or_default(),
            latest_firmware: latest.map(Release::to_string).unwrap_or_default(),
            firmware_update: latest.is_some_and(|latest| latest.is_newer_than(&state.firmware)),
            peer_update: state
                .peer_update
                .map_or_else(WirePeerUpdate::default, |peer| {
                    let (state, percent, detail) = wire_update(Some(peer.status));
                    (
                        peer.host.to_hex_string().to_uppercase(),
                        state,
                        percent,
                        detail,
                    )
                }),
            ..Self::default()
        }
    }
}

struct CalvadosService(Arc<Daemon>);

impl CalvadosService {
    fn snapshot(&self) -> Status {
        self.0.status.borrow().clone()
    }

    /// Change a setting in the daemon's loop and wait until it's applied, so
    /// the changed signal that follows carries the new value.
    async fn set(&self, setting: Setting, enabled: bool) -> zbus::fdo::Result<()> {
        tracing::info!(?setting, enabled, "D-Bus setting");
        let mut status = self.0.status.subscribe();
        self.0
            .inputs
            .send(Input::Set(setting, enabled))
            .map_err(|_| zbus::fdo::Error::Failed("the daemon is stopping".into()))?;
        let applied = status.wait_for(|status| {
            let current = match setting {
                Setting::AutoSwitch => status.auto_switch,
                Setting::AutoConnect => status.auto_connect,
            };
            current == enabled
        });
        match tokio::time::timeout(Duration::from_secs(1), applied).await {
            Ok(Ok(_)) => Ok(()),
            _ => Err(zbus::fdo::Error::Failed(
                "the setting wasn't applied".into(),
            )),
        }
    }
}

#[zbus::interface(name = "dev.kroner.Calvados1")]
impl CalvadosService {
    #[zbus(property)]
    fn status(&self) -> String {
        self.snapshot().summary
    }

    #[zbus(property)]
    fn connected(&self) -> bool {
        self.snapshot().connected
    }

    /// `not-owning` | `claiming` | `owning`
    #[zbus(property)]
    fn ownership_phase(&self) -> String {
        self.snapshot().ownership
    }

    #[zbus(property)]
    fn peers(&self) -> Vec<String> {
        self.snapshot().peers
    }

    /// The pods' name; empty until reported.
    #[zbus(property)]
    fn name(&self) -> String {
        self.snapshot().name
    }

    /// Battery and place per component; see [`WireComponent`].
    #[zbus(property)]
    fn buds(&self) -> Vec<WireComponent> {
        self.snapshot().buds
    }

    /// `anc`, `transparency`, `adaptive` or `off`; empty while unknown.
    #[zbus(property)]
    fn noise_mode(&self) -> String {
        self.snapshot().noise_mode
    }

    /// The running firmware, e.g. `7.1`; empty until reported.
    #[zbus(property)]
    fn firmware(&self) -> String {
        self.snapshot().firmware
    }

    /// Apple's current release for the pods, e.g. `9.0.348 (9A348)`; empty
    /// until checked (daily).
    #[zbus(property)]
    fn latest_firmware(&self) -> String {
        self.snapshot().latest_firmware
    }

    /// `LatestFirmware` is newer than `Firmware`.
    #[zbus(property)]
    fn firmware_update(&self) -> bool {
        self.snapshot().firmware_update
    }

    /// This host's firmware update; see [`WireUpdate`].
    #[zbus(property)]
    fn update(&self) -> WireUpdate {
        self.snapshot().update
    }

    /// Another calvados host's, as it tells; see [`WirePeerUpdate`].
    #[zbus(property)]
    fn peer_update(&self) -> WirePeerUpdate {
        self.snapshot().peer_update
    }

    /// Update the pods to `LatestFirmware`: both buds in the open case
    /// (the pods wait for the case to charge). Fails with the reason when
    /// it can't start;
    /// `Update` follows it from there. The pods are never told to apply:
    /// the case does once the lid closes.
    async fn start_update(&self) -> zbus::fdo::Result<()> {
        tracing::info!("D-Bus: start the firmware update");
        let (reply, answer) = oneshot::channel();
        self.0
            .inputs
            .send(Input::StartUpdate(reply))
            .map_err(|_| stopping())?;
        answer
            .await
            .map_err(|_| stopping())?
            .map_err(zbus::fdo::Error::Failed)
    }

    /// Withdraw the offer; what was transferred is dropped.
    fn cancel_update(&self) -> zbus::fdo::Result<()> {
        tracing::info!("D-Bus: cancel the firmware update");
        self.0
            .inputs
            .send(Input::CancelUpdate)
            .map_err(|_| stopping())
    }

    /// Takes effect once the pods echo it back (the property changes then).
    #[zbus(property)]
    fn set_noise_mode(&self, name: String) -> zbus::fdo::Result<()> {
        let mode = parse_mode(&name)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("unknown noise mode {name:?}")))?;
        if !self.snapshot().connected {
            return Err(zbus::fdo::Error::Failed(
                "the AirPods aren't connected".into(),
            ));
        }
        self.0
            .inputs
            .send(Input::Noise(mode))
            .map_err(|_| zbus::fdo::Error::Failed("the daemon is stopping".into()))
    }

    /// Media starting on this host takes the audio. Default on, remembered.
    #[zbus(property)]
    fn auto_switch(&self) -> bool {
        self.snapshot().auto_switch
    }

    #[zbus(property)]
    async fn set_auto_switch(&self, enabled: bool) -> zbus::fdo::Result<()> {
        self.set(Setting::AutoSwitch, enabled).await
    }

    /// Connect the AirPods when they show up nearby (needs BlueZ's
    /// `Experimental = true`). Default on, remembered.
    #[zbus(property)]
    fn auto_connect(&self) -> bool {
        self.snapshot().auto_connect
    }

    #[zbus(property)]
    async fn set_auto_connect(&self, enabled: bool) -> zbus::fdo::Result<()> {
        self.set(Setting::AutoConnect, enabled).await
    }
}

fn stopping() -> zbus::fdo::Error {
    zbus::fdo::Error::Failed("the daemon is stopping".into())
}

/// Keep the returned connection alive to stay registered.
pub async fn serve(daemon: Arc<Daemon>) -> anyhow::Result<zbus::Connection> {
    let conn = zbus::connection::Builder::session()?
        .serve_at(PATH, CalvadosService(daemon.clone()))?
        .build()
        .await?;
    // Fail rather than queue behind a running instance.
    let reply = conn
        .request_name_with_flags(NAME, RequestNameFlags::DoNotQueue.into())
        .await?;
    anyhow::ensure!(
        matches!(reply, RequestNameReply::PrimaryOwner),
        "{NAME} is taken: {reply:?}"
    );
    let iface = conn
        .object_server()
        .interface::<_, CalvadosService>(PATH)
        .await?;
    tokio::spawn(signal_changes(iface, daemon.status.subscribe()));
    Ok(conn)
}

/// Emit `PropertiesChanged` for each property that differs from the last
/// published status.
async fn signal_changes(iface: InterfaceRef<CalvadosService>, mut status: watch::Receiver<Status>) {
    let mut last = status.borrow().clone();
    while status.changed().await.is_ok() {
        let now = status.borrow_and_update().clone();
        let service = iface.get().await;
        let emitter = iface.signal_emitter();
        let mut signalled = Ok(());
        if now.summary != last.summary {
            signalled = signalled.and(service.status_changed(emitter).await);
        }
        if now.connected != last.connected {
            signalled = signalled.and(service.connected_changed(emitter).await);
        }
        if now.ownership != last.ownership {
            signalled = signalled.and(service.ownership_phase_changed(emitter).await);
        }
        if now.peers != last.peers {
            signalled = signalled.and(service.peers_changed(emitter).await);
        }
        if now.name != last.name {
            signalled = signalled.and(service.name_changed(emitter).await);
        }
        if now.buds != last.buds {
            signalled = signalled.and(service.buds_changed(emitter).await);
        }
        if now.noise_mode != last.noise_mode {
            signalled = signalled.and(service.noise_mode_changed(emitter).await);
        }
        if now.firmware != last.firmware {
            signalled = signalled.and(service.firmware_changed(emitter).await);
        }
        if now.latest_firmware != last.latest_firmware {
            signalled = signalled.and(service.latest_firmware_changed(emitter).await);
        }
        if now.firmware_update != last.firmware_update {
            signalled = signalled.and(service.firmware_update_changed(emitter).await);
        }
        if now.update != last.update {
            signalled = signalled.and(service.update_changed(emitter).await);
        }
        if now.peer_update != last.peer_update {
            signalled = signalled.and(service.peer_update_changed(emitter).await);
        }
        if let Err(e) = signalled {
            tracing::warn!("D-Bus PropertiesChanged: {e}");
        }
        last = now;
    }
}
