//! The pods' BLE "proximity pairing" advertisement (Apple manufacturer data,
//! type 0x07): sent while the case is open or a bud is out, from a rotating
//! private address that the pods' IRK resolves.

use aes::Aes128;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use num_enum::IntoPrimitive;

use crate::MacAddress;

/// Apple's Bluetooth company identifier.
pub const APPLE_COMPANY_ID: u16 = 0x004C;

/// Proximity pairing's message type, the first byte of Apple's manufacturer
/// data.
pub const PROXIMITY_PAIRING: u8 = 0x07;

/// Fields of the proximity pairing data (from the type byte, after the
/// company id), by byte (LibrePods' `BLEManager`).
#[derive(Debug, Clone, Copy, IntoPrimitive)]
#[repr(usize)]
enum Field {
    Status = 5,
    Lid = 8,
}

impl Field {
    fn read(self, data: &[u8]) -> Option<u8> {
        data.get(usize::from(self)).copied()
    }
}

bitflags::bitflags! {
    /// [`Field::Status`] bits used here.
    #[derive(Debug, Clone, Copy)]
    struct Status: u8 {
        /// The advertising bud is in the case. Only such a bud's lid bit
        /// holds: worn buds advertised "open" with the case closed (Oct 8),
        /// and only it knows the case's level (worn buds send `0xFF`).
        const IN_CASE = 0x40;
    }

    /// [`Field::Lid`] bits used here.
    #[derive(Debug, Clone, Copy)]
    struct Lid: u8 {
        const CLOSED = 0x08;
    }
}

/// Where the pods are, as their advertisement says.
#[derive(Debug, Clone, Copy)]
pub struct Proximity {
    pub lid_open: bool,
    /// The advertising bud is in the case.
    pub in_case: bool,
}

impl Proximity {
    /// From Apple's manufacturer data, starting at the type byte.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.first() != Some(&PROXIMITY_PAIRING) {
            return None;
        }
        let status = Status::from_bits_retain(Field::Status.read(data)?);
        let lid = Lid::from_bits_retain(Field::Lid.read(data)?);
        Some(Self {
            lid_open: !lid.contains(Lid::CLOSED),
            in_case: status.contains(Status::IN_CASE),
        })
    }

    /// Out to be used: the lid open, or a bud out of the case. Pods charging
    /// in a closed case advertise too.
    pub fn out(self) -> bool {
        self.lid_open || !self.in_case
    }
}

/// Manufacturer data and mask for a controller's scan filter, from the type
/// byte: proximity pairing advertisements, narrowed down.
#[derive(Debug, Clone, Copy)]
pub struct Pattern {
    pub data: [u8; Field::Lid as usize + 1],
    pub mask: [u8; Field::Lid as usize + 1],
}

impl Pattern {
    pub fn proximity() -> Self {
        let mut pattern = Self {
            data: Default::default(),
            mask: Default::default(),
        };
        (pattern.data[0], pattern.mask[0]) = (PROXIMITY_PAIRING, 0xFF);
        pattern
    }

    /// Not from the closed case (as far as the lid bit holds; see
    /// [`Status::IN_CASE`]).
    pub fn lid_open(self) -> Self {
        self.with(Field::Lid, Lid::CLOSED.bits(), 0)
    }

    fn with(mut self, field: Field, mask: u8, value: u8) -> Self {
        let byte = usize::from(field);
        self.mask[byte] |= mask;
        self.data[byte] = self.data[byte] & !mask | value;
        self
    }
}

/// A pair's stored keys, as the pods sent them.
#[derive(Debug, Clone, Copy)]
pub struct PodsKeys {
    pub irk: [u8; 16],
    pub encryption: Option<[u8; 16]>,
}

/// Which stored pair an advertisement from `address` comes from.
pub fn identify(
    address: [u8; 6],
    keys: impl IntoIterator<Item = (MacAddress, PodsKeys)>,
) -> Option<(MacAddress, PodsKeys)> {
    keys.into_iter()
        .find(|(_, keys)| resolves(&keys.irk, address))
}

