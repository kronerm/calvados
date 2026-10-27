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
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use futures_util::future::OptionFuture;
use protocol::MacAddress;
use protocol::aacp::{NoiseControlMode, Packet};
use protocol::connection::AIRPODS_PSM;
use protocol::engine::{Effect, Engine, LocalAudio};
use protocol::firmware::Release;
use protocol::protocol::parse_datagram;
use protocol::settings::{Setting, Settings};
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::{mpsc, watch};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt as _, util::SubscriberInitExt as _};

use crate::bluez::Link;
use crate::dbus::Status;
use crate::pipewire::Profile;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Pause before re-attaching, so a flapping channel can't spin.
const RECONNECT_DELAY: Duration = Duration::from_secs(3);
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
    /// An Apple device advertising nearby, with its manufacturer data (from
    /// the type byte).
    Nearby(bluer::Address, Vec<u8>),
    /// A firmware catalog check for these models (a product's): its
    /// current release, `None` when it failed.
    Catalog(Vec<String>, Option<Release>),
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
        .with(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!("{}=info,protocol=info", env!("CARGO_CRATE_NAME")).into()
            }),
        )
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

/// The daemon: attach whenever BlueZ reports the pods connected (never
/// paging them), and drive the engine with everything that happens.
async fn run(daemon: &Daemon, mut engine: Engine, inputs: &mut mpsc::UnboundedReceiver<Input>) {
    let mut session: Option<Session> = None;
    // Kept across iterations: cancelling it could cut an L2CAP connect.
    let mut attaching = None;
    let mut attach_after = Duration::ZERO;
    let mut playback = daemon.media.as_ref().map(mpris::Media::playback);
    let mut catalog = firmware::Catalog::new();
    loop {
        let model = engine.state().model.as_deref();
        if let Some(model) = model {
            catalog.check(model, &daemon.inputs, Instant::now());
        }
        daemon.publish(Status::of(
            &engine,
            model.and_then(|model| catalog.latest(model)),
        ));
        if session.is_none() && attaching.is_none() {
            attaching = Some(Box::pin(attach(daemon, attach_after)));
        }
        let wake = engine.next_wake().map(tokio::time::Instant::from_std);
        let effects = tokio::select! {
            Some(attached) = OptionFuture::from(attaching.as_mut()) => {
                attaching = None;
                match attached {
                    Ok(new) => {
                        let pods = MacAddress::new(new.device.address().0);
                        session = Some(new);
                        engine.connected(pods, Instant::now())
                    }
                    Err(e) => {
                        tracing::info!("attaching failed: {e:#}");
                        attach_after = RECONNECT_DELAY;
                        Vec::new()
                    }
                }
            }
            received = receive(session.as_mut()) => match received {
                Ok(packets) => {
                    let now = Instant::now();
                    packets
                        .into_iter()
                        .inspect(|packet| tracing::debug!(?packet, "rx"))
                        .flat_map(|packet| engine.handle(packet, now))
                        .collect()
                }
                Err(e) => {
                    end(&mut engine, &mut session, &e);
                    attach_after = RECONNECT_DELAY;
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
                Input::Nearby(address, advertisement) => engine.nearby(
                    address.0,
                    &advertisement,
                    presence::stored_keys(),
                    Instant::now(),
                ),
                Input::Catalog(models, release) => {
                    catalog.found(models, release);
                    Vec::new()
                }
            },
            Some(playing) = playback_change(&mut playback) => engine.local_audio(match playing {
                true => LocalAudio::Media,
                false => LocalAudio::Silent,
            }),
        };
        for effect in effects {
            if let Err(e) = perform(daemon, session.as_ref(), effect).await {
                end(&mut engine, &mut session, &e);
                attach_after = RECONNECT_DELAY;
                break;
            }
        }
        if let Some(session) = session.as_mut().filter(|session| session.unsettled)
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

fn end(engine: &mut Engine, session: &mut Option<Session>, reason: &anyhow::Error) {
    tracing::info!("AACP connection ended: {reason:#}");
    *session = None;
    engine.disconnected(Instant::now());
}

/// Never, without a deadline.
async fn sleep_until(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

/// The next datagram's packets; never, without a session.
async fn receive(session: Option<&mut Session>) -> anyhow::Result<Vec<Packet>> {
    let Some(session) = session else {
        return std::future::pending().await;
    };
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
        Effect::LearnedLocalMac(_) => {}
        Effect::LearnedKey { pods, kind, key } => presence::save_key(pods, kind, key),
        Effect::Pause => daemon.media.iter().for_each(mpris::Media::pause),
        Effect::Resume => daemon.media.iter().for_each(mpris::Media::resume),
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
    let stream = tokio::time::timeout(
        CONNECT_TIMEOUT,
        bluer::l2cap::Socket::new_seq_packet()?.connect(address),
    )
    .await??;
    bluez::wait_l2cap_established(&stream).await?;
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
async fn playback_change(playback: &mut Option<watch::Receiver<Option<bool>>>) -> Option<bool> {
    match playback {
        Some(rx) => {
            rx.changed().await.ok()?;
            *rx.borrow_and_update()
        }
        None => std::future::pending().await,
    }
}
