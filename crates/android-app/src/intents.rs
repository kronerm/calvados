//! Intents in and out of `PersistentService`. All are explicit, so actions
//! need no namespace.

use jni::Env;
use jni::objects::JObject;

use crate::jni::android::app::pending_intent::{Flags, PendingIntent};
use crate::jni::android::app::remote_input::RemoteInput;
use crate::jni::android::bluetooth::device::BluetoothDevice;
use crate::jni::android::content::context::Context;
use crate::jni::android::content::intent::Intent;
use crate::notification::Page;
use crate::proto::aacp::NoiseControlMode;
use crate::proto::connection::is_airpods_name;
use crate::proto::display;
use crate::proto::pod_settings::PodSetting;
use crate::proto::settings::Setting;
use crate::service::Event;

/// [`Request::Noise`]: `NOISE:` and the mode's name
/// ([`display::mode_name`]).
const ACTION_NOISE_PREFIX: &str = "NOISE:";

/// [`Request::Cycle`]: `CYCLE:` and the mode's name.
const ACTION_CYCLE_PREFIX: &str = "CYCLE:";

/// [`Request::Pod`]: `POD:` and the setting's name ([`PodSetting::name`]).
const ACTION_POD_PREFIX: &str = "POD:";

/// [`Request::Page`] of [`Page::Main`].
const ACTION_PAGE_MAIN: &str = "PAGE:main";

/// [`Request::Page`] of [`Page::Pods`].
const ACTION_PAGE_PODS: &str = "PAGE:pods";

/// [`Request::Chime`].
const ACTION_CHIME: &str = "CHIME";

/// [`Request::Rename`], with the name as the [`RENAME_KEY`] remote input.
const ACTION_RENAME: &str = "RENAME";

/// The rename action's `RemoteInput` key.
pub(crate) const RENAME_KEY: &str = "name";

/// [`Request::Toggle`] of [`Setting::AutoSwitch`].
const ACTION_AUTO_SWITCH: &str = "AUTO_SWITCH";

/// [`Request::Toggle`] of [`Setting::AutoConnect`].
const ACTION_AUTO_CONNECT: &str = "AUTO_CONNECT";

/// [`Request::StartUpdate`].
const ACTION_UPDATE_FIRMWARE: &str = "UPDATE_FIRMWARE";

/// [`Request::CancelUpdate`].
const ACTION_CANCEL_UPDATE: &str = "CANCEL_UPDATE";

/// BLE scan results (see [`crate::bluetooth::set_presence_scan`]).
pub(crate) const ACTION_NEARBY: &str = "NEARBY";

const ACL_CONNECTED: &str = "android.bluetooth.device.action.ACL_CONNECTED";

const ACL_DISCONNECTED: &str = "android.bluetooth.device.action.ACL_DISCONNECTED";

/// `BluetoothAdapter.ACTION_STATE_CHANGED`, with [`EXTRA_ADAPTER_STATE`].
const ADAPTER_STATE_CHANGED: &str = "android.bluetooth.adapter.action.STATE_CHANGED";

/// `BluetoothAdapter.EXTRA_STATE`; [`STATE_ON`] once on.
const EXTRA_ADAPTER_STATE: &str = "android.bluetooth.adapter.extra.STATE";

/// `BluetoothAdapter.STATE_ON`.
const STATE_ON: i32 = 12;

/// `TelephonyManager.ACTION_PHONE_STATE_CHANGED`, with [`EXTRA_STATE`].
const PHONE_STATE: &str = "android.intent.action.PHONE_STATE";

/// `TelephonyManager.EXTRA_STATE`: `IDLE`, `RINGING` or `OFFHOOK`.
const EXTRA_STATE: &str = "state";

/// `BluetoothDevice.EXTRA_DEVICE`.
const EXTRA_DEVICE: &str = "android.bluetooth.device.extra.DEVICE";

/// What the notification's buttons ask of the service, an intent action each.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Request {
    /// Set the noise control mode.
    Noise(NoiseControlMode),
    /// Add a mode to the press-and-hold cycle or remove it.
    Cycle(NoiseControlMode),
    Toggle(Setting),
    /// Update the pods to the catalog's release.
    StartUpdate,
    /// Withdraw this phone's firmware update.
    CancelUpdate,
    /// Step one of the pods' settings to its next value.
    Pod(PodSetting),
    /// Show a page of the expanded notification.
    Page(Page),
    /// Rename the pods; the name comes as a remote input, so it arrives as
    /// [`Event::Rename`].
    Rename,
    /// Chime on the pods to find them.
    Chime,
}

impl Request {
    pub(crate) fn action(self) -> String {
        match self {
            Self::Noise(mode) => format!("{ACTION_NOISE_PREFIX}{}", display::mode_name(Some(mode))),
            Self::Cycle(mode) => format!("{ACTION_CYCLE_PREFIX}{}", display::mode_name(Some(mode))),
            Self::Toggle(Setting::AutoSwitch) => ACTION_AUTO_SWITCH.to_owned(),
            Self::Toggle(Setting::AutoConnect) => ACTION_AUTO_CONNECT.to_owned(),
            Self::StartUpdate => ACTION_UPDATE_FIRMWARE.to_owned(),
            Self::CancelUpdate => ACTION_CANCEL_UPDATE.to_owned(),
            Self::Pod(setting) => format!("{ACTION_POD_PREFIX}{}", setting.name()),
            Self::Page(Page::Main) => ACTION_PAGE_MAIN.to_owned(),
            Self::Page(Page::Pods) => ACTION_PAGE_PODS.to_owned(),
            Self::Rename => ACTION_RENAME.to_owned(),
            Self::Chime => ACTION_CHIME.to_owned(),
        }
    }

