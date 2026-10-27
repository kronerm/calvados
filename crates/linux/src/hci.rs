//! The pods' Bluetooth traffic from the kernel's HCI monitor channel (what
//! `btmon` reads; needs `CAP_NET_RAW`), logged next to the engine's lines:
//! link events, AVDTP and AVRCP signaling, A2DP media starting to flow,
//! delivery stalls and flushes (the pods out of reach), and the AACP frames
//! we send.
//!
//! The pods' link is the one carrying AACP, learned from our AACP channel
//! request. L2CAP channels opened before the daemon started (e.g. A2DP's)
//! are unknown until reopened.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
use std::time::{Duration, Instant};

use protocol::MacAddress;
use protocol::connection::AIRPODS_PSM;

const BTPROTO_HCI: libc::c_int = 1;
const HCI_DEV_NONE: u16 = 0xffff;
const HCI_CHANNEL_MONITOR: u16 = 2;

/// Monitor packet opcodes (`hci_mon_hdr`).
const MON_COMMAND: u16 = 2;
const MON_EVENT: u16 = 3;
const MON_ACL_TX: u16 = 4;
const MON_ACL_RX: u16 = 5;

const PSM_AVCTP: u16 = 0x0017;
const PSM_AVDTP: u16 = 0x0019;
const CID_SIGNALING: u16 = 0x0001;

/// A delivery gap worth a line; packets normally complete within ~0.2 s.
const STALL: Duration = Duration::from_millis(500);

/// Open the monitor channel and log on a thread; without the capability,
/// say so once and carry on.
pub fn spawn() {
    let fd = match open() {
        Ok(fd) => fd,
        Err(e) => {
            tracing::info!("no HCI monitor (needs CAP_NET_RAW): {e}");
            return;
        }
    };
    let spawned = std::thread::Builder::new()
        .name("hci-monitor".into())
        .spawn(move || run(&fd));
    match spawned {
        Ok(_) => tracing::info!("HCI monitor on"),
        Err(e) => tracing::warn!("HCI monitor thread: {e}"),
    }
}

#[repr(C)]
struct SockaddrHci {
    family: libc::sa_family_t,
    dev: u16,
    channel: u16,
}

fn open() -> std::io::Result<OwnedFd> {
    // SAFETY: plain socket(2); the fd is owned right away.
    let fd = unsafe {
        libc::socket(
            libc::AF_BLUETOOTH,
            libc::SOCK_RAW | libc::SOCK_CLOEXEC,
            BTPROTO_HCI,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `fd` is a fresh descriptor nobody else owns.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let addr = SockaddrHci {
        family: libc::AF_BLUETOOTH as libc::sa_family_t,
        dev: HCI_DEV_NONE,
        channel: HCI_CHANNEL_MONITOR,
    };
    // SAFETY: `addr` is a valid `sockaddr_hci` of the given size.
    let bound = unsafe {
        libc::bind(
            fd.as_raw_fd(),
            (&raw const addr).cast(),
            size_of::<SockaddrHci>() as libc::socklen_t,
        )
    };
    match bound {
        0 => Ok(fd),
        _ => Err(std::io::Error::last_os_error()),
    }
}

fn run(fd: &OwnedFd) {
    let mut monitor = Monitor::default();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        // SAFETY: `buf` is valid for `buf.len()` bytes.
        let len = unsafe { libc::recv(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len(), 0) };
        let Ok(len) = usize::try_from(len) else {
            tracing::warn!("HCI monitor stopped: {}", std::io::Error::last_os_error());
            return;
        };
        // `hci_mon_hdr`: opcode, controller index, length, all LE.
        let Some((header, data)) = buf[..len].split_at_checked(6) else {
            continue;
        };
        let opcode = u16::from_le_bytes([header[0], header[1]]);
        monitor
            .packet(opcode, data, Instant::now())
            .iter()
            .for_each(log);
    }
}

fn log(note: &Note) {
    match note {
        Note::Paging { mac } => tracing::debug!(%mac, "hci: paging"),
        Note::LinkUp {
            handle,
            mac,
            status,
        } => tracing::debug!(%mac, handle, status, "hci: link up"),
        Note::LinkDown {
            handle,
            reason,
            silent_for,
            undelivered,
        } => tracing::debug!(
            handle,
            reason = %Hex(*reason),
            meaning = hci_error(*reason),
            silent_for = ?silent_for,
            undelivered = undelivered.map(|(packets, _)| packets),
            undelivered_for = ?undelivered.map(|(_, since)| since),
            "hci: pods link down"
        ),
        Note::Stalled {
            stalled_for,
            outstanding,
            completed,
        } => tracing::debug!(
            stalled_for = ?stalled_for,
            outstanding,
            completed,
            "hci: delivery to the pods resumed"
        ),
        Note::Flushed => tracing::debug!("hci: undelivered packets to the pods flushed"),
        Note::Role { central, status } => {
            tracing::debug!(central, status, "hci: pods link role")
        }
        Note::Mode { mode, interval } => tracing::debug!(
            mode = match mode {
                0 => "active",
                2 => "sniff",
                _ => "other",
            },
            interval,
            "hci: pods link mode"
        ),
        Note::Avdtp {
            dir,
            label,
            signal,
            message,
            rest,
        } => tracing::debug!(
            %dir,
            label,
            signal = avdtp_signal(*signal),
            kind = avdtp_message(*message),
            rest = %HexBytes(rest),
            "hci: avdtp"
        ),
        Note::Avrcp {
            dir,
            ctype,
            op,
            released,
        } => tracing::debug!(
            %dir,
            ctype = avc_ctype(*ctype),
            op = avc_op(*op),
            released,
            "hci: avrcp"
        ),
        Note::MediaFlowing { dir } => tracing::debug!(%dir, "hci: a2dp media flowing"),
        Note::Aacp { frame } => tracing::debug!(hex = %HexBytes(frame), "hci: aacp tx"),
    }
}

/// Which way an ACL packet went, from this host's side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Dir {
    Tx,
    Rx,
}