/// The case's level from the advertisement's encrypted part (its last 16
/// bytes, AES-128 decrypted with the encryption key): byte 3, the top bit
/// charging. `None` when unknown (`0xFF` or `0xFF & 0x7F` while charging,
/// LibrePods).
pub fn case_level(advertisement: &[u8], encryption_key: &[u8; 16]) -> Option<u8> {
    let start = advertisement.len().checked_sub(16)?;
    if advertisement.first() != Some(&PROXIMITY_PAIRING) || start < usize::from(Field::Lid) {
        return None;
    }
    let mut block = <[u8; 16]>::try_from(&advertisement[start..]).ok()?.into();
    Aes128::new(&(*encryption_key).into()).decrypt_block(&mut block);
    let level = block[3] & 0x7F;
    (level <= 100).then_some(level)
}

/// Whether `address` (display order) is a resolvable private address made
/// from `irk` (Bluetooth Core, Vol 3 Part H, 2.2.2). `irk` is least
/// significant byte first, as the pods send it.
pub fn resolves(irk: &[u8; 16], address: [u8; 6]) -> bool {
    let (prand, hash) = address.split_at(3);
    let mut key = *irk;
    key.reverse();
    let mut block = [0u8; 16];
    block[13..].copy_from_slice(prand);
    let mut block = block.into();
    Aes128::new(&key.into()).encrypt_block(&mut block);
    block[13..] == *hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decrypts_the_case_level() {
        let key: [u8; 16] = std::array::from_fn(|i| i as u8 * 7);
        let encrypt = |plain: [u8; 16]| {
            let mut block = plain.into();
            Aes128::new(&key.into()).encrypt_block(&mut block);
            <[u8; 16]>::from(block)
        };
        let advertisement = |case| {
            let mut plain = [0u8; 16];
            plain[3] = case;
            [&[PROXIMITY_PAIRING; 1][..], &[0u8; 8], &encrypt(plain)].concat()
        };
        assert_eq!(case_level(&advertisement(59), &key), Some(59));
        assert_eq!(
            case_level(&advertisement(0x80 | 42), &key),
            Some(42),
            "charging"
        );
        assert_eq!(case_level(&advertisement(0xFF), &key), None);
        assert_eq!(case_level(&advertisement(0x80 | 127), &key), None);
        assert_eq!(
            case_level(&advertisement(59)[..20], &key),
            None,
            "too short"
        );
    }

    #[test]
    fn reads_the_lid_and_the_case() {
        let (status, lid) = (usize::from(Field::Status), usize::from(Field::Lid));
        let mut data = [0u8; 25];
        data[0] = PROXIMITY_PAIRING;
        data[status] = Status::IN_CASE.bits();
        data[lid] = Lid::CLOSED.bits();
        let charging = Proximity::parse(&data).unwrap();
        assert!(!charging.lid_open && charging.in_case && !charging.out());
        data[lid] = 0;
        assert!(Proximity::parse(&data).unwrap().out(), "lid open");
        data[lid] = Lid::CLOSED.bits();
        data[status] = 0;
        assert!(Proximity::parse(&data).unwrap().out(), "a bud out");
        // Captured Oct 8, a worn bud with the case closed.
        let worn = hex::decode("07190124200b448f110009403f32fb9df2890e14c05e07c81fd314").unwrap();
        let worn = Proximity::parse(&worn).unwrap();
        assert!(worn.lid_open && !worn.in_case, "its lid bit reads open");
        assert!(Proximity::parse(&data[..5]).is_none());
        assert!(Proximity::parse(&[0x12, 0, 0, 0, 0, 0, 0, 0, 0]).is_none());
    }

    #[test]
    fn patterns_match_the_wanted_bits() {
        let pattern = Pattern::proximity().lid_open();
        let (status, lid) = (usize::from(Field::Status), usize::from(Field::Lid));
        assert_eq!(
            (pattern.data[0], pattern.mask[0]),
            (PROXIMITY_PAIRING, 0xFF)
        );
        assert_eq!(pattern.mask[status], 0);
        assert_eq!((pattern.data[lid], pattern.mask[lid]), (0, 0x08));
    }

    /// Core spec Vol 3 Part H, Appendix D.7: IRK ec0234a3…, prand 708194,
    /// hash 0dfbaa.
    #[test]
    fn resolves_the_spec_sample_address() {
        let mut irk = 0xec0234a357c8ad05341010a60a397d9b_u128.to_be_bytes();
        irk.reverse();
        assert!(resolves(&irk, [0x70, 0x81, 0x94, 0x0d, 0xfb, 0xaa]));
        assert!(!resolves(&irk, [0x70, 0x81, 0x94, 0x0d, 0xfb, 0xab]));
    }
}
