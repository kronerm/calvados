//! The pods' saved crash logs, over opcode 0x17 (docs/firmware.md): the
//! wrapper is `[class u32 LE] [len u16 LE]` + body; class 0x400 is the
//! pods' log channel to the host, class 0x200 a host's requests.

use nom::{
    IResult, Parser as _,
    branch::alt,
    bytes::complete::{tag, take},
    combinator::{all_consuming, map, rest, verify},
    multi::length_count,
    number::complete::{le_u16, le_u32, u8},
};

use super::frame::data_frame;
use crate::MacAddress;

pub const OPCODE: u16 = 0x17;
/// The pods' log channel to the host.
const FROM_PODS: u32 = 0x400;
/// A host's requests to the log module.
const TO_PODS: u32 = 0x200;

/// One class-0x400 frame of the crash-log transfer.
#[derive(Clone)]
pub enum CrashLog {
    /// The cores holding a saved crash log: `AP AOP RTP BTM DSP1 DSP2`.
    Saved(Vec<String>),
    /// Part of a bud's logs, in order; `more` is clear on the last.
    Chunk { bud: u8, more: bool, data: Vec<u8> },
    /// The transfer is over.
    End,
    /// The pods gave up.
    Aborted,
}

/// A chunk by its length (the bytes are in the `aacp_raw` trace).
impl std::fmt::Debug for CrashLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Saved(cores) => f.debug_tuple("Saved").field(cores).finish(),
            Self::Chunk { bud, more, data } => f
                .debug_struct("Chunk")
                .field("bud", bud)
                .field("more", more)
                .field("bytes", &data.len())
                .finish(),
            Self::End => f.write_str("End"),
            Self::Aborted => f.write_str("Aborted"),
        }
    }
}

/// `06 00 00 [count]` (`fe` before an auto-push), then per core
/// `[name 4, NUL-padded] [size u32]`.
fn saved(input: &[u8]) -> IResult<&[u8], CrashLog> {
    let core = map((take(4usize), le_u32), |(name, _size): (&[u8], u32)| {
        String::from_utf8_lossy(name)
            .trim_end_matches('\0')
            .to_owned()
    });
    let (input, _) = (alt((tag(&[0x06][..]), tag(&[0xFE][..]))), tag(&[0, 0][..])).parse(input)?;
    map(length_count(u8, core), CrashLog::Saved).parse(input)
}

/// `01 [counter | 0x80 more] [bud 1/2 | 0x80 holding the link]` + data.
fn chunk(input: &[u8]) -> IResult<&[u8], CrashLog> {
    let (input, (_, counter, bud)) = (tag(&[0x01][..]), u8, u8).parse(input)?;
    map(rest, |data: &[u8]| CrashLog::Chunk {
        bud: bud & 0x7F,
        more: counter & 0x80 != 0,
        data: data.to_vec(),
    })
    .parse(input)
}

/// The body of a class-0x400 0x17; other classes and kinds fail.
pub fn parse(input: &[u8]) -> IResult<&[u8], CrashLog> {
    let (input, _) = verify(le_u32, |&class| class == FROM_PODS).parse(input)?;
    let (input, body) = all_consuming(nom::multi::length_data(le_u16)).parse(input)?;
    all_consuming(alt((
        saved,
        chunk,
        map(tag(&[0xFF][..]), |_| CrashLog::End),
        map(tag(&[0xFC, 0x01][..]), |_| CrashLog::Aborted),
    )))
    .parse(body)
    .map(|(_, log)| (input, log))
}

/// Ask the pods for their saved crash logs, from both buds: type 5 (send),
/// bud mask `ff`, sub 1 (crash logs). The firmware builds the same request
/// itself to push them [rkof 0x1c4154].
pub fn request() -> Vec<u8> {
    const BODY: [u8; 6] = [0x05, 0xFF, 0xFF, 0x00, 0x01, 0x02];
    let header = [
        &TO_PODS.to_le_bytes()[..],
        &(BODY.len() as u16).to_le_bytes(),
    ]
    .concat();
    data_frame(OPCODE, &[&header[..], &BODY].concat())
}

/// Where a bud's logs are kept, under the platform's own directory, by
/// when they came: `crash-logs/<pods>-<unix time>-bud<n>.bin`.
pub fn file_name(pods: MacAddress, bud: u8, unix_secs: u64) -> String {
    format!(
        "crash-logs/{}-{unix_secs}-bud{bud}.bin",
        pods.to_hex_string()
    )
}

/// One RTKit crash log (`CLHE`): what went wrong, as the pods saved it.
#[derive(Debug, Clone)]
pub struct Record {
    /// The exception (low byte: undefined instruction, panic, hard fault…).
    pub kind: u32,
    /// The panic or assert message (`Cstr`), if any.
    pub message: Option<String>,
}

