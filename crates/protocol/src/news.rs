//! What calvados hosts tell each other through the pods' smart-routing
//! relay (0x10 to a peer, arriving as 0x11:
//! [`crate::aacp::smart_routing::news`]).

use crate::aacp::control_command::Cycle;
use crate::firmware::ExactVersion;
use crate::uarp::Rejection;
use crate::update::{Failure, Refusal, UpdateStatus};

/// What one host knows and the others don't. Starts with [`News::MAGIC`].
#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub enum News {
    /// The pods' exact running firmware.
    Firmware(ExactVersion),
    /// The sender's update.
    Update(UpdateStatus),
    /// The press-and-hold cycle, as a host set it.
    Cycle(Cycle),
    /// The pods' name, as a host set it: the pods tell the other hosts
    /// only at their next settings burst.
    Name(String),
}

impl News {
    pub const MAGIC: &[u8; 8] = b"calvados";
    /// This encoding's version: a host ignores another's.
    const FORMAT: u8 = 1;

    /// `MAGIC`, `FORMAT`, then `1` + version (3 × u32 BE), `2` + state +
    /// two arguments + version (zero unless done), `3` + the cycle's mask,
    /// or `4` + the name (UTF-8).
    pub fn encode(&self) -> Vec<u8> {
        let body = match self {
            Self::Firmware(running) => [&[1][..], &running.to_be_bytes()].concat(),
            &Self::Update(status) => {
                let (state, a, b) = match status {
                    UpdateStatus::Downloading => (1, 0, 0),
                    UpdateStatus::Offered => (2, 0, 0),
                    UpdateStatus::Deferred => (3, 0, 0),
                    UpdateStatus::Transferring { percent } => (4, percent, 0),
                    UpdateStatus::Interrupted { percent } => (5, percent, 0),
                    UpdateStatus::Staged => (6, 0, 0),
                    UpdateStatus::Done(_) => (7, 0, 0),
                    UpdateStatus::Failed(failure) => {
                        let (code, argument) = failure_code(failure);
                        (8, code, argument)
                    }
                };
                let done = match status {
                    UpdateStatus::Done(version) => version.to_be_bytes(),
                    _ => [0; 12],
                };
                [&[2, state, a, b][..], &done].concat()
            }
            Self::Cycle(cycle) => vec![3, cycle.modes().bits()],
            Self::Name(name) => [&[4][..], name.as_bytes()].concat(),
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
        let version = |bytes: &[u8]| bytes.first_chunk().map(ExactVersion::from_be_bytes);
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
            [3, modes] => Cycle::from_bits(*modes).map(Self::Cycle),
            [4, name @ ..] if (1..=32).contains(&name.len()) => {
                String::from_utf8(name.to_vec()).ok().map(Self::Name)
            }
            _ => None,
        }
    }
}

fn failure_code(failure: Failure) -> (u8, u8) {
    match failure {
        Failure::Download => (1, 0),
        Failure::Refused(refusal) => (2, refusal.into()),
        // 3, 4, 5.
        Failure::Rejected(rejection, reason) => (u8::from(rejection) + 1, reason),
        Failure::NotStarted => (6, 0),
        Failure::Stalled => (7, 0),
        Failure::Lost => (8, 0),
        Failure::Cancelled => (9, 0),
        Failure::NoAnswer => (10, 0),
        Failure::NoCase => (11, 0),
    }
}

fn failure_of(code: u8, argument: u8) -> Option<Failure> {
    Some(match code {
        1 => Failure::Download,
        2 => Failure::Refused(Refusal::try_from(argument).ok()?),
        3..=5 => Failure::Rejected(Rejection::try_from(code - 1).ok()?, argument),
        6 => Failure::NotStarted,
        7 => Failure::Stalled,
        8 => Failure::Lost,
        9 => Failure::Cancelled,
        10 => Failure::NoAnswer,
        11 => Failure::NoCase,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
            UpdateStatus::Failed(Failure::Rejected(Rejection::Denied, 2)),
            UpdateStatus::Failed(Failure::Refused(Refusal::NotInCase)),
            UpdateStatus::Failed(Failure::Cancelled),
        ];
        let news = statuses.into_iter().map(News::Update).chain([
            News::Firmware(version),
            News::Cycle(Cycle::OFFERED),
            News::Name("ucho soother".into()),
        ]);
        for news in news {
            assert_eq!(News::decode(&news.encode()), Some(news));
        }
        // The wire format, after `calvados` and the format.
        let zeros = "0".repeat(24);
        for (news, hex) in [
            (
                News::Firmware(version),
                "0100000009000000000000015c".to_owned(),
            ),
            (
                News::Update(UpdateStatus::Done(version)),
                "0207000000000009000000000000015c".to_owned(),
            ),
            (
                News::Update(UpdateStatus::Transferring { percent: 40 }),
                format!("02042800{zeros}"),
            ),
            (
                News::Update(UpdateStatus::Failed(Failure::Rejected(
                    Rejection::Denied,
                    2,
                ))),
                format!("02080302{zeros}"),
            ),
            (
                News::Update(UpdateStatus::Failed(Failure::Refused(Refusal::NotInCase))),
                format!("02080204{zeros}"),
            ),
            (News::Cycle(Cycle::OFFERED), "030e".to_owned()),
        ] {
            assert_eq!(hex::encode(&news.encode()[9..]), hex, "{news:?}");
        }
        let firmware = News::Firmware(version).encode();
        assert!(firmware.starts_with(b"calvados\x01\x01"));
        assert_eq!(News::decode(&firmware[..firmware.len() - 1]), None);
        let mut future = firmware.clone();
        future[8] = 2;
        assert_eq!(News::decode(&future), None, "another format");
        assert_eq!(News::decode(b"\x01\xe5\x4alocalscore"), None);
        assert_eq!(News::decode(b"calvados\x01\x03\x02"), None, "one mode");
        let mut unknown = News::Update(UpdateStatus::Staged).encode();
        unknown[10] = 99;
        assert_eq!(News::decode(&unknown), None);
    }
}