    /// A `PendingIntent` sending this to the service, one per
    /// `request_code`; immutable but for [`Self::Rename`]'s, which the
    /// system adds the typed name to.
    pub(crate) fn pending_intent<'local>(
        self,
        env: &mut Env<'local>,
        context: &Context<impl jni::refs::Reference + AsRef<JObject<'local>>>,
        request_code: i32,
    ) -> anyhow::Result<PendingIntent<JObject<'local>>> {
        let intent = service_intent(env, context, &self.action())?;
        let flags = match self {
            Self::Rename => Flags::MUTABLE,
            _ => Flags::IMMUTABLE,
        } | Flags::UPDATE_CURRENT;
        PendingIntent::get_foreground_service(env, context, request_code, &intent, flags)
    }

    fn parse(action: &str) -> Option<Self> {
        let mode = |prefix| action.strip_prefix(prefix).and_then(display::parse_mode);
        match action {
            ACTION_AUTO_SWITCH => Some(Self::Toggle(Setting::AutoSwitch)),
            ACTION_AUTO_CONNECT => Some(Self::Toggle(Setting::AutoConnect)),
            ACTION_UPDATE_FIRMWARE => Some(Self::StartUpdate),
            ACTION_CANCEL_UPDATE => Some(Self::CancelUpdate),
            ACTION_PAGE_MAIN => Some(Self::Page(Page::Main)),
            ACTION_PAGE_PODS => Some(Self::Page(Page::Pods)),
            ACTION_RENAME => Some(Self::Rename),
            ACTION_CHIME => Some(Self::Chime),
            _ => (mode(ACTION_NOISE_PREFIX).map(Self::Noise))
                .or_else(|| mode(ACTION_CYCLE_PREFIX).map(Self::Cycle))
                .or_else(|| {
                    action
                        .strip_prefix(ACTION_POD_PREFIX)
                        .and_then(PodSetting::from_name)
                        .map(Self::Pod)
                }),
        }
    }
}

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

/// The loop event for an incoming intent (service start or broadcast), if
/// it matters.
pub(crate) fn parse<'local>(
    env: &mut Env<'local>,
    intent: Intent<JObject<'local>>,
) -> anyhow::Result<Option<Event>> {
    if intent.0.is_null() {
        return Ok(None);
    }
    let Some(action) = intent.get_action(env)? else {
        return Ok(None);
    };
    let event = match action.as_str() {
        ACTION_NEARBY => {
            let advertisers = crate::bluetooth::nearby_advertisers(env, &intent)?;
            return Ok(Some(Event::Nearby(advertisers)));
        }
        ACL_CONNECTED | ACL_DISCONNECTED => {
            let Some(device) = intent.get_parcelable_extra(env, EXTRA_DEVICE)? else {
                return Ok(None);
            };
            // `getName` needs BLUETOOTH_CONNECT; treat a failure as not ours.
            let name = BluetoothDevice(device).get_name(env).ok().flatten();
            if !name.is_some_and(|name| is_airpods_name(&name)) {
                env.exception_clear();
                return Ok(None);
            }
            match action.as_str() {
                ACL_CONNECTED => Event::AclUp,
                _ => Event::AclDown,
            }
        }
        ADAPTER_STATE_CHANGED => match intent.get_int_extra(env, EXTRA_ADAPTER_STATE, -1)? {
            STATE_ON => Event::BluetoothOn,
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
        _ => match Request::parse(&action) {
            // Empty without text; the notification is posted again either
            // way, ending the reply's spinner.
            Some(Request::Rename) => {
                let name = match RemoteInput::get_results_from_intent(env, &intent)? {
                    Some(results) => results.get_char_sequence(env, RENAME_KEY)?,
                    None => None,
                };
                Event::Rename(name.unwrap_or_default())
            }
            Some(request) => Event::Request(request),
            None => {
                tracing::warn!(action, "unknown action");
                return Ok(None);
            }
        },
    };
    tracing::info!(?event, "Service: intent");
    Ok(Some(event))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_survive_their_action() {
        let requests = [
            Request::Noise(NoiseControlMode::Adaptive),
            Request::Cycle(NoiseControlMode::Transparency),
            Request::Toggle(Setting::AutoSwitch),
            Request::Toggle(Setting::AutoConnect),
            Request::StartUpdate,
            Request::CancelUpdate,
            Request::Pod(PodSetting::ConversationAwareness),
            Request::Pod(PodSetting::VolumeSwipeLength),
            Request::Page(Page::Main),
            Request::Page(Page::Pods),
            Request::Rename,
            Request::Chime,
        ];
        for request in requests {
            let action = request.action();
            let parsed = Request::parse(&action).map(Request::action);
            assert_eq!(parsed.as_ref(), Some(&action));
        }
        assert_eq!(
            Request::Noise(NoiseControlMode::NoiseCancellation).action(),
            "NOISE:anc"
        );
        assert!(Request::parse("NOISE:loud").is_none());
        assert!(matches!(
            Request::parse("POD:personalized-volume"),
            Some(Request::Pod(PodSetting::PersonalizedVolume))
        ));
        assert!(Request::parse("POD:loudness").is_none());
        assert!(matches!(
            Request::parse("CYCLE:adaptive"),
            Some(Request::Cycle(NoiseControlMode::Adaptive))
        ));
    }
}
