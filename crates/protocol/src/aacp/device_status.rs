use nom::{IResult, Parser as _, bytes::complete::take, number::complete::u8, sequence::preceded};

use num_enum::TryFromPrimitive;

use super::{byte, reversed_mac};
use crate::MacAddress;

pub const CONNECTION_OPCODE: u16 = 0x0C;
pub const PRIMARY_BUD_OPCODE: u16 = 0x08;
pub const NOISE_LEVEL_OPCODE: u16 = 0x55;

/// A device's link to the pods. Sent for every device at connect (hosts and
/// the other bud) and when a host disconnects. A host *connecting* later
/// comes as a new `ConnectedDevices` list instead.
#[derive(Debug, Clone, Copy)]
pub struct DeviceConnectionStatus {
    pub mac: MacAddress,
    pub connected: bool,
    pub role: DeviceRole,
}

#[derive(Debug, Clone, Copy, Hash, TryFromPrimitive)]
#[repr(u8)]
pub enum DeviceRole {
    /// A phone or computer.
    Host = 1,
    /// The other earbud (each bud has its own address).
    Bud = 2,
}

/// `[mac 6, reversed] [connected 0/1] [role]`
pub fn parse_connection(input: &[u8]) -> IResult<&[u8], DeviceConnectionStatus> {
    (reversed_mac, u8, byte)
        .map(|(mac, connected, role)| DeviceConnectionStatus {
            mac,
            connected: connected != 0,
            role,
        })
        .parse(input)
}

#[derive(Debug, Clone, Copy, Hash, TryFromPrimitive)]
#[cfg_attr(test, derive(PartialEq, Eq))]
#[repr(u8)]
pub enum Bud {
    Left = 1,
    Right = 2,
}

/// Which physical bud holds the primary (microphone) role.
#[derive(Debug, Clone, Copy)]
pub struct PrimaryBud {
    pub bud: Bud,
    /// Inferred: toggles around a switch; meaning unknown.
    pub active: bool,
}

/// `[bud] 00 01 [active]`
pub fn parse_primary_bud(input: &[u8]) -> IResult<&[u8], PrimaryBud> {
    (byte, take(2usize), u8)
        .map(|(bud, _, active)| PrimaryBud {
            bud,
            active: active != 0,
        })
        .parse(input)
}

/// `[?] [?] [?] [level 0-100]`. Inferred: the applied adaptive noise level.
/// Observed: only at switches, to the host gaining the audio: 25 as the old
/// stream drops, then 50, the configured `AdaptiveNoiseLevel`.
pub fn parse_noise_level(input: &[u8]) -> IResult<&[u8], u8> {
    preceded(take(3usize), u8).parse(input)
}
