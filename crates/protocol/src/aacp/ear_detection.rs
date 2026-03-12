use nom::{IResult, Parser as _, number::complete::u8};

pub const OPCODE: u16 = 0x06;

#[derive(Debug, Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub enum EarStatus {
    InEar,
    NotInEar,
    InCase,
    Disconnected,
}

impl From<u8> for EarStatus {
    fn from(byte: u8) -> Self {
        match byte {
            0x00 => Self::InEar,
            0x01 => Self::NotInEar,
            0x02 => Self::InCase,
            _ => Self::Disconnected,
        }
    }
}

impl std::fmt::Display for EarStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InEar => "In",
            Self::NotInEar => "Out",
            Self::InCase => "Case",
            // Not linked: typically in a closed case (or out of range / flat).
            Self::Disconnected => "Off",
        })
    }
}

/// Ear status by role, not by physical bud (see `PrimaryBud`).
#[derive(Debug, Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub struct EarDetection {
    pub primary: EarStatus,
    pub secondary: EarStatus,
}

/// `[primary] [secondary]`
pub fn parse(input: &[u8]) -> IResult<&[u8], EarDetection> {
    (u8, u8)
        .map(|(p, s)| EarDetection {
            primary: p.into(),
            secondary: s.into(),
        })
        .parse(input)
}