impl Dir {
    fn reverse(self) -> Self {
        match self {
            Self::Tx => Self::Rx,
            Self::Rx => Self::Tx,
        }
    }
}

impl fmt::Display for Dir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Tx => "tx",
            Self::Rx => "rx",
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum Channel {
    Aacp,
    /// AVDTP's first channel on a link.
    AvdtpSignaling,
    /// AVDTP channels after the first: media transport.
    AvdtpMedia,
    Avrcp,
}

#[derive(Debug)]
enum Note {
    /// Create Connection: this host pages a device.
    Paging {
        mac: MacAddress,
    },
    /// Any BR/EDR link (the pods' isn't known until AACP opens).
    LinkUp {
        handle: u16,
        mac: MacAddress,
        status: u8,
    },
    LinkDown {
        handle: u16,
        reason: u8,
        /// Since the pods' last packet: dates a crash, as a timeout comes
        /// only after the supervision timeout of silence.
        silent_for: Option<Duration>,
        /// Packets the controller hadn't delivered, and since when it
        /// had delivered none.
        undelivered: Option<(u32, Duration)>,
    },
    /// The controller delivered packets again after a gap of `stalled_for`
    /// with `outstanding` waiting (out of reach, or the pods busy).
    Stalled {
        stalled_for: Duration,
        outstanding: u32,
        completed: u16,
    },
    /// The controller dropped packets it couldn't deliver in time.
    Flushed,
    Role {
        central: bool,
        status: u8,
    },
    Mode {
        mode: u8,
        interval: u16,
    },
    Avdtp {
        dir: Dir,
        label: u8,
        signal: u8,
        message: u8,
        rest: Vec<u8>,
    },
    /// An AV/C passthrough (play, pause…).
    Avrcp {
        dir: Dir,
        ctype: u8,
        op: u8,
        released: bool,
    },
    /// The first media packet each way since AVDTP last signaled.
    MediaFlowing {
        dir: Dir,
    },
    /// Received frames are logged by the session already.
    Aacp {
        frame: Vec<u8>,
    },
}

