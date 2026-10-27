use nom::{
    IResult, Parser as _,
    bytes::complete::{tag, take, take_until},
    combinator::{map, map_res},
    multi::count,
    sequence::terminated,
};

pub const OPCODE: u16 = 0x1D;

/// What the pods say about themselves (slot meanings observed, the layout
/// from their firmware: docs/firmware.md).
#[derive(Debug, Clone, Default)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub struct DeviceInformation {
    pub name: Option<String>,
    /// The primary bud's model number.
    pub model: Option<String>,
    /// E.g. `71.2256000021000000.6663`: version 7.1.
    pub firmware: Option<String>,
}

/// Slots 0–10 of 15, each a NUL-terminated string (empty: just the NUL);
/// 11 and 12 are 16 binary bytes, so parsing stops before them.
const STRING_SLOTS: usize = 11;

/// `[02] [len 2] [2 bytes]`, then the slots: name, model, manufacturer,
/// serial, firmware, firmware again, `1.0.0`, the updater's id, two serials,
/// a number.
pub fn parse(input: &[u8]) -> IResult<&[u8], DeviceInformation> {
    let slot = map_res(
        terminated(take_until(&[0][..]), tag(&[0][..])),
        std::str::from_utf8,
    );
    let (input, _) = take(5usize).parse(input)?;
    map(count(slot, STRING_SLOTS), |slots| {
        let at = |i: usize| Some(slots[i]).filter(|s| !s.is_empty()).map(str::to_owned);
        DeviceInformation {
            name: at(0),
            model: at(1),
            firmware: at(4),
        }
    })
    .parse(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_by_position() {
        let strings = [
            "AirPods Pro",
            "",
            "Apple Inc.",
            "SERIAL0001",
            "71.2256000021000000.6663",
            "71.2256000021000000.6663",
            "1.0.0",
            "com.apple.accessory.updater.app.71",
            "SERIAL000002",
            "SERIAL000003",
            "1234567",
        ];
        let body = [
            &[0x02, 0x00, 0x00, 0x04, 0x01][..],
            &strings.map(|s| [s.as_bytes(), &[0]].concat()).concat(),
            &[0x00; 16],
            &[0],
        ]
        .concat();
        let (_, info) = parse(&body).unwrap();
        assert_eq!(
            info,
            DeviceInformation {
                name: Some("AirPods Pro".into()),
                model: None,
                firmware: Some("71.2256000021000000.6663".into()),
            }
        );
    }
}
