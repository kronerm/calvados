use nom::{
    IResult, Parser as _,
    bytes::complete::{tag, take_till, take_until},
    combinator::map_res,
    multi::many0,
    sequence::{preceded, terminated},
};

pub const OPCODE: u16 = 0x1D;

/// `[binary preamble] (utf8 \0)*` → non-empty strings in order: name, model,
/// serials, firmware, … (later fields are only partly understood). The model
/// is the primary bud's: `A3048` with the left primary, `A3047` the right.
pub fn parse(input: &[u8]) -> IResult<&[u8], Vec<String>> {
    let string = map_res(
        terminated(take_until(&[0][..]), tag(&[0][..])),
        std::str::from_utf8,
    );
    preceded(take_till(|b: u8| (0x20..=0x7E).contains(&b)), many0(string))
        .map(|fields| {
            fields
                .into_iter()
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .parse(input)
}
