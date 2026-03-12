//! Sans-IO engine shared by both platforms: feed it the link's lifecycle,
//! packets, local playback, nearby advertisements and the time; perform the
//! returned [`Effect`]s in order, and call [`Engine::tick`] at
//! [`Engine::next_wake`].

use std::fmt;
use std::time::{Duration, Instant};

use crate::MacAddress;
use crate::aacp::{
    AudioSource, AudioSourceType, ControlCommand, ControlId, DeviceRole, EarDetection, EarStatus,
    KeyType, NoiseControlMode, Packet, Routing, RoutingResponse,
};
use crate::ble;
use crate::protocol::{
    handshake, hijack_sequence, request_notifications, session_setup_frames, set_noise_control,
    set_ownership,
};
use crate::settings::{Setting, Settings};
use crate::state::DeviceState;

/// How long after the handshake [`Engine::check_state_burst`] runs.
const STATE_BURST_WAIT: Duration = Duration::from_secs(2);

/// How long a disconnection may last for the next session with the same pods
/// to start from the last one's display state ([`Engine::connected`]); a
/// primary bud handing over is back within ~6 s.
pub const CARRY_OVER: Duration = Duration::from_secs(60);

/// After the pods drop while not stowed, connect them this long after the
/// drop and after each attempt (a primary handover takes ~3 s), this many
/// times. Also the least gap between two connects for nearby pods (both buds
/// advertise).
const REDIAL_GAP: Duration = Duration::from_secs(3);
const REDIAL_ATTEMPTS: u8 = 3;

/// How long after a confirmed take, and after each restart,
/// [`Engine::check_stream`] runs. A healthy take streams within ~0.7 s.
const STREAM_WAIT: Duration = Duration::from_secs(1);

/// Audio ownership.
#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub enum Ownership {
    #[default]
    Idle,
    /// Take requested, until the firmware confirms.
    Claiming(Handoff),
    /// We hold the audio: the firmware confirmed it, we're streaming, or
    /// we're the only host. Leaving it drops any ear pause with it.
    Owning(Playback),
}

/// The handoff from the other hosts, while claiming.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub enum Handoff {
    /// No other host known yet: sent once one is.
    Pending,
    Sent,
}

/// Our playback while we hold the audio.
#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub enum Playback {
    #[default]
    AsIs,
    /// The ear rule paused it: resume once both buds are back in.
    EarPaused,
}

impl Ownership {
    /// Holding or claiming the link.
    pub fn is_held(self) -> bool {
        !matches!(self, Self::Idle)
    }
}

impl fmt::Display for Ownership {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Idle => "not-owning",
            Self::Claiming(_) => "claiming",
            Self::Owning(_) => "owning",
        })
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub enum Effect {
    /// Send raw AACP bytes.
    Send(Vec<u8>),
    /// Route audio here: connect the A2DP sink and let the OS stream to it.
    RouteAudio,
    /// Stop streaming to the pods (keep the link), so the device that owns
    /// the audio keeps it. Also the platform's default when the pods connect.
    ReleaseAudio,
    /// The session is dead (the pods never answered): close the AACP
    /// connection, report [`Engine::disconnected`], and open a new one.
    Reconnect,
    /// Connect (page) the pods: they're nearby, or dropped unexpectedly.
    /// A no-op if they already are.
    ConnectPods(MacAddress),
    /// Restart the A2DP stream: it didn't start after a take (a start sent
    /// while the pods were still switching gets lost, or the pods' remote
    /// suspend stays in place).
    RestartStream,
    /// The local MAC was learned from the pods; persist it.
    LearnedLocalMac(MacAddress),
    /// One of the pods' proximity keys, as sent: the IRK (least significant
    /// byte first) recognises their BLE advertisement, the encryption key
    /// decrypts its case level ([`crate::ble`]). Persist it, keyed by pods
    /// and kind, for [`Engine::nearby`].
    LearnedKey {
        pods: MacAddress,
        kind: KeyType,
        key: [u8; 16],
    },
    /// Pause local media playback (a bud left an ear).
    Pause,
    /// Resume the playback [`Effect::Pause`] paused.
    Resume,
}

/// What we know of this host's MAC, needed for the peer hijack.
#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(test, derive(PartialEq, Eq))]
enum LocalMac {
    #[default]
    Unknown,
    /// The session's first announced host: the pods announce the new
    /// connection, us, first (every capture). Used, not persisted.
    Inferred(MacAddress),
    /// Configured, or confirmed by being the only host.
    Known(MacAddress),
}

impl LocalMac {
    fn get(self) -> Option<MacAddress> {
        match self {
            Self::Unknown => None,
            Self::Inferred(mac) | Self::Known(mac) => Some(mac),
        }
    }
}

#[derive(Debug, Default)]
pub struct Engine {
    settings: Settings,
    local_mac: LocalMac,
    /// The pods of the current or last session.
    pods: Option<MacAddress>,
    link: Link,
    /// The session's state; after it, kept for display and the next one.
    state: DeviceState,
    ownership: Ownership,
    /// Where this session's startup stands, for [`Engine::check_state_burst`].
    startup: Startup,
    /// When [`Engine::check_state_burst`] runs next.
    burst_check: Option<Instant>,
    stream: Stream,
}

/// The AACP session.
#[derive(Debug, Clone, Copy)]
enum Link {
    Up,
    /// Since when (`None`: never up), and the last connect we asked for.
    Down {
        since: Option<Instant>,
        dial: Option<Dial>,
    },
}

impl Default for Link {
    fn default() -> Self {
        Self::Down {
            since: None,
            dial: None,
        }
    }
}

/// An [`Effect::ConnectPods`] (or the drop that schedules the first), and
/// how many redials are left after it.
#[derive(Debug, Clone, Copy)]
struct Dial {
    at: Instant,
    redials: u8,
}

/// Our A2DP stream after a confirmed take, for [`Engine::check_stream`].
#[derive(Debug, Clone, Copy, Default)]
enum Stream {
    #[default]
    Unwatched,
    /// Not reported streaming yet; restarted this many times, checked at
    /// `check`.
    Awaited { restarts: u8, check: Instant },
}

/// [`Engine::check_stream`] restarts at most this many times per take. The
/// pods ignore a new source for some seconds after switching to one (a take
/// right after the peer started: restarts at 1.5 and 3.5 s were ignored, 7.5 s
/// worked), so it keeps trying past that.
const STREAM_RESTARTS: u8 = 10;

/// This session's startup, as far as the pods have answered.
#[derive(Debug, Clone, Copy)]
enum Startup {
    /// Handshake sent, and again this many times, no ack: sessions opened
    /// right after the pods boot can stay silent.
    Sent { retries: u8 },
    /// Acked, and nothing else since; asked again this many times. Once the
    /// pods sent nothing more for 53 minutes (Oct 18 13:36:54), not even a
    /// noise mode's echo, where a new connection worked at once.
    Acked { retries: u8 },
    /// Answering since the ack; asked again this many times for a missing
    /// part (the state half can be held back for ~25 s).
    Answering { retries: u8 },
}

impl Default for Startup {
    fn default() -> Self {
        Self::Sent { retries: 0 }
    }
}

/// [`Engine::check_state_burst`] asks again at most this many times.
const STARTUP_RETRIES: u8 = 3;

