use nom::{
    IResult, Parser as _,
    bytes::complete::take,
    multi::length_count,
    number::complete::{be_u16, u8},
};
use num_enum::TryFromPrimitive;

use super::byte;

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

/// `[type] [len u16 BE] [reserved] [data]`
fn key(input: &[u8]) -> IResult<&[u8], ProximityKey> {
    let (input, (kind, len, _)) = (byte, be_u16, u8).parse(input)?;
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
