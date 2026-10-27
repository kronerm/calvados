//! A UARP SuperBinary (`firmware.uarp` in Apple's asset), checked the way
//! the pods check it before they stage anything (docs/uarp.md §4), so a
//! broken download is never offered. Big-endian throughout.

use std::fmt;
use std::ops::Range;

use sha2::{Digest as _, Sha256};

use crate::firmware::{ExactVersion, Version};

/// Format, header length, binary length, version (4 × u32), metadata
/// (offset, length), payload headers (offset, length).
const HEADER_LEN: u32 = 0x2c;
/// The newest format the pods take.
const MAX_FORMAT: u32 = 4;
/// Header length, tag, version (4 × u32), metadata and data (offset, length).
const PAYLOAD_HEADER_LEN: u32 = 0x28;
/// The buds' firmware table: the only payload they stage.
const FTAB: [u8; 4] = *b"FTAB";
/// Metadata TLV: the payload's SHA-256.
const DIGEST: u32 = 0xCCD2_8107;

/// A checked SuperBinary, ready to offer.
pub struct Package {
    bytes: Vec<u8>,
    /// As in the header, for the offer.
    raw_version: [u8; 16],
    version: ExactVersion,
    /// The header's binary length: what's offered and served.
    length: u32,
    payloads: u16,
    /// `FTAB`'s data.
    ftab: Range<u32>,
}

/// The bytes aren't: 16 MB.
impl fmt::Debug for Package {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Package")
            .field("version", &self.version)
            .field("length", &self.length)
            .field("payloads", &self.payloads)
            .field("ftab", &self.ftab)
            .finish_non_exhaustive()
    }
}

/// Why a SuperBinary isn't offered.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub enum PackageError {
    TooShort,
    Format(u32),
    HeaderLength(u32),
    /// The header claims more than the file holds.
    Length(u32),
    /// A range past the binary length.
    Range(&'static str),
    MetadataTlv,
    NoFtab,
    NoDigest,
    DigestMismatch,
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => f.write_str("too short for a SuperBinary"),
            Self::Format(format) => write!(f, "unknown SuperBinary format {format}"),
            Self::HeaderLength(len) => write!(f, "header length {len:#x}, not 0x2c"),
            Self::Length(len) => write!(f, "binary length {len} past the file's end"),
            Self::Range(what) => write!(f, "{what} past the binary's end"),
            Self::MetadataTlv => f.write_str("a metadata entry past its range"),
            Self::NoFtab => f.write_str("no FTAB payload"),
            Self::NoDigest => f.write_str("FTAB has no SHA-256"),
            Self::DigestMismatch => f.write_str("FTAB doesn't match its SHA-256"),
        }
    }
}

impl std::error::Error for PackageError {}

fn be32(bytes: &[u8], at: u32) -> Option<u32> {
    let at = usize::try_from(at).ok()?;
    let word = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes(word.try_into().ok()?))
}

/// `(offset, length)` at `at`, as a range within `limit`.
fn range(
    bytes: &[u8],
    at: u32,
    limit: u32,
    what: &'static str,
) -> Result<Range<u32>, PackageError> {
    let (start, len) = be32(bytes, at)
        .zip(be32(bytes, at + 4))
        .ok_or(PackageError::TooShort)?;
    start
        .checked_add(len)
        .filter(|&end| end <= limit)
        .map(|end| start..end)
        .ok_or(PackageError::Range(what))
}

fn slice<'a>(bytes: &'a [u8], range: &Range<u32>) -> &'a [u8] {
    // Ranges are checked against the binary length, within the file.
    &bytes[range.start as usize..range.end as usize]
}

/// One payload header.
struct Payload {
    tag: [u8; 4],
    metadata: Range<u32>,
    data: Range<u32>,
}

/// Metadata entries `[type 4] [length 4] [value]`.
fn tlvs(metadata: &[u8]) -> Result<Vec<(u32, &[u8])>, PackageError> {
    let mut entries = Vec::new();
    let mut rest = metadata;
    while !rest.is_empty() {
        let (head, tail) = rest
            .split_first_chunk::<8>()
            .ok_or(PackageError::MetadataTlv)?;
        let [kind, len] =
            [0, 4].map(|at| u32::from_be_bytes(head[at..at + 4].try_into().unwrap_or_default()));
        let len = usize::try_from(len).map_err(|_| PackageError::MetadataTlv)?;
        let (value, tail) = tail
            .split_at_checked(len)
            .ok_or(PackageError::MetadataTlv)?;
        entries.push((kind, value));
        rest = tail;
    }
    Ok(entries)
}

