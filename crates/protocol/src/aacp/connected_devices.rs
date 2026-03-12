use nom::{
    IResult, Parser as _,
    bytes::complete::take,
    multi::length_count,
    number::complete::u8,
    sequence::{preceded, terminated},
};

use crate::MacAddress;

pub const OPCODE: u16 = 0x2E;

/// One entry: `[mac 6, canonical order] [info 2]`.
fn device(input: &[u8]) -> IResult<&[u8], MacAddress> {
    terminated(take(6usize), take(2usize))
        .map(|b: &[u8]| MacAddress::new(b.try_into().expect("take(6)")))
        .parse(input)
}

/// `[?? ??] [count] (entry) * count`
pub fn parse(input: &[u8]) -> IResult<&[u8], Vec<MacAddress>> {
    preceded(take(2usize), length_count(u8, device)).parse(input)
}
