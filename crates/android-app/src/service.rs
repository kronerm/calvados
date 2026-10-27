//! The service loop: one event channel driving the AACP link and the shared
//! [`Engine`], with the notification as output.

use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use firmware_fetch::{Catalog, Client};
use jni::JavaVM;

use crate::intents::{
    ACTION_AIRPODS_AVAILABLE, ACTION_AIRPODS_GONE, ACTION_AUTO_CONNECT, ACTION_AUTO_SWITCH,
    ACTION_BLUETOOTH_ON, ACTION_CANCEL_UPDATE, ACTION_NOISE_PREFIX, ACTION_UPDATE_FIRMWARE,
};
use crate::link::{self, BluetoothEvent};
use crate::notification::Content;
use crate::proto::MacAddress;
use crate::proto::display;
use crate::proto::engine::{Effect, Engine, LocalAudio};
use crate::proto::firmware::Release;
use crate::proto::protocol::Decoder;
use crate::proto::settings::{Setting, Settings};
use crate::proto::superbinary::Package;
use crate::{bluetooth, media};

/// Android sheds notification updates beyond ~5/s; stay well under that.
const NOTIFY_MIN_INTERVAL: Duration = Duration::from_millis(300);

/// Minimum gap between attach attempts, so a flapping channel can't spin.
const RECONNECT_DELAY: Duration = Duration::from_secs(3);

/// Everything the service loop reacts to.
#[derive(Debug)]
pub(crate) enum Event {
    /// An intent action: notification button or ACL change.
    Intent(String),
    Bluetooth(BluetoothEvent),
    /// Active playbacks changed: whether media or a VoIP call is among them.
    Playback {
        media: bool,
        voip: bool,
    },
    /// A phone call is off-hook (answered or placed), or ended.
    PhoneCall(bool),
    /// Apple devices advertising nearby: address and manufacturer data.
    Nearby(Vec<(String, Vec<u8>)>),
    /// A firmware catalog check for these models (a product's): its
    /// current release, `None` when it failed.
    Catalog(Vec<String>, Option<Release>),
    /// The release's package for the update, `None` when it couldn't be had.
    Package(Option<Package>),
    Stop,
}

/// What plays on this phone, from the playback callback and the phone state.
#[derive(Debug, Default)]
struct Playing {
    media: bool,
    voip: bool,
    phone_call: bool,
}

impl Playing {
    fn in_call(&self) -> bool {
        self.voip || self.phone_call
    }

    /// Apply `event`; what plays now, a call over media, if it was about that.
    fn update(&mut self, event: &Event) -> Option<LocalAudio> {
        match *event {
            Event::Playback { media, voip } => (self.media, self.voip) = (media, voip),
            Event::PhoneCall(active) => self.phone_call = active,
            _ => return None,
        }
        Some(match (self.in_call(), self.media) {
            (true, _) => LocalAudio::Call,
            (false, true) => LocalAudio::Media,
            (false, false) => LocalAudio::Silent,
        })
    }
}

/// Sender into the running service loop.
static EVENTS: Mutex<Option<crossbeam_channel::Sender<Event>>> = Mutex::new(None);

/// Spawn the service loop and route [`post`]ed events to it.
pub(crate) fn start(jvm: JavaVM) {
    let (events, rx) = crossbeam_channel::unbounded();
    *EVENTS.lock().unwrap() = Some(events);
    let spawned = thread::Builder::new()
        .name("service-main".into())
        .spawn(move || service_main(jvm, rx));
    if let Err(e) = spawned {
        tracing::error!("failed to spawn the service loop: {e}");
    }
}

/// Stop the loop; later events are dropped until the next [`start`].
pub(crate) fn stop() {
    post(Event::Stop);
    *EVENTS.lock().unwrap() = None;
}

pub(crate) fn post(event: Event) {
    match EVENTS.lock().unwrap().as_ref() {
        Some(events) => events.send(event).unwrap_or_default(),
        None => tracing::debug!(?event, "service loop not running, dropped"),
    }
}

