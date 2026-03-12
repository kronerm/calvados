//! Connection history (0x2B), sent once per session after the handshake:
//! the pods' recent link events, oldest first. Observed: the structure (its
//! lengths and MACs fit every capture). Inferred: the clock unit and the
//! event kinds.

use std::fmt;
use std::time::Duration;

use nom::{
    IResult, Parser as _,
    bytes::complete::{tag, take},
    multi::many0,
    number::complete::{le_u16, le_u64, u8},
};

use super::reversed_mac;
use crate::MacAddress;

pub const OPCODE: u16 = 0x2B;

#[derive(Clone)]
pub struct ConnectionHistory {
    /// The pods' clock when sent. Inferred µs: fits the gaps between
    /// captures, and it restarts when the pods power on.
    pub now: Duration,
    pub events: Vec<HistoryEvent>,
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub struct HistoryEvent {
    /// On the pods' clock, like [`ConnectionHistory::now`].
    pub time: Duration,
    pub mac: MacAddress,
    pub change: LinkChange,
}

/// The event kind. Inferred: per MAC the kinds alternate between connecting
/// and leaving, and only leaving carries a value (`0x02c4`–`0x02d2`, maybe a
/// reason).
#[derive(Debug, Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub enum LinkChange {
    /// Kind `0x0a`.
    HostConnected,
    /// Kind `0x02`.
    HostLeft {
        reason: u16,
    },
    /// Kind `0x05`: the other bud.
    BudConnected,
    /// Kind `0x06`.
    BudLeft {
        reason: u16,
    },
    /// Kind `0x09`: the pods connecting out to a host, `status` 0 on
    /// success. Seen only after the pods power on: once 0 as the phone
    /// connected, and three tries 6 s apart, all `0x02c0`, while the phone's
    /// Bluetooth was off.
    PagedHost {
        status: u16,
    },
    Unknown {
        kind: u8,
        value: u16,
    },
}

impl ConnectionHistory {
    /// The host whose connection is the newest event, at most `within` old.
    /// Sent at AACP connect, which follows our link right away, so on a
    /// fresh connect that's us (observed: 0 s, the laptop 7 s before).
    pub fn just_connected_host(&self, within: Duration) -> Option<MacAddress> {
        let last = self.events.last()?;
        let connected = matches!(
            last.change,
            LinkChange::HostConnected | LinkChange::PagedHost { status: 0 }
        );
        (connected && self.now.saturating_sub(last.time) <= within).then_some(last.mac)
    }
}

impl ConnectionHistory {
    /// Hosts whose latest event is a connect: still connected, as far as the
    /// history goes.
    pub fn connected_hosts(&self) -> Vec<MacAddress> {
        self.events
            .iter()
            .filter_map(|event| match event.change {
                LinkChange::HostConnected | LinkChange::PagedHost { status: 0 } => {
                    Some((event.mac, true))
                }
                LinkChange::HostLeft { .. } | LinkChange::PagedHost { .. } => {
                    Some((event.mac, false))
                }
                _ => None,
            })
            .fold(
                Vec::new(),
                |mut hosts: Vec<MacAddress>, (mac, connected)| {
                    hosts.retain(|host| *host != mac);
                    if connected {
                        hosts.push(mac);
                    }
                    hosts
                },
            )
    }
}

impl LinkChange {
    fn new(kind: u8, value: u16) -> Self {
        match kind {
            0x0a => Self::HostConnected,
            0x02 => Self::HostLeft { reason: value },
            0x05 => Self::BudConnected,
            0x06 => Self::BudLeft { reason: value },
            0x09 => Self::PagedHost { status: value },
            kind => Self::Unknown { kind, value },
        }
    }
}

/// `ConnectionHistory(up 1h 57m 54s: [11m 45s ago 08:8b:… HostLeft …, …])`.
impl fmt::Debug for ConnectionHistory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct Aged<'a>(&'a HistoryEvent, Duration);
        impl fmt::Debug for Aged<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let Self(event, now) = self;
                let age = now.saturating_sub(event.time);
                write!(f, "{} ago {:?} {:?}", seconds(age), event.mac, event.change)
            }
        }
        write!(f, "ConnectionHistory(up {}: ", seconds(self.now))?;
        f.debug_list()
            .entries(self.events.iter().map(|event| Aged(event, self.now)))
            .finish()?;
        f.write_str(")")
    }
}

fn seconds(duration: Duration) -> humantime::FormattedDuration {
    humantime::format_duration(Duration::from_secs(duration.as_secs()))
}

/// `[time 8 LE] [mac 6, reversed] [kind] [value 2 LE]`
fn event(input: &[u8]) -> IResult<&[u8], HistoryEvent> {
    (le_u64, reversed_mac, u8, le_u16)
        .map(|(time, mac, kind, value)| HistoryEvent {
            time: Duration::from_micros(time),
            mac,
            change: LinkChange::new(kind, value),
        })
        .parse(input)
}

/// `01 [events length 2 LE] [now 8 LE] (event 17) * n`
pub fn parse(input: &[u8]) -> IResult<&[u8], ConnectionHistory> {
    let (input, (_, len, now)) = (tag(&[0x01][..]), le_u16, le_u64).parse(input)?;
    let (input, events) = take(len).and_then(many0(event)).parse(input)?;
    let now = Duration::from_micros(now);
    Ok((input, ConnectionHistory { now, events }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_captured_history() {
        // Android: the phone left, then reconnected.
        let payload = hex::decode(
            "012200 6000a8a501000000 \
             32b2da9e01000000 aa9988776602 02 d102 \
             aa0050a201000000 aa9988776602 0a 0000"
                .replace(' ', ""),
        )
        .unwrap();
        let (rest, history) = parse(&payload).unwrap();
        assert!(rest.is_empty());
        let phone = "02:66:77:88:99:aa".parse().unwrap();
        assert_eq!(
            history.events,
            [
                HistoryEvent {
                    time: Duration::from_micros(6_960_099_890),
                    mac: phone,
                    change: LinkChange::HostLeft { reason: 0x02d1 },
                },
                HistoryEvent {
                    time: Duration::from_micros(7_018_119_338),
                    mac: phone,
                    change: LinkChange::HostConnected,
                },
            ]
        );
        assert_eq!(history.connected_hosts(), [phone]);
        let left = ConnectionHistory {
            events: history.events[..1].to_vec(),
            ..history.clone()
        };
        assert!(left.connected_hosts().is_empty());
        assert_eq!(
            format!("{history:?}"),
            "ConnectionHistory(up 1h 57m 54s: [\
             1m 54s ago 02:66:77:88:99:aa HostLeft { reason: 721 }, \
             56s ago 02:66:77:88:99:aa HostConnected])"
        );
    }
}
