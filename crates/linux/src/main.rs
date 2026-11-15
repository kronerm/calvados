//! Calvados Linux daemon: a passive AACP connection; media starting here
//! takes the audio.
//! See AGENTS.md for the model.

mod bluez;
mod cli;
mod dbus;
mod firmware;
mod hci;
mod mpris;
mod pipewire;
mod presence;
mod state;

use std::io::IsTerminal as _;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use protocol::MacAddress;
use protocol::aacp::{Cycle, NoiseControlMode, Packet};
use protocol::connection::AIRPODS_PSM;
use protocol::engine::{Effect, Engine, LocalAudio};
use protocol::firmware::Release;
use protocol::pod_settings::PodSetting;
use protocol::protocol::parse_datagram;
use protocol::settings::{Setting, Settings};
use protocol::superbinary::Package;
use protocol::update::Goal;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::{mpsc, oneshot, watch};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt as _, util::SubscriberInitExt as _};

use crate::bluez::Link;
use crate::dbus::Status;
use crate::pipewire::Profile;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Pause before re-attaching, so a flapping channel can't spin.
const RECONNECT_DELAY: Duration = Duration::from_secs(3);
/// The pods send crash logs in frames of 1002 bytes (2565 with the larger
/// chunk) and give up on a channel too small for them; the kernel's default
/// is 672.
const RECV_MTU: u16 = 4096;
/// Retry while no AirPods are paired (connects are signalled, not polled).
const PAIRING_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Process-wide context, shared with the D-Bus object and the presence
/// monitor; everything they change goes through [`Input`]s to the loop.
struct Daemon {
    adapter: bluer::Adapter,
    inputs: mpsc::UnboundedSender<Input>,
    /// Published to D-Bus; only changes are signalled.
    status: watch::Sender<Status>,
    /// `None` without a session bus: no automatic pause/resume.
    media: Option<mpris::Media>,
    /// The pods' PipeWire profile: `off` unless we hold the audio.
    audio: pipewire::Audio,
}

/// What the D-Bus object and the presence monitor tell the loop.
#[derive(Debug)]
enum Input {
    Set(Setting, bool),
    /// Takes effect once the pods echo it back.
    Noise(NoiseControlMode),
    /// The press-and-hold cycle.
    Cycle(Cycle),
    /// An Apple device advertising nearby, with its manufacturer data (from
    /// the type byte).
    Nearby(bluer::Address, Vec<u8>, Option<i16>),
    /// A firmware catalog check for these models (a product's): its
    /// current release, `None` when it failed.
    Catalog(Vec<String>, Option<Release>),
    /// Update the pods to the catalog's release; refused with a reason.
    StartUpdate(Goal, oneshot::Sender<Result<(), String>>),
    CancelUpdate,
    /// One of the pods' settings, as the byte to send.
    Pod(PodSetting, u8),
    Rename(String),
    /// Start finding the pods (or extend it), or stop.
    Find(bool),
    /// The release's package for the update, `None` when it couldn't be had.
    Package(Option<Package>),
}

impl Daemon {
    fn publish(&self, status: Status) {
        self.status.send_if_modified(|current| {
            let changed = *current != status;
            *current = status;
            changed
        });
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match std::env::args().skip(1).collect::<Vec<_>>()[..] {
        [] => daemon().await,
        ref args => cli::run(args).await,
    }
}

async fn daemon() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            format!(
                "{}=info,protocol=info,firmware_fetch=info",
                env!("CARGO_CRATE_NAME")
            )
            .into()
        }))
        // No colour codes in the journal.
        .with(tracing_subscriber::fmt::layer().with_ansi(std::io::stdout().is_terminal()))
        .init();

    let session = bluer::Session::new().await?;
    let adapter = session.default_adapter().await?;

    let local_mac = adapter
        .address()
        .await
        .map(|address| MacAddress::new(address.0))
        .inspect(|mac| tracing::info!(%mac, "local adapter"))
        .inspect_err(|e| tracing::warn!("local adapter MAC unknown, smart routing disabled: {e}"))
        .ok();

    if !bluez::apple_device_id_configured() {
        tracing::warn!(
            "no Apple DeviceID in /etc/bluetooth/main.conf: the AirPods won't keep \
             multipoint with this host (see README)"
        );
    }

    hci::spawn();

    let media = mpris::Media::spawn()
        .await
        .inspect_err(|e| tracing::warn!("MPRIS unavailable, no automatic pause: {e}"))
        .ok();
    let engine = Engine::new(local_mac, Settings::load(state::load));
    let (inputs, mut received) = mpsc::unbounded_channel();
    let daemon = Arc::new(Daemon {
        adapter,
        inputs,
        status: watch::Sender::new(Status::of(&engine, None)),
        media,
        audio: pipewire::Audio::spawn(),
    });
    // Also the single-instance guard: two daemons would fight over AACP.
    let _dbus = dbus::serve(daemon.clone())
        .await
        .context("claiming dev.kroner.Calvados on the session bus (already running?)")?;
    presence::spawn(daemon.clone());

    tokio::select! {
        () = run(&daemon, engine, &mut received) => {}
        () = shutdown_signal() => tracing::info!("shutdown signal received, exiting"),
    }
    Ok(())
}

