use nom::{
    IResult, Parser as _,
    bytes::complete::{tag, take},
    multi::fill,
    number::complete::{le_f32, u8},
};

pub const OPCODE: u16 = 0x53;

/// Headphone accommodation EQ.
#[derive(Debug, Clone)]
pub struct EqData {
    pub phone_enabled: bool,
    pub media_enabled: bool,
    /// Four blocks of 8 bands, each 0-100.
    pub bands: [[f32; 8]; 4],
}

/// `84 00 02 [??] [phone] [media] ([f32 LE] × 8) × 4`
pub fn parse(input: &[u8]) -> IResult<&[u8], EqData> {
    let (input, (_, _, phone, media)) =
        (tag(&[0x84, 0x00, 0x02][..]), take(1usize), u8, u8).parse(input)?;
    let mut flat = [0f32; 32];
    let (input, ()) = fill(le_f32, &mut flat).parse(input)?;
    let bands = std::array::from_fn(|block| std::array::from_fn(|band| flat[block * 8 + band]));
    Ok((
        input,
        EqData {
            phone_enabled: phone == 1,
            media_enabled: media == 1,
            bands,
        },
    ))
}
