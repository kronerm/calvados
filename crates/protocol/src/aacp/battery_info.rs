use nom::{IResult, Parser as _, bytes::complete::tag, multi::length_count, number::complete::u8};

use num_enum::TryFromPrimitive;

pub const OPCODE: u16 = 0x04;

#[derive(Debug, Clone, Copy, Hash, TryFromPrimitive)]
#[cfg_attr(test, derive(PartialEq, Eq))]
#[repr(u8)]
pub enum BatteryComponent {
    Right = 0x02,
    Left = 0x04,
    Case = 0x08,
}

#[derive(Debug, Clone, Copy, Hash, TryFromPrimitive)]
#[cfg_attr(test, derive(PartialEq, Eq))]
#[repr(u8)]
pub enum ChargingStatus {
    NotCharging = 0x00,
    Charging = 0x01,
    Discharging = 0x02,
    /// Component absent; its level is meaningless.
    Disconnected = 0x04,
}

#[derive(Debug, Clone, Copy)]
pub struct BatteryInfo {
    pub component: BatteryComponent,
    pub level: u8,
    /// `None` for an unrecognised status byte.
    pub status: Option<ChargingStatus>,
}

/// `[component] 01 [level] [status] 01`; `None` for an unknown component.
fn entry(input: &[u8]) -> IResult<&[u8], Option<BatteryInfo>> {
    (u8, tag(&[0x01][..]), u8, u8, tag(&[0x01][..]))
        .map(|(component, _, level, status, _)| {
            Some(BatteryInfo {
                component: component.try_into().ok()?,
                level,
                status: status.try_into().ok(),
            })
        })
        .parse(input)
}

/// `[count] (entry) * count`
pub fn parse(input: &[u8]) -> IResult<&[u8], Vec<BatteryInfo>> {
    length_count(u8, entry)
        .map(|entries| entries.into_iter().flatten().collect())
        .parse(input)
}