/// AACP channel lifecycle; ownership lives in the [`Engine`].
#[derive(Debug, Clone)]
pub(crate) enum Conn {
    Disconnected,
    /// Awaiting `BluetoothEvent::Connected`.
    Connecting {
        mac: String,
    },
    Connected {
        mac: String,
    },
}

fn service_main(jvm: JavaVM, events: crossbeam_channel::Receiver<Event>) {
    // `None` disables the peer handoff; local playback still works.
    let settings = Settings::load(bluetooth::load_setting);
    let mut engine = Engine::new(resolve_local_mac(), settings);
    // L2CAP is a byte stream here; reset per socket.
    let mut decoder = Decoder::default();

    let mut conn = Conn::Disconnected;
    // The AACP channel opens only over an ACL the OS / the AirPods brought up
    // (opening the socket over a live ACL doesn't dial); paging happens only
    // through an A2DP connect (`Effect::ConnectPods`). `available` mirrors
    // the ACL_CONNECTED/DISCONNECTED broadcasts (AIRPODS_AVAILABLE/GONE); we
    // attach only while it's set. Starts false — an AVAILABLE broadcast
    // attaches us. `reconnect_at` just rate-limits re-attach attempts so a
    // flapping channel can't busy-loop.
    let mut available = false;
    let mut reconnect_at = Instant::now();
    // Shown notification content, and when it was last posted.
    let mut shown: Option<(Content, Instant)> = None;
    let mut playing = Playing::default();
    // When to (re)post the notification: now, to replace the "Starting…"
    // placeholder, and later for a change held back by NOTIFY_MIN_INTERVAL.
    let mut notify_at = Some(Instant::now());
    // Apple's catalog, checked off the loop; packages cached in the app's cache.
    let mut catalog = Catalog::default();
    let client = Client::default();
    let cache = cache_dir()
        .inspect_err(|e| tracing::warn!("no cache directory for firmware: {e:#}"))
        .ok();
    // The connect broadcast may predate us (app update, restart): ask.
    if let Err(e) = bluetooth::check_airpods_connected() {
        tracing::warn!("checking for connected AirPods failed: {e:#}");
    }
    // The pods page only the host that last had the audio: watch for them.
    if let Err(e) = bluetooth::set_presence_scan(true) {
        tracing::warn!("no AirPods presence scan: {e:#}");
    }

    loop {
        // Sleep until an event or the next timed job.
        let deadline = [
            notify_at,
            engine.next_wake(),
            (matches!(conn, Conn::Disconnected) && available).then_some(reconnect_at),
        ]
        .into_iter()
        .flatten()
        .min();
        let received = match deadline {
            Some(deadline) => events.recv_deadline(deadline),
            None => events
                .recv()
                .map_err(|_| crossbeam_channel::RecvTimeoutError::Disconnected),
        };
        let event = match received {
            Ok(event) => Some(event),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => None,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => Some(Event::Stop),
        };
        let now = Instant::now();

        let mut effects = event
            .as_ref()
            .and_then(|event| playing.update(event))
            .map_or_else(Vec::new, |audio| engine.local_audio(audio));

        match event {
            None | Some(Event::Playback { .. } | Event::PhoneCall(_)) => {}
            Some(Event::Catalog(models, release)) => catalog.found(models, release),
            Some(Event::Package(package)) => effects.extend(engine.package(package, now)),
            Some(Event::Nearby(advertisers)) => {
                let keys = bluetooth::stored_keys()
                    .inspect_err(|e| tracing::warn!("reading the AirPods' keys: {e:#}"))
                    .ok()
                    .flatten();
                effects.extend(
                    advertisers
                        .iter()
                        .inspect(|(address, data)| {
                            let data = hex::encode(data);
                            tracing::debug!(address, data, "Apple device advertising nearby");
                        })
                        .filter_map(|(address, data)| {
                            Some((address.parse::<MacAddress>().ok()?, data))
                        })
                        .flat_map(|(address, data)| {
                            engine.nearby(address.to_array(), data, keys, now)
                        }),
                );
            }
            Some(Event::Stop) => {
                tracing::info!("Service shutting down");
                if let Err(e) = bluetooth::set_presence_scan(false) {
                    tracing::warn!("stopping the AirPods presence scan: {e:#}");
                }
                if !matches!(conn, Conn::Disconnected) {
                    link::disconnect();
                }
                break;
            }
            Some(Event::Intent(action)) => match action.as_str() {
                // ACL presence: AVAILABLE (the pods connected, not us) lets us
                // attach; GONE (case / out of range / other device) stops us.
                ACTION_AIRPODS_AVAILABLE => {
                    available = true;
                    if matches!(conn, Conn::Disconnected) {
                        reconnect_at = now;
                    }
                }
                ACTION_AIRPODS_GONE => available = false,
                ACTION_BLUETOOTH_ON => {
                    if let Err(e) = bluetooth::set_presence_scan(true) {
                        tracing::warn!("no AirPods presence scan: {e:#}");
                    }
                }
                ACTION_AUTO_SWITCH => toggle(&mut engine, Setting::AutoSwitch),
                ACTION_AUTO_CONNECT => toggle(&mut engine, Setting::AutoConnect),
                ACTION_UPDATE_FIRMWARE => {
                    let release = engine
                        .state()
                        .firmware
                        .model
                        .as_deref()
                        .and_then(|model| catalog.latest(model))
                        .cloned();
                    match engine.prepare_update(release.as_ref().map(|release| release.version)) {
                        Ok(started) => {
                            effects.extend(started);
                            if let Some(release) = release {
                                fetch_package(&client, release, cache.clone());
                            }
                        }
                        // The engine shows it as the update's status.
                        Err(refusal) => tracing::info!(%refusal, "firmware update refused"),
                    }
                }
                ACTION_CANCEL_UPDATE => effects.extend(engine.cancel_update()),
                other => match other
                    .strip_prefix(ACTION_NOISE_PREFIX)
                    .and_then(display::parse_mode)
                {
                    Some(mode) => effects.extend(engine.set_noise_mode(mode)),
                    None => tracing::warn!(action = other, "unknown action"),
                },
            },
            Some(Event::Bluetooth(event)) => match event {
                BluetoothEvent::Connected => match conn.clone() {
                    Conn::Disconnected => {
                        tracing::warn!("BT: ignoring connected event while disconnected");
                    }
                    Conn::Connecting { mac } | Conn::Connected { mac } => {
                        available = true;
                        decoder = Decoder::default();
                        match mac.parse::<MacAddress>() {
                            Ok(pods) => {
                                effects.extend(engine.connected(pods, now));
                                match bluetooth::stored_firmware(pods) {
                                    Ok(stored) => engine.stored_firmware(stored),
                                    Err(e) => tracing::warn!("reading the firmware: {e:#}"),
                                }
                            }
                            Err(_) => tracing::warn!(mac, "the pods' address is unusable"),
                        }
                        conn = Conn::Connected { mac };
                    }
                },
                // The bridge reports a dead link once; a stale report while
                // already disconnected is ignored.
                BluetoothEvent::Closed(_) if matches!(conn, Conn::Disconnected) => {}
                BluetoothEvent::Closed(reason) => {
                    tracing::info!("BT: link closed: {reason}");
                    link::disconnect();
                    engine.disconnected(now);
                    conn = Conn::Disconnected;
                    // An AACP blip over a live ACL reconnects on the next try.
                    reconnect_at = now + RECONNECT_DELAY;
                }
                BluetoothEvent::Received(data) => {
                    effects.extend(
                        decoder
                            .push(&data)
                            .into_iter()
                            .inspect(|packet| tracing::debug!(?packet, "rx"))
                            .flat_map(|packet| engine.handle(packet, now)),
                    );
                }
            },
        }

        effects.extend(engine.tick(now));
        for effect in effects {
            if !execute_effect(&conn, effect, playing.in_call()) {
                link::disconnect();
                engine.disconnected(now);
                conn = Conn::Disconnected;
                reconnect_at = now + RECONNECT_DELAY;
                break;
            }
        }

        // --- Attach our AACP channel while the pods are ACL-connected. Over a
        // live ACL this just opens the channel (no paging); if the ACL is actually
        // down it would page, so a failed open clears `available` and we wait for
        // the next AVAILABLE broadcast instead of dialing again. ---
        if matches!(conn, Conn::Disconnected) && available && now >= reconnect_at {
            match connect_to_airpods() {
                Ok(mac) => {
                    conn = Conn::Connecting { mac };
                }
                Err(e) => {
                    tracing::warn!("connect failed: {e}");
                    available = false;
                    reconnect_at = now + RECONNECT_DELAY;
                }
            }
        }

        let model = engine.state().firmware.model.as_deref();
        if let Some(models) = model.and_then(|model| catalog.due(model, now)) {
            check_catalog(&client, models);
        }

        // Post only changes, throttled; a change during the pause is posted
        // when it ends, so the latest state always lands.
        let latest = model.and_then(|model| catalog.latest(model));
        let content = Content::of(&engine, &conn, latest);
        match &shown {
            Some((current, _)) if *current == content => notify_at = None,
            Some((_, posted)) if now < *posted + NOTIFY_MIN_INTERVAL => {
                notify_at = Some(*posted + NOTIFY_MIN_INTERVAL);
            }
            _ => {
                content.post(&jvm);
                shown = Some((content, now));
                notify_at = None;
            }
        }
    }
}

