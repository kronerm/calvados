//! UARP, Apple's firmware update protocol (docs/uarp.md), as a controller:
//! the messages calvados sends and understands, their GATT fragments, and
//! the read-only running-version query. The update itself is
//! [`crate::update`]. There is no apply request (0x0A) here, by design: the
//! case applies a staged image once the lid closes.

use crate::firmware::{ExactVersion, Version};

/// The GATT characteristic (write, indicate); BlueZ serves it over the
/// classic link too.
pub const CHARACTERISTIC: u128 = 0x94110001_6d9b_4225_a4f1_6a4a7f01b0de;

/// The fragment header's bit on a message's last fragment.
const LAST_FRAGMENT: u8 = 0x01;

/// The protocol version proposed; 7B21 answers 4 to anything higher.
const PROTOCOL_VERSION: u16 = 4;

/// The header: type, payload length, message id (big-endian u16 each).
const HEADER_LEN: usize = 6;

/// A write as `uarp_offer.py` sent them, the fragment header included:
/// within BlueZ's MTU (672), and a 475-byte data response fits whole.
pub const MAX_WRITE: usize = 501;

/// The types sent. Apply (0x0A) is deliberately missing.
#[derive(Debug, Clone, Copy)]
#[repr(u16)]
enum Sent {
    Sync = 0x00,
    VersionDiscovery = 0x01,
    InformationRequest = 0x03,
    AssetAvailable = 0x05,
    DataResponse = 0x07,
    Rescind = 0x0C,
    ProcessingAck = 0x0F,
    DownstreamAddedAck = 0x18,
}

/// The types understood when received.
#[derive(Debug, Clone, Copy, num_enum::TryFromPrimitive)]
#[repr(u16)]
enum ReceivedType {
    Sync = 0x00,
    VersionResponse = 0x02,
    InformationResponse = 0x04,
    DataRequest = 0x06,
    Processing = 0x09,
    AssetAvailableAck = 0x0D,
    RescindAck = 0x10,
    DownstreamAdded = 0x17,
}

/// Accessory information tags; only the one asked for.
#[derive(Debug, Clone, Copy)]
#[repr(u32)]
enum InfoTag {
    /// Four u32: major, minor, release, build (`7.1.21.0`).
    RunningFirmware = 5,
}

/// Asset Data Response status: an unknown asset, or an offset past it.
pub const UNKNOWN_ASSET: u16 = 0x11;

/// A message to the pods.
#[derive(Debug, Clone, Copy)]
pub enum Message<'a> {
    Sync,
    /// The protocol version we propose; see [`PROTOCOL_VERSION`].
    VersionDiscovery,
    /// Asset Available: a SuperBinary (tag 0, flags 1).
    Offer {
        asset: u16,
        version: [u8; 16],
        length: u32,
        payloads: u16,
    },
    DataResponse {
        status: u16,
        asset: u16,
        offset: u32,
        requested: u16,
        data: &'a [u8],
    },
    /// Withdraw an offer; the pods drop what they staged of it.
    Rescind {
        asset: u16,
    },
    /// Echo of a processing notification.
    ProcessingAck {
        asset: u16,
        flags: u16,
    },
    DownstreamAddedAck {
        id: u16,
    },
}

impl Message<'_> {
    /// The whole message, with message id `id`.
    pub fn encode(&self, id: u16) -> Vec<u8> {
        let be16 = u16::to_be_bytes;
        let be32 = u32::to_be_bytes;
        let (kind, payload): (Sent, Vec<u8>) = match *self {
            Self::Sync => (Sent::Sync, Vec::new()),
            Self::VersionDiscovery => (Sent::VersionDiscovery, be16(PROTOCOL_VERSION).to_vec()),
            Self::Offer {
                asset,
                version,
                length,
                payloads,
            } => (
                Sent::AssetAvailable,
                [
                    &be32(0)[..],
                    &be16(SUPERBINARY),
                    &be16(asset),
                    &version,
                    &be32(length),
                    &be16(payloads),
                ]
                .concat(),
            ),
            Self::DataResponse {
                status,
                asset,
                offset,
                requested,
                data,
            } => {
                let actual = u16::try_from(data.len()).expect("at most what was requested");
                (
                    Sent::DataResponse,
                    [
                        &be16(status)[..],
                        &be16(asset),
                        &be32(offset),
                        &be16(requested),
                        &be16(actual),
                        data,
                    ]
                    .concat(),
                )
            }
            Self::Rescind { asset } => (Sent::Rescind, be16(asset).to_vec()),
            Self::ProcessingAck { asset, flags } => {
                (Sent::ProcessingAck, [be16(asset), be16(flags)].concat())
            }
            Self::DownstreamAddedAck { id } => {
                (Sent::DownstreamAddedAck, [be16(0), be16(id)].concat())
            }
        };
        message(kind, id, &payload)
    }
}

