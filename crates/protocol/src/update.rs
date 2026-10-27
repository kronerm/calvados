//! Updating the buds' firmware over UARP, sans-IO: [`Updater`] offers a
//! checked [`Package`] and serves what the primary bud asks for until it
//! reports the image staged; [`check`] says whether to start at all; and
//! [`News`] tells the other calvados hosts how it goes, through the pods'
//! smart-routing relay.
//!
//! Never an apply: with both buds in the case, the case copies the staged
//! image to the other bud and applies both once the lid is closed (~30
//! minutes; docs/sibling-and-case.md). [`crate::uarp`] has no message for it.

use std::fmt;
use std::time::{Duration, Instant};

use crate::firmware::{ExactVersion, Version};
use crate::state::DeviceState;
use crate::superbinary::Package;
use crate::uarp::{MAX_DATA, Message, Processing, Received, UNKNOWN_ASSET};

/// Our offer's asset id, as `uarp_offer.py` used.
const ASSET: u16 = 1;

/// No request for this long (or no transfer starting this long after the
/// offer): rescind. The pods keep no timeout of their own.
pub const WATCHDOG: Duration = Duration::from_secs(60);

/// How long a dropped transport may stay down before giving up.
const INTERRUPTED_WAIT: Duration = Duration::from_secs(5 * 60);

/// Transport drops in a row (without a request served in between) taken
/// before giving up.
const DROPS: u8 = 3;

/// Where an update stands, local or a peer's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateStatus {
    /// Fetching the package (the platform's part).
    Downloading,
    /// Offered, or about to be; nothing asked for yet.
    Offered,
    /// The pods acknowledged the offer, asked for nothing yet: they decide
    /// at once when charging in an open case, else defer.
    Deferred,
    /// `percent` of `FTAB` is with the pods.
    Transferring {
        percent: u8,
    },
    /// The transport dropped: offered again (the pods continue where they
    /// were) once it's back.
    Interrupted {
        percent: u8,
    },
    /// Fully staged on the primary: close the lid.
    Staged,
    /// The pods run it.
    Done(ExactVersion),
    Failed(Failure),
}

impl UpdateStatus {
    /// Not ended yet: [`Self::Staged`] is up to the case.
    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Downloading
                | Self::Offered
                | Self::Deferred
                | Self::Transferring { .. }
                | Self::Interrupted { .. }
        )
    }

    /// As D-Bus and `calvados watch` name it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Downloading => "downloading",
            Self::Offered => "offered",
            Self::Deferred => "deferred",
            Self::Transferring { .. } => "transferring",
            Self::Interrupted { .. } => "interrupted",
            Self::Staged => "staged",
            Self::Done(_) => "done",
            Self::Failed(_) => "failed",
        }
    }

    /// Of `FTAB`, transferred.
    pub fn percent(self) -> u8 {
        match self {
            Self::Transferring { percent } | Self::Interrupted { percent } => percent,
            Self::Staged | Self::Done(_) => 100,
            _ => 0,
        }
    }

    /// The failure's reason, or the version done; empty otherwise.
    pub fn detail(self) -> String {
        match self {
            Self::Failed(failure) => failure.to_string(),
            Self::Done(version) => version.to_string(),
            _ => String::new(),
        }
    }
}

/// Why an update ended without staging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The package couldn't be fetched or didn't check out.
    Download,
    /// Things changed while downloading.
    Refused(Refusal),
    /// Processing notification `0x?2`: e.g. state left by an earlier offer;
    /// closing and reopening the case clears it.
    Denied {
        reason: u8,
    },
    /// `0x?3`: superseded, or merged into another host's identical offer.
    Abandoned {
        reason: u8,
    },
    /// `0x?4`: a bad header or data.
    Corrupt {
        reason: u8,
    },
    /// Nothing asked for within [`WATCHDOG`] of the offer.
    NotStarted,
    /// No request for [`WATCHDOG`] mid-transfer.
    Stalled,
    /// The transport kept dropping, or stayed down.
    Lost,
    Cancelled,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Download => f.write_str("the download failed"),
            Self::Refused(refusal) => write!(f, "{refusal}"),
            Self::Denied { reason } => {
                write!(f, "declined (reason {reason}); reopen the case and retry")
            }
            Self::Abandoned { reason } => write!(f, "abandoned (reason {reason})"),
            Self::Corrupt { reason } => write!(f, "found corrupt (reason {reason})"),
            Self::NotStarted => f.write_str("never started; reopen the case and retry"),
            Self::Stalled => f.write_str("the transfer stalled"),
            Self::Lost => f.write_str("the connection was lost"),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}

