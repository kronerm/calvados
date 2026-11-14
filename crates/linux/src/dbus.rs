//! The `dev.kroner.Calvados1` session-bus object: status properties, which
//! signal `PropertiesChanged` (no polling needed), the two remembered
//! settings, the press-and-hold cycle, and the firmware update's methods.

use std::sync::Arc;
use std::time::Duration;

use protocol::display::{self, Buds, Place, cycle_names, mode_name, parse_cycle, parse_mode};
use protocol::engine::Engine;
use protocol::firmware::Release;
use protocol::pod_settings::PodSetting;
use protocol::settings::Setting;
use protocol::update::{Goal, UpdateStatus};
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
    name: String,
    noise_mode: String,
    cycle: Vec<String>,
    buds: Vec<WireComponent>,
    firmware: String,
    latest_firmware: String,
    firmware_update: bool,
    update: WireUpdate,
    peer_update: WirePeerUpdate,
    auto_switch: bool,
    auto_connect: bool,
    /// The pods' settings as reported: name → value.
    pods: Vec<(String, String)>,
    /// Their advertisement's signal, dBm; 0 when not heard lately.
    signal: i16,
}

impl Status {
    /// `latest`: the catalog's release for the pods, once checked.
    pub fn of(engine: &Engine, latest: Option<&Release>) -> Self {
        let settings = engine.settings();
        let session = match engine.is_connected() {
            true => Self::connected(engine, latest),
            false => Self {
                ownership: "not-owning".into(),
                ..Self::default()
            },
        };
        Self {
            signal: engine.signal(std::time::Instant::now()).unwrap_or(0),
            // Also while the pods are away: an interrupted update waits.
            update: wire_update(engine.update_status()),
            auto_switch: settings.auto_switch,
            auto_connect: settings.auto_connect,
            ..session
        }
    }