/// Asset Available flags: a SuperBinary (not a dynamic asset).
const SUPERBINARY: u16 = 0x0001;

fn message(kind: Sent, id: u16, payload: &[u8]) -> Vec<u8> {
    let len = u16::try_from(payload.len()).expect("a payload within u16");
    [kind as u16, len, id]
        .into_iter()
        .flat_map(u16::to_be_bytes)
        .chain(payload.iter().copied())
        .collect()
}

/// `message` as writes of at most `max_write` bytes, each with the
/// fragment header (bit 0 on the last).
pub fn fragments(message: &[u8], max_write: usize) -> Vec<Vec<u8>> {
    let chunks = message.chunks(max_write.saturating_sub(1).max(1));
    let last = chunks.len().saturating_sub(1);
    chunks
        .enumerate()
        .map(|(i, chunk)| {
            let header = if i == last { LAST_FRAGMENT } else { 0 };
            [&[header][..], chunk].concat()
        })
        .collect()
}

/// How UARP messages travel between host and pods; the messages are the
/// same on both.
#[derive(Debug, Clone, Copy)]
pub enum Framing {
    /// The GATT characteristic (the pods' transport 4): writes of at most
    /// `max_write` bytes, each with the fragment header.
    Gatt { max_write: usize },
    /// AACP opcode 0x4F on the AACP session (their transport 0xd): one whole
    /// message per frame, `[len 2 LE]` first, no fragment header, at most
    /// [`AACP_MAX_MESSAGE`] bytes (the pods' requests keep data responses
    /// within it). Opening it tears down the host's GATT endpoint. Unused
    /// so far: a 0x4F frame reaching a bud that isn't primary aborts it, so
    /// only on a settled session with the primary.
    Aacp,
}

/// AACP's opcode for UARP.
const AACP_OPCODE: u16 = 0x4F;

/// The longest message one 0x4F frame carries.
pub const AACP_MAX_MESSAGE: usize = 1007;

impl Framing {
    /// As `uarp_offer.py` wrote, the one proven live.
    pub const GATT: Self = Self::Gatt {
        max_write: MAX_WRITE,
    };

    /// `message` as what's written or sent, in order.
    pub fn frames(self, message: &[u8]) -> Vec<Vec<u8>> {
        match self {
            Self::Gatt { max_write } => fragments(message, max_write),
            Self::Aacp => {
                let len = u16::try_from(message.len()).expect("a message within u16");
                vec![crate::aacp::frame::data_frame(
                    AACP_OPCODE,
                    &[&len.to_le_bytes()[..], message].concat(),
                )]
            }
        }
    }
}

/// A message from the pods, as far as an updater cares.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub enum Received {
    Sync,
    /// Version discovery's answer: the negotiated protocol version.
    Version {
        status: u16,
        version: u16,
    },
    /// The offer acknowledged (every field echoed).
    OfferAck {
        asset: u16,
    },
    /// Asset Data Request: serve `length` bytes at `offset`.
    DataRequest {
        asset: u16,
        offset: u32,
        length: u16,
    },
    /// Asset Processing Notification: see [`Processing`].
    Processing {
        asset: u16,
        flags: u16,
    },
    RescindAck {
        asset: u16,
    },
    /// Downstream Endpoint Added (never seen over GATT).
    DownstreamAdded {
        id: u16,
    },
    /// An information response, or a type an updater ignores.
    Other {
        kind: u16,
    },
}

/// A processing notification's flags: the outcome in the low byte, a
/// reason in the high one.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub enum Processing {
    /// The asset is fully staged.
    Staged,
    Denied {
        reason: u8,
    },
    /// E.g. superseded by a newer offer, or merged into another host's.
    Abandoned {
        reason: u8,
    },
    /// A bad header or a data error.
    Corrupt {
        reason: u8,
    },
    Other {
        flags: u16,
    },
}

impl Processing {
    pub fn of(flags: u16) -> Self {
        let [reason, outcome] = flags.to_be_bytes();
        match outcome {
            1 => Self::Staged,
            2 => Self::Denied { reason },
            3 => Self::Abandoned { reason },
            4 => Self::Corrupt { reason },
            _ => Self::Other { flags },
        }
    }
}

/// The header and the payload, `None` if it's shorter than the header says.
fn split(message: &[u8]) -> Option<(u16, &[u8])> {
    let (header, payload) = message.split_first_chunk::<HEADER_LEN>()?;
    let [kind, len, _id] = [0, 2, 4].map(|at| u16::from_be_bytes([header[at], header[at + 1]]));
    Some((kind, payload.get(..usize::from(len))?))
}