async fn shutdown_signal() {
    let Ok(mut term) = signal(SignalKind::terminate()) else {
        return;
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

/// One AACP session.
struct Session {
    device: bluer::Device,
    stream: bluer::l2cap::SeqPacket,
    buf: Vec<u8>,
    /// On an existing link the card was left alone: settle it once the pods
    /// say who's playing.
    unsettled: bool,
}

/// The AACP connection: being attached, or up.
enum Aacp<'a> {
    /// Kept across iterations: cancelling it could cut an L2CAP connect.
    Attaching(Pin<Box<dyn Future<Output = anyhow::Result<Session>> + 'a>>),
    Up(Session),
}

/// What the connection brought.
enum Arrived {
    Attached(MacAddress),
    Packets(Vec<Packet>),
}

impl<'a> Aacp<'a> {
    fn attach(daemon: &'a Daemon, after: Duration) -> Self {
        Self::Attaching(Box::pin(attach(daemon, after)))
    }

    fn session(&self) -> Option<&Session> {
        match self {
            Self::Up(session) => Some(session),
            Self::Attaching(_) => None,
        }
    }

    fn session_mut(&mut self) -> Option<&mut Session> {
        match self {
            Self::Up(session) => Some(session),
            Self::Attaching(_) => None,
        }
    }

    /// The session once attached, then its datagrams' packets; an error
    /// is to be followed by [`Self::down`].
    async fn next(&mut self) -> anyhow::Result<Arrived> {
        match self {
            Self::Attaching(attaching) => {
                let session = attaching.await?;
                let pods = MacAddress::new(session.device.address().0);
                *self = Self::Up(session);
                Ok(Arrived::Attached(pods))
            }
            Self::Up(session) => receive(session).await.map(Arrived::Packets),
        }
    }

    /// Attaching failed or the session ended: attach again after a pause.
    fn down(&mut self, daemon: &'a Daemon, engine: &mut Engine, reason: &anyhow::Error) {
        match self {
            Self::Attaching(_) => tracing::info!("attaching failed: {reason:#}"),
            Self::Up(_) => {
                tracing::info!("AACP connection ended: {reason:#}");
                engine.disconnected(Instant::now());
            }
        }
        *self = Self::attach(daemon, RECONNECT_DELAY);
    }
}

/// The daemon: attach whenever BlueZ reports the pods connected (never
/// paging them), and drive the engine with everything that happens.
async fn run(daemon: &Daemon, mut engine: Engine, inputs: &mut mpsc::UnboundedReceiver<Input>) {
    let mut aacp = Aacp::attach(daemon, Duration::ZERO);
    let mut playback = daemon.media.as_ref().map(mpris::Media::playback);
    let mut catalog = firmware::Catalog::new();
    loop {
        let model = engine.state().firmware.model.as_deref();
        if let Some(model) = model {
            catalog.check(model, &daemon.inputs, Instant::now());
        }
        daemon.publish(Status::of(
            &engine,
            model.and_then(|model| catalog.latest(model)),
        ));
        let wake = engine.next_wake().map(tokio::time::Instant::from_std);
        let effects = tokio::select! {
            arrived = aacp.next() => match arrived {
                Ok(Arrived::Attached(pods)) => engine.connected(pods, Instant::now()),
                Ok(Arrived::Packets(packets)) => {
                    let now = Instant::now();
                    packets
                        .into_iter()
                        .inspect(|packet| tracing::debug!(?packet, "rx"))
                        .flat_map(|packet| engine.handle(packet, now))
                        .collect()
                }
                Err(e) => {
                    aacp.down(daemon, &mut engine, &e);
                    Vec::new()
                }
            },
            () = sleep_until(wake) => engine.tick(Instant::now()),
            Some(input) = inputs.recv() => match input {
                Input::Set(setting, enabled) => {
                    engine.set(setting, enabled);
                    state::save(setting, enabled);
                    Vec::new()
                }
                Input::Noise(mode) => engine.set_noise_mode(mode),
                Input::Cycle(cycle) => engine.set_cycle(cycle),
                Input::Nearby(address, advertisement, rssi) => engine.nearby(
                    address.0,
                    &advertisement,
                    rssi,
                    presence::stored_keys(),
                    Instant::now(),
                ),
                Input::Catalog(models, release) => {
                    let known = release.clone().map(|release| engine.release_known(release));
                    catalog.found(models, release);
                    known.unwrap_or_default()
                }
                Input::StartUpdate(goal, reply) => {
                    let release = engine
                        .state()
                        .firmware
                        .model
                        .as_deref()
                        .and_then(|model| catalog.latest(model))
                        .cloned();
                    let prepared = engine.prepare_update(release.clone(), goal);
                    let (answer, effects) = match (prepared, release) {
                        (Ok(effects), Some(release)) => {
                            catalog.fetch_package(release, &daemon.inputs);
                            (Ok(()), effects)
                        }
                        (Ok(effects), None) => (Ok(()), effects),
                        (Err(refusal), _) => {
                            tracing::info!(%refusal, "firmware update refused");
                            (Err(refusal.to_string()), Vec::new())
                        }
                    };
                    // The caller may have given up waiting.
                    let _ = reply.send(answer);
                    effects
                }
                Input::CancelUpdate => engine.cancel_update(),
                Input::Pod(setting, raw) => engine.set_pod(setting, raw),
                Input::Rename(name) => engine.rename(&name),
                Input::Find(true) => engine.find(Instant::now()),
                Input::Find(false) => engine.stop_finding(),
                Input::Package(package) => engine.package(package, Instant::now()),
            },
            Some(playing) = playback_change(&mut playback) => {
                let audio = match playing {
                    true => LocalAudio::Media,
                    false => LocalAudio::Silent,
                };
                engine.local_audio(audio, Instant::now())
            }
        };
        for effect in effects {
            if let Err(e) = perform(daemon, aacp.session(), effect).await {
                aacp.down(daemon, &mut engine, &e);
                break;
            }
        }
        if let Some(session) = aacp.session_mut().filter(|session| session.unsettled)
            && engine.state().audio_source.is_some()
        {
            session.unsettled = false;
            if !engine.ownership().is_held() {
                tracing::info!("another device has the audio: card off");
                daemon.audio.set(session.device.address(), Profile::Off);
            }
        }
    }
}

/// Never, without a deadline.
async fn sleep_until(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

/// The next datagram's packets.
async fn receive(session: &mut Session) -> anyhow::Result<Vec<Packet>> {
    match session.stream.recv(&mut session.buf).await? {
        0 => anyhow::bail!("AirPods closed the connection"),
        len => Ok(parse_datagram(&session.buf[..len])),
    }
}

/// Perform one engine [`Effect`]; an error ends the session.
async fn perform(daemon: &Daemon, session: Option<&Session>, effect: Effect) -> anyhow::Result<()> {
    let pods = session.map(|session| session.device.address());
    match effect {
        Effect::Send(bytes) => {
            if let Some(session) = session {
                session.stream.send(&bytes).await?;
            }
        }
        Effect::RouteAudio => {
            if let Some(session) = session {
                bluez::activate_a2dp(&session.device);
                daemon.audio.set(session.device.address(), Profile::A2dp);
            }
        }
        // Like macOS: pause rather than fall back to the speakers.
        Effect::ReleaseAudio => {
            pods.into_iter()
                .for_each(|pods| daemon.audio.set(pods, Profile::Off));
            daemon.media.iter().for_each(mpris::Media::pause);
        }
        Effect::Reconnect => anyhow::bail!("the pods never answered the handshake"),
        Effect::ConnectPods(pods) => match daemon.adapter.device(bluer::Address(pods.to_array())) {
            Ok(device) => bluez::connect(device),
            Err(e) => tracing::warn!(%pods, "connecting the pods: {e}"),
        },
        Effect::RestartStream => pods
            .into_iter()
            .for_each(|pods| daemon.audio.restart_stream(pods)),
        // BlueZ always knows our MAC.
        // BlueZ's monitor reports every advertisement's signal anyway.
        Effect::LearnedLocalMac(_) | Effect::FindScan(_) => {}
        Effect::LearnedKey { pods, kind, key } => presence::save_key(pods, kind, key),
        Effect::Pause => daemon.media.iter().for_each(mpris::Media::pause),
        Effect::Resume => daemon.media.iter().for_each(mpris::Media::resume),
        Effect::CrashLog { pods, bud, data } => state::save_crash_log(pods, bud, &data),
        Effect::Duck | Effect::Unduck => pods
            .into_iter()
            .for_each(|pods| daemon.audio.duck(pods, matches!(effect, Effect::Duck))),
    }
    Ok(())
}

/// Wait `after`, then for the pods to be connected, and open AACP over that
/// link.
async fn attach(daemon: &Daemon, after: Duration) -> anyhow::Result<Session> {
    tokio::time::sleep(after).await;
    let (device, link) = connected_airpods(&daemon.adapter).await;
    // Don't stream until we take the audio: connecting alone mustn't pull
    // it away from the phone. Needs no AACP. An existing link may be ours
    // and playing: the session decides once it knows the source.
    if matches!(link, Link::Fresh) {
        daemon.audio.set(device.address(), Profile::Off);
    }
    // Opening L2CAP to a disconnected device would page it.
    anyhow::ensure!(device.is_connected().await?, "AirPods not connected");
    let address =
        bluer::l2cap::SocketAddr::new(device.address(), bluer::AddressType::BrEdr, AIRPODS_PSM);
    let socket = bluer::l2cap::Socket::new_seq_packet()?;
    socket.set_recv_mtu(RECV_MTU)?;
    let stream = tokio::time::timeout(CONNECT_TIMEOUT, socket.connect(address)).await??;
    bluez::wait_l2cap_established(&stream).await?;
    tracing::debug!(
        recv = stream.recv_mtu()?,
        send = stream.send_mtu()?,
        "AACP MTU"
    );
    Ok(Session {
        buf: vec![0u8; stream.recv_mtu()?],
        device,
        stream,
        unsettled: matches!(link, Link::Existing),
    })
}

/// Wait for paired AirPods that BlueZ reports connected. Looked up each time,
/// so pods paired later are picked up. BlueZ state only: no radio traffic.
async fn connected_airpods(adapter: &bluer::Adapter) -> (bluer::Device, Link) {
    loop {
        let found = match bluez::find_airpods(adapter).await {
            Ok(airpods) => bluez::wait_connected(&airpods)
                .await
                .map(|link| (airpods, link)),
            Err(e) => Err(e),
        };
        match found {
            Ok((airpods, link)) => {
                tracing::info!(address = %airpods.address(), ?link, "AirPods connected");
                return (airpods, link);
            }
            Err(e) => tracing::debug!("{e:#}"),
        }
        tokio::time::sleep(PAIRING_POLL_INTERVAL).await;
    }
}

/// The next local player status change; never, without MPRIS.
async fn playback_change(playback: &mut Option<watch::Receiver<bool>>) -> Option<bool> {
    match playback {
        Some(rx) => {
            rx.changed().await.ok()?;
            Some(*rx.borrow_and_update())
        }
        None => std::future::pending().await,
    }
}
