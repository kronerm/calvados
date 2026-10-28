//! A take after another host had the audio: the pods suspended our stream
//! when that host took it, and `btif_av` then refuses to start it until a
//! local suspend or the remote's AVRCP PLAY (`kFlagRemoteSuspend`), which
//! never comes; playback stays silent until a seek. Calvados' claim to the
//! pods (AACP `OwnsConnection = 1`) clears the flag, as that PLAY would, and
//! starts the stream: the audio HAL doesn't ask again (Pixel, Android 17).

use std::ffi::c_void;
use std::sync::OnceLock;

/// `L2CA_DataWrite(cid, BT_HDR*)`: every L2CAP send, the AACP socket's too.
pub const DATA_WRITE: &str = "_Z14L2CA_DataWritetP6BT_HDR";
/// The same as a member of the L2CAP interface (Android 17), which the
/// free function forwards to and callers may use directly.
pub const IMPL_DATA_WRITE: &str = "_ZN9bluetooth5stack5l2cap4Impl14L2CA_DataWriteEtP6BT_HDR";
/// `btif_av_clear_remote_suspend_flag(A2dpType)`: posts to the main thread.
pub const CLEAR_REMOTE_SUSPEND: &str = "_Z33btif_av_clear_remote_suspend_flag8A2dpType";
/// `btif_av_stream_start_with_latency(bool)`, as for the HAL's start
/// request: posts after the clear; ignored once started.
pub const START: &str = "_Z33btif_av_stream_start_with_latencyb";

pub type DataWrite = unsafe extern "C" fn(u16, *mut BtHdr) -> u8;
pub type ImplDataWrite = unsafe extern "C" fn(*mut c_void, u16, *mut BtHdr) -> u8;
pub type ClearRemoteSuspend = unsafe extern "C" fn(A2dpType);
pub type Start = unsafe extern "C" fn(bool);

pub static ORIGINAL_DATA_WRITE: OnceLock<DataWrite> = OnceLock::new();
pub static ORIGINAL_IMPL_DATA_WRITE: OnceLock<ImplDataWrite> = OnceLock::new();
pub static CLEAR: OnceLock<ClearRemoteSuspend> = OnceLock::new();
pub static START_FN: OnceLock<Start> = OnceLock::new();

/// `BT_HDR`: the payload is `len` bytes from `data + offset`.
#[repr(C)]
pub struct BtHdr {
    event: u16,
    len: u16,
    offset: u16,
    layer_specific: u16,
    data: [u8; 0],
}

#[repr(i32)]
#[allow(dead_code)] // FFI enum: variants mirror the C++ definition
#[derive(Debug, Clone, Copy)]
pub enum A2dpType {
    Source = 0,
    Sink = 1,
    Unknown = 2,
}

/// AACP control `OwnsConnection = 1`, as `protocol::set_ownership(true)`.
const CLAIM: [u8; 11] = [
    0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x06, 0x01, 0x00, 0x00, 0x00,
];

unsafe fn observe(p_data: *const BtHdr) {
    if p_data.is_null() {
        return;
    }
    let header = unsafe { &*p_data };
    if usize::from(header.len) != CLAIM.len() {
        return;
    }
    let payload = unsafe {
        std::slice::from_raw_parts(
            header.data.as_ptr().add(usize::from(header.offset)),
            CLAIM.len(),
        )
    };
    if payload != CLAIM {
        return;
    }
    match (CLEAR.get(), START_FN.get()) {
        (Some(clear), Some(start)) => {
            tracing::info!("audio claimed: clearing the A2DP remote suspend, starting");
            unsafe {
                clear(A2dpType::Source);
                start(false);
            }
        }
        _ => tracing::warn!("audio claimed, but no way to restart the stream"),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn L2CA_DataWrite(cid: u16, p_data: *mut BtHdr) -> u8 {
    // Before the original takes the buffer.
    unsafe { observe(p_data) };
    match ORIGINAL_DATA_WRITE.get() {
        Some(original) => unsafe { original(cid, p_data) },
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn calvados_impl_l2ca_data_write(
    this: *mut c_void,
    cid: u16,
    p_data: *mut BtHdr,
) -> u8 {
    unsafe { observe(p_data) };
    match ORIGINAL_IMPL_DATA_WRITE.get() {
        Some(original) => unsafe { original(this, cid, p_data) },
        None => 0,
    }
}