/// L2CAP channel state across links; notes only for the pods' link.
#[derive(Debug, Default)]
struct Monitor {
    /// BR/EDR links by handle.
    links: HashMap<u16, MacAddress>,
    /// Handles that carry AACP.
    pods: HashSet<u16>,
    /// By (handle, direction, destination CID).
    channels: HashMap<(u16, Dir, u16), Channel>,
    /// Connection requests awaiting a response: (handle, identifier) →
    /// (PSM, requester's direction, requester's CID).
    requests: HashMap<(u16, u8), (u16, Dir, u16)>,
    /// (handle, direction) with media flowing.
    flowing: HashSet<(u16, Dir)>,
    /// When each link last received a packet.
    last_rx: HashMap<u16, Instant>,
    /// Packets sent on each link the controller hasn't reported delivered.
    delivery: HashMap<u16, Undelivered>,
}

#[derive(Debug, Clone, Copy)]
struct Undelivered {
    packets: u32,
    /// The last delivery, or the first packet sent after all were.
    since: Instant,
}

impl Monitor {
    fn packet(&mut self, opcode: u16, data: &[u8], now: Instant) -> Vec<Note> {
        match opcode {
            MON_COMMAND => self.command(data).into_iter().collect(),
            MON_EVENT => self.event(data, now).into_iter().collect(),
            MON_ACL_TX => {
                if let Some(flags) = u16_at(data, 0) {
                    self.sent(flags & 0x0fff, now);
                }
                self.acl(Dir::Tx, data)
            }
            MON_ACL_RX => {
                if let Some(flags) = u16_at(data, 0) {
                    self.last_rx.insert(flags & 0x0fff, now);
                }
                self.acl(Dir::Rx, data)
            }
            _ => Vec::new(),
        }
    }

    fn command(&mut self, data: &[u8]) -> Option<Note> {
        const CREATE_CONNECTION: u16 = 0x0405;
        let opcode = u16::from_le_bytes([*data.first()?, *data.get(1)?]);
        match opcode {
            CREATE_CONNECTION => Some(Note::Paging {
                mac: mac_at(data, 3)?,
            }),
            _ => None,
        }
    }

    fn event(&mut self, data: &[u8], now: Instant) -> Option<Note> {
        const CONNECTION_COMPLETE: u8 = 0x03;
        const DISCONNECTION_COMPLETE: u8 = 0x05;
        const ROLE_CHANGE: u8 = 0x12;
        const MODE_CHANGE: u8 = 0x14;
        const FLUSH_OCCURRED: u8 = 0x11;
        const NUMBER_OF_COMPLETED_PACKETS: u8 = 0x13;
        const ACL: u8 = 0x01;
        let (&code, params) = data.split_first()?;
        let params = params.get(1..)?;
        let handle_at = |at: usize| Some(u16_at(params, at)? & 0x0fff);
        match code {
            CONNECTION_COMPLETE if params.get(9) == Some(&ACL) => {
                let (handle, mac, status) = (handle_at(1)?, mac_at(params, 3)?, params[0]);
                if status == 0 {
                    self.links.insert(handle, mac);
                }
                Some(Note::LinkUp {
                    handle,
                    mac,
                    status,
                })
            }
            DISCONNECTION_COMPLETE => {
                let (handle, reason) = (handle_at(1)?, *params.get(3)?);
                self.links.remove(&handle);
                self.channels.retain(|&(h, ..), _| h != handle);
                self.requests.retain(|&(h, _), _| h != handle);
                self.flowing.retain(|&(h, _)| h != handle);
                let silent_for = self
                    .last_rx
                    .remove(&handle)
                    .map(|last| now.saturating_duration_since(last));
                let undelivered = self
                    .delivery
                    .remove(&handle)
                    .filter(|u| u.packets > 0)
                    .map(|u| (u.packets, now.saturating_duration_since(u.since)));
                self.pods.remove(&handle).then_some(Note::LinkDown {
                    handle,
                    reason,
                    silent_for,
                    undelivered,
                })
            }
            ROLE_CHANGE => {
                let mac = mac_at(params, 1)?;
                self.is_pods_mac(mac).then(|| Note::Role {
                    central: params.get(7) == Some(&0),
                    status: params[0],
                })
            }
            MODE_CHANGE => {
                let handle = handle_at(1)?;
                let note = Note::Mode {
                    mode: *params.get(3)?,
                    interval: u16_at(params, 4)?,
                };
                self.pods.contains(&handle).then_some(note)
            }
            FLUSH_OCCURRED => self.pods.contains(&handle_at(0)?).then_some(Note::Flushed),
            NUMBER_OF_COMPLETED_PACKETS => self.completed(params, now),
            _ => None,
        }
    }