/// Why an update isn't started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    NotConnected,
    /// No release known for the pods yet (the platform's catalog check).
    NoRelease,
    /// This host already updates them.
    Running,
    /// Another host updates them (its [`News`]).
    PeerRunning,
    NotInCase,
    /// Neither the exact nor the reported running firmware is known.
    VersionUnknown,
    /// They run it already, or newer (the pods refuse downgrades anyway).
    NotNewer,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotConnected => "the AirPods aren't connected",
            Self::NoRelease => "no firmware release known for these AirPods yet",
            Self::Running => "an update is already running",
            Self::PeerRunning => "another device is updating the AirPods",
            Self::NotInCase => "put both AirPods in the case, and keep the lid open",
            Self::VersionUnknown => "the AirPods' firmware version isn't known yet",
            Self::NotNewer => "the AirPods already run this firmware or a newer one",
        })
    }
}

impl std::error::Error for Refusal {}

/// Whether pods in `state` take `version`: both buds in the open case (an
/// AACP session reports their ears) and `version` newer than what they run:
/// exactly when known, else by major.minor (equal counts as not newer).
/// Power is the pods' call: they defer an offer until the case charges.
pub fn check(state: &DeviceState, version: ExactVersion) -> Result<(), Refusal> {
    let newer = match (state.firmware.exact, state.firmware.version()) {
        (Some(exact), _) => version > exact,
        (None, Some(running)) => version.version > running,
        (None, None) => return Err(Refusal::VersionUnknown),
    };
    match (state.in_case(), newer) {
        (false, _) => Err(Refusal::NotInCase),
        (true, false) => Err(Refusal::NotNewer),
        (true, true) => Ok(()),
    }
}

/// What an [`Updater`] asks for.
#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub enum Output {
    /// Send this UARP message, after the ones before.
    Write(Vec<u8>),
    /// The status changed.
    Status(UpdateStatus),
}

#[derive(Debug, Clone, Copy)]
enum Phase {
    Offered,
    Deferred,
    Transferring,
    Staged,
    Done(ExactVersion),
    Failed(Failure),
}

/// Whether UARP may be sent: the session's settled (the engine decides).
#[derive(Debug, Clone, Copy)]
enum Transport {
    /// Not up yet.
    Opening,
    Up,
    /// Was up, dropped.
    Lost,
}

/// One update of one pair: report the transport with [`Updater::up`] and
/// [`Updater::down`], feed it the pods' messages and [`Updater::tick`] at
/// [`Updater::next_wake`], perform the [`Output`]s.
#[derive(Debug)]
pub struct Updater {
    package: Package,
    phase: Phase,
    transport: Transport,
    next_id: u16,
    /// When the watchdog fires, while active.
    deadline: Instant,
    /// Transport drops since the last request served.
    drops: u8,
    /// How much of `FTAB` the pods have, by their last request.
    position: u32,
}

impl Updater {
    pub fn new(package: Package, now: Instant) -> Self {
        tracing::info!(version = %package.version(), "firmware update: starting");
        Self {
            package,
            phase: Phase::Offered,
            transport: Transport::Opening,
            next_id: 1,
            deadline: now + WATCHDOG,
            drops: 0,
            position: 0,
        }
    }

    pub fn version(&self) -> ExactVersion {
        self.package.version()
    }

    pub fn status(&self) -> UpdateStatus {
        let ftab = self.package.ftab();
        let size = u64::from(ftab.end - ftab.start).max(1);
        let percent = u8::try_from(u64::from(self.position) * 100 / size).unwrap_or(100);
        match (self.phase, self.transport) {
            (Phase::Offered | Phase::Deferred | Phase::Transferring, Transport::Lost) => {
                UpdateStatus::Interrupted { percent }
            }
            (Phase::Offered, _) => UpdateStatus::Offered,
            (Phase::Deferred, _) => UpdateStatus::Deferred,
            (Phase::Transferring, _) => UpdateStatus::Transferring { percent },
            (Phase::Staged, _) => UpdateStatus::Staged,
            (Phase::Done(version), _) => UpdateStatus::Done(version),
            (Phase::Failed(failure), _) => UpdateStatus::Failed(failure),
        }
    }