impl Package {
    /// Check `bytes` (a whole `firmware.uarp`) as the pods would, and
    /// `FTAB` against its digest.
    pub fn parse(bytes: Vec<u8>) -> Result<Self, PackageError> {
        let word = |at| be32(&bytes, at).ok_or(PackageError::TooShort);
        let format = word(0)?;
        if format > MAX_FORMAT {
            return Err(PackageError::Format(format));
        }
        let header_len = word(4)?;
        if header_len != HEADER_LEN {
            return Err(PackageError::HeaderLength(header_len));
        }
        let length = word(8)?;
        if usize::try_from(length).map_or(true, |len| len > bytes.len()) || length < HEADER_LEN {
            return Err(PackageError::Length(length));
        }
        let [major, minor, release, _build] =
            [12, 16, 20, 24].map(|at| be32(&bytes, at).unwrap_or(0));
        let raw_version = bytes[12..28]
            .try_into()
            .map_err(|_| PackageError::TooShort)?;
        range(&bytes, 28, length, "metadata")?;
        let headers = range(&bytes, 36, length, "payload headers")?;
        let payloads = (headers.start..headers.end)
            .step_by(PAYLOAD_HEADER_LEN as usize)
            .take_while(|at| at + PAYLOAD_HEADER_LEN <= headers.end)
            .map(|at| {
                Ok(Payload {
                    tag: bytes[at as usize + 4..at as usize + 8]
                        .try_into()
                        .map_err(|_| PackageError::TooShort)?,
                    metadata: range(&bytes, at + 24, length, "payload metadata")?,
                    data: range(&bytes, at + 32, length, "payload data")?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let ftab = payloads
            .iter()
            .find(|payload| payload.tag == FTAB)
            .ok_or(PackageError::NoFtab)?;
        let digest = tlvs(slice(&bytes, &ftab.metadata))?
            .into_iter()
            .find_map(|(kind, value)| (kind == DIGEST).then_some(value))
            .ok_or(PackageError::NoDigest)?;
        if Sha256::digest(slice(&bytes, &ftab.data)).as_slice() != digest {
            return Err(PackageError::DigestMismatch);
        }
        Ok(Self {
            raw_version,
            version: ExactVersion {
                version: Version { major, minor },
                release,
            },
            length,
            payloads: u16::try_from(payloads.len())
                .map_err(|_| PackageError::Range("payload count"))?,
            ftab: ftab.data.clone(),
            bytes,
        })
    }

    /// The SuperBinary's version, e.g. `9.0.348`.
    pub fn version(&self) -> ExactVersion {
        self.version
    }

    /// The header's version bytes, as offered.
    pub fn raw_version(&self) -> [u8; 16] {
        self.raw_version
    }

    /// The binary length offered.
    pub fn length(&self) -> u32 {
        self.length
    }

    pub fn payloads(&self) -> u16 {
        self.payloads
    }

    /// Where `FTAB`'s data lies: the bulk of a transfer, and its progress.
    pub fn ftab(&self) -> Range<u32> {
        self.ftab.clone()
    }

    /// Up to `length` bytes at `offset`, cut at the binary's end; `None`
    /// past it.
    pub fn read(&self, offset: u32, length: u16) -> Option<&[u8]> {
        let end = offset.saturating_add(length.into()).min(self.length);
        (offset <= self.length).then(|| slice(&self.bytes, &(offset..end)))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const AUCH_DATA: &[u8] = b"sound";
    const FTAB_DATA: &[u8] = b"rkosftab and the buds' images";

    /// A small SuperBinary like 9A348's: a sound, then FTAB with its
    /// digest and `0xCCD28112` = 1, then 7 trailing bytes past the binary
    /// length (Apple's files have some).
    pub(crate) fn package_bytes() -> Vec<u8> {
        let payloads = 2u32;
        let headers_at = HEADER_LEN;
        let metadata_at = headers_at + payloads * PAYLOAD_HEADER_LEN;
        let ftab_metadata = [
            &DIGEST.to_be_bytes()[..],
            &32u32.to_be_bytes(),
            &Sha256::digest(FTAB_DATA),
            &0xCCD2_8112u32.to_be_bytes(),
            &2u32.to_be_bytes(),
            &[0, 1],
        ]
        .concat();
        let metadata_len = ftab_metadata.len() as u32;
        let auch_at = metadata_at + metadata_len;
        let ftab_at = auch_at + AUCH_DATA.len() as u32;
        let length = ftab_at + FTAB_DATA.len() as u32;
        let be = |words: &[u32]| {
            words
                .iter()
                .flat_map(|w| w.to_be_bytes())
                .collect::<Vec<_>>()
        };
        let payload = |tag: &[u8; 4], version: [u32; 4], metadata: [u32; 2], data: [u32; 2]| {
            [
                &PAYLOAD_HEADER_LEN.to_be_bytes()[..],
                tag,
                &be(&version),
                &be(&metadata),
                &be(&data),
            ]
            .concat()
        };
        [
            be(&[
                3,
                HEADER_LEN,
                length,
                9,
                0,
                348,
                0,
                metadata_at,
                0,
                headers_at,
            ]),
            be(&[payloads * PAYLOAD_HEADER_LEN]),
            payload(
                b"AUCH",
                [0, 0, 3, 3],
                [metadata_at, 0],
                [auch_at, AUCH_DATA.len() as u32],
            ),
            payload(
                b"FTAB",
                [90, 224_854_016, 26_214_400, 6807],
                [metadata_at, metadata_len],
                [ftab_at, FTAB_DATA.len() as u32],
            ),
            ftab_metadata,
            AUCH_DATA.to_vec(),
            FTAB_DATA.to_vec(),
            b"trailer".to_vec(),
        ]
        .concat()
    }

    pub(crate) fn package() -> Package {
        Package::parse(package_bytes()).unwrap()
    }

    #[test]
    fn a_superbinary_is_checked_and_read() {
        let bytes = package_bytes();
        let package = package();
        assert_eq!(package.version().to_string(), "9.0.348");
        assert_eq!(package.raw_version()[..4], [0, 0, 0, 9]);
        assert_eq!(package.payloads(), 2);
        assert_eq!(package.length() as usize, bytes.len() - b"trailer".len());
        let ftab = package.ftab();
        assert_eq!(&bytes[ftab.start as usize..ftab.end as usize], FTAB_DATA);
        assert_eq!(package.read(0, 4), Some(&[0, 0, 0, 3][..]));
        let end = package.length();
        assert_eq!(
            package.read(end - 2, 475),
            Some(&FTAB_DATA[FTAB_DATA.len() - 2..])
        );
        assert_eq!(package.read(end, 4), Some(&[][..]));
        assert_eq!(package.read(end + 1, 4), None);
        assert_eq!(package.read(u32::MAX, u16::MAX), None);
    }

    #[test]
    fn broken_superbinaries_are_refused() {
        let parse = |edit: &dyn Fn(&mut Vec<u8>)| {
            let mut bytes = package_bytes();
            edit(&mut bytes);
            Package::parse(bytes).err()
        };
        let set = |at: usize, word: u32| {
            move |bytes: &mut Vec<u8>| {
                bytes[at..at + 4].copy_from_slice(&word.to_be_bytes());
            }
        };
        assert_eq!(parse(&|_| {}), None);
        assert_eq!(parse(&set(0, 5)), Some(PackageError::Format(5)));
        assert_eq!(parse(&set(4, 0x30)), Some(PackageError::HeaderLength(0x30)));
        assert_eq!(
            parse(&set(8, u32::MAX)),
            Some(PackageError::Length(u32::MAX))
        );
        assert_eq!(
            parse(&set(40, 0x1000)),
            Some(PackageError::Range("payload headers"))
        );
        assert_eq!(
            parse(&set(0x2c + 0x28 + 4, u32::from_be_bytes(*b"STFC"))),
            Some(PackageError::NoFtab)
        );
        // FTAB's data one byte longer than the binary.
        let ftab_len_at = 0x2c + 0x28 + 36;
        let len = u32::try_from(FTAB_DATA.len()).unwrap();
        assert_eq!(
            parse(&set(ftab_len_at, len + 1)),
            Some(PackageError::Range("payload data"))
        );
        assert_eq!(
            parse(&|bytes| {
                let at = bytes.len() - b"trailer".len() - 1;
                bytes[at] ^= 1;
            }),
            Some(PackageError::DigestMismatch)
        );
        // FTAB's metadata cut to its first entry's header.
        let metadata_len_at = 0x2c + 0x28 + 28;
        assert_eq!(
            parse(&set(metadata_len_at, 8)),
            Some(PackageError::MetadataTlv)
        );
        assert_eq!(
            parse(&set(metadata_len_at, 0)),
            Some(PackageError::NoDigest)
        );
        assert_eq!(
            Package::parse(vec![0; 3]).err(),
            Some(PackageError::TooShort)
        );
    }
}