/// The app's cache directory, for the firmware package.
fn cache_dir() -> anyhow::Result<PathBuf> {
    crate::jni::with_env(|env| {
        crate::jni::app_context(env)?
            .get_cache_dir(env)?
            .get_absolute_path(env)
    })
    .map(PathBuf::from)
}

/// Ask the catalog for `models` on a thread; the answer comes back as
/// [`Event::Catalog`].
fn check_catalog(client: &Client, models: Vec<String>) {
    let client = client.clone();
    thread::spawn(move || {
        let release = client.release(&models);
        post(Event::Catalog(models, release));
    });
}

/// `release`'s package, cached or downloaded, on a thread; reported as
/// [`Event::Package`].
fn fetch_package(client: &Client, release: Release, cache: Option<PathBuf>) {
    let client = client.clone();
    thread::spawn(move || {
        let package = cache
            .context("no cache directory")
            .and_then(|cache| client.package(&release, &cache))
            .inspect(|package| tracing::info!(?package, "firmware package ready"))
            .inspect_err(|e| tracing::warn!(%release, "firmware package: {e:#}"))
            .ok();
        post(Event::Package(package));
    });
}

/// Perform one engine [`Effect`]; `false` if the link is dead.
fn execute_effect(conn: &Conn, effect: Effect, in_call: bool) -> bool {
    match effect {
        Effect::Send(bytes) => return link::send(&bytes).is_ok(),
        // Off-thread: the reflection call can stall.
        Effect::RouteAudio => {
            if let Conn::Connecting { mac } | Conn::Connected { mac } = conn {
                let mac = mac.clone();
                thread::spawn(move || {
                    if let Err(e) = bluetooth::connect_a2dp(&mac) {
                        tracing::warn!("A2DP connect failed (audio may not route): {e:#}");
                    }
                    if !in_call {
                        toggle_a2dp_output("A2DP output restarted", TAKE_HOLD);
                    }
                });
            }
        }
        // The output is restarted on every RouteAudio instead: after another
        // host had the audio, every take stalls, and waiting first costs ~1 s.
        Effect::RestartStream => {}
        // Like an iPhone: pause. Playing on would leave the pods' remote
        // suspend in place, and the stream would only restart once Android
        // gives up on the stalled output (~10 s).
        // Not followed by an A2DP toggle: its `false` starts the stream again,
        // the pods refuse it, and the HAL blocks further toggles for ~5 s.
        Effect::ReleaseAudio => log_media("pause on release", media::pause()),
        // Reported as a dead link: the loop closes it and attaches again.
        Effect::Reconnect => return false,
        // Through A2DP; the ACL broadcast then attaches. Off-thread, like
        // RouteAudio. A connect under way already makes this a no-op.
        Effect::ConnectPods(pods) => {
            thread::spawn(move || {
                if let Err(e) = bluetooth::connect_a2dp(&bluetooth::address(pods)) {
                    tracing::warn!("connecting the pods failed: {e:#}");
                }
            });
        }
        Effect::LearnedKey { pods, kind, key } => {
            if let Err(e) = bluetooth::save_key(pods, kind, key) {
                tracing::warn!(?kind, "storing the AirPods' key: {e:#}");
            }
        }
        Effect::LearnedFirmware { pods, firmware } => {
            if let Err(e) = bluetooth::save_firmware(pods, &firmware) {
                tracing::warn!("storing the firmware: {e:#}");
            }
        }
        Effect::LearnedLocalMac(mac) => {
            if let Err(e) = bluetooth::save_local_mac(mac) {
                tracing::warn!("failed to persist local MAC: {e:#}");
            }
        }
        Effect::Pause => log_media("pause", media::pause()),
        Effect::Resume => log_media("resume", media::resume()),
    }
    true
}