    /// Whether it may send.
    pub fn is_up(&self) -> bool {
        matches!(self.transport, Transport::Up)
    }

    /// When [`Self::tick`] is due.
    pub fn next_wake(&self) -> Option<Instant> {
        self.status().is_active().then_some(self.deadline)
    }

    /// UARP may be sent (again): offer. Re-offering the identical package
    /// within the pods' boot continues where they were (docs/update-host.md).
    pub fn up(&mut self, now: Instant) -> Vec<Output> {
        if !self.status().is_active() {
            return Vec::new();
        }
        tracing::info!(
            resume = matches!(self.transport, Transport::Lost),
            "firmware update: offering"
        );
        self.change(|updater| {
            updater.transport = Transport::Up;
            updater.next_id = 1;
            updater.deadline = now + WATCHDOG;
            if let Phase::Deferred = updater.phase {
                updater.phase = Phase::Offered;
            }
            let offer = Message::Offer {
                asset: ASSET,
                version: updater.package.raw_version(),
                length: updater.package.length(),
                payloads: updater.package.payloads(),
            };
            [Message::Sync, Message::VersionDiscovery, offer]
                .into_iter()
                .map(|message| updater.encode(message))
                .collect()
        })
    }

    /// UARP may not be sent any more (the session dropped, the primary
    /// changes): wait for it, a few times.
    pub fn down(&mut self, now: Instant) -> Vec<Output> {
        if !self.status().is_active() {
            self.transport = Transport::Lost;
            return Vec::new();
        }
        self.change(|updater| {
            updater.drops += 1;
            updater.transport = Transport::Lost;
            match updater.drops > DROPS {
                true => updater.phase = Phase::Failed(Failure::Lost),
                false => updater.deadline = now + INTERRUPTED_WAIT,
            }
            tracing::info!(drops = updater.drops, "firmware update: transport down");
            Vec::new()
        })
    }

    /// A message from the pods.
    pub fn received(&mut self, message: &[u8], now: Instant) -> Vec<Output> {
        let Some(received) = Received::parse(message) else {
            tracing::warn!(
                len = message.len(),
                "firmware update: malformed UARP message"
            );
            return Vec::new();
        };
        self.change(|updater| updater.handle(received, now))
    }

    fn handle(&mut self, received: Received, now: Instant) -> Vec<Vec<u8>> {
        let active = self.status().is_active();
        match received {
            Received::OfferAck { asset: ASSET } => {
                tracing::info!("firmware update: offer acknowledged");
                if let Phase::Offered = self.phase {
                    self.phase = Phase::Deferred;
                }
                Vec::new()
            }
            Received::DataRequest {
                asset,
                offset,
                length,
            } if active => vec![self.serve(asset, offset, length, now)],
            Received::Processing { asset, flags } => {
                let ack = self.encode(Message::ProcessingAck { asset, flags });
                let outcome = Processing::of(flags);
                tracing::info!(asset, ?outcome, "firmware update: processing notification");
                self.phase = match (asset, outcome, active) {
                    (ASSET, Processing::Staged, true) => Phase::Staged,
                    (ASSET, Processing::Denied { reason }, true) => {
                        Phase::Failed(Failure::Denied { reason })
                    }
                    (ASSET, Processing::Abandoned { reason }, true) => {
                        Phase::Failed(Failure::Abandoned { reason })
                    }
                    (ASSET, Processing::Corrupt { reason }, true) => {
                        Phase::Failed(Failure::Corrupt { reason })
                    }
                    _ => self.phase,
                };
                vec![ack]
            }
            Received::Version { status, version } => {
                tracing::info!(status, version, "firmware update: protocol version");
                Vec::new()
            }
            other => {
                tracing::debug!(?other, "firmware update: received");
                Vec::new()
            }
        }
    }

    /// What's asked, from the package, as much as fits a message (the
    /// pods ask on from where it ends); an unknown asset or an offset past
    /// the end gets [`UNKNOWN_ASSET`] (the pods then abandon).
    fn serve(&mut self, asset: u16, offset: u32, length: u16, now: Instant) -> Vec<u8> {
        let id = self.take_id();
        let data = match asset {
            ASSET => self.package.read(offset, length.min(MAX_DATA)),
            _ => None,
        };
        let Some(data) = data else {
            tracing::warn!(
                asset,
                offset,
                length,
                "firmware update: request for nothing we have"
            );
            return Message::DataResponse {
                status: UNKNOWN_ASSET,
                asset,
                offset,
                requested: length,
                data: &[],
            }
            .encode(id);
        };
        let ftab = self.package.ftab();
        if ftab.contains(&offset) {
            let served = u32::try_from(data.len()).unwrap_or(0);
            self.position = offset + served - ftab.start;
        }
        if let Phase::Offered | Phase::Deferred = self.phase {
            tracing::info!(offset, length, "firmware update: the pods started pulling");
            self.phase = Phase::Transferring;
        }
        self.deadline = now + WATCHDOG;
        self.drops = 0;
        Message::DataResponse {
            status: 0,
            asset,
            offset,
            requested: length,
            data,
        }
        .encode(id)
    }

