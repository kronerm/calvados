//! Wire level: splitting transport bytes into frames, and building commands.

use crate::MacAddress;
use crate::aacp::frame::{DATA_MAGIC, HANDSHAKE_ACK_MAGIC, HANDSHAKE_MAGIC, data_frame};
use crate::aacp::{ControlId, HexDump, NoiseControlMode, Packet, smart_routing};

pub use smart_routing::{hijack_request, media_information, news, show_ui};

// === Commands ===

const SET_FEATURE_FLAGS: u16 = 0x4D;
const REQUEST_NOTIFICATIONS: u16 = 0x0F;
const ENABLE_EQ: u16 = 0x29;
const REQUEST_PROXIMITY_KEYS: u16 = 0x30;
/// Key type mask for [`REQUEST_PROXIMITY_KEYS`]: the IRK (0x01) only.
/// Both keys: identity (0x01) and encryption (0x04), as LibrePods asks.
const PROXIMITY_KEYS: u8 = 0x05;
/// Adaptive Transparency + Conversational Awareness while playing, etc.
const FEATURE_FLAGS: u64 = 0xD7;

/// Opens an AACP session; send [`session_setup_frames`] once it's acked.
pub fn handshake() -> Vec<u8> {
    [&HANDSHAKE_MAGIC[..], &[0x01, 0x00, 0x02, 0x00], &[0; 8]].concat()
}

/// Sent after the handshake ack, in wire order. Sent before it, the pods can
/// drop them when they ack late: without the feature flags they then report
/// Adaptive as Off to this host.
pub fn session_setup_frames() -> [Vec<u8>; 4] {
    [
        data_frame(SET_FEATURE_FLAGS, &FEATURE_FLAGS.to_le_bytes()),
        request_notifications(),
        data_frame(ENABLE_EQ, &[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]),
        request_proximity_keys(),
    ]
}

/// Ask for the pods' IRK and encryption key (reply:
/// [`crate::aacp::proximity_keys`]): the IRK recognises their BLE
/// advertisement, the other decrypts its case level ([`crate::ble`]).
pub fn request_proximity_keys() -> Vec<u8> {
    data_frame(REQUEST_PROXIMITY_KEYS, &[PROXIMITY_KEYS, 0x00])
}

/// Ask for the notification stream (its reply is opcode 0x02).
pub fn request_notifications() -> Vec<u8> {
    data_frame(REQUEST_NOTIFICATIONS, &[0xFF; 4])
}

pub fn control(id: ControlId, value: [u8; 4]) -> Vec<u8> {
    data_frame(
        crate::aacp::control_command::OPCODE,
        &[&[id as u8][..], &value].concat(),
    )
}

/// Claim or release the audio link.
pub fn set_ownership(owns: bool) -> Vec<u8> {
    control(ControlId::OwnsConnection, [owns.into(), 0, 0, 0])
}

pub fn set_noise_control(mode: NoiseControlMode) -> Vec<u8> {
    control(ControlId::ListeningMode, [mode as u8, 0, 0, 0])
}

/// The takeover sequence for one peer, sent after [`set_ownership`]`(true)`.
pub fn hijack_sequence(local: MacAddress, peer: MacAddress) -> [Vec<u8>; 3] {
    [
        media_information(local, peer, true),
        show_ui(peer),
        hijack_request(peer),
    ]
}

// === Framing ===

/// Frame length from the header, when the opcode allows knowing it.
#[derive(Debug)]
enum Length {
    Known(usize),
    /// Need more bytes to tell.
    Incomplete,
    /// Variable; the frame ends where the next magic starts.
    Unknown,
}