    fn sent(&mut self, handle: u16, now: Instant) {
        let undelivered = self.delivery.entry(handle).or_insert(Undelivered {
            packets: 0,
            since: now,
        });
        if undelivered.packets == 0 {
            undelivered.since = now;
        }
        undelivered.packets += 1;
    }

    /// `Number_of_Completed_Packets`: (handle, count) pairs; a stall noted
    /// for the pods' link (at most one per event).
    fn completed(&mut self, params: &[u8], now: Instant) -> Option<Note> {
        let (&count, pairs) = params.split_first()?;
        pairs
            .as_chunks::<4>()
            .0
            .iter()
            .take(usize::from(count))
            .filter_map(|pair| {
                let (handle, completed) = (u16_at(pair, 0)? & 0x0fff, u16_at(pair, 2)?);
                let undelivered = self.delivery.get_mut(&handle)?;
                let stalled_for = now.saturating_duration_since(undelivered.since);
                let outstanding = undelivered.packets;
                undelivered.packets = outstanding.saturating_sub(completed.into());
                undelivered.since = now;
                (self.pods.contains(&handle) && outstanding > 0 && stalled_for >= STALL).then_some(
                    Note::Stalled {
                        stalled_for,
                        outstanding,
                        completed,
                    },
                )
            })
            .last()
    }

    fn is_pods_mac(&self, mac: MacAddress) -> bool {
        self.pods
            .iter()
            .any(|handle| self.links.get(handle) == Some(&mac))
    }

    fn acl(&mut self, dir: Dir, data: &[u8]) -> Vec<Note> {
        const CONTINUATION: u16 = 0b01;
        let Some(flags) = u16_at(data, 0) else {
            return Vec::new();
        };
        let (handle, boundary) = (flags & 0x0fff, (flags >> 12) & 0b11);
        // Fragments past the first carry no L2CAP header: skipped.
        let Some(l2cap) = data.get(4..).filter(|_| boundary != CONTINUATION) else {
            return Vec::new();
        };
        let (Some(cid), Some(payload)) = (u16_at(l2cap, 2), l2cap.get(4..)) else {
            return Vec::new();
        };
        if cid == CID_SIGNALING {
            self.signaling(handle, dir, payload);
            return Vec::new();
        }
        if !self.pods.contains(&handle) {
            return Vec::new();
        }
        match self.channels.get(&(handle, dir, cid)) {
            Some(Channel::Aacp) if matches!(dir, Dir::Tx) => vec![Note::Aacp {
                frame: payload.to_vec(),
            }],
            Some(Channel::AvdtpSignaling) => self.avdtp(handle, dir, payload).into_iter().collect(),
            Some(Channel::AvdtpMedia) => self
                .flowing
                .insert((handle, dir))
                .then_some(Note::MediaFlowing { dir })
                .into_iter()
                .collect(),
            Some(Channel::Avrcp) => avrcp(dir, payload).into_iter().collect(),
            _ => Vec::new(),
        }
    }

