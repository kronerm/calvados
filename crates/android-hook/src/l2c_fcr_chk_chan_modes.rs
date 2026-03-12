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
    tracing::trace!("l2c_fcr_chk_chan_modes called");

    // TODO: do this only for AirPods and similar devices, not for all devices.
    // It might be possible to retrieve the MAC address via `p_ccb`.

    if let Some(original_fn) = ORIGINAL_FN.get() {
        let result = unsafe { original_fn(p_ccb) };
        tracing::trace!(original_fn_return = result, "original function returned");
    }

    1
}
