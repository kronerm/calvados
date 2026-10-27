//! Intents in and out of `PersistentService`. All are explicit, so actions
//! need no namespace.

use jni::Env;
use jni::objects::JObject;

use crate::jni::android::bluetooth::device::BluetoothDevice;
use crate::jni::android::content::context::Context;
use crate::jni::android::content::intent::Intent;
use crate::service::Event;

/// Set the noise control mode: `NOISE:` and its name
/// ([`crate::proto::display::mode_name`]).
pub(crate) const ACTION_NOISE_PREFIX: &str = "NOISE:";

/// Flip automatic switching (media and calls here take the audio).
pub(crate) const ACTION_AUTO_SWITCH: &str = "AUTO_SWITCH";

/// Flip connecting the pods when they show up nearby.
pub(crate) const ACTION_AUTO_CONNECT: &str = "AUTO_CONNECT";

/// Update the pods to the catalog's release (the notification's action).
pub(crate) const ACTION_UPDATE_FIRMWARE: &str = "UPDATE_FIRMWARE";

/// Withdraw this phone's firmware update.
pub(crate) const ACTION_CANCEL_UPDATE: &str = "CANCEL_UPDATE";

/// The pods' ACL link came up (from the ACL broadcast): attach if idle.
pub(crate) const ACTION_AIRPODS_AVAILABLE: &str = "AIRPODS_AVAILABLE";

/// BLE scan results (see [`crate::bluetooth::set_presence_scan`]).
pub(crate) const ACTION_NEARBY: &str = "NEARBY";

/// Bluetooth turned on: scans by `PendingIntent` don't survive it off, so
/// the presence scan starts again.
pub(crate) const ACTION_BLUETOOTH_ON: &str = "BLUETOOTH_ON";

/// The pods' ACL link went down: stop attaching.
pub(crate) const ACTION_AIRPODS_GONE: &str = "AIRPODS_GONE";

pub(crate) const ACL_CONNECTED: &str = "android.bluetooth.device.action.ACL_CONNECTED";

pub(crate) const ACL_DISCONNECTED: &str = "android.bluetooth.device.action.ACL_DISCONNECTED";

/// `BluetoothAdapter.ACTION_STATE_CHANGED`, with [`EXTRA_ADAPTER_STATE`].
pub(crate) const ADAPTER_STATE_CHANGED: &str = "android.bluetooth.adapter.action.STATE_CHANGED";

/// `BluetoothAdapter.EXTRA_STATE`; [`STATE_ON`] once on.
const EXTRA_ADAPTER_STATE: &str = "android.bluetooth.adapter.extra.STATE";

/// `BluetoothAdapter.STATE_ON`.
const STATE_ON: i32 = 12;

/// `TelephonyManager.ACTION_PHONE_STATE_CHANGED`, with [`EXTRA_STATE`].
pub(crate) const PHONE_STATE: &str = "android.intent.action.PHONE_STATE";

/// `TelephonyManager.EXTRA_STATE`: `IDLE`, `RINGING` or `OFFHOOK`.
pub(crate) const EXTRA_STATE: &str = "state";

/// `BluetoothDevice.EXTRA_DEVICE`.
pub(crate) const EXTRA_DEVICE: &str = "android.bluetooth.device.extra.DEVICE";

/// An explicit intent for `PersistentService` with `action`.
pub(crate) fn service_intent<'local>(
    env: &mut Env<'local>,
    context: &Context<impl jni::refs::Reference + AsRef<JObject<'local>>>,
    action: &str,
) -> anyhow::Result<Intent<JObject<'local>>> {
    let package = context.get_package_name(env)?;
    Intent::new(env)?.set_action(env, action)?.set_class_name(
        env,
        &package,
        &format!("{package}.PersistentService"),
    )
}

/// The loop event for an incoming intent (service start or ACL broadcast),
/// if it matters.
pub(crate) fn parse<'local>(
    env: &mut Env<'local>,
    intent: Intent<JObject<'local>>,
) -> anyhow::Result<Option<Event>> {
    if intent.0.is_null() {
        return Ok(None);
    }
    let action = intent.get_action(env)?;
    let action = match action.as_str() {
        ACTION_NEARBY => {
            let advertisers = crate::bluetooth::nearby_advertisers(env, &intent)?;
            return Ok(Some(Event::Nearby(advertisers)));
        }
        ACL_CONNECTED | ACL_DISCONNECTED => {
            let Some(device) = intent.get_parcelable_extra(env, EXTRA_DEVICE)? else {
                return Ok(None);
            };
            // `getName` needs BLUETOOTH_CONNECT; treat a failure as not ours.
            let name = BluetoothDevice(device).get_name(env).unwrap_or_default();
            if !name.to_lowercase().starts_with("airpod") {
                env.exception_clear();
                return Ok(None);
            }
            match action.as_str() {
                ACL_CONNECTED => ACTION_AIRPODS_AVAILABLE,
                _ => ACTION_AIRPODS_GONE,
            }
            .to_owned()
        }
        ADAPTER_STATE_CHANGED => match intent.get_int_extra(env, EXTRA_ADAPTER_STATE, -1)? {
            STATE_ON => ACTION_BLUETOOTH_ON.to_owned(),
            _ => return Ok(None),
        },
        // Off-hook: answered or placed. Ringing alone doesn't take the audio.
        PHONE_STATE => {
            let state = intent.get_string_extra(env, EXTRA_STATE)?;
            tracing::info!(?state, "phone state");
            return Ok(match state.as_deref() {
                Some("OFFHOOK") => Some(Event::PhoneCall(true)),
                Some("IDLE") => Some(Event::PhoneCall(false)),
                _ => None,
            });
        }
        "" => return Ok(None),
        _ => action,
    };
    tracing::info!(action, "Service: intent");
    Ok(Some(Event::Intent(action)))
}
