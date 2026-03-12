//! AACP framing: `[msg_type u16 LE] [protocol 0x0004 LE]`, then for data
//! frames `[opcode u16 LE] [payload]`.

/// Host → AirPods session request; must precede everything else.
pub const HANDSHAKE_MAGIC: [u8; 4] = [0x00, 0x00, 0x04, 0x00];
/// AirPods → host handshake acknowledgement (undocumented payload).
pub const HANDSHAKE_ACK_MAGIC: [u8; 4] = [0x01, 0x00, 0x04, 0x00];
/// Bidirectional data frame.
pub const DATA_MAGIC: [u8; 4] = [0x04, 0x00, 0x04, 0x00];

/// Build a data frame: `[04 00 04 00] [opcode LE] [payload]`.
pub fn data_frame(opcode: u16, payload: &[u8]) -> Vec<u8> {
    [&DATA_MAGIC[..], &opcode.to_le_bytes(), payload].concat()
}