    /// The watchdog: nothing asked for in time (rescinded), or the
    /// transport stayed down.
    pub fn tick(&mut self, now: Instant) -> Vec<Output> {
        if !self.status().is_active() || now < self.deadline {
            return Vec::new();
        }
        let failure = match (self.phase, self.transport) {
            (_, Transport::Lost) => Failure::Lost,
            (Phase::Transferring, _) => Failure::Stalled,
            _ => Failure::NotStarted,
        };
        tracing::warn!(%failure, "firmware update: giving up");
        self.change(|updater| updater.fail(failure))
    }

    /// Withdraw the offer.
    pub fn cancel(&mut self) -> Vec<Output> {
        if !self.status().is_active() {
            return Vec::new();
        }
        tracing::info!("firmware update: cancelled");
        self.change(|updater| updater.fail(Failure::Cancelled))
    }

    /// The pods' running firmware, learned anew: once it's the staged one,
    /// done.
    pub fn running(&mut self, running: ExactVersion) -> Vec<Output> {
        match self.phase {
            Phase::Staged if running >= self.package.version() => {
                tracing::info!(%running, "firmware update: done");
                self.change(|updater| {
                    updater.phase = Phase::Done(running);
                    Vec::new()
                })
            }
            _ => Vec::new(),
        }
    }

    /// Rescind (when the transport's up) and fail.
    fn fail(&mut self, failure: Failure) -> Vec<Vec<u8>> {
        self.phase = Phase::Failed(failure);
        match self.transport {
            Transport::Up => vec![self.encode(Message::Rescind { asset: ASSET })],
            Transport::Opening | Transport::Lost => Vec::new(),
        }
    }

    fn take_id(&mut self) -> u16 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }

    fn encode(&mut self, message: Message<'_>) -> Vec<u8> {
        message.encode(self.take_id())
    }

    /// Run `step` (returning whole messages), then the writes and, if the
    /// status changed, the new one.
    fn change(&mut self, step: impl FnOnce(&mut Self) -> Vec<Vec<u8>>) -> Vec<Output> {
        let before = self.status();
        let messages = step(self);
        let after = self.status();
        let writes = match self.transport {
            Transport::Up => messages.into_iter().map(Output::Write).collect(),
            Transport::Opening | Transport::Lost => Vec::new(),
        };
        if after != before {
            tracing::info!(status = ?after, "firmware update");
        }
        writes
            .into_iter()
            .chain((after != before).then_some(Output::Status(after)))
            .collect()
    }
}

/// What calvados hosts tell each other through the pods' smart-routing
/// relay (sent as 0x10 to a peer, arriving as 0x11): only a host that asks
/// UARP knows the exact firmware (Linux), and only the updating host knows
/// the update's progress. Starts with [`News::MAGIC`], so nothing else's
/// frames are taken for ours.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub enum News {
    /// The pods' exact running firmware.
    Firmware(ExactVersion),
    /// The sender's update.
    Update(UpdateStatus),
}

impl News {
    pub const MAGIC: &[u8; 8] = b"calvados";
    /// This encoding's version: a host ignores another's.
    const FORMAT: u8 = 1;

