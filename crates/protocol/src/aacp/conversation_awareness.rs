use nom::{IResult, Parser as _, bytes::complete::take, number::complete::u8, sequence::preceded};

pub const OPCODE: u16 = 0x4B;

/// `[3 bytes] [level]`. Inferred: conversation awareness' level (LibrePods:
/// low while speaking ducks the volume, higher ramps it back). Observed:
/// only 7, to the host losing the audio at a switch, with conversation
/// awareness off: likely restoring its volume.
pub fn parse(input: &[u8]) -> IResult<&[u8], u8> {
    preceded(take(3usize), u8).parse(input)
}