    /// Track channels from L2CAP signaling (several commands per packet).
    fn signaling(&mut self, handle: u16, dir: Dir, mut payload: &[u8]) {
        const CONNECTION_REQUEST: u8 = 0x02;
        const CONNECTION_RESPONSE: u8 = 0x03;
        const DISCONNECTION_REQUEST: u8 = 0x06;
        const SUCCESS: u16 = 0;
        const PENDING: u16 = 1;
        while let (Some(&code), Some(&ident), Some(len)) =
            (payload.first(), payload.get(1), u16_at(payload, 2))
        {
            let Some(data) = payload.get(4..4 + usize::from(len)) else {
                return;
            };
            match code {
                CONNECTION_REQUEST => {
                    if let (Some(psm), Some(scid)) = (u16_at(data, 0), u16_at(data, 2)) {
                        self.requests.insert((handle, ident), (psm, dir, scid));
                    }
                }
                CONNECTION_RESPONSE => {
                    if let (Some(dcid), Some(result)) = (u16_at(data, 0), u16_at(data, 4))
                        && result != PENDING
                        && let Some((psm, requester, scid)) = self.requests.remove(&(handle, ident))
                        && result == SUCCESS
                    {
                        self.open(handle, psm, requester, scid, dcid);
                    }
                }
                DISCONNECTION_REQUEST => {
                    if let (Some(dcid), Some(scid)) = (u16_at(data, 0), u16_at(data, 2)) {
                        // Sent towards `dcid`'s owner; `scid` is the sender's.
                        self.channels.remove(&(handle, dir, dcid));
                        self.channels.remove(&(handle, dir.reverse(), scid));
                    }
                }
                _ => {}
            }
            payload = &payload[4 + usize::from(len)..];
        }
    }

    /// A channel opened: `scid` is the requester's end, `dcid` the
    /// responder's; packets are keyed by the destination's end.
    fn open(&mut self, handle: u16, psm: u16, requester: Dir, scid: u16, dcid: u16) {
        let channel = match psm {
            AIRPODS_PSM => {
                self.pods.insert(handle);
                Channel::Aacp
            }
            PSM_AVDTP
                if self
                    .channels
                    .iter()
                    .any(|(&(h, ..), c)| h == handle && matches!(c, Channel::AvdtpSignaling)) =>
            {
                Channel::AvdtpMedia
            }
            PSM_AVDTP => Channel::AvdtpSignaling,
            PSM_AVCTP => Channel::Avrcp,
            _ => return,
        };
        // Towards the requester (sent by the responder) and towards the responder.
        self.channels
            .insert((handle, requester.reverse(), scid), channel);
        self.channels.insert((handle, requester, dcid), channel);
    }

    fn avdtp(&mut self, handle: u16, dir: Dir, payload: &[u8]) -> Option<Note> {
        const SINGLE: u8 = 0;
        const START: u8 = 1;
        let first = *payload.first()?;
        let (label, kind, message) = (first >> 4, (first >> 2) & 0b11, first & 0b11);
        let at = match kind {
            SINGLE => 1,
            START => 2,
            _ => return None,
        };
        // Any signaling restarts the "media flowing" note on this link.
        self.flowing.retain(|&(h, _)| h != handle);
        Some(Note::Avdtp {
            dir,
            label,
            signal: payload.get(at)? & 0x3f,
            message,
            rest: payload.get(at + 1..)?.to_vec(),
        })
    }
}

/// AVCTP header, then AV/C: ctype, subunit, opcode; a passthrough's operand
/// is the operation with the release bit.
fn avrcp(dir: Dir, payload: &[u8]) -> Option<Note> {
    const PASSTHROUGH: u8 = 0x7c;
    let avc = payload.get(3..)?;
    if avc.get(2) != Some(&PASSTHROUGH) {
        return None;
    }
    let operand = *avc.get(3)?;
    Some(Note::Avrcp {
        dir,
        ctype: avc[0] & 0x0f,
        op: operand & 0x7f,
        released: operand & 0x80 != 0,
    })
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*data.get(at)?, *data.get(at + 1)?]))
}

/// A BD_ADDR, little-endian on the wire.
fn mac_at(data: &[u8], at: usize) -> Option<MacAddress> {
    let mut bytes: [u8; 6] = data.get(at..at + 6)?.try_into().ok()?;
    bytes.reverse();
    Some(MacAddress::new(bytes))
}

/// HCI error codes (Core spec, Vol 1 Part F), as disconnect reasons.
fn hci_error(code: u8) -> &'static str {
    match code {
        0x05 => "authentication failure",
        // The link supervision timer ran out: the other side went silent
        // (the pods rebooting, or out of range).
        0x08 => "connection timeout",
        0x13 => "remote user terminated",
        0x14 => "remote low resources",
        0x15 => "remote power off",
        0x16 => "local host terminated",
        0x1a => "unsupported remote feature",
        0x22 => "LMP response timeout",
        _ => "other",
    }
}

