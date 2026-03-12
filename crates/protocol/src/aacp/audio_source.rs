use nom::{IResult, Parser as _};

use num_enum::TryFromPrimitive;

use super::{byte, reversed_mac};
use crate::MacAddress;

pub const OPCODE: u16 = 0x0E;

/// What the current audio source is playing.
#[derive(Debug, Clone, Copy, Hash, TryFromPrimitive)]
#[cfg_attr(test, derive(PartialEq, Eq))]
#[repr(u8)]
pub enum AudioSourceType {
    None = 0x00,
    Call = 0x01,
    Media = 0x02,
}

/// Which device the firmware considers the active audio source.
#[derive(Debug, Clone, Copy)]
pub struct AudioSource {
    pub mac: MacAddress,
    pub kind: AudioSourceType,
}

impl AudioSource {
    /// Whether `mac` is actively streaming (media or call).
    pub fn is_active(&self, mac: MacAddress) -> bool {
        self.mac == mac && !matches!(self.kind, AudioSourceType::None)
    }
}

/// `[mac 6, reversed] [type]`
pub fn parse(input: &[u8]) -> IResult<&[u8], AudioSource> {
    (reversed_mac, byte)
        .map(|(mac, kind)| AudioSource { mac, kind })
        .parse(input)
}