fn frame_length(frame: &[u8]) -> Length {
    const HEADER: usize = 6;
    let Some((magic, rest)) = frame.split_first_chunk::<4>() else {
        return Length::Incomplete;
    };
    if *magic != DATA_MAGIC {
        return Length::Unknown;
    }
    let Some(opcode) = rest.first_chunk::<2>().map(|op| u16::from_le_bytes(*op)) else {
        return Length::Incomplete;
    };
    // Lengths from the pods' firmware (docs/firmware.md) and captures.
    // Body length from a byte at `at`: `base + byte * per_item`.
    let counted = |at: usize, base: usize, per_item: usize| {
        frame.get(at).map_or(Length::Incomplete, |&n| {
            Length::Known(base + usize::from(n) * per_item)
        })
    };
    let u16_at = |at: usize| {
        frame
            .get(at..at + 2)
            .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
    };
    // `[len 2]` + body
    let prefixed = |at: usize, base: usize| {
        u16_at(at).map_or(Length::Incomplete, |len| Length::Known(base + len))
    };
    match opcode {
        0x04 => counted(HEADER, HEADER + 1, 5), // battery: count × 5
        0x06 | 0x19 => Length::Known(HEADER + 2), // ear detection, stem press
        0x25 | 0x47 => Length::Known(HEADER),
        0x08 | 0x37 | 0x4B | 0x55 | 0x60 => Length::Known(HEADER + 4),
        0x09 => Length::Known(HEADER + 5), // control command
        0x15 | 0x40 => Length::Known(HEADER + 6),
        0x0E | 0x13 => Length::Known(HEADER + 7), // audio source, …
        0x0C => Length::Known(HEADER + 8),        // device connection
        0x4E => Length::Known(HEADER + 8),        // all zeros, once per session
        // `[len 2]` + body; 0x52 is `03 00 02 01 xx` before ear changes
        0x4C | 0x52 | 0x57 => prefixed(HEADER, HEADER + 2),
        // UARP: one whole message
        crate::uarp::OPCODE => prefixed(HEADER, HEADER + 2),
        // device information: `02 [len 2]`, counting from its own byte 3
        0x1D => prefixed(HEADER + 1, HEADER + 3),
        // `01 [x] [len] 00` + body
        0x21 => counted(HEADER + 2, HEADER + 4, 1),
        0x31 => proximity_keys_length(frame),
        // smart routing out and relayed in: `[mac 6] [len 2]` + body
        0x10 | 0x11 => u16_at(12).map_or(Length::Incomplete, |len| Length::Known(14 + len)),
        // connection history: `01 [len 2] [now 8]` + events
        0x2B => {
            u16_at(HEADER + 1).map_or(Length::Incomplete, |len| Length::Known(HEADER + 11 + len))
        }
        // `[class 4] [len 2]` + body
        0x17 => {
            u16_at(HEADER + 4).map_or(Length::Incomplete, |len| Length::Known(HEADER + 6 + len))
        }
        0x2E => counted(HEADER + 2, HEADER + 3, 8), // connected devices: count × 8
        0x53 => Length::Known(140),                 // EQ
        _ => Length::Unknown,
    }
}

/// `[count]`, then `[type 2] [len 2]` + key, `count` times.
fn proximity_keys_length(frame: &[u8]) -> Length {
    const START: usize = 7;
    let Some(&count) = frame.get(START - 1) else {
        return Length::Incomplete;
    };
    (0..count)
        .try_fold(START, |at, _| {
            frame
                .get(at + 2..at + 4)
                .map(|len| at + 4 + usize::from(u16::from_le_bytes([len[0], len[1]])))
        })
        .map_or(Length::Incomplete, Length::Known)
}

fn is_magic(window: &[u8]) -> bool {
    window == DATA_MAGIC || window == HANDSHAKE_ACK_MAGIC
}

/// Offset of the first frame magic after position 0.
/// Only data frames end a variable-length frame: the handshake ack's magic
/// (`01 00 04 00`) also occurs across a payload's end and the next header.
fn next_magic(buf: &[u8]) -> Option<usize> {
    buf.windows(4)
        .skip(1)
        .position(|w| w == DATA_MAGIC)
        .map(|i| i + 1)
}

/// What to do with the head of the buffer.
#[derive(Debug)]
enum Step {
    Wait,
    Drop(usize),
    Frame(usize),
}

/// Decide the next step. `eof` means no more bytes will follow `buf`.
fn step(buf: &[u8], eof: bool) -> Step {
    let magic = buf.get(..4).is_some_and(is_magic);
    let until_magic = next_magic(buf);
    match (magic, frame_length(buf)) {
        (_, _) if buf.is_empty() => Step::Wait,
        (false, _) => match until_magic {
            Some(offset) => Step::Drop(offset),
            None if eof => Step::Drop(buf.len()),
            None => match buf.len() - partial_magic_suffix(buf) {
                0 => Step::Wait,
                junk => Step::Drop(junk),
            },
        },
        (true, Length::Known(n)) if n <= buf.len() => Step::Frame(n),
        (true, Length::Known(_) | Length::Incomplete) if eof => Step::Drop(buf.len()),
        (true, Length::Known(_) | Length::Incomplete) => Step::Wait,
        (true, Length::Unknown) => match until_magic {
            Some(offset) => Step::Frame(offset),
            None if eof => Step::Frame(buf.len()),
            None => Step::Wait,
        },
    }
}