fn avdtp_signal(id: u8) -> &'static str {
    match id {
        0x01 => "discover",
        0x02 => "get-capabilities",
        0x03 => "set-configuration",
        0x04 => "get-configuration",
        0x05 => "reconfigure",
        0x06 => "open",
        0x07 => "start",
        0x08 => "close",
        0x09 => "suspend",
        0x0a => "abort",
        0x0b => "security-control",
        0x0c => "get-all-capabilities",
        0x0d => "delay-report",
        _ => "unknown",
    }
}

fn avdtp_message(message: u8) -> &'static str {
    match message {
        0 => "command",
        1 => "general-reject",
        2 => "accept",
        _ => "reject",
    }
}

fn avc_ctype(ctype: u8) -> &'static str {
    match ctype {
        0x0 => "control",
        0x8 => "not-implemented",
        0x9 => "accepted",
        0xa => "rejected",
        0xf => "interim",
        _ => "other",
    }
}

fn avc_op(op: u8) -> &'static str {
    match op {
        0x41 => "volume-up",
        0x42 => "volume-down",
        0x44 => "play",
        0x45 => "stop",
        0x46 => "pause",
        0x4b => "forward",
        0x4c => "backward",
        _ => "other",
    }
}

struct Hex(u8);

impl fmt::Display for Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:02x}", self.0)
    }
}

struct HexBytes<'a>(&'a [u8]);