    /// `MAGIC`, `FORMAT`, then `1` + version (3 × u32 BE), or `2` + state +
    /// two arguments + version (zero unless done).
    pub fn encode(self) -> Vec<u8> {
        let version = |version: ExactVersion| {
            [
                version.version.major,
                version.version.minor,
                version.release,
            ]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect::<Vec<_>>()
        };
        let none = ExactVersion {
            version: Version { major: 0, minor: 0 },
            release: 0,
        };
        let body = match self {
            Self::Firmware(running) => [&[1][..], &version(running)].concat(),
            Self::Update(status) => {
                let (state, a, b, done) = match status {
                    UpdateStatus::Downloading => (1, 0, 0, none),
                    UpdateStatus::Offered => (2, 0, 0, none),
                    UpdateStatus::Deferred => (3, 0, 0, none),
                    UpdateStatus::Transferring { percent } => (4, percent, 0, none),
                    UpdateStatus::Interrupted { percent } => (5, percent, 0, none),
                    UpdateStatus::Staged => (6, 0, 0, none),
                    UpdateStatus::Done(version) => (7, 0, 0, version),
                    UpdateStatus::Failed(failure) => {
                        let (code, argument) = failure_code(failure);
                        (8, code, argument, none)
                    }
                };
                [&[2, state, a, b][..], &version(done)].concat()
            }
        };
        [&Self::MAGIC[..], &[Self::FORMAT], &body].concat()
    }

    /// `None` unless it's ours, in a format and with codes we know.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let rest = bytes.strip_prefix(&Self::MAGIC[..])?;
        let (&format, rest) = rest.split_first()?;
        if format != Self::FORMAT {
            return None;
        }
        let version = |bytes: &[u8]| {
            let words = bytes
                .get(..12)?
                .as_chunks::<4>()
                .0
                .iter()
                .map(|word| u32::from_be_bytes(*word))
                .collect::<Vec<_>>();
            let [major, minor, release] = words[..] else {
                return None;
            };
            Some(ExactVersion {
                version: Version { major, minor },
                release,
            })
        };
        match rest {
            [1, running @ ..] => Some(Self::Firmware(version(running)?)),
            [2, state, a, b, done @ ..] => Some(Self::Update(match (state, a) {
                (1, _) => UpdateStatus::Downloading,
                (2, _) => UpdateStatus::Offered,
                (3, _) => UpdateStatus::Deferred,
                (4, &percent) => UpdateStatus::Transferring { percent },
                (5, &percent) => UpdateStatus::Interrupted { percent },
                (6, _) => UpdateStatus::Staged,
                (7, _) => UpdateStatus::Done(version(done)?),
                (8, &code) => UpdateStatus::Failed(failure_of(code, *b)?),
                _ => return None,
            })),
            _ => None,
        }
    }
}

const REFUSALS: [Refusal; 7] = [
    Refusal::NotConnected,
    Refusal::NoRelease,
    Refusal::Running,
    Refusal::PeerRunning,
    Refusal::NotInCase,
    Refusal::VersionUnknown,
    Refusal::NotNewer,
];

fn failure_code(failure: Failure) -> (u8, u8) {
    match failure {
        Failure::Download => (1, 0),
        Failure::Refused(refusal) => (
            2,
            REFUSALS
                .iter()
                .position(|known| *known == refusal)
                .and_then(|at| u8::try_from(at).ok())
                .unwrap_or(u8::MAX),
        ),
        Failure::Denied { reason } => (3, reason),
        Failure::Abandoned { reason } => (4, reason),
        Failure::Corrupt { reason } => (5, reason),
        Failure::NotStarted => (6, 0),
        Failure::Stalled => (7, 0),
        Failure::Lost => (8, 0),
        Failure::Cancelled => (9, 0),
    }
}