impl Received {
    /// `None` when malformed (shorter than its type needs).
    pub fn parse(message: &[u8]) -> Option<Self> {
        let (kind, payload) = split(message)?;
        let be16 = |at: usize| {
            payload
                .get(at..at + 2)
                .map(|b| u16::from_be_bytes([b[0], b[1]]))
        };
        let be32 = |at: usize| {
            payload
                .get(at..at + 4)
                .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        };
        let Ok(known) = ReceivedType::try_from(kind) else {
            return Some(Self::Other { kind });
        };
        Some(match known {
            ReceivedType::Sync => Self::Sync,
            ReceivedType::VersionResponse => Self::Version {
                status: be16(0)?,
                version: be16(2)?,
            },
            ReceivedType::AssetAvailableAck => Self::OfferAck { asset: be16(6)? },
            ReceivedType::DataRequest => Self::DataRequest {
                asset: be16(0)?,
                offset: be32(2)?,
                length: be16(6)?,
            },
            ReceivedType::Processing => Self::Processing {
                asset: be16(0)?,
                flags: be16(2)?,
            },
            ReceivedType::RescindAck => Self::RescindAck { asset: be16(0)? },
            ReceivedType::DownstreamAdded => Self::DownstreamAdded { id: be16(0)? },
            ReceivedType::InformationResponse => Self::Other { kind },
        })
    }
}

/// The writes asking for the running firmware, in order: Sync, version
/// discovery, the information request. Each is one whole message after
/// the fragment header.
pub fn running_version_query() -> [Vec<u8>; 3] {
    [
        Message::Sync.encode(1),
        Message::VersionDiscovery.encode(2),
        message(
            Sent::InformationRequest,
            3,
            &(InfoTag::RunningFirmware as u32).to_be_bytes(),
        ),
    ]
    .map(|message| [&[LAST_FRAGMENT][..], &message].concat())
}

/// Indications back into messages: each starts with the fragment header.
#[derive(Debug, Default)]
pub struct Reassembler {
    pending: Vec<u8>,
}

impl Reassembler {
    /// A whole message once `fragment` was its last.
    pub fn push(&mut self, fragment: &[u8]) -> Option<Vec<u8>> {
        let (&header, data) = fragment.split_first()?;
        self.pending.extend_from_slice(data);
        (header & LAST_FRAGMENT != 0).then(|| std::mem::take(&mut self.pending))
    }
}