    fn connected(engine: &Engine, latest: Option<&Release>) -> Self {
        let state = engine.state();
        Self {
            connected: true,
            ownership: engine.ownership().to_string(),
            name: state.name.clone().unwrap_or_default(),
            noise_mode: mode_name(state.noise_mode).to_owned(),
            cycle: cycle_names(state.cycle)
                .into_iter()
                .map(str::to_owned)
                .collect(),
            buds: wire_buds(Buds::of(state)),
            pods: PodSetting::ALL
                .into_iter()
                .filter_map(|setting| {
                    let raw = state.pods.get(setting)?;
                    Some((setting.name().to_owned(), setting.text(raw)?))
                })
                .collect(),
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

    fn ensure_connected(&self) -> zbus::fdo::Result<()> {
        match self.0.status.borrow().connected {
            true => Ok(()),
            false => Err(zbus::fdo::Error::Failed(
                "the AirPods aren't connected".into(),
            )),
        }
    }

    /// Change a setting in the daemon's loop and wait until it's applied, so
    /// the changed signal that follows carries the new value.
    async fn set(&self, setting: Setting, enabled: bool) -> zbus::fdo::Result<()> {
        tracing::info!(?setting, enabled, "D-Bus setting");
        self.apply(Input::Set(setting, enabled), |status| {
            let current = match setting {
                Setting::AutoSwitch => status.auto_switch,
                Setting::AutoConnect => status.auto_connect,
            };
            current == enabled
        })
        .await
    }

    /// Send `input` to the loop and wait until the status shows it.
    async fn apply(
        &self,
        input: Input,
        applied: impl Fn(&Status) -> bool,
    ) -> zbus::fdo::Result<()> {
        let mut status = self.0.status.subscribe();
        self.0.inputs.send(input).map_err(|_| stopping())?;
        match tokio::time::timeout(Duration::from_secs(1), status.wait_for(applied)).await {
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
    fn connected(&self) -> bool {
        self.snapshot().connected
    }

    /// The pods' own settings as they report them, setting → value
    /// (`conversation-awareness` → `on`, `mic` → `auto`, …; see
    /// `protocol::pod_settings`). A setting shows once reported.
    #[zbus(property)]
    fn pods(&self) -> std::collections::HashMap<String, String> {
        self.snapshot().pods.into_iter().collect()
    }

    /// Set one of `Pods`: `value` as it shows (`on`, `off`, a level 0-100,
    /// a choice's name), or `toggle` for the next one. Done once the pods
    /// report it.
    async fn set_pod(&self, name: String, value: String) -> zbus::fdo::Result<()> {
        let setting = PodSetting::from_name(&name)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("unknown setting {name:?}")))?;
        self.ensure_connected()?;
        let shown = |status: &Status| {
            status
                .pods
                .iter()
                .find(|(each, _)| *each == name)
                .map(|(_, value)| value.clone())
        };
        let raw = match value.as_str() {
            "toggle" => {
                let current = shown(&self.snapshot())
                    .and_then(|text| setting.parse(&text))
                    .ok_or_else(|| {
                        zbus::fdo::Error::Failed(format!("{name} isn't reported yet"))
                    })?;
                setting.next(current)
            }
            text => setting.parse(text).ok_or_else(|| {
                zbus::fdo::Error::InvalidArgs(format!("{value:?} isn't a value of {name}"))
            })?,
        };
        let wanted = setting.text(raw);
        tracing::info!(name, value, "D-Bus: set a pods' setting");
        self.apply(Input::Pod(setting, raw), |status| shown(status) == wanted)
            .await
    }

    /// Our AirPods' Bluetooth signal in dBm, from their advertisement
    /// (about -40 right by them, -90 at the edge); 0 when not heard for 10 s.
    /// For finding them: poll it (it changes with every advertisement, so it
    /// sends no `PropertiesChanged`).
    #[zbus(property)]
    fn signal(&self) -> i16 {
        self.snapshot().signal
    }

    /// Play a loud chime on the AirPods to find them; not while one is in an
    /// ear. The audio comes here for it and goes back after.
    async fn chime(&self) -> zbus::fdo::Result<()> {
        tracing::info!("D-Bus: chime");
        let (reply, answer) = oneshot::channel();
        self.0
            .inputs
            .send(Input::Chime(reply))
            .map_err(|_| stopping())?;
        answer
            .await
            .map_err(|_| stopping())?
            .map_err(zbus::fdo::Error::Failed)
    }

    /// Rename the AirPods (1-32 bytes); `Name` shows it once they report it.
    async fn rename(&self, name: String) -> zbus::fdo::Result<()> {
        let name = name.trim().to_owned();
        if name.is_empty() || name.len() > 32 {
            return Err(zbus::fdo::Error::InvalidArgs(
                "a name of 1 to 32 bytes".into(),
            ));
        }
        self.ensure_connected()?;
        tracing::info!(name, "D-Bus: rename the pods");
        self.0
            .inputs
            .send(Input::Rename(name))
            .map_err(|_| stopping())
    }

    /// `not-owning` | `claiming` | `owning`
    #[zbus(property)]
    fn ownership_phase(&self) -> String {
        self.snapshot().ownership
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
            .send(Input::StartUpdate(Goal::Install, reply))
            .map_err(|_| stopping())?;
        answer
            .await
            .map_err(|_| stopping())?
            .map_err(zbus::fdo::Error::Failed)
    }

    /// Offer `LatestFirmware` only to hold the pods across a lid close, so
    /// the case tunnels through the primary and is asked its version. Never
    /// staged; withdrawn once the case answers, or after 3 minutes. Needs a
    /// release newer than what the buds run, like `StartUpdate`.
    async fn try_probe_case(&self) -> zbus::fdo::Result<()> {
        tracing::info!("D-Bus: probe the case");
        let (reply, answer) = oneshot::channel();
        self.0
            .inputs
            .send(Input::StartUpdate(Goal::ProbeCase, reply))
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
        self.ensure_connected()?;
        self.0
            .inputs
            .send(Input::Noise(mode))
            .map_err(|_| stopping())
    }

    /// The modes press and hold cycles through: `anc`, `transparency`,
    /// `adaptive` (`off` too if the pods report it). Empty until set from
    /// here: the pods don't report it. Remembered per pods.
    #[zbus(property)]
    fn cycle(&self) -> Vec<String> {
        self.snapshot().cycle
    }

    /// Two modes at least, of `anc`, `transparency` and `adaptive`.
    #[zbus(property)]
    async fn set_cycle(&self, names: Vec<String>) -> zbus::fdo::Result<()> {
        let cycle = parse_cycle(names.iter().map(String::as_str))
            .filter(|_| !names.iter().any(|name| name == "off"))
            .ok_or_else(|| {
                zbus::fdo::Error::InvalidArgs(format!(
                    "{names:?}: two at least of anc, transparency and adaptive"
                ))
            })?;
        self.ensure_connected()?;
        tracing::info!(?cycle, "D-Bus: set the press-and-hold cycle");
        let names = cycle_names(Some(cycle));
        self.apply(Input::Cycle(cycle), |status| status.cycle == names)
            .await
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

/// `PropertiesChanged` for each listed property whose field differs.
macro_rules! signal_changed {
    ($service:expr, $emitter:expr, $now:expr, $last:expr, [$($field:ident: $signal:ident),* $(,)?]) => {{
        let mut signalled = Ok(());
        $(
            if $now.$field != $last.$field {
                signalled = signalled.and($service.$signal($emitter).await);
            }
        )*
        signalled
    }};
}

/// Emit `PropertiesChanged` for each property that differs from the last
/// published status. The writable ones too: the cycle also changes from
/// other hosts (zbus' setters signal a write once more).
async fn signal_changes(iface: InterfaceRef<CalvadosService>, mut status: watch::Receiver<Status>) {
    let mut last = status.borrow().clone();
    while status.changed().await.is_ok() {
        let now = status.borrow_and_update().clone();
        let service = iface.get().await;
        let emitter = iface.signal_emitter();
        let signalled = signal_changed!(service, emitter, now, last, [
            connected: connected_changed,
            ownership: ownership_phase_changed,
            name: name_changed,
            buds: buds_changed,
            noise_mode: noise_mode_changed,
            cycle: cycle_changed,
            firmware: firmware_changed,
            latest_firmware: latest_firmware_changed,
            firmware_update: firmware_update_changed,
            update: update_changed,
            peer_update: peer_update_changed,
            auto_switch: auto_switch_changed,
            auto_connect: auto_connect_changed,
            pods: pods_changed,
        ]);
        if let Err(e) = signalled {
            tracing::warn!("D-Bus PropertiesChanged: {e}");
        }
        last = now;
    }
}
