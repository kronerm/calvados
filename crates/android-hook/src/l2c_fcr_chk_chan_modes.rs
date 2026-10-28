use std::{ffi::c_void, sync::OnceLock};

pub const SYMBOL: &str = "l2c_fcr_chk_chan_modes";
pub type Signature = unsafe extern "C" fn(*mut c_void) -> u8;

pub static ORIGINAL_FN: OnceLock<Signature> = OnceLock::new();

/// The original function checks if L2CAP Flow Control/Retransmission modes
/// are compatible. AirPods don't respond to mode negotiation until they
/// receive the AACP handshake, so we force this check to always pass.
#[unsafe(no_mangle)]
#[tracing::instrument]
pub unsafe extern "C" fn l2c_fcr_chk_chan_modes(p_ccb: *mut c_void) -> u8 {
    // TODO: do this only for AirPods and similar devices, not for all devices.
    // It might be possible to retrieve the MAC address via `p_ccb`.

    // Logged to see which Android releases still need it.
    match ORIGINAL_FN
        .get()
        .map(|original_fn| unsafe { original_fn(p_ccb) })
    {
        Some(0) => tracing::info!("channel modes rejected by the stack, accepted anyway"),
        Some(_) => tracing::info!("channel modes accepted by the stack"),
        None => tracing::warn!("no original function"),
    }
    1
}
