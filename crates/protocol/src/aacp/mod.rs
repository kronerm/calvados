//! Typed AACP packets and their nom decoders, one module per opcode.

pub mod audio_source;
pub mod battery_info;
pub mod connected_devices;
pub mod connection_history;
pub mod control_command;
pub mod conversation_awareness;
pub mod device_information;
pub mod device_status;
pub mod ear_detection;
pub mod eq_data;
pub mod frame;
pub mod proximity_keys;
pub mod smart_routing;
pub mod stem_press;

use nom::{IResult, Parser as _, combinator::map};

use crate::MacAddress;

pub use audio_source::{Activity, AudioSource};
pub use battery_info::{BatteryComponent, BatteryInfo, ChargingStatus};
pub use connection_history::{ConnectionHistory, HistoryEvent, LinkChange};
pub use control_command::{ControlCommand, ControlId, NoiseControlMode};
pub use device_status::{Bud, DeviceConnectionStatus, DeviceRole, PrimaryBud};
pub use ear_detection::{EarDetection, EarStatus};
pub use eq_data::EqData;
pub use proximity_keys::{KeyType, ProximityKey};
pub use smart_routing::{Routing, RoutingResponse};
pub use stem_press::{StemBud, StemPress, StemPressType};

/// nom parser for a `TryFromPrimitive` byte enum; fails on unknown bytes.
pub(crate) fn byte<T: TryFrom<u8>>(input: &[u8]) -> IResult<&[u8], T> {
    nom::combinator::map_opt(nom::number::complete::u8, |b| T::try_from(b).ok()).parse(input)
}

/// nom parser for a 6-byte MAC stored least-significant octet first.
pub(crate) fn reversed_mac(input: &[u8]) -> IResult<&[u8], MacAddress> {
    map(nom::bytes::complete::take(6usize), |b: &[u8]| {
        from_reversed(b.try_into().expect("take(6)"))
    })
    .parse(input)
}

/// A MAC from bytes least-significant octet first, as most fields carry it.
pub(crate) fn from_reversed(mut bytes: [u8; 6]) -> MacAddress {
    bytes.reverse();
    MacAddress::new(bytes)
}

/// `mac`'s bytes least-significant octet first.
pub(crate) fn to_reversed(mac: MacAddress) -> [u8; 6] {
    let mut bytes = mac.to_array();
    bytes.reverse();
    bytes
}

/// One decoded AACP packet.
#[derive(Debug, Clone)]
pub enum Packet {
    HandshakeAck,
    AudioSource(AudioSource),
    Battery(Vec<BatteryInfo>),
    ConnectedDevices(Vec<MacAddress>),
    ConnectionHistory(ConnectionHistory),
    Control(ControlCommand),
    ConversationAwareness(u8),
    DeviceConnection(DeviceConnectionStatus),
    DeviceInformation(device_information::DeviceInformation),
    EarDetection(EarDetection),
    Eq(EqData),
    NoiseLevel(u8),
    PrimaryBud(PrimaryBud),
    ProximityKeys(Vec<proximity_keys::ProximityKey>),
    RoutingResponse(RoutingResponse),
    StemPress(StemPress),
    /// One UARP message (opcode 0x4F; [`crate::uarp`]).
    Uarp(Vec<u8>),
    Opaque(Opaque),
}

/// Known or unknown opcode whose payload we don't decode.
#[derive(Clone)]
pub struct Opaque {
    pub opcode: u16,
    pub payload: Vec<u8>,
}

/// `0x2b, 96 bytes: 01 55 …`: the opcode in hex and a short prefix (the full
/// bytes are in the `aacp_raw` trace).
impl std::fmt::Debug for Opaque {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const SHOWN: usize = 16;
        let (shown, more) = (
            &self.payload[..self.payload.len().min(SHOWN)],
            self.payload.len() > SHOWN,
        );
        write!(
            f,
            "{:#04x}, {} bytes: {:?}",
            self.opcode,
            self.payload.len(),
            HexDump(shown)
        )?;
        if more {
            f.write_str(" …")?;
        }
        Ok(())
    }
}

/// Bytes as space-separated hex, `01 55 00`.
pub(crate) struct HexDump<'a>(pub &'a [u8]);

impl std::fmt::Debug for HexDump<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.iter().enumerate().try_for_each(|(i, b)| match i {
            0 => write!(f, "{b:02x}"),
            _ => write!(f, " {b:02x}"),
        })
    }
}

impl Packet {
    /// Decode one complete frame. `None` if it isn't an AACP frame at all.
    pub fn parse(frame: &[u8]) -> Option<Self> {
        match frame.split_first_chunk::<4>()? {
            (&frame::HANDSHAKE_ACK_MAGIC, _) => Some(Self::HandshakeAck),
            (&frame::DATA_MAGIC, rest) => {
                let (opcode, body) = rest.split_first_chunk::<2>()?;
                let opcode = u16::from_le_bytes(*opcode);
                Some(Self::parse_body(opcode, body).map_or_else(
                    |_| {
                        Self::Opaque(Opaque {
                            opcode,
                            payload: body.to_vec(),
                        })
                    },
                    |(_, packet)| packet,
                ))
            }
            _ => None,
        }
    }

    fn parse_body(opcode: u16, body: &[u8]) -> IResult<&[u8], Self> {
        match opcode {
            audio_source::OPCODE => map(audio_source::parse, Self::AudioSource).parse(body),
            battery_info::OPCODE => map(battery_info::parse, Self::Battery).parse(body),
            connected_devices::OPCODE => {
                map(connected_devices::parse, Self::ConnectedDevices).parse(body)
            }
            connection_history::OPCODE => {
                map(connection_history::parse, Self::ConnectionHistory).parse(body)
            }
            control_command::OPCODE => map(control_command::parse, Self::Control).parse(body),
            conversation_awareness::OPCODE => {
                map(conversation_awareness::parse, Self::ConversationAwareness).parse(body)
            }
            device_status::CONNECTION_OPCODE => {
                map(device_status::parse_connection, Self::DeviceConnection).parse(body)
            }
            device_status::NOISE_LEVEL_OPCODE => {
                map(device_status::parse_noise_level, Self::NoiseLevel).parse(body)
            }
            device_status::PRIMARY_BUD_OPCODE => {
                map(device_status::parse_primary_bud, Self::PrimaryBud).parse(body)
            }
            device_information::OPCODE => {
                map(device_information::parse, Self::DeviceInformation).parse(body)
            }
            ear_detection::OPCODE => map(ear_detection::parse, Self::EarDetection).parse(body),
            proximity_keys::OPCODE => map(proximity_keys::parse, Self::ProximityKeys).parse(body),
            eq_data::OPCODE => map(eq_data::parse, Self::Eq).parse(body),
            smart_routing::RESPONSE_OPCODE => {
                map(smart_routing::parse_response, Self::RoutingResponse).parse(body)
            }
            stem_press::OPCODE => map(stem_press::parse, Self::StemPress).parse(body),
            crate::uarp::OPCODE => map(crate::uarp::parse_frame, Self::Uarp).parse(body),
            _ => Err(nom::Err::Error(nom::error::Error::new(
                body,
                nom::error::ErrorKind::Switch,
            ))),
        }
    }
}