/// How long [`media::toggle_a2dp_output`] holds the suspend on a take: long
/// enough for the output to be torn down.
const TAKE_HOLD: Duration = Duration::from_millis(200);

fn toggle_a2dp_output(done: &str, hold: Duration) {
    match media::toggle_a2dp_output(hold) {
        Ok(()) => tracing::info!("{done}"),
        Err(e) => tracing::warn!("A2DP output toggle failed: {e:#}"),
    }
}

fn log_media(action: &str, result: anyhow::Result<()>) {
    match result {
        Ok(()) => tracing::info!("media: {action}"),
        Err(e) => tracing::warn!("media {action} failed: {e:#}"),
    }
}

fn toggle(engine: &mut Engine, setting: Setting) {
    let enabled = !engine.settings().get(setting);
    engine.set(setting, enabled);
    bluetooth::save_setting(setting, enabled);
}

/// Start connecting to the paired AirPods; returns their MAC.
pub(crate) fn connect_to_airpods() -> anyhow::Result<String> {
    let mac =
        bluetooth::paired_airpods()?.ok_or_else(|| anyhow::anyhow!("no paired AirPods found"))?;
    link::connect(mac.clone(), |event| post(Event::Bluetooth(event)));
    Ok(mac)
}

pub(crate) fn resolve_local_mac() -> Option<MacAddress> {
    match bluetooth::local_mac() {
        Ok(Some(mac)) => {
            tracing::info!(%mac, "local MAC known, smart routing enabled");
            Some(mac)
        }
        Ok(None) => {
            tracing::debug!("local MAC unknown until connect");
            None
        }
        Err(e) => {
            tracing::error!("local MAC lookup failed: {e:#}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_over_media() {
        let mut playing = Playing::default();
        let media = Event::Playback {
            media: true,
            voip: false,
        };
        assert!(matches!(playing.update(&media), Some(LocalAudio::Media)));
        assert!(matches!(
            playing.update(&Event::PhoneCall(true)),
            Some(LocalAudio::Call)
        ));
        assert!(matches!(
            playing.update(&Event::PhoneCall(false)),
            Some(LocalAudio::Media)
        ));
        assert!(playing.update(&Event::Stop).is_none());
    }
}
