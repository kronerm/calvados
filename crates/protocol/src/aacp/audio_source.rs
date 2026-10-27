use nom::{IResult, Parser as _, number::complete::u8};

use super::reversed_mac;
use crate::MacAddress;

pub const OPCODE: u16 = 0x0E;

bitflags::bitflags! {
    /// What's playing, over all hosts: the pods keep one mask, set and
    /// cleared as each host's audio starts and stops (firmware:
    /// docs/firmware.md). Other bits are activities not identified yet.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Activity: u8 {
        const CALL = 0x01;
        const MEDIA = 0x02;
        const _ = !0;
    }
}

impl Activity {
    pub fn is_playing(self) -> bool {
        self.intersects(Self::CALL | Self::MEDIA)
    }
}

/// The host whose audio changed last, and what's playing: not who owns the
/// audio. With nothing playing the pods send a zero address.
#[derive(Debug, Clone, Copy)]
pub struct AudioSource {
    pub mac: MacAddress,
    pub kind: Activity,
}

impl AudioSource {
    /// Whether `mac` is actively streaming (media or call).
    pub fn is_active(&self, mac: MacAddress) -> bool {
        self.mac == mac && self.kind.is_playing()
    }
}

/// `[mac 6, reversed] [activity]`
pub fn parse(input: &[u8]) -> IResult<&[u8], AudioSource> {
    (reversed_mac, u8)
        .map(|(mac, kind)| AudioSource {
            mac,
            kind: Activity::from_bits_retain(kind),
        })
        .parse(input)
}