/// The `CLHE` records in a bud's logs: a 32-byte header (`CLHE`, +8 the
/// record's size, +0xc the exception), sections `[tag 4] [4] [4] [size 4]`
/// counting their 16-byte header, and a copy of the header (Asahi Linux
/// reads the same format). Tags are little-endian u32s: `CLHE` is `EHLC`
/// in memory.
pub fn records(data: &[u8]) -> Vec<Record> {
    const HEADER: &[u8] = b"EHLC";
    let find = |from: usize| {
        data.get(from..)?
            .windows(HEADER.len())
            .position(|window| window == HEADER)
            .map(|at| &data[from + at..])
    };
    // Past each record's end, which repeats its header.
    std::iter::successors(find(0), |record| {
        let size = word(record, 8).map_or(0, |size| size as usize);
        find(data.len() - record.len() + size.max(HEADER.len()))
    })
    .filter_map(|record| {
        let size = (word(record, 8)? as usize).min(record.len());
        let record = &record[..size];
        Some(Record {
            kind: word(record, 0xC)?,
            message: message(record),
        })
    })
    .collect()
}

/// The `Cstr` section's text: `[id 4]` + the string, NUL-terminated.
fn message(record: &[u8]) -> Option<String> {
    const MESSAGE: &[u8] = b"rtsC";
    let sections = std::iter::successors(Some(32usize), |&at| {
        let size = word(record, at + 12)? as usize;
        (size >= 16).then_some(at + size)
    });
    let body = sections
        .map_while(|at| {
            let size = word(record, at + 12)? as usize;
            Some((record.get(at..at + 4)?, record.get(at + 16..at + size)?))
        })
        .find_map(|(tag, body)| (tag == MESSAGE).then_some(body))?;
    let text = body.get(4..)?.split(|&b| b == 0).next()?;
    Some(String::from_utf8_lossy(text).into_owned())
}

fn word(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class_400(body: &[u8]) -> Vec<u8> {
        [
            &[0x00, 0x04, 0x00, 0x00][..],
            &(body.len() as u16).to_le_bytes(),
            body,
        ]
        .concat()
    }

    #[test]
    fn the_inventory() {
        // Nov 4 15:51:41, the phone, after a reboot.
        let body = [
            &[0x06, 0x00, 0x00, 0x02][..],
            b"AP\0\0",
            &[0x00, 0x80, 0x00, 0x00],
            b"DSP1",
            &[0x00, 0x80, 0x00, 0x00],
        ]
        .concat();
        let (_, log) = parse(&class_400(&body)).unwrap();
        assert!(matches!(log, CrashLog::Saved(cores) if cores == ["AP", "DSP1"]));
    }

    #[test]
    fn chunks_and_the_end() {
        let (_, log) = parse(&class_400(&[0x01, 0x80, 0x81, 0xAA, 0xBB])).unwrap();
        assert!(
            matches!(log, CrashLog::Chunk { bud: 1, more: true, data } if data == [0xAA, 0xBB])
        );
        let (_, log) = parse(&class_400(&[0x01, 0x03, 0x02, 0xCC])).unwrap();
        assert!(matches!(
            log,
            CrashLog::Chunk {
                bud: 2,
                more: false,
                ..
            }
        ));
        assert!(matches!(parse(&class_400(&[0xFF])), Ok((_, CrashLog::End))));
        assert!(matches!(
            parse(&class_400(&[0xFC, 0x01])),
            Ok((_, CrashLog::Aborted))
        ));
    }

    #[test]
    fn other_classes_and_kinds_are_not_ours() {
        // HID, class 0x100000.
        let hid = [&[0x00, 0x00, 0x10, 0x00, 0x02, 0x00][..], &[0x08, 0x01]].concat();
        assert!(parse(&hid).is_err());
        // ULLA statistics on the same class.
        assert!(parse(&class_400(&[0x08, 0x80, 0x81])).is_err());
    }

    #[test]
    fn the_request() {
        assert_eq!(
            request(),
            [
                0x04, 0x00, 0x04, 0x00, 0x17, 0x00, 0x00, 0x02, 0x00, 0x00, 0x06, 0x00, 0x05, 0xFF,
                0xFF, 0x00, 0x01, 0x02
            ]
        );
    }

    #[test]
    fn a_record_s_message() {
        let message = [&[0x01, 0x00, 0x00, 0x00][..], b"assert: x\0\0\0"].concat();
        let section = [
            &b"rtsC"[..],
            &[0; 8],
            &(16 + message.len() as u32).to_le_bytes(),
            &message,
        ]
        .concat();
        let version = [&b"revC"[..], &[0; 8], &16u32.to_le_bytes()].concat();
        let size = 32 + version.len() + section.len() + 32;
        let header = [
            &b"EHLC"[..],
            &[0, 3, 0, 0],
            &(size as u32).to_le_bytes(),
            &0x02u32.to_le_bytes(),
            &[0; 16],
        ]
        .concat();
        // Ends with a copy of its header, not another record.
        let data = [&[0xEE; 3][..], &header, &version, &section, &header].concat();
        let found = records(&data);
        assert!(matches!(
            &found[..],
            [Record { kind: 2, message: Some(text) }] if text == "assert: x"
        ));
    }
}