/// Length of a suffix that may become frame magic once more bytes arrive.
fn partial_magic_suffix(buf: &[u8]) -> usize {
    (1..4)
        .rev()
        .find(|&len| {
            [DATA_MAGIC, HANDSHAKE_ACK_MAGIC]
                .iter()
                .any(|m| buf.ends_with(&m[..len]))
        })
        .unwrap_or(0)
}

/// Split and decode as many frames as `step` allows, consuming them from `buf`.
fn drain(buf: &mut Vec<u8>, eof: bool) -> Vec<Packet> {
    std::iter::from_fn(|| {
        loop {
            match step(buf, eof) {
                Step::Wait => return None,
                // Odd bytes carry their hex: these are the cases worth a look.
                Step::Drop(n) => {
                    let dropped = buf.drain(..n).collect::<Vec<_>>();
                    tracing::warn!(hex = ?HexDump(&dropped), "dropping bytes outside an AACP frame");
                }
                Step::Frame(n) => {
                    let frame = buf.drain(..n).collect::<Vec<_>>();
                    match Packet::parse(&frame) {
                        Some(packet) => return Some(packet),
                        None => tracing::warn!(hex = ?HexDump(&frame), "unparseable frame"),
                    }
                }
            }
        }
    })
    .collect()
}

/// Emit the raw `rx` trace (`RUST_LOG=protocol::aacp_raw=trace`).
fn trace_raw(bytes: &[u8]) {
    tracing::trace!(target: "protocol::aacp_raw", len = bytes.len(), hex = ?HexDump(bytes), "rx");
}

/// The same trace for what a platform sends (Linux has it from the HCI
/// monitor instead).
pub fn trace_tx(bytes: &[u8]) {
    tracing::trace!(target: "protocol::aacp_raw", len = bytes.len(), hex = ?HexDump(bytes), "tx");
}

/// Decode a message-preserving read (Linux `SOCK_SEQPACKET`), which may still
/// carry several concatenated frames.
pub fn parse_datagram(bytes: &[u8]) -> Vec<Packet> {
    trace_raw(bytes);
    drain(&mut bytes.to_vec(), true)
}

/// Stateful decoder for byte-stream transports (Android L2CAP `InputStream`).
/// Variable-length frames are held until the next frame's magic arrives.
#[derive(Debug, Default)]
pub struct Decoder {
    buf: Vec<u8>,
}

impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Packet> {
        trace_raw(bytes);
        self.buf.extend_from_slice(bytes);
        drain(&mut self.buf, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aacp::{Activity, ControlCommand, EarDetection, EarStatus, Opaque, Routing};

    const EAR_DETECTION: &[u8] = &[0x04, 0x00, 0x04, 0x00, 0x06, 0x00, 0x00, 0x01];
    const CONNECTED_DEVICES: &[u8] = &[
        0x04, 0x00, 0x04, 0x00, 0x2e, 0x00, 0x01, 0x00, 0x02, 0x02, 0x66, 0x77, 0x88, 0x99, 0xaa,
        0x02, 0x04, 0x02, 0x11, 0x22, 0x33, 0x44, 0x55, 0x02, 0x06,
    ];
    const BATTERY: &[u8] = &[
        0x04, 0x00, 0x04, 0x00, 0x04, 0x00, 0x03, 0x04, 0x01, 0x64, 0x02, 0x01, 0x02, 0x01, 0x5f,
        0x00, 0x01, 0x08, 0x01, 0x50, 0x01, 0x01,
    ];
    /// Start of a captured 0x02 reply, whose length isn't encoded.
    const NOTIFICATIONS_REPLY: &[u8] = &[
        0x04, 0x00, 0x04, 0x00, 0x02, 0x00, 0x1d, 0x01, 0x0e, 0x02, 0x02,
    ];

    /// A relayed routing frame (0x11): `[mac 6] [len 2 LE] [body]`.
    fn ownership_to_false() -> Vec<u8> {
        let body = b"audioRoutingSetOwnershipToFalse";
        [
            &[
                0x04, 0x00, 0x04, 0x00, 0x11, 0x00, 0xaa, 0x99, 0x88, 0x77, 0x66, 0x02,
            ][..],
            &(body.len() as u16).to_le_bytes(),
            body,
        ]
        .concat()
    }

    fn anc() -> Vec<u8> {
        set_noise_control(NoiseControlMode::NoiseCancellation)
    }

    fn debug(packets: &[Packet]) -> Vec<String> {
        packets.iter().map(|p| format!("{p:?}")).collect()
    }

    /// Feed `bytes` to a fresh decoder in chunks of the given sizes (cycled).
    fn decode_chunked(bytes: &[u8], mut sizes: impl Iterator<Item = usize>) -> Vec<Packet> {
        let mut decoder = Decoder::default();
        let mut rest = bytes;
        let chunks = std::iter::from_fn(|| {
            let size = sizes.next()?.min(rest.len());
            let (chunk, tail) = rest.split_at(size);
            rest = tail;
            (!chunk.is_empty()).then_some(chunk)
        });
        chunks.flat_map(|chunk| decoder.push(chunk)).collect()
    }

    fn assert_all_chunkings(bytes: &[u8], expected: &[Packet]) {
        let expected = debug(expected);
        (1..bytes.len()).for_each(|split| {
            let mut decoder = Decoder::default();
            let mut actual = decoder.push(&bytes[..split]);
            actual.extend(decoder.push(&bytes[split..]));
            assert_eq!(debug(&actual), expected, "split at {split}");
        });
        [&[1][..], &[2, 7, 1, 13], &[5, 3, 11], &[17, 4, 9, 2]]
            .into_iter()
            .for_each(|sizes| {
                let actual = decode_chunked(bytes, sizes.iter().copied().cycle());
                assert_eq!(debug(&actual), expected, "chunk sizes {sizes:?}");
            });
        let lcg = std::iter::successors(Some(0x5eed_u32), |s| {
            Some(s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223))
        });
        let actual = decode_chunked(bytes, lcg.map(|s| (s >> 24) as usize % 17 + 1));
        assert_eq!(debug(&actual), expected, "pseudorandom chunks");
    }

    #[test]
    fn commands_match_captured_bytes() {
        assert_eq!(hex::encode(handshake()), "00000400010002000000000000000000");
        assert_eq!(
            session_setup_frames().map(hex::encode),
            [
                "040004004d00d700000000000000",
                "040004000f00ffffffff",
                "04000400290000ffffffffffffff",
                "0400040030000500",
            ]
        );
        assert_eq!(hex::encode(set_ownership(true)), "0400040009000601000000");
        assert_eq!(hex::encode(set_ownership(false)), "0400040009000600000000");
        assert_eq!(hex::encode(anc()), "0400040009000d02000000");
    }

    #[test]
    fn decodes_typed_packets() {
        let packets = parse_datagram(&[EAR_DETECTION, BATTERY, CONNECTED_DEVICES, &anc()].concat());
        let [
            Packet::EarDetection(ear),
            Packet::Battery(battery),
            Packet::ConnectedDevices(devices),
            Packet::Control(control),
        ] = packets.as_slice()
        else {
            panic!("unexpected packets: {packets:?}");
        };
        assert_eq!(
            *ear,
            EarDetection {
                primary: EarStatus::InEar,
                secondary: EarStatus::NotInEar
            }
        );
        assert_eq!(battery.len(), 3);
        assert_eq!(devices.len(), 2);
        assert_eq!(
            *control,
            ControlCommand {
                id: ControlId::ListeningMode,
                value: [2, 0, 0, 0]
            }
        );
    }

    #[test]
    fn unknown_length_frames_end_at_next_magic() {
        let packets =
            parse_datagram(&[NOTIFICATIONS_REPLY, &ownership_to_false(), EAR_DETECTION].concat());
        assert!(matches!(
            packets.as_slice(),
            [
                Packet::Opaque(Opaque { opcode: 0x02, .. }),
                Packet::RoutingResponse(r),
                Packet::EarDetection(_),
            ] if matches!(r.kind, Routing::YieldRequest)
        ));
    }

    #[test]
    fn streaming_matches_datagram_for_every_chunking() {
        let terminated = |frame: &[u8]| [frame, &anc()].concat();
        [
            EAR_DETECTION.to_vec(),
            CONNECTED_DEVICES.to_vec(),
            BATTERY.to_vec(),
            anc(),
            terminated(NOTIFICATIONS_REPLY),
            terminated(&ownership_to_false()),
            [
                EAR_DETECTION,
                CONNECTED_DEVICES,
                &ownership_to_false(),
                BATTERY,
                NOTIFICATIONS_REPLY,
                &anc(),
            ]
            .concat(),
        ]
        .iter()
        .for_each(|bytes| assert_all_chunkings(bytes, &parse_datagram(bytes)));
    }

    #[test]
    fn payload_ending_in_01_00_is_not_cut_short() {
        // Captured on Android: a 0x52 payload ending `01 00` followed by the
        // next header spells the handshake-ack magic `01 00 04 00`.
        let media_routing = [
            0x04, 0x00, 0x04, 0x00, 0x52, 0x00, 0x03, 0x00, 0x02, 0x01, 0x00,
        ];
        let opaque_head = [0x04, 0x00, 0x04, 0x00, 0x02, 0x00, 0x01, 0x00];
        let bytes = [&opaque_head[..], &media_routing, EAR_DETECTION].concat();
        let packets = Decoder::default().push(&bytes);
        assert!(
            matches!(
                packets.as_slice(),
                [
                    Packet::Opaque(Opaque { opcode: 0x02, .. }),
                    Packet::Opaque(Opaque { opcode: 0x52, payload }),
                    Packet::EarDetection(_),
                ] if payload == &[0x03, 0x00, 0x02, 0x01, 0x00]
            ),
            "{packets:?}"
        );
    }

    #[test]
    fn relayed_routing_frame_completes_without_the_next_magic() {
        // Android's stream: a peer's yield request must not wait for
        // whatever frame comes next (it did, 11.5 s in one capture).
        assert!(matches!(
            Decoder::default().push(&ownership_to_false()).as_slice(),
            [Packet::RoutingResponse(r)] if matches!(r.kind, Routing::YieldRequest)
        ));
    }

    #[test]
    fn audio_source_activity_is_a_mask() {
        let frame = [&DATA_MAGIC[..], &[0x0E, 0x00, 1, 2, 3, 4, 5, 6, 0x03]].concat();
        assert!(matches!(
            Decoder::default().push(&frame).as_slice(),
            [Packet::AudioSource(s)] if s.kind == Activity::CALL | Activity::MEDIA
        ));
    }

    #[test]
    fn firmware_lengths_complete_without_the_next_magic() {
        let keys = [
            &DATA_MAGIC[..],
            &[0x31, 0x00, 2, 0x01, 0x00, 0x10, 0x00],
            &[0xAA; 16],
            &[0x04, 0x00, 0x10, 0x00],
            &[0xBB; 16],
        ]
        .concat();
        let ear_hint = [&DATA_MAGIC[..], &[0x52, 0x00, 0x03, 0x00, 0x02, 0x01, 0x00]].concat();
        let info = [
            &DATA_MAGIC[..],
            &[0x1D, 0x00, 0x02, 17, 0x00, 0x04, 0x01],
            &[0; 15],
            &[0; 2],
        ]
        .concat();
        let mut decoder = Decoder::default();
        assert!(matches!(
            decoder.push(&keys).as_slice(),
            [Packet::ProximityKeys(k)] if k.len() == 2
        ));
        assert!(matches!(
            decoder.push(&ear_hint).as_slice(),
            [Packet::Opaque(Opaque { opcode: 0x52, payload })] if payload.len() == 5
        ));
        assert!(matches!(
            decoder.push(&info).as_slice(),
            [Packet::DeviceInformation(_)]
        ));
    }

    #[test]
    fn streaming_holds_trailing_unknown_length_frame() {
        let mut decoder = Decoder::default();
        assert!(decoder.push(NOTIFICATIONS_REPLY).is_empty());
        assert!(matches!(
            decoder.push(&anc()).as_slice(),
            [
                Packet::Opaque(Opaque { opcode: 0x02, .. }),
                Packet::Control(_)
            ]
        ));
    }

    #[test]
    fn streaming_resynchronizes_after_garbage() {
        let bytes = [&[0xde, 0xad, 0xbe, 0xef, 0xaa, 0xbb][..], EAR_DETECTION].concat();
        assert!(matches!(
            Decoder::default().push(&bytes).as_slice(),
            [Packet::EarDetection(_)]
        ));
    }

    #[test]
    fn packet_debug_is_compact() {
        let opaque = Packet::Opaque(Opaque {
            opcode: 0x2B,
            payload: (0..20).collect(),
        });
        assert_eq!(
            format!("{opaque:?}"),
            "Opaque(0x2b, 20 bytes: 00 01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f …)"
        );
    }
}