impl Engine {
    pub fn new(local_mac: Option<MacAddress>, settings: Settings) -> Self {
        Self {
            settings,
            local_mac: local_mac.map_or(LocalMac::Unknown, LocalMac::Known),
            ..Self::default()
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    /// Change a setting; the platform stores it.
    pub fn set(&mut self, setting: Setting, enabled: bool) {
        tracing::info!(?setting, enabled, "setting changed");
        self.settings.set(setting, enabled);
    }

    /// An AACP session is up.
    pub fn is_connected(&self) -> bool {
        matches!(self.link, Link::Up)
    }

    /// The pods of the current or last session.
    pub fn pods(&self) -> Option<MacAddress> {
        self.pods
    }

    pub fn state(&self) -> &DeviceState {
        &self.state
    }

    pub fn ownership(&self) -> Ownership {
        self.ownership
    }

    pub fn local_mac(&self) -> Option<MacAddress> {
        self.local_mac.get()
    }

    /// An AACP session with `pods` opened: starts it from scratch but for a
    /// known local MAC and, for the same pods within [`CARRY_OVER`], the
    /// display state ([`DeviceState::carried_over`]). Sends the handshake.
    pub fn connected(&mut self, pods: MacAddress, now: Instant) -> Vec<Effect> {
        let recent = match self.link {
            Link::Up => true,
            Link::Down { since, .. } => {
                since.is_some_and(|since| now.saturating_duration_since(since) <= CARRY_OVER)
            }
        };
        let carry_over = recent && self.pods == Some(pods);
        tracing::info!(%pods, carry_over, "session opened, handshaking");
        let known = match self.local_mac {
            LocalMac::Known(mac) => Some(mac),
            _ => None,
        };
        *self = Self {
            pods: Some(pods),
            link: Link::Up,
            state: match carry_over {
                true => self.state.carried_over(),
                false => DeviceState::default(),
            },
            burst_check: Some(now + STATE_BURST_WAIT),
            ..Self::new(known, self.settings)
        };
        vec![Effect::Send(handshake())]
    }

    /// The AACP session ended. Unless both buds were put away (the case
    /// closing), the drop was unexpected: the pods page back only the host
    /// that last had the audio, and the OS doesn't retry soon (BlueZ: not
    /// after a primary bud handing over; Android: never), so redial.
    pub fn disconnected(&mut self, now: Instant) {
        if !self.is_connected() {
            return;
        }
        let redial = self.settings.auto_connect && !self.state.is_stowed();
        tracing::info!(redial, "session ended");
        self.link = Link::Down {
            since: Some(now),
            dial: redial.then_some(Dial {
                at: now,
                redials: REDIAL_ATTEMPTS,
            }),
        };
        self.ownership = Ownership::Idle;
        self.stream = Stream::Unwatched;
        self.burst_check = None;
    }

    /// An Apple proximity pairing advertisement from `address`, with its
    /// manufacturer data (from the type byte). From pods of ours (by `keys`):
    /// take the case's level from it, and connect them unless connected or
    /// put away (lid closed, the advertising bud in the case; they advertise
    /// while charging).
    pub fn nearby(
        &mut self,
        address: [u8; 6],
        advertisement: &[u8],
        keys: impl IntoIterator<Item = (MacAddress, ble::PodsKeys)>,
        now: Instant,
    ) -> Vec<Effect> {
        let Some((pods, keys)) = ble::identify(address, keys) else {
            return Vec::new();
        };
        if self.pods.is_none_or(|ours| ours == pods) {
            let level = keys
                .encryption
                .and_then(|key| ble::case_level(advertisement, &key));
            if let Some(level) = level {
                self.state.advertised_case_level(level);
            }
        }
        let Link::Down { since, dial } = self.link else {
            return Vec::new();
        };
        if !self.settings.auto_connect || dial.is_some_and(|dial| now < dial.at + REDIAL_GAP) {
            return Vec::new();
        }
        let proximity = ble::Proximity::parse(advertisement);
        if proximity.is_some_and(|proximity| !proximity.out()) {
            tracing::debug!(%pods, ?proximity, "pods nearby, in the closed case");
            return Vec::new();
        }
        tracing::info!(%pods, ?proximity, "pods nearby: connecting");
        self.link = Link::Down {
            since,
            dial: Some(Dial {
                at: now,
                redials: dial.map_or(0, |dial| dial.redials),
            }),
        };
        vec![Effect::ConnectPods(pods)]
    }

    /// When [`Engine::tick`] has something to do next.
    pub fn next_wake(&self) -> Option<Instant> {
        let stream = match self.stream {
            Stream::Awaited { check, .. } => Some(check),
            Stream::Unwatched => None,
        };
        [self.burst_check, stream, self.redial_at()]
            .into_iter()
            .flatten()
            .min()
    }

    /// Run what's due by `now`.
    pub fn tick(&mut self, now: Instant) -> Vec<Effect> {
        let due = |at: Option<Instant>| at.is_some_and(|at| at <= now);
        let burst = match due(self.burst_check) {
            true => self.check_state_burst(now),
            false => Vec::new(),
        };
        let stream = match self.stream {
            Stream::Awaited { check, .. } if check <= now => self.check_stream(now),
            _ => Vec::new(),
        };
        let redial = match due(self.redial_at()) {
            true => self.redial(now),
            false => Vec::new(),
        };
        burst.into_iter().chain(stream).chain(redial).collect()
    }

    fn redial_at(&self) -> Option<Instant> {
        match self.link {
            Link::Down {
                dial: Some(Dial { at, redials: 1.. }),
                ..
            } if self.settings.auto_connect => Some(at + REDIAL_GAP),
            _ => None,
        }
    }

    fn redial(&mut self, now: Instant) -> Vec<Effect> {
        let (
            Link::Down {
                since,
                dial: Some(dial),
            },
            Some(pods),
        ) = (self.link, self.pods)
        else {
            return Vec::new();
        };
        tracing::info!(%pods, "pods dropped unexpectedly: connecting");
        self.link = Link::Down {
            since,
            dial: Some(Dial {
                at: now,
                redials: dial.redials.saturating_sub(1),
            }),
        };
        vec![Effect::ConnectPods(pods)]
    }

    /// Set the noise control mode; the pods echo it back.
    pub fn set_noise_mode(&mut self, mode: NoiseControlMode) -> Vec<Effect> {
        if !self.is_connected() {
            return Vec::new();
        }
        tracing::info!(?mode, "setting the noise mode");
        vec![Effect::Send(set_noise_control(mode))]
    }

    pub fn handle(&mut self, packet: Packet, now: Instant) -> Vec<Effect> {
        if let (Startup::Acked { retries }, false) =
            (self.startup, matches!(packet, Packet::HandshakeAck))
        {
            self.startup = Startup::Answering { retries };
        }
        let previous_ears = self.state.ear_detection;
        let was_stowed = self.state.is_stowed();
        self.state.apply(&packet);
        let learned = self.learn_local_mac(&packet).map(Effect::LearnedLocalMac);
        let reaction = match packet {
            // The firmware's word on who owns the link is final.
            Packet::Control(ControlCommand {
                id: ControlId::OwnsConnection,
                value: [owns, ..],
            }) => {
                tracing::info!(owns = owns != 0, was = %self.ownership, "firmware ownership");
                let reaction = match owns {
                    0 => self.release(),
                    _ => self.confirm(now),
                };
                return learned.into_iter().chain(reaction).collect();
            }
            Packet::AudioSource(source) => self.track_audio_source(source),
            Packet::ProximityKeys(keys) => keys
                .iter()
                .filter_map(|key| {
                    Some(Effect::LearnedKey {
                        pods: self.pods?,
                        kind: key.kind,
                        key: key.data.as_slice().try_into().ok()?,
                    })
                })
                .collect(),
            // The rest of the setup goes out only now (see
            // [`session_setup_frames`]); a retried handshake's ack resends it.
            Packet::HandshakeAck => {
                if let Startup::Sent { .. } = self.startup {
                    self.startup = Startup::Acked { retries: 0 };
                }
                session_setup_frames()
                    .into_iter()
                    .map(Effect::Send)
                    .collect()
            }
            Packet::EarDetection(ears) => {
                let media = self.ear_media(previous_ears, ears);
                // Taken out of the case: what waited for it.
                let alone = match was_stowed && !self.state.is_stowed() {
                    true => self.take_if_alone(),
                    false => Vec::new(),
                };
                media.into_iter().chain(alone).collect()
            }
            // Only when the hosts change: a list (a host joined) or a host
            // leaving, not the per-device updates of a burst.
            Packet::ConnectedDevices(_) => self.take_if_alone(),
            Packet::DeviceConnection(device)
                if matches!(device.role, DeviceRole::Host) && !device.connected =>
            {
                self.take_if_alone()
            }
            Packet::RoutingResponse(RoutingResponse {
                kind: Routing::YieldRequest,
                ..
            }) => self.yield_audio(),
            Packet::RoutingResponse(RoutingResponse {
                kind: Routing::PeerMedia { streaming },
                from,
            }) => {
                tracing::info!(?from, streaming, "peer reports its playback");
                Vec::new()
            }
            Packet::RoutingResponse(RoutingResponse {
                kind: Routing::HijackDeclined,
                ..
            }) => {
                tracing::info!("audio handoff declined");
                self.ownership = Ownership::Idle;
                vec![Effect::ReleaseAudio]
            }
            _ => Vec::new(),
        };
        learned
            .into_iter()
            .chain(reaction)
            .chain(self.notify_peers())
            .collect()
    }

    /// Nobody else is connected to the pods: take the audio. There's no peer
    /// to hand off from, so nothing is sent to the firmware. Not while the
    /// buds are in the case (or their ears not known yet): pods connected in
    /// the case took the audio from other headphones; taking them out does.
    fn take_if_alone(&mut self) -> Vec<Effect> {
        let (Some(local), Some(hosts)) = (self.local_mac.get(), &self.state.hosts) else {
            return Vec::new();
        };
        let alone = hosts.contains(&local) && hosts.iter().all(|mac| *mac == local);
        if !alone || self.ownership.is_held() || self.state.is_stowed() {
            return Vec::new();
        }
        tracing::info!("only device connected to the pods: taking audio");
        self.own();
        vec![Effect::RouteAudio]
    }

    /// [`STATE_BURST_WAIT`] after the handshake, and again after each check
    /// that asked for something. Without an ack, handshakes again (the
    /// session stayed silent) a few times, then gives up on the session
    /// ([`Effect::Reconnect`]); acked but without battery (only the settings
    /// half of the burst came), requests notifications again, a few times.
    fn check_state_burst(&mut self, now: Instant) -> Vec<Effect> {
        let effects = self.startup_step();
        self.burst_check = match effects[..] {
            [] | [Effect::Reconnect] => None,
            _ => Some(now + STATE_BURST_WAIT),
        };
        effects
    }

    fn startup_step(&mut self) -> Vec<Effect> {
        // Live: carried-over levels don't count as this session's burst.
        match (self.startup, !self.state.battery.is_live()) {
            (Startup::Sent { retries }, _) if retries >= STARTUP_RETRIES => {
                tracing::warn!("the pods never answered the handshake: reconnecting");
                vec![Effect::Reconnect]
            }
            (Startup::Sent { retries }, _) => {
                tracing::info!("no handshake ack: handshaking again");
                self.startup = Startup::Sent {
                    retries: retries + 1,
                };
                vec![Effect::Send(handshake())]
            }
            (Startup::Acked { retries }, _) if retries >= STARTUP_RETRIES => {
                tracing::warn!("the pods went silent after the handshake: reconnecting");
                vec![Effect::Reconnect]
            }
            (Startup::Acked { retries }, _) => {
                tracing::info!("nothing since the handshake ack: requesting notifications again");
                self.startup = Startup::Acked {
                    retries: retries + 1,
                };
                vec![Effect::Send(request_notifications())]
            }
            (Startup::Answering { retries }, true) if retries < STARTUP_RETRIES => {
                tracing::info!("no battery since the handshake: requesting notifications again");
                self.startup = Startup::Answering {
                    retries: retries + 1,
                };
                vec![Effect::Send(request_notifications())]
            }
            (Startup::Answering { .. }, _) => Vec::new(),
        }
    }

    /// Media started playing on this host: take the audio, like Apple's
    /// automatic switching. Not while the other host is on a call, nor before
    /// the pods have said what's playing (a session without the state burst).
    pub fn media_started(&mut self) -> Vec<Effect> {
        if !self.may_switch() {
            return Vec::new();
        }
        match self.state.audio_source {
            Some(source) if !matches!(source.kind, AudioSourceType::Call) => {
                tracing::info!(?source, "auto-take: media started here");
                self.take()
            }
            source => {
                tracing::info!(
                    ?source,
                    "media started, not taking: a call, or nothing known"
                );
                Vec::new()
            }
        }
    }

    /// A call started on this host (answered or placed): take the audio. A
    /// call wins over whatever the other host plays.
    pub fn call_started(&mut self) -> Vec<Effect> {
        if !self.may_switch() {
            return Vec::new();
        }
        tracing::info!("auto-take: call started here");
        self.take()
    }

    /// Automatic switching is on, and there's a session whose audio we don't
    /// hold, or whose take the pods haven't confirmed yet (see [`Self::take`]).
    fn may_switch(&self) -> bool {
        self.settings.auto_switch
            && self.is_connected()
            && matches!(
                self.ownership,
                Ownership::Idle | Ownership::Claiming(Handoff::Sent)
            )
    }

    /// Claim the audio, for media or a call starting here. A handoff the pods
    /// never confirmed is sent again: once they didn't answer at all (Oct 9
    /// 18:22:52) though the other host got it and paused, and the same
    /// request a minute later worked.
    fn take(&mut self) -> Vec<Effect> {
        match self.ownership {
            Ownership::Owning(_) | Ownership::Claiming(Handoff::Pending) => return Vec::new(),
            Ownership::Claiming(Handoff::Sent) => {
                tracing::info!("the pods never confirmed the take: handing off again");
            }
            Ownership::Idle => {}
        }
        // Without our MAC there's no handoff to send, so a claim could never
        // complete: just route the audio here.
        if self.local_mac.get().is_none() {
            tracing::warn!("local MAC unknown: routing audio without a handoff");
            return vec![Effect::RouteAudio];
        }
        self.ownership = Ownership::Claiming(Handoff::Pending);
        // With a handoff out, the audio is routed once the pods confirm it
        // ([`Engine::confirm`]): a stream started while they switch hosts
        // rebooted them (every reboot was a take that streamed first).
        let handoff = self.notify_peers();
        if handoff.is_empty() {
            tracing::info!("no other host known yet: the handoff waits for one");
            return vec![Effect::RouteAudio];
        }
        handoff
    }

    /// The pods confirmed our ownership. After a handoff, route the audio
    /// now; re-routed on every take, since after a yield the pods suspend our
    /// A2DP stream.
    fn confirm(&mut self, now: Instant) -> Vec<Effect> {
        let Ownership::Claiming(handoff) = self.ownership else {
            self.own();
            return Vec::new();
        };
        self.own();
        if !self.streaming_here() {
            self.stream = Stream::Awaited {
                restarts: 0,
                check: now + STREAM_WAIT,
            };
        }
        match handoff {
            Handoff::Sent => vec![Effect::RouteAudio],
            Handoff::Pending => Vec::new(),
        }
    }

    /// [`STREAM_WAIT`] after a confirmed take: restarts the stream if the
    /// pods still don't report us streaming, and checks again, a few times.
    /// Healthy takes are left alone: a restart cuts a running stream.
    fn check_stream(&mut self, now: Instant) -> Vec<Effect> {
        let Stream::Awaited { restarts, .. } = self.stream else {
            return Vec::new();
        };
        let owning = matches!(self.ownership, Ownership::Owning(_));
        if !owning || self.streaming_here() {
            self.stream = Stream::Unwatched;
            return Vec::new();
        }
        if restarts >= STREAM_RESTARTS {
            tracing::warn!("still not streaming after {restarts} restarts: giving up");
            self.stream = Stream::Unwatched;
            return Vec::new();
        }
        tracing::info!(source = ?self.state.audio_source, "not streaming since the take: restarting the stream");
        self.stream = Stream::Awaited {
            restarts: restarts + 1,
            check: now + STREAM_WAIT,
        };
        vec![Effect::RestartStream]
    }

    /// Enter `Owning`, keeping the playback state if already there.
    fn own(&mut self) {
        if !matches!(self.ownership, Ownership::Owning(_)) {
            self.ownership = Ownership::Owning(Playback::AsIs);
        }
    }

    /// iOS-like automatic pause: a bud leaving an ear while we play media
    /// pauses; both back in resumes, if we paused. Both out forgets the pause.
    fn ear_media(&mut self, previous: Option<EarDetection>, now: EarDetection) -> Option<Effect> {
        let in_ear = |status| matches!(status, EarStatus::InEar);
        let both_in = |ears: EarDetection| in_ear(ears.primary) && in_ear(ears.secondary);
        let any_in = in_ear(now.primary) || in_ear(now.secondary);
        let Ownership::Owning(playback) = self.ownership else {
            return None;
        };
        let (playback, effect) = match (playback, previous.is_some_and(both_in), both_in(now)) {
            // Playing media here means we own it: `AudioSource` naming us did that.
            (_, true, false) if self.playing_media_here() => match any_in {
                true => (Playback::EarPaused, Some(Effect::Pause)),
                false => (Playback::AsIs, Some(Effect::Pause)),
            },
            (Playback::EarPaused, false, true) => (Playback::AsIs, Some(Effect::Resume)),
            (Playback::EarPaused, _, _) if !any_in => (Playback::AsIs, None),
            (playback, _, _) => (playback, None),
        };
        self.ownership = Ownership::Owning(playback);
        effect
    }

    /// The firmware reports this host as the source of media or a call: our
    /// stream started (a call's goes over SCO, which needs no restart).
    fn streaming_here(&self) -> bool {
        let (Some(local), Some(source)) = (self.local_mac.get(), self.state.audio_source) else {
            return false;
        };
        source.is_active(local)
    }

    /// The firmware reports this host as the source of playing media.
    fn playing_media_here(&self) -> bool {
        let (Some(local), Some(source)) = (self.local_mac.get(), self.state.audio_source) else {
            return false;
        };
        source.mac == local && matches!(source.kind, AudioSourceType::Media)
    }

    /// Mirror the firmware's reported audio source in the displayed ownership.
    /// Display-only: nothing is sent to the firmware.
    /// Another device becoming the source while we hold the audio is losing
    /// it: the pods switch to a host that starts streaming, and the revoke
    /// (`OwnsConnection = 0`) only follows. Nothing is sent; they've switched.
    fn track_audio_source(&mut self, source: AudioSource) -> Vec<Effect> {
        let Some(local) = self.local_mac.get() else {
            return Vec::new();
        };
        if source.is_active(local) {
            self.own();
            return Vec::new();
        }
        match (source.kind, self.ownership) {
            (AudioSourceType::None, _) | (_, Ownership::Idle | Ownership::Claiming(_)) => {
                Vec::new()
            }
            (_, Ownership::Owning(_)) => {
                tracing::info!(peer = %source.mac, "another device took the audio");
                self.ownership = Ownership::Idle;
                vec![Effect::ReleaseAudio]
            }
        }
    }

    /// A peer takes the audio: release, and stop local playback even when we
    /// don't hold it (playing without a take, e.g. right after an app
    /// restart). Playing on, the OS restarts its suspended stream after a few
    /// seconds and the pods switch back to us, bare.
    fn yield_audio(&mut self) -> Vec<Effect> {
        match self.ownership.is_held() {
            true => self.release(),
            false => {
                tracing::info!("yield request while not holding the audio: pausing");
                vec![Effect::ReleaseAudio]
            }
        }
    }

    /// Give up ownership once; repeats are no-ops.
    fn release(&mut self) -> Vec<Effect> {
        if !self.ownership.is_held() {
            return Vec::new();
        }
        tracing::info!("releasing audio ownership");
        self.ownership = Ownership::Idle;
        vec![Effect::Send(set_ownership(false)), Effect::ReleaseAudio]
    }

    /// Infer the local MAC from the session's first host announcement or, on
    /// a fresh connect, the history's just-connected host, and confirm it
    /// (returned, to persist) once we're the only host.
    fn learn_local_mac(&mut self, packet: &Packet) -> Option<MacAddress> {
        const JUST_CONNECTED: Duration = Duration::from_secs(2);
        let announced = match packet {
            Packet::ConnectionHistory(history) => history
                .just_connected_host(JUST_CONNECTED)
                .map(|mac| (mac, "host that just connected")),
            Packet::DeviceConnection(device)
                if device.connected && matches!(device.role, DeviceRole::Host) =>
            {
                Some((device.mac, "first host announced"))
            }
            _ => None,
        };
        match (announced, self.local_mac, self.state.hosts.as_deref()) {
            (Some((mac, source)), LocalMac::Unknown, None) => {
                tracing::info!(%mac, source, "local MAC inferred");
                self.local_mac = LocalMac::Inferred(mac);
            }
            // The device list must contain the inferred MAC.
            (_, LocalMac::Inferred(mac), Some(hosts)) if !hosts.contains(&mac) => {
                tracing::warn!(%mac, "inferred local MAC not among the hosts, dropped");
                self.local_mac = LocalMac::Unknown;
            }
            _ => {}
        }
        let [mac] = self.state.hosts.as_deref()? else {
            return None;
        };
        let mac = *mac;
        (!matches!(self.local_mac, LocalMac::Known(known) if known == mac)).then(|| {
            tracing::info!(%mac, "learned local MAC");
            self.local_mac = LocalMac::Known(mac);
            mac
        })
    }

    /// While claiming, once a peer is known, assert ownership and hijack each
    /// peer. Never a bare ownership claim without a peer to take from.
    fn notify_peers(&mut self) -> Vec<Effect> {
        let (Ownership::Claiming(Handoff::Pending), Some(local)) =
            (self.ownership, self.local_mac.get())
        else {
            return Vec::new();
        };
        let peers = self.state.peers_except(local).collect::<Vec<_>>();
        if peers.is_empty() {
            return Vec::new();
        }
        tracing::info!(?peers, "handing the audio off from the other hosts");
        self.ownership = Ownership::Claiming(Handoff::Sent);
        std::iter::once(set_ownership(true))
            .chain(
                peers
                    .into_iter()
                    .flat_map(|peer| hijack_sequence(local, peer)),
            )
            .map(Effect::Send)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::parse_datagram;

    const LOCAL: &str = "02:11:22:33:44:55";
    const PEER: &str = "02:66:77:88:99:aa";
    /// `ConnectedDevices` listing PEER and LOCAL.
    const CONNECTED_DEVICES: &[u8] = &[
        0x04, 0x00, 0x04, 0x00, 0x2e, 0x00, 0x01, 0x00, 0x02, 0x02, 0x66, 0x77, 0x88, 0x99, 0xaa,
        0x02, 0x04, 0x02, 0x11, 0x22, 0x33, 0x44, 0x55, 0x02, 0x06,
    ];

    fn mac(s: &str) -> MacAddress {
        s.parse().unwrap()
    }

    const PODS: &str = "ec:46:54:22:7b:cd";

    /// When every test happens, unless it says otherwise.
    fn now() -> Instant {
        static NOW: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now);
        *NOW
    }

    /// An open session with PODS, with `local` known.
    fn session(local: Option<MacAddress>) -> Engine {
        let mut e = Engine::new(local, Settings::default());
        e.connected(mac(PODS), now());
        e
    }

    fn engine() -> Engine {
        session(Some(mac(LOCAL)))
    }

    /// The session with PODS drops at `now()`, and a new one opens `after`.
    fn reconnect(e: &mut Engine, after: Duration) {
        e.disconnected(now());
        e.connected(mac(PODS), now() + after);
    }

    fn feed(engine: &mut Engine, bytes: &[u8]) -> Vec<Effect> {
        parse_datagram(bytes)
            .into_iter()
            .flat_map(|packet| engine.handle(packet, now()))
            .collect()
    }

    fn sends(fx: &[Effect]) -> usize {
        fx.iter().filter(|e| matches!(e, Effect::Send(_))).count()
    }

    fn owns_connection(owns: bool) -> Vec<u8> {
        set_ownership(owns)
    }

    /// A peer's 0x11 "SetOwnershipToFalse".
    fn yield_frame() -> Vec<u8> {
        routing_frame(b"audioRoutingSetOwnershipToFalse")
    }

    /// A routing frame from PEER, relayed by the pods (0x11).
    fn routing_frame(body: &[u8]) -> Vec<u8> {
        [
            &[
                0x04, 0x00, 0x04, 0x00, 0x11, 0x00, 0xaa, 0x99, 0x88, 0x77, 0x66, 0x02,
            ][..],
            &(body.len() as u16).to_le_bytes(),
            body,
        ]
        .concat()
    }

    fn released() -> Vec<Effect> {
        vec![Effect::Send(set_ownership(false)), Effect::ReleaseAudio]
    }

    #[test]
    fn no_bare_claim_without_a_peer() {
        let mut e = engine();
        assert_eq!(e.take(), vec![Effect::RouteAudio]);
        assert!(e.ownership().is_held());
    }

    #[test]
    fn take_without_local_mac_only_routes() {
        let mut e = session(None);
        feed(&mut e, CONNECTED_DEVICES);
        assert_eq!(e.take(), vec![Effect::RouteAudio]);
        assert_eq!(
            e.ownership(),
            Ownership::Idle,
            "no claim that can't complete"
        );
    }

    #[test]
    fn take_with_peer_sends_targeted_hijack() {
        let mut e = engine();
        feed(&mut e, CONNECTED_DEVICES);
        let fx = e.take();
        assert_eq!(fx[0], Effect::Send(set_ownership(true)));
        assert_eq!(sends(&fx), 4);
        assert!(
            !fx.contains(&Effect::RouteAudio),
            "no stream before the pods confirm"
        );
        assert_eq!(feed(&mut e, &owns_connection(true)), [Effect::RouteAudio]);
    }

    #[test]
    fn peer_learned_while_claiming_triggers_hijack() {
        let mut e = engine();
        e.take();
        assert_eq!(sends(&feed(&mut e, CONNECTED_DEVICES)), 4);
        assert_eq!(e.ownership(), Ownership::Claiming(Handoff::Sent));
    }

    #[test]
    fn second_take_is_a_noop() {
        let mut e = engine();
        e.take();
        assert!(e.take().is_empty());
    }

    #[test]
    fn re_take_after_yield_routes_again() {
        let mut e = engine();
        feed(&mut e, CONNECTED_DEVICES);
        e.take();
        feed(&mut e, &yield_frame());
        assert!(!e.ownership().is_held());
        e.take();
        assert!(feed(&mut e, &owns_connection(true)).contains(&Effect::RouteAudio));
    }

    #[test]
    fn ear_detection_produces_no_effects() {
        let mut e = engine();
        e.take();
        feed(&mut e, &owns_connection(true));
        assert_eq!(e.ownership(), Ownership::Owning(Playback::AsIs));
        let ear = Packet::EarDetection(EarDetection {
            primary: EarStatus::InEar,
            secondary: EarStatus::NotInEar,
        });
        assert!(e.handle(ear, now()).is_empty());
    }

    #[test]
    fn a_confirmed_take_watches_the_stream_once() {
        let mut e = engine();
        feed(&mut e, CONNECTED_DEVICES);
        e.take();
        assert_eq!(feed(&mut e, &owns_connection(true)), [Effect::RouteAudio]);
        assert!(
            feed(&mut e, &owns_connection(true)).is_empty(),
            "already owning"
        );
    }

    #[test]
    fn a_yield_pauses_even_without_holding_the_audio() {
        let mut e = engine();
        assert_eq!(feed(&mut e, &yield_frame()), [Effect::ReleaseAudio]);
        assert_eq!(e.ownership(), Ownership::Idle);
    }

    #[test]
    fn a_stream_that_starts_is_left_alone() {
        let mut e = engine();
        feed(&mut e, CONNECTED_DEVICES);
        e.take();
        feed(&mut e, &owns_connection(true));
        e.handle(
            Packet::AudioSource(AudioSource {
                mac: mac(LOCAL),
                kind: AudioSourceType::Media,
            }),
            now(),
        );
        assert!(e.check_stream(now()).is_empty());
    }

    #[test]
    fn a_call_here_needs_no_stream_restart() {
        // Captured Oct 7 16:03: a call take, the pods naming us the call's
        // source; the check restarted 10 times.
        let mut e = engine();
        feed(&mut e, CONNECTED_DEVICES);
        e.call_started();
        feed(&mut e, &owns_connection(true));
        playing(&mut e, AudioSourceType::Call);
        let at = e.next_wake().expect("the startup check at least");
        assert!(
            !e.tick(at + Duration::from_secs(2))
                .contains(&Effect::RestartStream)
        );
    }

    #[test]
    fn a_stalled_stream_is_restarted_a_few_times() {
        let mut e = engine();
        feed(&mut e, CONNECTED_DEVICES);
        e.take();
        feed(&mut e, &owns_connection(true));
        let checks = std::iter::from_fn(|| {
            let at = e.next_wake()?;
            Some((at, e.tick(at)))
        })
        .take(30)
        .collect::<Vec<_>>();
        // The unanswered handshake's checks happen among them.
        let restarts = checks
            .iter()
            .filter(|(_, fx)| fx.iter().any(|fx| matches!(fx, Effect::RestartStream)))
            .map(|(at, _)| (*at - now()).as_secs())
            .collect::<Vec<_>>();
        assert_eq!(restarts, (1..=STREAM_RESTARTS as u64).collect::<Vec<_>>());
        assert_eq!(e.next_wake(), None, "gave up");
    }

    #[test]
    fn no_restart_after_losing_the_audio() {
        let mut e = engine();
        feed(&mut e, CONNECTED_DEVICES);
        e.take();
        feed(&mut e, &owns_connection(true));
        feed(&mut e, &owns_connection(false));
        assert!(e.check_stream(now()).is_empty());
    }

    #[test]
    fn firmware_revocation_yields_once() {
        let mut e = engine();
        feed(&mut e, CONNECTED_DEVICES);
        e.take();
        feed(&mut e, &owns_connection(true));
        assert_eq!(feed(&mut e, &owns_connection(false)), released());
        assert!(feed(&mut e, &owns_connection(false)).is_empty());
    }

    #[test]
    fn yield_request_yields_once() {
        let mut e = engine();
        feed(&mut e, CONNECTED_DEVICES);
        e.take();
        assert_eq!(feed(&mut e, &yield_frame()), released());
        assert_eq!(
            feed(&mut e, &yield_frame()),
            [Effect::ReleaseAudio],
            "pauses again, no second ownership release"
        );
        assert!(feed(&mut e, &owns_connection(false)).is_empty());
    }

    #[test]
    fn audio_source_naming_us_counts_as_owning() {
        let mut e = engine();
        let source = |mac, kind| Packet::AudioSource(AudioSource { mac, kind });
        e.handle(source(mac(PEER), AudioSourceType::Media), now());
        assert_eq!(e.ownership(), Ownership::Idle);
        e.handle(source(mac(LOCAL), AudioSourceType::None), now());
        assert_eq!(e.ownership(), Ownership::Idle);
        assert!(
            e.handle(source(mac(LOCAL), AudioSourceType::Media), now())
                .is_empty()
        );
        assert_eq!(e.ownership(), Ownership::Owning(Playback::AsIs));
        // Another device streaming: the pods switched to it, we lost the audio.
        assert_eq!(
            e.handle(source(mac(PEER), AudioSourceType::Call), now()),
            vec![Effect::ReleaseAudio]
        );
        assert_eq!(e.ownership(), Ownership::Idle);
        // The revoke that follows (captured 30 ms later) releases nothing more.
        assert!(feed(&mut e, &owns_connection(false)).is_empty());
    }

    #[test]
    fn audio_source_does_not_cancel_a_claim() {
        let mut e = engine();
        e.take();
        e.handle(
            Packet::AudioSource(AudioSource {
                mac: mac(PEER),
                kind: AudioSourceType::Media,
            }),
            now(),
        );
        assert!(e.ownership().is_held());
    }

    fn ears(primary: EarStatus, secondary: EarStatus) -> Packet {
        Packet::EarDetection(EarDetection { primary, secondary })
    }

    fn playing(e: &mut Engine, kind: AudioSourceType) {
        e.handle(
            Packet::AudioSource(AudioSource {
                mac: mac(LOCAL),
                kind,
            }),
            now(),
        );
    }

    use EarStatus::{InCase, InEar, NotInEar};

    #[test]
    fn bud_out_pauses_and_back_in_resumes() {
        let mut e = engine();
        e.handle(ears(InEar, InEar), now());
        playing(&mut e, AudioSourceType::Media);
        assert_eq!(e.handle(ears(InEar, NotInEar), now()), vec![Effect::Pause]);
        assert_eq!(e.handle(ears(InEar, InEar), now()), vec![Effect::Resume]);
        // Still playing here: the next removal pauses again.
        assert_eq!(e.handle(ears(NotInEar, InEar), now()), vec![Effect::Pause]);
    }

    #[test]
    fn another_device_streaming_forgets_an_ear_pause() {
        let mut e = engine();
        e.handle(ears(InEar, InEar), now());
        playing(&mut e, AudioSourceType::Media);
        assert_eq!(e.handle(ears(InEar, NotInEar), now()), vec![Effect::Pause]);
        assert_eq!(e.ownership(), Ownership::Owning(Playback::EarPaused));
        e.handle(
            Packet::AudioSource(AudioSource {
                mac: mac(PEER),
                kind: AudioSourceType::Media,
            }),
            now(),
        );
        assert!(e.handle(ears(InEar, InEar), now()).is_empty(), "no resume");
    }

    #[test]
    fn handshakes_again_without_an_ack_then_reconnects() {
        let mut e = engine();
        (0..STARTUP_RETRIES).for_each(|_| {
            assert_eq!(e.check_state_burst(now()), [Effect::Send(handshake())]);
        });
        assert_eq!(e.check_state_burst(now()), [Effect::Reconnect]);
        let mut e = engine();
        e.handle(Packet::HandshakeAck, now());
        assert_eq!(
            e.check_state_burst(now()),
            [Effect::Send(request_notifications())]
        );
    }

    #[test]
    fn reports_both_proximity_keys() {
        let mut e = engine();
        let irk: [u8; 16] = std::array::from_fn(|i| i as u8);
        let body = [
            &[0x02, 0x01, 0x00, 0x10, 0x00][..],
            &irk,
            &[0x04, 0x00, 0x10, 0x00],
            &[0xee; 16],
        ]
        .concat();
        let frame = [&[0x04, 0x00, 0x04, 0x00, 0x31, 0x00][..], &body].concat();
        assert_eq!(
            feed(&mut e, &frame),
            [
                Effect::LearnedKey {
                    pods: mac(PODS),
                    kind: KeyType::Irk,
                    key: irk
                },
                Effect::LearnedKey {
                    pods: mac(PODS),
                    kind: KeyType::EncryptionKey,
                    key: [0xee; 16]
                }
            ]
        );
    }

    #[test]
    fn takes_the_case_level_from_the_advertisement() {
        use aes::cipher::{BlockEncrypt, KeyInit};
        // The Core spec's sample IRK and address (see `ble`).
        let mut irk = 0xec0234a357c8ad05341010a60a397d9b_u128.to_be_bytes();
        irk.reverse();
        let address = [0x70, 0x81, 0x94, 0x0d, 0xfb, 0xaa];
        let encryption = [0x42; 16];
        let mut block = [0u8; 16];
        block[3] = 0x80 | 57;
        let mut block = block.into();
        aes::Aes128::new(&encryption.into()).encrypt_block(&mut block);
        let advertisement = [&[crate::ble::PROXIMITY_PAIRING][..], &[0; 8], &block[..]].concat();
        let keys = || {
            let keys = crate::ble::PodsKeys {
                irk,
                encryption: Some(encryption),
            };
            [(mac(PODS), keys)]
        };
        let mut e = engine();
        e.handle(
            Packet::Battery(vec![crate::aacp::BatteryInfo {
                component: crate::aacp::BatteryComponent::Case,
                level: 60,
                status: Some(crate::aacp::ChargingStatus::Disconnected),
            }]),
            now(),
        );
        assert!(
            e.nearby(address, &advertisement, keys(), now()).is_empty(),
            "connected"
        );
        let case = e.state().battery.case.unwrap();
        assert_eq!(case.level, 57);
        assert!(matches!(
            case.status,
            Some(crate::aacp::ChargingStatus::Disconnected)
        ));
    }

    #[test]
    fn the_setup_follows_the_handshake_ack() {
        let mut e = engine();
        let setup = session_setup_frames().map(Effect::Send);
        assert_eq!(e.handle(Packet::HandshakeAck, now()), setup);
    }

    #[test]
    fn a_reset_keeps_what_is_shown_but_asks_for_the_burst_again() {
        let mut e = engine();
        e.handle(
            Packet::Battery(vec![crate::aacp::BatteryInfo {
                component: crate::aacp::BatteryComponent::Left,
                level: 70,
                status: Some(crate::aacp::ChargingStatus::Discharging),
            }]),
            now(),
        );
        feed(
            &mut e,
            &crate::protocol::set_noise_control(crate::aacp::NoiseControlMode::Transparency),
        );
        feed(&mut e, CONNECTED_DEVICES);
        let mut long_gone = Engine {
            state: e.state.clone(),
            ..engine()
        };
        reconnect(&mut long_gone, CARRY_OVER + Duration::from_secs(1));
        assert!(long_gone.state().battery.is_empty(), "too long ago");
        let mut other_pods = Engine {
            state: e.state.clone(),
            ..engine()
        };
        other_pods.disconnected(now());
        other_pods.connected(mac(PEER), now());
        assert!(other_pods.state().battery.is_empty(), "other pods");
        reconnect(&mut e, Duration::from_secs(6));
        let state = e.state();
        assert!(matches!(
            state.noise_mode,
            Some(crate::aacp::NoiseControlMode::Transparency)
        ));
        let left = state.battery.left.expect("carried over");
        assert_eq!(left.level, 70);
        assert!(!state.battery.is_live(), "last known only");
        assert!(state.hosts.is_none(), "routing starts afresh");
        e.handle(Packet::HandshakeAck, now());
        assert_eq!(
            e.check_state_burst(now()),
            [Effect::Send(request_notifications())],
            "carried levels aren't this session's burst"
        );
    }

    fn battery(
        component: crate::aacp::BatteryComponent,
        status: crate::aacp::ChargingStatus,
    ) -> Packet {
        Packet::Battery(vec![crate::aacp::BatteryInfo {
            component,
            level: 80,
            status: Some(status),
        }])
    }

    /// When the redials after a drop at `now()` come.
    fn redials(e: &mut Engine) -> Vec<u64> {
        std::iter::from_fn(|| {
            let at = e.next_wake()?;
            let fx = e.tick(at);
            assert_eq!(fx, [Effect::ConnectPods(mac(PODS))]);
            Some((at - now()).as_secs())
        })
        .collect()
    }

    #[test]
    fn redials_after_an_unexpected_drop() {
        use crate::aacp::{BatteryComponent::*, Bud, ChargingStatus::*, PrimaryBud};
        let mut e = engine();
        e.handle(
            Packet::PrimaryBud(PrimaryBud {
                bud: Bud::Right,
                active: false,
            }),
            now(),
        );
        // Captured Oct 4 17:14: the other bud asleep in the case, then the
        // primary into the case as that one came out: its live battery
        // report, but no ear report yet.
        e.handle(ears(InEar, EarStatus::Disconnected), now());
        e.handle(ears(InCase, EarStatus::Disconnected), now());
        e.handle(battery(Left, Discharging), now());
        e.disconnected(now());
        assert_eq!(redials(&mut e), [3, 6, 9]);
        // Back: no more redials, and none while the session is up.
        let mut e = engine();
        e.handle(ears(InEar, InEar), now());
        e.disconnected(now());
        e.connected(mac(PODS), now() + Duration::from_secs(1));
        let fx = e.tick(now() + Duration::from_secs(5));
        assert!(!fx.iter().any(|fx| matches!(fx, Effect::ConnectPods(_))));
    }

    #[test]
    fn no_redial_once_stowed_or_without_auto_connect() {
        use crate::aacp::{BatteryComponent::*, Bud, ChargingStatus::*, PrimaryBud};
        let mut e = engine();
        e.handle(
            Packet::PrimaryBud(PrimaryBud {
                bud: Bud::Left,
                active: false,
            }),
            now(),
        );
        e.handle(battery(Right, Disconnected), now());
        e.handle(ears(InCase, EarStatus::Disconnected), now());
        e.disconnected(now());
        assert_eq!(e.next_wake(), None, "the case closed");
        let mut e = engine();
        e.set(Setting::AutoConnect, false);
        e.handle(ears(InEar, InEar), now());
        e.disconnected(now());
        assert_eq!(e.next_wake(), None);
    }

    #[test]
    fn nearby_pods_are_connected_once_per_gap() {
        // The Core spec's sample (see `ble`).
        let mut irk = 0xec0234a357c8ad05341010a60a397d9b_u128.to_be_bytes();
        irk.reverse();
        let address = [0x70, 0x81, 0x94, 0x0d, 0xfb, 0xaa];
        let keys = || {
            let keys = crate::ble::PodsKeys {
                irk,
                encryption: None,
            };
            [(mac(PODS), keys)]
        };
        let mut e = Engine::new(None, Settings::default());
        assert!(
            e.nearby([0x40, 1, 2, 3, 4, 5], &[], keys(), now())
                .is_empty(),
            "a stranger"
        );
        assert_eq!(
            e.nearby(address, &[], keys(), now()),
            [Effect::ConnectPods(mac(PODS))]
        );
        assert!(
            e.nearby(address, &[], keys(), now()).is_empty(),
            "the other bud"
        );
        let later = now() + REDIAL_GAP;
        assert_eq!(
            e.nearby(address, &[], keys(), later),
            [Effect::ConnectPods(mac(PODS))]
        );
        e.connected(mac(PODS), later);
        assert!(
            e.nearby(address, &[], keys(), later + REDIAL_GAP)
                .is_empty(),
            "connected"
        );
        e.disconnected(later);
        e.set(Setting::AutoConnect, false);
        assert!(
            e.nearby(address, &[], keys(), later + REDIAL_GAP)
                .is_empty()
        );
    }

    #[test]
    fn no_switching_without_auto_switch_or_a_session() {
        let mut e = engine();
        feed(&mut e, CONNECTED_DEVICES);
        e.set(Setting::AutoSwitch, false);
        assert!(e.call_started().is_empty());
        e.set(Setting::AutoSwitch, true);
        e.disconnected(now());
        assert!(e.call_started().is_empty());
    }

    #[test]
    fn a_session_silent_after_the_ack_reconnects() {
        // Captured Oct 18 13:36:54: the ack, then nothing for 53 minutes.
        let mut e = engine();
        e.handle(Packet::HandshakeAck, now());
        (0..STARTUP_RETRIES).for_each(|_| {
            assert_eq!(
                e.check_state_burst(now()),
                [Effect::Send(request_notifications())]
            );
        });
        assert_eq!(e.check_state_burst(now()), [Effect::Reconnect]);
        // Answering but holding the state half back: keeps waiting.
        let mut e = engine();
        e.handle(Packet::HandshakeAck, now());
        e.handle(Packet::DeviceInformation(vec!["AirPods Pro".into()]), now());
        (0..STARTUP_RETRIES).for_each(|_| {
            e.check_state_burst(now());
        });
        assert!(e.check_state_burst(now()).is_empty());
    }

    #[test]
    fn rerequests_notifications_only_without_battery() {
        let mut e = engine();
        e.handle(Packet::HandshakeAck, now());
        // Only the settings half.
        e.handle(Packet::DeviceInformation(vec!["AirPods Pro".into()]), now());
        (0..STARTUP_RETRIES).for_each(|_| {
            assert_eq!(
                e.check_state_burst(now()),
                [Effect::Send(request_notifications())]
            );
        });
        assert!(e.check_state_burst(now()).is_empty(), "gives up");
        let mut e = engine();
        e.handle(Packet::HandshakeAck, now());
        e.handle(
            Packet::Battery(vec![crate::aacp::BatteryInfo {
                component: crate::aacp::BatteryComponent::Left,
                level: 80,
                status: None,
            }]),
            now(),
        );
        assert!(e.check_state_burst(now()).is_empty());
    }

    #[test]
    fn a_call_here_takes_the_audio_even_from_a_call() {
        let mut e = engine();
        feed(&mut e, CONNECTED_DEVICES);
        e.handle(
            Packet::AudioSource(AudioSource {
                mac: mac(PEER),
                kind: AudioSourceType::Call,
            }),
            now(),
        );
        assert_eq!(sends(&e.call_started()), 4);
        assert_eq!(sends(&e.call_started()), 4, "unconfirmed: again");
    }

    #[test]
    fn media_starting_here_takes_the_audio() {
        let source = |mac, kind| Packet::AudioSource(AudioSource { mac, kind });
        let mut e = engine();
        feed(&mut e, CONNECTED_DEVICES);
        assert!(e.media_started().is_empty(), "nothing known yet");
        e.handle(source(mac(PEER), AudioSourceType::Call), now());
        assert!(e.media_started().is_empty(), "the peer is on a call");
        e.handle(source(mac(PEER), AudioSourceType::Media), now());
        assert_eq!(sends(&e.media_started()), 4, "ownership + 3 handoff frames");
        assert_eq!(sends(&e.media_started()), 4, "unconfirmed: again");
        feed(&mut e, &owns_connection(true));
        assert!(e.media_started().is_empty(), "confirmed");
    }

    #[test]
    fn release_forgets_an_ear_pause() {
        let mut e = engine();
        playing(&mut e, AudioSourceType::Media);
        e.handle(ears(EarStatus::InEar, EarStatus::InEar), now());
        assert_eq!(
            e.handle(ears(EarStatus::InEar, EarStatus::NotInEar), now()),
            vec![Effect::Pause]
        );
        feed(&mut e, &yield_frame());
        assert!(
            e.handle(ears(EarStatus::InEar, EarStatus::InEar), now())
                .is_empty()
        );
    }

    #[test]
    fn both_out_pauses_without_resume() {
        let mut e = engine();
        e.handle(ears(InEar, InEar), now());
        playing(&mut e, AudioSourceType::Media);
        assert_eq!(e.handle(ears(InEar, NotInEar), now()), vec![Effect::Pause]);
        assert!(e.handle(ears(InCase, NotInEar), now()).is_empty());
        assert!(e.handle(ears(InEar, InEar), now()).is_empty());
    }

    #[test]
    fn no_pause_unless_playing_media_here() {
        let mut e = engine();
        e.handle(ears(InEar, InEar), now());
        assert!(
            e.handle(ears(InEar, NotInEar), now()).is_empty(),
            "nothing playing"
        );
        e.handle(ears(InEar, InEar), now());
        playing(&mut e, AudioSourceType::Call);
        assert!(
            e.handle(ears(InEar, NotInEar), now()).is_empty(),
            "calls are not paused"
        );
        e.handle(ears(InEar, InEar), now());
        e.handle(
            Packet::AudioSource(AudioSource {
                mac: mac(PEER),
                kind: AudioSourceType::Media,
            }),
            now(),
        );
        assert!(
            e.handle(ears(InEar, NotInEar), now()).is_empty(),
            "another device plays"
        );
    }

    #[test]
    fn declined_handoff_releases_audio_without_a_message() {
        let mut e = engine();
        feed(&mut e, CONNECTED_DEVICES);
        e.take();
        let declined = routing_frame(
            b"audioRoutingSetOwnershipToFalse audioRoutingShowReverseUI reason ReverseBannerTapped",
        );
        assert_eq!(feed(&mut e, &declined), vec![Effect::ReleaseAudio]);
        assert_eq!(e.ownership(), Ownership::Idle);
    }

    /// Both buds in an ear.
    fn worn(e: &mut Engine) {
        e.handle(
            Packet::PrimaryBud(crate::aacp::PrimaryBud {
                bud: crate::aacp::Bud::Left,
                active: false,
            }),
            now(),
        );
        e.handle(ears(InEar, InEar), now());
    }

    #[test]
    fn alone_with_the_buds_in_the_case_takes_once_one_is_out() {
        // Captured 22:36: pods charging in the case, connected; as the only
        // host the laptop took the audio from other headphones.
        let mut e = engine();
        e.handle(
            Packet::PrimaryBud(crate::aacp::PrimaryBud {
                bud: crate::aacp::Bud::Left,
                active: false,
            }),
            now(),
        );
        assert!(
            feed(&mut e, &only_local_connected()).is_empty(),
            "ears unknown"
        );
        assert!(e.handle(ears(InCase, InCase), now()).is_empty());
        assert_eq!(
            e.handle(ears(NotInEar, InCase), now()),
            [Effect::RouteAudio]
        );
    }

    /// `ConnectedDevices` listing only LOCAL.
    fn only_local_connected() -> Vec<u8> {
        [&CONNECTED_DEVICES[..8], &[0x01], &CONNECTED_DEVICES[17..]].concat()
    }

    #[test]
    fn takes_audio_when_alone() {
        let mut e = engine();
        worn(&mut e);
        assert_eq!(
            feed(&mut e, &only_local_connected()),
            vec![Effect::RouteAudio]
        );
        assert_eq!(e.ownership(), Ownership::Owning(Playback::AsIs));
        // Already holding it: no repeat.
        assert!(feed(&mut e, &only_local_connected()).is_empty());
    }

    #[test]
    fn no_auto_take_with_another_device_connected() {
        let mut e = engine();
        assert!(feed(&mut e, CONNECTED_DEVICES).is_empty());
        assert_eq!(e.ownership(), Ownership::Idle);
    }

    #[test]
    fn takes_audio_when_the_peer_leaves() {
        let mut e = engine();
        worn(&mut e);
        feed(&mut e, CONNECTED_DEVICES);
        assert_eq!(
            feed(&mut e, &only_local_connected()),
            vec![Effect::RouteAudio]
        );
    }

    /// `DeviceConnection`: PEER (a host) disconnected, as captured.
    fn peer_disconnected() -> Vec<u8> {
        vec![
            0x04, 0x00, 0x04, 0x00, 0x0c, 0x00, 0xaa, 0x99, 0x88, 0x77, 0x66, 0x02, 0x00, 0x01,
        ]
    }

    #[test]
    fn no_auto_take_from_the_partial_burst_before_the_list() {
        // Captured order at session start: DeviceConnection(us, connected)
        // arrives before the ConnectedDevices list naming the other host too.
        let mut e = engine();
        let us_connected = [
            0x04, 0x00, 0x04, 0x00, 0x0c, 0x00, 0x55, 0x44, 0x33, 0x22, 0x11, 0x02, 0x01, 0x01,
        ];
        assert!(feed(&mut e, &us_connected).is_empty());
        assert!(feed(&mut e, CONNECTED_DEVICES).is_empty());
        assert_eq!(e.ownership(), Ownership::Idle);
    }

    #[test]
    fn takes_audio_when_the_peer_disconnects() {
        // The pods announce a host leaving with DeviceConnection; lists only
        // come again with a primary switch.
        let mut e = engine();
        worn(&mut e);
        assert!(feed(&mut e, CONNECTED_DEVICES).is_empty());
        assert_eq!(feed(&mut e, &peer_disconnected()), vec![Effect::RouteAudio]);
        assert_eq!(e.ownership(), Ownership::Owning(Playback::AsIs));
    }

    #[test]
    fn no_auto_take_from_a_bud_reconnecting() {
        // Released while alone (yielded): a bud reconnecting changes no
        // hosts, so it mustn't take the audio back.
        let mut e = engine();
        worn(&mut e);
        feed(&mut e, CONNECTED_DEVICES);
        feed(&mut e, &peer_disconnected());
        assert_eq!(feed(&mut e, &yield_frame()), released());
        let bud_connected = [
            0x04, 0x00, 0x04, 0x00, 0x0c, 0x00, 0x02, 0xff, 0xee, 0xdd, 0xcc, 0x02, 0x01, 0x02,
        ];
        assert!(feed(&mut e, &bud_connected).is_empty());
        assert_eq!(e.ownership(), Ownership::Idle);
    }

    /// `DeviceConnection(PEER, connected, host)`: the phone's own entry, which
    /// leads every session's burst on the phone.
    fn first_host_announced() -> Vec<u8> {
        vec![
            0x04, 0x00, 0x04, 0x00, 0x0c, 0x00, 0xaa, 0x99, 0x88, 0x77, 0x66, 0x02, 0x01, 0x01,
        ]
    }

    #[test]
    fn infers_local_mac_from_the_first_announced_host() {
        // Fresh install, laptop connected: smart routing works right away.
        let mut e = session(None);
        assert!(
            feed(&mut e, &first_host_announced()).is_empty(),
            "not persisted"
        );
        assert!(feed(&mut e, CONNECTED_DEVICES).is_empty());
        assert_eq!(e.local_mac(), Some(mac(PEER)));
        // Take targets the other host (LOCAL from the test's perspective).
        assert_eq!(sends(&e.take()), 4);
        // Not kept across sessions until confirmed.
        reconnect(&mut e, Duration::from_secs(1));
        assert_eq!(e.local_mac(), None);
    }

    /// History as captured on a fresh connect: LOCAL (the laptop) connected
    /// `laptop_age` ago, PEER (us, from the test's perspective) `own_age` ago.
    fn history(laptop_age: u64, own_age: u64) -> Packet {
        use crate::aacp::{ConnectionHistory, HistoryEvent, LinkChange};
        let now = Duration::from_secs(629);
        let event = |mac, age| HistoryEvent {
            time: now - Duration::from_secs(age),
            mac,
            change: LinkChange::HostConnected,
        };
        Packet::ConnectionHistory(ConnectionHistory {
            now,
            events: vec![event(mac(LOCAL), laptop_age), event(mac(PEER), own_age)],
        })
    }

    #[test]
    fn infers_local_mac_from_the_just_connected_host() {
        // Fresh connect with the laptop connected: the list comes before any
        // DeviceConnection, so only the history tells.
        let mut e = session(None);
        assert!(e.handle(history(7, 0), now()).is_empty(), "not persisted");
        feed(&mut e, CONNECTED_DEVICES);
        assert_eq!(e.local_mac(), Some(mac(PEER)));
    }

    #[test]
    fn take_hands_off_to_a_host_named_only_by_the_history() {
        // Linux capture: the session got the history but never a device list.
        let mut e = engine();
        e.handle(history(1, 33), now());
        assert_eq!(sends(&e.take()), 4, "ownership + 3 handoff frames");
    }

    #[test]
    fn no_inference_from_an_old_connection() {
        let mut e = session(None);
        e.handle(history(60, 30), now());
        feed(&mut e, CONNECTED_DEVICES);
        assert_eq!(e.local_mac(), None);
    }

    #[test]
    fn inferred_local_mac_is_confirmed_when_alone() {
        let mut e = session(None);
        feed(&mut e, &first_host_announced());
        feed(&mut e, CONNECTED_DEVICES);
        let laptop_left = [
            0x04, 0x00, 0x04, 0x00, 0x0c, 0x00, 0x55, 0x44, 0x33, 0x22, 0x11, 0x02, 0x00, 0x01,
        ];
        let fx = feed(&mut e, &laptop_left);
        assert_eq!(fx[0], Effect::LearnedLocalMac(mac(PEER)), "now persisted");
        reconnect(&mut e, Duration::from_secs(1));
        assert_eq!(e.local_mac(), Some(mac(PEER)));
    }

    #[test]
    fn inferred_local_mac_missing_from_the_list_is_dropped() {
        let mut e = session(None);
        let stranger = [
            0x04, 0x00, 0x04, 0x00, 0x0c, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x01, 0x01,
        ];
        feed(&mut e, &stranger);
        feed(&mut e, CONNECTED_DEVICES);
        assert_eq!(e.local_mac(), None);
    }

    #[test]
    fn learns_local_mac_when_the_other_host_disconnects() {
        // Fresh install with the laptop connected: no one-entry list ever
        // comes, but the laptop leaving leaves us as the only host.
        let mut e = session(None);
        worn(&mut e);
        assert!(feed(&mut e, CONNECTED_DEVICES).is_empty());
        let laptop_left = [
            0x04, 0x00, 0x04, 0x00, 0x0c, 0x00, 0x55, 0x44, 0x33, 0x22, 0x11, 0x02, 0x00, 0x01,
        ];
        assert_eq!(
            feed(&mut e, &laptop_left),
            vec![Effect::LearnedLocalMac(mac(PEER)), Effect::RouteAudio]
        );
        assert_eq!(e.local_mac(), Some(mac(PEER)));
    }

    #[test]
    fn learns_local_mac_from_single_connected_device() {
        let mut e = session(None);
        worn(&mut e);
        let single = only_local_connected();
        assert_eq!(
            feed(&mut e, &single),
            vec![Effect::LearnedLocalMac(mac(LOCAL)), Effect::RouteAudio]
        );
        assert_eq!(e.local_mac(), Some(mac(LOCAL)));
        assert!(feed(&mut e, &single).is_empty());
    }
}