impl fmt::Display for HexBytes<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().enumerate().try_for_each(|(i, byte)| match i {
            0 => write!(f, "{byte:02x}"),
            _ => write!(f, " {byte:02x}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HANDLE: u16 = 0x000b;
    const PODS: [u8; 6] = [0x01, 0xff, 0xee, 0xdd, 0xcc, 0x02];

    fn event(code: u8, params: &[u8]) -> Vec<u8> {
        [&[code, params.len() as u8][..], params].concat()
    }

    fn acl(cid: u16, payload: &[u8]) -> Vec<u8> {
        let l2cap = [
            &(payload.len() as u16).to_le_bytes()[..],
            &cid.to_le_bytes(),
            payload,
        ]
        .concat();
        [
            &HANDLE.to_le_bytes()[..],
            &(l2cap.len() as u16).to_le_bytes(),
            &l2cap,
        ]
        .concat()
    }

    fn signal(code: u8, ident: u8, data: &[u8]) -> Vec<u8> {
        acl(
            CID_SIGNALING,
            &[&[code, ident][..], &(data.len() as u16).to_le_bytes(), data].concat(),
        )
    }

    fn le(values: &[u16]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    /// Up, AACP opened by us (local 0x40, remote 0x41), AVDTP signaling
    /// opened by the pods (remote 0x50, local 0x60).
    impl Monitor {
        fn feed(&mut self, opcode: u16, data: &[u8]) -> Vec<Note> {
            self.packet(opcode, data, Instant::now())
        }
    }

    fn monitor() -> Monitor {
        let mut m = Monitor::default();
        let up = [&[0][..], &HANDLE.to_le_bytes(), &PODS, &[1, 0]].concat();
        let notes = m.feed(MON_EVENT, &event(0x03, &up));
        assert!(matches!(notes[..], [Note::LinkUp { status: 0, .. }]));
        m.feed(MON_ACL_TX, &signal(0x02, 1, &le(&[AIRPODS_PSM, 0x40])));
        m.feed(MON_ACL_RX, &signal(0x03, 1, &le(&[0x41, 0x40, 0, 0])));
        m.feed(MON_ACL_RX, &signal(0x02, 7, &le(&[PSM_AVDTP, 0x50])));
        m.feed(MON_ACL_TX, &signal(0x03, 7, &le(&[0x60, 0x50, 0, 0])));
        m
    }

    #[test]
    fn our_aacp_frames_are_noted() {
        let mut m = monitor();
        let frame = [0x04, 0x00, 0x04, 0x00, 0x09, 0x00];
        let notes = m.feed(MON_ACL_TX, &acl(0x41, &frame));
        assert!(matches!(&notes[..], [Note::Aacp { frame: f }] if f == &frame));
        assert!(m.feed(MON_ACL_RX, &acl(0x40, &frame)).is_empty());
    }

    #[test]
    fn avdtp_signals_by_direction() {
        let mut m = monitor();
        // The pods start (label 3, command); we accept.
        let notes = m.feed(MON_ACL_RX, &acl(0x60, &[0x30, 0x07, 0x04]));
        assert!(matches!(
            notes[..],
            [Note::Avdtp {
                dir: Dir::Rx,
                label: 3,
                signal: 0x07,
                message: 0,
                ..
            }]
        ));
        let notes = m.feed(MON_ACL_TX, &acl(0x50, &[0x32, 0x07]));
        assert!(matches!(
            notes[..],
            [Note::Avdtp {
                dir: Dir::Tx,
                signal: 0x07,
                message: 2,
                ..
            }]
        ));
    }

    #[test]
    fn media_flowing_once_per_start() {
        let mut m = monitor();
        // Second AVDTP channel, opened by us: media.
        m.feed(MON_ACL_TX, &signal(0x02, 2, &le(&[PSM_AVDTP, 0x42])));
        m.feed(MON_ACL_RX, &signal(0x03, 2, &le(&[0x43, 0x42, 0, 0])));
        let media = acl(0x43, &[0x80, 0x60]);
        assert!(matches!(
            m.feed(MON_ACL_TX, &media)[..],
            [Note::MediaFlowing { dir: Dir::Tx }]
        ));
        assert!(m.feed(MON_ACL_TX, &media).is_empty());
        m.feed(MON_ACL_RX, &acl(0x60, &[0x40, 0x09, 0x04]));
        assert_eq!(m.feed(MON_ACL_TX, &media).len(), 1, "again after a suspend");
    }

    #[test]
    fn other_links_stay_quiet() {
        let mut m = monitor();
        m.pods.clear();
        assert!(
            m.feed(MON_ACL_RX, &acl(0x60, &[0x30, 0x07, 0x04]))
                .is_empty()
        );
    }

    #[test]
    fn pods_link_down_with_reason() {
        let mut m = monitor();
        let down = [&[0][..], &HANDLE.to_le_bytes(), &[0x08]].concat();
        let start = Instant::now();
        m.packet(MON_ACL_RX, &acl(0x40, &[0x00]), start);
        let notes = m.packet(
            MON_EVENT,
            &event(0x05, &down),
            start + Duration::from_secs(5),
        );
        assert!(matches!(
            notes[..],
            [Note::LinkDown { reason: 0x08, silent_for: Some(silent), .. }] if silent.as_secs() == 5
        ));
        assert!(m.channels.is_empty() && m.pods.is_empty());
    }

    fn completed(count: u16) -> Vec<u8> {
        event(
            0x13,
            &[&[1][..], &HANDLE.to_le_bytes(), &count.to_le_bytes()].concat(),
        )
    }

    #[test]
    fn delivery_stalls_are_noted_when_they_end() {
        let mut m = monitor();
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        // The setup's two signaling packets.
        m.packet(MON_EVENT, &completed(2), start);
        // Delivered in time: nothing.
        m.packet(MON_ACL_TX, &acl(0x41, &[0x00]), at(0));
        assert!(m.packet(MON_EVENT, &completed(1), at(100)).is_empty());
        // Two sent, delivered 2 s after the first.
        m.packet(MON_ACL_TX, &acl(0x41, &[0x00]), at(1000));
        m.packet(MON_ACL_TX, &acl(0x41, &[0x00]), at(1500));
        let notes = m.packet(MON_EVENT, &completed(2), at(3000));
        assert!(matches!(
            notes[..],
            [Note::Stalled { outstanding: 2, completed: 2, stalled_for }]
                if stalled_for == Duration::from_secs(2)
        ));
        // One left undelivered at the link's end.
        m.packet(MON_ACL_TX, &acl(0x41, &[0x00]), at(4000));
        let down = [&[0][..], &HANDLE.to_le_bytes(), &[0x08]].concat();
        let notes = m.packet(MON_EVENT, &event(0x05, &down), at(9000));
        assert!(matches!(
            notes[..],
            [Note::LinkDown { undelivered: Some((1, since)), .. }] if since.as_secs() == 5
        ));
    }
}
