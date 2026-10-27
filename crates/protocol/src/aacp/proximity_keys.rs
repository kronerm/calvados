use nom::{
    IResult, Parser as _,
    bytes::complete::take,
    combinator::map_opt,
    multi::length_count,
    number::complete::{le_u16, u8},
};
use num_enum::TryFromPrimitive;

/// The pods' proximity keys, the reply to [`crate::protocol::request_proximity_keys`].
pub const OPCODE: u16 = 0x31;

#[derive(Debug, Clone, Copy, TryFromPrimitive)]
#[cfg_attr(test, derive(PartialEq, Eq))]
#[repr(u8)]
pub enum KeyType {
    /// Identity Resolving Key: resolves the pods' rotating BLE address.
    Irk = 0x01,
    /// Decrypts the encrypted part of the BLE advertisement.
    EncryptionKey = 0x04,
}

#[derive(Debug, Clone)]
pub struct ProximityKey {
    pub kind: KeyType,
    pub data: Vec<u8>,
}

/// `[type u16 LE] [len u16 LE] [data]` (firmware: docs/firmware.md).
fn key(input: &[u8]) -> IResult<&[u8], ProximityKey> {
    let kind = map_opt(le_u16, |t| {
        u8::try_from(t).ok().and_then(|t| KeyType::try_from(t).ok())
    });
    let (input, (kind, len)) = (kind, le_u16).parse(input)?;
    take(len)
        .map(|data: &[u8]| ProximityKey {
            kind,
            data: data.to_vec(),
        })
        .parse(input)
}

/// `[count] (key) * count`
pub fn parse(input: &[u8]) -> IResult<&[u8], Vec<ProximityKey>> {
    length_count(u8, key).parse(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_keys() {
        let body = [
            &[2, 0x01, 0x00, 0x10, 0x00][..],
            &[0xAA; 16],
            &[0x04, 0x00, 0x10, 0x00],
            &[0xBB; 16],
        ]
        .concat();
        let (rest, keys) = parse(&body).unwrap();
        assert!(rest.is_empty());
        assert!(matches!(
            &keys[..],
            [
                ProximityKey { kind: KeyType::Irk, data: irk },
                ProximityKey { kind: KeyType::EncryptionKey, data: enc },
            ] if irk == &[0xAA; 16] && enc == &[0xBB; 16]
        ));
    }
}
