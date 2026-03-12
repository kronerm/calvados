use nom::{IResult, Parser as _};

use num_enum::TryFromPrimitive;

use super::byte;

pub const OPCODE: u16 = 0x19;

#[derive(Debug, Clone, Copy, Hash, TryFromPrimitive)]
#[cfg_attr(test, derive(PartialEq, Eq))]
#[repr(u8)]
pub enum StemPressType {
    Single = 0x05,
    Double = 0x06,
    Triple = 0x07,
    Long = 0x08,
}

#[derive(Debug, Clone, Copy, Hash, TryFromPrimitive)]
#[cfg_attr(test, derive(PartialEq, Eq))]
#[repr(u8)]
pub enum StemBud {
    Left = 0x01,
    Right = 0x02,
}

#[derive(Debug, Clone, Copy)]
pub struct StemPress {
    pub kind: StemPressType,
    pub bud: StemBud,
}

/// `[kind] [bud]`
pub fn parse(input: &[u8]) -> IResult<&[u8], StemPress> {
    (byte, byte)
        .map(|(kind, bud)| StemPress { kind, bud })
        .parse(input)
}
