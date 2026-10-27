//! UARP, Apple's firmware update protocol (docs/uarp.md), as far as asking
//! the pods for their exact running version: Sync, version discovery and
//! one information request. Read-only by design: nothing here offers,
//! transfers or applies firmware.

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

#[derive(Debug, Clone, Copy)]
#[repr(u16)]
enum MessageType {
    Sync = 0,
    VersionDiscovery = 1,
    InformationRequest = 3,
    InformationResponse = 4,
}

/// Accessory information tags; only the one asked for.
#[derive(Debug, Clone, Copy)]
#[repr(u32)]
enum InfoTag {
    /// Four u32: major, minor, release, build (`7.1.21.0`).
    RunningFirmware = 5,
}

fn message(kind: MessageType, id: u16, payload: &[u8]) -> Vec<u8> {
    let len = u16::try_from(payload.len()).expect("a short payload");
    [kind as u16, len, id]
        .into_iter()
        .flat_map(u16::to_be_bytes)
        .chain(payload.iter().copied())
        .collect()
}

/// The writes asking for the running firmware, in order: Sync, version
/// discovery, the information request. Each is one whole message after
/// the fragment header.
pub fn running_version_query() -> [Vec<u8>; 3] {
    [
        message(MessageType::Sync, 1, &[]),
        message(
            MessageType::VersionDiscovery,
            2,
            &PROTOCOL_VERSION.to_be_bytes(),
        ),
        message(
            MessageType::InformationRequest,
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
    let (header, payload) = message.split_first_chunk::<HEADER_LEN>()?;
    let [kind, len, _id] = [0, 2, 4].map(|at| u16::from_be_bytes([header[at], header[at + 1]]));
    let payload = payload.get(..usize::from(len))?;
    let (status, rest) = payload.split_first_chunk::<2>()?;
    let (tag, rest) = rest.split_first_chunk::<4>()?;
    let (value_len, value) = rest.split_first_chunk::<4>()?;
    let value = value.get(..usize::try_from(u32::from_be_bytes(*value_len)).ok()?)?;
    let running = kind == MessageType::InformationResponse as u16
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
}