/// The running firmware from an information response to
/// [`running_version_query`]: status 0, tag 5, four u32.
pub fn running_version(message: &[u8]) -> Option<ExactVersion> {
    let (kind, payload) = split(message)?;
    let (status, rest) = payload.split_first_chunk::<2>()?;
    let (tag, rest) = rest.split_first_chunk::<4>()?;
    let (value_len, value) = rest.split_first_chunk::<4>()?;
    let value = value.get(..usize::try_from(u32::from_be_bytes(*value_len)).ok()?)?;
    let running = kind == ReceivedType::InformationResponse as u16
        && u16::from_be_bytes(*status) == 0
        && u32::from_be_bytes(*tag) == InfoTag::RunningFirmware as u32;
    let words = value
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| u32::from_be_bytes(*word))
        .collect::<Vec<_>>();
    match (running, words.as_slice()) {
        (true, &[major, minor, release, ..]) => Some(ExactVersion {
            version: Version { major, minor },
            release,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_query_is_three_whole_messages() {
        let [sync, discovery, request] = running_version_query();
        assert_eq!(sync, [0x01, 0, 0, 0, 0, 0, 1]);
        assert_eq!(discovery, [0x01, 0, 1, 0, 2, 0, 2, 0, 4]);
        assert_eq!(request, [0x01, 0, 3, 0, 4, 0, 3, 0, 0, 0, 5]);
    }

    /// The response 7B21 sent: status 0, tag 5, 16 bytes, `7.1.21.0`.
    const RESPONSE: [u8; 32] = [
        0, 4, 0, 26, 0, 3, // header: type 4, 26 bytes, id 3
        0, 0, // status
        0, 0, 0, 5, // tag
        0, 0, 0, 16, // length
        0, 0, 0, 7, 0, 0, 0, 1, 0, 0, 0, 21, 0, 0, 0, 0,
    ];

    #[test]
    fn the_response_names_the_exact_version() {
        let version = running_version(&RESPONSE).map(|v| v.to_string());
        assert_eq!(version.as_deref(), Some("7.1.21"));
        let mut failed = RESPONSE;
        failed[7] = 1;
        assert_eq!(running_version(&failed), None);
        let mut staged = RESPONSE;
        staged[11] = 6;
        assert_eq!(running_version(&staged), None);
        assert_eq!(running_version(&RESPONSE[..20]), None);
        assert_eq!(running_version(&[]), None);
        assert_eq!(
            Received::parse(&RESPONSE),
            Some(Received::Other { kind: 4 })
        );
    }

    #[test]
    fn fragments_are_reassembled() {
        let mut reassembler = Reassembler::default();
        let (first, rest) = RESPONSE.split_at(10);
        assert_eq!(reassembler.push(&[&[0x00][..], first].concat()), None);
        let message = reassembler.push(&[&[0x01][..], rest].concat());
        assert_eq!(message.as_deref(), Some(&RESPONSE[..]));
        assert_eq!(reassembler.push(&[0x01, 9]).as_deref(), Some(&[9][..]));
        assert_eq!(reassembler.push(&[]), None);
    }

    #[test]
    fn long_messages_are_split_with_the_last_marked() {
        let message = (0..=9).collect::<Vec<u8>>();
        let writes = fragments(&message, 5);
        assert_eq!(
            writes,
            [vec![0, 0, 1, 2, 3], vec![0, 4, 5, 6, 7], vec![1, 8, 9]]
        );
        let mut reassembler = Reassembler::default();
        let back = writes.iter().find_map(|write| reassembler.push(write));
        assert_eq!(back, Some(message));
        assert_eq!(fragments(&[7], MAX_WRITE), [vec![1, 7]]);
    }

    /// As `uarp_offer.py` built them for 9A348.
    #[test]
    fn messages_match_the_tool() {
        let version = [0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 1, 0x5c, 0, 0, 0, 0];
        let offer = Message::Offer {
            asset: 1,
            version,
            length: 16_571_997,
            payloads: 20,
        };
        assert_eq!(
            hex::encode(offer.encode(3)),
            "0005001e0003000000000001000100000009000000000000015c0000000000fcde5d0014"
        );
        let response = Message::DataResponse {
            status: 0,
            asset: 1,
            offset: 0x2c,
            requested: 4,
            data: &[1, 2, 3],
        };
        assert_eq!(
            hex::encode(response.encode(9)),
            "0007000f0009000000010000002c00040003010203"
        );
        assert_eq!(
            hex::encode(Message::Rescind { asset: 1 }.encode(1)),
            "000c000200010001"
        );
        assert_eq!(
            hex::encode(
                Message::ProcessingAck {
                    asset: 1,
                    flags: 0x0202
                }
                .encode(2)
            ),
            "000f0004000200010202"
        );
        assert_eq!(
            hex::encode(Message::DownstreamAddedAck { id: 7 }.encode(2)),
            "00180004000200000007"
        );
    }

    #[test]
    fn received_messages() {
        let parse = |hex: &str| Received::parse(&hex::decode(hex).unwrap());
        assert_eq!(
            parse("00060008000500010000253c0032"),
            Some(Received::DataRequest {
                asset: 1,
                offset: 0x253c,
                length: 0x32
            })
        );
        assert_eq!(
            parse("00090004000700010101"),
            Some(Received::Processing {
                asset: 1,
                flags: 0x0101
            })
        );
        assert_eq!(
            parse("00020004000100000004"),
            Some(Received::Version {
                status: 0,
                version: 4
            })
        );
        // The ack echoes the offer: the asset id after tag and flags.
        let ack = Message::Offer {
            asset: 1,
            version: [0; 16],
            length: 9,
            payloads: 1,
        }
        .encode(1);
        let ack = [&[0x00, 0x0d][..], &ack[2..]].concat();
        assert_eq!(Received::parse(&ack), Some(Received::OfferAck { asset: 1 }));
        assert_eq!(parse("0006000200050001"), None, "too short for its type");
        assert_eq!(
            parse("0006000900050001"),
            None,
            "shorter than its header says"
        );
        assert_eq!(
            parse("0a0a00000001"),
            Some(Received::Other { kind: 0x0a0a })
        );
        assert_eq!(parse("0000"), None);
    }

    #[test]
    fn aacp_frames_carry_whole_messages() {
        let message = Message::Rescind { asset: 1 }.encode(1);
        assert_eq!(
            Framing::Aacp.frames(&message),
            [hex::decode("040004004f000800000c000200010001").unwrap()]
        );
        assert_eq!(
            Framing::GATT.frames(&message),
            [[&[1][..], &message].concat()]
        );
    }

    #[test]
    fn processing_flags() {
        assert_eq!(Processing::of(0x0001), Processing::Staged);
        assert_eq!(Processing::of(0x0202), Processing::Denied { reason: 2 });
        assert_eq!(Processing::of(0x0803), Processing::Abandoned { reason: 8 });
        assert_eq!(Processing::of(0x0004), Processing::Corrupt { reason: 0 });
        assert_eq!(Processing::of(0x0106), Processing::Other { flags: 0x0106 });
    }
}
