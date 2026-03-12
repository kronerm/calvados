use std::{ffi::c_char, sync::OnceLock};

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // FFI enum — variants mirror the C definition
pub enum BtaStatus {
    Success = 0,
    Failure = 1,
    Pending = 2,
    Busy = 3,
    NoResources = 4,
    WrongMode = 5,
}

pub type Signature = unsafe extern "C" fn(*mut SdpDiRecord, *mut u32) -> BtaStatus;

pub const SYMBOL: &str = "BTA_DmSetLocalDiRecord";
pub static ORIGINAL_FN: OnceLock<Signature> = OnceLock::new();

/// SDP Device Information Record
#[repr(C)]
pub struct SdpDiRecord {
    pub vendor: u16,
    pub vendor_id_source: u16,
    pub product: u16,
    pub version: u16,
    pub primary_record: bool,
    pub client_executable_url: [c_char; 400],
    pub service_description: [c_char; 400],
    pub documentation_url: [c_char; 400],
}

/// This function sets the local Device Information record in SDP.
/// We modify it to spoof Apple's vendor ID (0x004C) so AirPods
/// trust the connection.
#[unsafe(no_mangle)]
#[tracing::instrument]
pub unsafe extern "C" fn BTA_DmSetLocalDiRecord(
    p_device_info: *mut SdpDiRecord,
    p_handle: *mut u32,
) -> BtaStatus {
    if !p_device_info.is_null() {
        unsafe {
            (*p_device_info).vendor = 0x004C; // Apple Inc.
            (*p_device_info).vendor_id_source = 0x0001; // Bluetooth SIG
        }
        tracing::trace!("Spoofed vendor ID to Apple (0x004C)");
    }

    if let Some(original_fn) = ORIGINAL_FN.get() {
        unsafe { original_fn(p_device_info, p_handle) }
    } else {
        BtaStatus::Failure
    }
}