fn failure_of(code: u8, argument: u8) -> Option<Failure> {
    Some(match code {
        1 => Failure::Download,
        2 => Failure::Refused(*REFUSALS.get(usize::from(argument))?),
        3 => Failure::Denied { reason: argument },
        4 => Failure::Abandoned { reason: argument },
        5 => Failure::Corrupt { reason: argument },
        6 => Failure::NotStarted,
        7 => Failure::Stalled,
        8 => Failure::Lost,
        9 => Failure::Cancelled,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aacp::{
        BatteryComponent, BatteryInfo, ChargingStatus, EarDetection, EarStatus, Packet,
    };
    use crate::superbinary::tests::{package, package_bytes, package_bytes_with};
    use crate::uarp;

    fn now() -> Instant {
        static NOW: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now);
        *NOW
    }

    /// A message from the pods.
    fn from_pods(kind: u16, id: u16, payload: &[u8]) -> Vec<u8> {
        let len = u16::try_from(payload.len()).unwrap();
        [
            &kind.to_be_bytes()[..],
            &len.to_be_bytes(),
            &id.to_be_bytes(),
            payload,
        ]
        .concat()
    }

    fn request(offset: u32, length: u16) -> Vec<u8> {
        from_pods(
            6,
            9,
            &[
                &1u16.to_be_bytes()[..],
                &offset.to_be_bytes(),
                &length.to_be_bytes(),
            ]
            .concat(),
        )
    }

    fn processing(flags: u16) -> Vec<u8> {
        from_pods(
            9,
            9,
            &[&1u16.to_be_bytes()[..], &flags.to_be_bytes()].concat(),
        )
    }

    /// The writes' messages, by type.
    fn kinds(outputs: &[Output]) -> Vec<u16> {
        outputs
            .iter()
            .filter_map(|output| match output {
                Output::Write(write) => Some(u16::from_be_bytes([write[0], write[1]])),
                Output::Status(_) => None,
            })
            .collect()
    }

    fn statuses(outputs: &[Output]) -> Vec<UpdateStatus> {
        outputs
            .iter()
            .filter_map(|output| match output {
                Output::Status(status) => Some(*status),
                Output::Write(_) => None,
            })
            .collect()
    }

    /// The data of a data response write.
    fn served(outputs: &[Output]) -> Vec<u8> {
        outputs
            .iter()
            .find_map(|output| match output {
                Output::Write(write) if write[0..2] == [0, 7] => Some(write[18..].to_vec()),
                _ => None,
            })
            .unwrap()
    }

    fn started() -> Updater {
        let mut updater = Updater::new(package(), now());
        updater.up(now());
        updater
    }

    #[test]
    fn a_whole_update_serves_exactly_what_is_asked_and_never_applies() {
        let bytes = package_bytes();
        let ftab = package().ftab();
        let mut updater = Updater::new(package(), now());
        assert_eq!(updater.status(), UpdateStatus::Offered);
        let mut sent = Vec::new();
        let offered = updater.up(now());
        assert_eq!(kinds(&offered), [0x00, 0x01, 0x05]);
        sent.extend(offered);
        // The offer acknowledged: deferred until the first request.
        let mut echo = [0; 30];
        echo[7] = 1;
        let ack = from_pods(0x0d, 1, &echo);
        assert_eq!(
            statuses(&updater.received(&ack, now())),
            [UpdateStatus::Deferred]
        );
        // Header, metadata, then FTAB in two halves, the last one cut at
        // the binary's end.
        let half = (ftab.end - ftab.start) / 2;
        for (offset, length) in [
            (0, 0x2c),
            (0x2c, 0x28),
            (ftab.start, half as u16),
            (ftab.start + half, 475),
        ] {
            let outputs = updater.received(&request(offset, length), now());
            let end =
                (offset as usize + usize::from(length)).min(updater.package.length() as usize);
            assert_eq!(served(&outputs), bytes[offset as usize..end]);
            sent.extend(outputs);
        }
        assert_eq!(
            updater.status(),
            UpdateStatus::Transferring { percent: 100 }
        );
        let staged = updater.received(&processing(0x0001), now());
        assert_eq!(kinds(&staged), [0x0f]);
        assert_eq!(statuses(&staged), [UpdateStatus::Staged]);
        sent.extend(staged);
        assert!(!kinds(&sent).contains(&0x0a), "never an apply");
        assert!(!kinds(&sent).contains(&0x0c), "a staged image stays");
        assert_eq!(updater.next_wake(), None);
        assert_eq!(updater.tick(now() + WATCHDOG * 10), []);
        // Once the case applied it, the pods run it.
        assert_eq!(updater.running("9.0.347".parse().unwrap()), []);
        let done = updater.running("9.0.348".parse().unwrap());
        assert_eq!(
            statuses(&done),
            [UpdateStatus::Done("9.0.348".parse().unwrap())]
        );
    }

    #[test]
    fn progress_counts_ftab_only() {
        let ftab = package().ftab();
        let mut updater = started();
        updater.received(&request(0, 0x2c), now());
        assert_eq!(updater.status(), UpdateStatus::Transferring { percent: 0 });
        let quarter = u16::try_from((ftab.end - ftab.start) / 4).unwrap();
        let outputs = updater.received(&request(ftab.start, quarter), now());
        assert_eq!(
            statuses(&outputs),
            [UpdateStatus::Transferring { percent: 24 }]
        );
        // No change, no status.
        let outputs = updater.received(&request(0, 1), now());
        assert_eq!(statuses(&outputs), []);
    }

    #[test]
    fn unknown_assets_and_offsets_get_a_status() {
        let mut updater = started();
        let past = updater.package.length() + 1;
        let outputs = updater.received(&request(past, 4), now());
        let write = match &outputs[0] {
            Output::Write(write) => write.clone(),
            Output::Status(_) => panic!("a write first"),
        };
        assert_eq!(write[6..8], UNKNOWN_ASSET.to_be_bytes());
        let other = from_pods(6, 9, &[0, 2, 0, 0, 0, 0, 0, 4]);
        let outputs = updater.received(&other, now());
        assert_eq!(kinds(&outputs), [0x07]);
        assert_eq!(
            updater.status(),
            UpdateStatus::Offered,
            "nothing served yet"
        );
    }

    #[test]
    fn the_pods_verdicts() {
        for (flags, failure) in [
            (0x0202, Failure::Denied { reason: 2 }),
            (0x0803, Failure::Abandoned { reason: 8 }),
            (0x0004, Failure::Corrupt { reason: 0 }),
        ] {
            let mut updater = started();
            let outputs = updater.received(&processing(flags), now());
            assert_eq!(kinds(&outputs), [0x0f], "acknowledged, nothing rescinded");
            assert_eq!(statuses(&outputs), [UpdateStatus::Failed(failure)]);
        }
        // Another asset's notification is only acknowledged.
        let mut updater = started();
        let other = from_pods(9, 9, &[0, 2, 0, 1]);
        assert_eq!(kinds(&updater.received(&other, now())), [0x0f]);
        assert_eq!(updater.status(), UpdateStatus::Offered);
    }

    #[test]
    fn the_watchdog_rescinds() {
        let mut updater = started();
        assert_eq!(updater.next_wake(), Some(now() + WATCHDOG));
        assert_eq!(updater.tick(now() + WATCHDOG / 2), []);
        let outputs = updater.tick(now() + WATCHDOG);
        assert_eq!(kinds(&outputs), [0x0c]);
        assert_eq!(
            statuses(&outputs),
            [UpdateStatus::Failed(Failure::NotStarted)]
        );

        let mut updater = started();
        let later = now() + WATCHDOG / 2;
        updater.received(&request(0, 0x2c), later);
        assert_eq!(updater.next_wake(), Some(later + WATCHDOG));
        let outputs = updater.tick(later + WATCHDOG);
        assert_eq!(kinds(&outputs), [0x0c]);
        assert_eq!(statuses(&outputs), [UpdateStatus::Failed(Failure::Stalled)]);
        // Ended: nothing more is served.
        assert_eq!(updater.received(&request(0, 4), later), []);
    }

    #[test]
    fn a_dropped_transport_resumes_with_the_same_offer() {
        let ftab = package().ftab();
        let mut updater = started();
        updater.received(&request(ftab.start, 4), now());
        let outputs = updater.down(now());
        assert_eq!(
            statuses(&outputs),
            [UpdateStatus::Interrupted { percent: 13 }]
        );
        assert_eq!(kinds(&outputs), []);
        // Back: Sync, version, the identical offer, still transferring.
        let first = Updater::new(package(), now()).up(now());
        let again = updater.up(now());
        assert_eq!(kinds(&again), [0x00, 0x01, 0x05]);
        assert_eq!(again[..3], first[..3]);
        assert_eq!(updater.status(), UpdateStatus::Transferring { percent: 13 });
        // Drops without a request in between: given up after a few.
        let outputs = (0..=DROPS)
            .flat_map(|_| updater.down(now()))
            .collect::<Vec<_>>();
        assert_eq!(
            statuses(&outputs).last(),
            Some(&UpdateStatus::Failed(Failure::Lost))
        );
        // Down for too long, too.
        let mut updater = started();
        updater.down(now());
        assert_eq!(updater.next_wake(), Some(now() + INTERRUPTED_WAIT));
        let outputs = updater.tick(now() + INTERRUPTED_WAIT);
        assert_eq!(statuses(&outputs), [UpdateStatus::Failed(Failure::Lost)]);
        assert_eq!(kinds(&outputs), [], "no transport to rescind on");
    }

    #[test]
    fn cancelling_rescinds_once() {
        let mut updater = started();
        let outputs = updater.cancel();
        assert_eq!(kinds(&outputs), [0x0c]);
        assert_eq!(
            statuses(&outputs),
            [UpdateStatus::Failed(Failure::Cancelled)]
        );
        assert_eq!(updater.cancel(), []);
        let mut updater = Updater::new(package(), now());
        assert_eq!(kinds(&updater.cancel()), [], "no transport yet");
    }

    #[test]
    fn large_requests_are_served_as_far_as_a_message_goes() {
        let large = Package::parse(package_bytes_with(&[7; 4096])).unwrap();
        let mut updater = Updater::new(large, now());
        updater.up(now());
        let outputs = updater.received(&request(0, u16::MAX), now());
        let Output::Write(response) = &outputs[0] else {
            panic!("a response");
        };
        assert_eq!(response.len(), uarp::MAX_MESSAGE);
        assert_eq!(
            response[14..16],
            u16::MAX.to_be_bytes(),
            "the request echoed"
        );
        assert_eq!(response[16..18], uarp::MAX_DATA.to_be_bytes());
    }

    #[test]
    fn nothing_is_sent_while_down() {
        let mut updater = Updater::new(package(), now());
        assert_eq!(kinds(&updater.received(&request(0, 4), now())), []);
        updater.up(now());
        updater.down(now());
        assert!(!updater.is_up());
        assert_eq!(kinds(&updater.received(&request(0, 4), now())), []);
        assert_eq!(kinds(&updater.cancel()), []);
    }

    fn state(left: EarStatus, case: ChargingStatus, reported: &str) -> DeviceState {
        let mut state = DeviceState::default();
        state.apply(&Packet::EarDetection(EarDetection {
            primary: left,
            secondary: EarStatus::InCase,
        }));
        state.apply(&Packet::Battery(vec![BatteryInfo {
            component: BatteryComponent::Case,
            level: 50,
            status: Some(case),
        }]));
        state.firmware.reported = Some(reported.into());
        state
    }

    #[test]
    fn preconditions() {
        const OLD: &str = "71.2256000021000000.6663";
        let version = "9.0.348".parse().unwrap();
        let ready = state(EarStatus::InCase, ChargingStatus::Charging, OLD);
        assert_eq!(check(&ready, version), Ok(()));
        let worn = state(EarStatus::InEar, ChargingStatus::Charging, OLD);
        assert_eq!(check(&worn, version), Err(Refusal::NotInCase));
        let unplugged = state(EarStatus::InCase, ChargingStatus::Discharging, OLD);
        assert_eq!(check(&unplugged, version), Ok(()), "the pods defer, not us");
        let same = state(
            EarStatus::InCase,
            ChargingStatus::Charging,
            "90.3431000025000000.6807",
        );
        assert_eq!(
            check(&same, version),
            Err(Refusal::NotNewer),
            "same major.minor"
        );
        let mut exact = same.clone();
        exact.firmware.exact = "9.0.300".parse().ok();
        assert_eq!(check(&exact, version), Ok(()), "exactly older");
        exact.firmware.exact = Some(version);
        assert_eq!(check(&exact, version), Err(Refusal::NotNewer));
        let mut unknown = ready;
        unknown.firmware.reported = None;
        assert_eq!(check(&unknown, version), Err(Refusal::VersionUnknown));
    }

    #[test]
    fn news_round_trips_and_is_recognisable() {
        let version = "9.0.348".parse().unwrap();
        let statuses = [
            UpdateStatus::Downloading,
            UpdateStatus::Offered,
            UpdateStatus::Deferred,
            UpdateStatus::Transferring { percent: 40 },
            UpdateStatus::Interrupted { percent: 41 },
            UpdateStatus::Staged,
            UpdateStatus::Done(version),
            UpdateStatus::Failed(Failure::Denied { reason: 2 }),
            UpdateStatus::Failed(Failure::Refused(Refusal::NotInCase)),
            UpdateStatus::Failed(Failure::Cancelled),
        ];
        let news = statuses
            .into_iter()
            .map(News::Update)
            .chain([News::Firmware(version)]);
        for news in news {
            assert_eq!(News::decode(&news.encode()), Some(news));
        }
        let firmware = News::Firmware(version).encode();
        assert!(firmware.starts_with(b"calvados\x01\x01"));
        assert_eq!(News::decode(&firmware[..firmware.len() - 1]), None);
        let mut future = firmware.clone();
        future[8] = 2;
        assert_eq!(News::decode(&future), None, "another format");
        assert_eq!(News::decode(b"\x01\xe5\x4alocalscore"), None);
        let mut unknown = News::Update(UpdateStatus::Staged).encode();
        unknown[10] = 99;
        assert_eq!(News::decode(&unknown), None);
    }
}
