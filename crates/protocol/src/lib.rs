//! AirPods AACP protocol: packet decoding ([`aacp`]), framing and commands
//! ([`protocol`]), device state ([`state`]) and the ownership [`engine`].

pub mod aacp;
pub mod ble;
pub mod connection;
pub mod display;
pub mod engine;
mod mac_address;
pub mod protocol;
pub mod settings;
pub mod state;

pub use mac_address::MacAddress;
