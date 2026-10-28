//! Android Bluetooth beyond the AACP link: paired devices, the local MAC
//! and A2DP routing.

use std::sync::Arc;

use anyhow::Context as _;
use jni::Env;
use jni::objects::JObject;
use jni::refs::Global;
use parking_lot::Mutex;

use crate::intents::{ACTION_NEARBY, service_intent};
use crate::jni::android::app::pending_intent::{Flags, PendingIntent};
use crate::jni::android::bluetooth::a2dp::{BluetoothA2dp, Profile};
use crate::jni::android::bluetooth::adapter::BluetoothAdapter;
use crate::jni::android::bluetooth::device::BluetoothDevice;
use crate::jni::android::bluetooth::le::scan_filter::ScanFilter;
use crate::jni::android::bluetooth::le::scan_result::ScanResult;
use crate::jni::android::bluetooth::le::scan_settings::{CallbackType, ScanMode, ScanSettings};
use crate::jni::android::content::intent::Intent;
use crate::jni::java::util::list::List;
use crate::jni::{app_context, with_env};
use crate::proto::MacAddress;
use crate::proto::aacp::KeyType;
use crate::proto::ble::{APPLE_COMPANY_ID, Pattern, PodsKeys};
use crate::proto::connection::is_airpods_name;
use crate::proto::settings::Setting;

const KEY_LOCAL_MAC: &str = "local_bt_mac";

/// `BluetoothLeScanner.EXTRA_LIST_SCAN_RESULT`.
const EXTRA_LIST_SCAN_RESULT: &str = "android.bluetooth.le.extra.LIST_SCAN_RESULT";

const NEARBY_REQUEST_CODE: i32 = 0x0B1E;

/// A remembered setting (preferences), on until first turned off.
pub(crate) fn load_setting(setting: Setting) -> bool {
    prefs::boolean(setting.key(), true)
        .inspect_err(|e| tracing::warn!(?setting, "reading the setting: {e:#}"))
        .unwrap_or(true)
}

pub(crate) fn save_setting(setting: Setting, enabled: bool) {
    if let Err(e) = prefs::put_boolean(setting.key(), enabled) {
        tracing::warn!(?setting, "remembering the setting: {e:#}");
    }
}

/// `AA:BB:…`, as `BluetoothDevice.getAddress()` has it.
pub(crate) fn address(mac: MacAddress) -> String {
    mac.to_hex_string().to_uppercase()
}

/// A key's preference: `identity_key:<MAC>` or `encryption_key:<MAC>` (hex).
fn key_preference(pods: MacAddress, kind: KeyType) -> String {
    let name = match kind {
        KeyType::Irk => "identity_key",
        KeyType::EncryptionKey => "encryption_key",
    };
    format!("{name}:{}", address(pods))
}

pub(crate) fn save_key(pods: MacAddress, kind: KeyType, key: [u8; 16]) -> anyhow::Result<()> {
    prefs::put_string(&key_preference(pods, kind), &hex::encode(key))
}

/// The paired pods and their stored keys, if the IRK is known.
pub(crate) fn stored_keys() -> anyhow::Result<Option<(MacAddress, PodsKeys)>> {
    let Some(pods) = paired_airpods()? else {
        return Ok(None);
    };
    let read = |kind| -> anyhow::Result<Option<[u8; 16]>> {
        Ok(prefs::string(&key_preference(pods, kind))?
            .and_then(|stored| hex::decode(stored).ok())
            .and_then(|key| key.try_into().ok()))
    };
    let Some(irk) = read(KeyType::Irk)? else {
        return Ok(None);
    };
    let encryption = read(KeyType::EncryptionKey)?;
    Ok(Some((pods, PodsKeys { irk, encryption })))
}

/// Start or stop watching for Apple proximity pairing advertisements
/// (`4c 00 07`) not from the closed case (pods charging in it advertise
/// too), filtered in the controller, one result per newly found device.
/// Results start the service as [`ACTION_NEARBY`] intents (see
/// [`nearby_advertisers`]); the system keeps scanning while the app is gone.
pub(crate) fn set_presence_scan(enabled: bool) -> anyhow::Result<()> {
    with_env(|env| {
        let context = app_context(env)?;
        let intent = service_intent(env, &context, ACTION_NEARBY)?;
        // Mutable: the system adds the results as extras.
        let flags = Flags::MUTABLE | Flags::UPDATE_CURRENT;
        let callback = PendingIntent::get_foreground_service(
            env,
            &context,
            NEARBY_REQUEST_CODE,
            &intent,
            flags,
        )?;
        let scanner = BluetoothAdapter::default_adapter(env)?.get_bluetooth_le_scanner(env)?;
        if !enabled {
            return scanner.stop_scan(env, &callback);
        }
        let pattern = Pattern::proximity().lid_open();
        let filter = ScanFilter::builder(env)?
            .set_manufacturer_data(
                env,
                i32::from(APPLE_COMPANY_ID),
                &pattern.data,
                &pattern.mask,
            )?
            .build(env)?;
        let filters = List::singleton(env, filter.as_obj())?;
        let settings = ScanSettings::builder(env)?
            .set_scan_mode(env, ScanMode::LowPower)?
            .set_callback_type(env, CallbackType::FIRST_MATCH)?
            .build(env)?;
        match scanner.start_scan(env, &filters, &settings, &callback)? {
            0 => Ok(()),
            code => anyhow::bail!("startScan failed ({code})"),
        }
    })
}

/// The advertisers in a [`ACTION_NEARBY`] intent: address, and Apple's
/// manufacturer data (from the type byte; empty if missing).
pub(crate) fn nearby_advertisers<'local>(
    env: &mut Env<'local>,
    intent: &Intent<JObject<'local>>,
) -> anyhow::Result<Vec<(MacAddress, Vec<u8>)>> {
    let Some(results) = intent.get_parcelable_array_list_extra(env, EXTRA_LIST_SCAN_RESULT)? else {
        return Ok(Vec::new());
    };
    (0..results.size(env)?)
        .map(|i| {
            let result = ScanResult(results.get(env, i)?);
            let address = result.get_device(env)?.get_address(env)?;
            let data = result
                .get_scan_record(env)?
                .get_manufacturer_specific_data(env, i32::from(APPLE_COMPANY_ID))?;
            let data = data.unwrap_or_default();
            tracing::debug!(
                address,
                data = hex::encode(&data),
                "Apple device advertising nearby"
            );
            Ok(address.parse().ok().map(|address| (address, data)))
        })
        .filter_map(Result::transpose)
        .collect()
}

/// MAC of the first bonded device named like AirPods ([`is_airpods_name`]).
pub(crate) fn paired_airpods() -> anyhow::Result<Option<MacAddress>> {
    with_env(|env| {
        let devices = BluetoothAdapter::default_adapter(env)?
            .get_bonded_devices(env)?
            .iterator(env)?;
        while devices.has_next(env)? {
            let device = BluetoothDevice(devices.next(env)?);
            if device
                .get_name(env)?
                .is_some_and(|name| is_airpods_name(&name))
            {
                return Ok(Some(device.get_address(env)?.parse()?));
            }
        }
        Ok(None)
    })
}

/// This phone's Bluetooth MAC, as learned from the AirPods and saved; apps
/// can't read it (`getAddress()` only answers privileged ones).
pub(crate) fn local_mac() -> anyhow::Result<Option<MacAddress>> {
    Ok(prefs::string(KEY_LOCAL_MAC)?.and_then(|saved| saved.parse().ok()))
}

pub(crate) fn save_local_mac(mac: MacAddress) -> anyhow::Result<()> {
    prefs::put_string(KEY_LOCAL_MAC, &mac.to_hex_string())
}

/// The app's preferences (`calvados_prefs`).
mod prefs {
    use jni::Env;
    use jni::objects::JObject;

    use crate::jni::android::content::shared_preferences::{Editor, SharedPreferences};
    use crate::jni::{app_context, with_env};

    fn open<'local>(env: &mut Env<'local>) -> anyhow::Result<SharedPreferences<JObject<'local>>> {
        app_context(env)?.get_shared_preferences(env, "calvados_prefs")
    }

    fn edit(
        put: impl for<'local> FnOnce(
            &mut Env<'local>,
            Editor<JObject<'local>>,
        ) -> anyhow::Result<Editor<JObject<'local>>>,
    ) -> anyhow::Result<()> {
        with_env(|env| {
            let editor = open(env)?.edit(env)?;
            put(env, editor)?.apply(env)
        })
    }

    pub(super) fn string(key: &str) -> anyhow::Result<Option<String>> {
        with_env(|env| open(env)?.get_string(env, key))
    }

    pub(super) fn boolean(key: &str, default: bool) -> anyhow::Result<bool> {
        with_env(|env| open(env)?.get_boolean(env, key, default))
    }

    pub(super) fn put_string(key: &str, value: &str) -> anyhow::Result<()> {
        edit(|env, editor| editor.put_string(env, key, value))
    }

    pub(super) fn put_boolean(key: &str, value: bool) -> anyhow::Result<()> {
        edit(|env, editor| editor.put_boolean(env, key, value))
    }
}

/// The running `PersistentService`, which is also the A2DP `ServiceListener`.
pub(crate) static SERVICE: Mutex<Option<Arc<Global<JObject<'static>>>>> = Mutex::new(None);

/// MAC to connect once the A2DP proxy arrives.
pub(crate) static PROXY_JOBS: Mutex<Vec<ProxyJob>> = Mutex::new(Vec::new());

/// Work waiting for the A2DP profile proxy, which arrives asynchronously.
#[derive(Debug)]
pub(crate) enum ProxyJob {
    /// Connect A2DP to this MAC.
    Connect(MacAddress),
    /// Report whether paired AirPods are connected right now.
    ReportConnected,
}

pub(crate) fn set_service(service: Option<Global<JObject<'static>>>) {
    *SERVICE.lock() = service.map(Arc::new);
}

/// Ask Android to connect A2DP to `mac`, so the OS actually routes audio after
/// an AACP takeover.
pub(crate) fn connect_a2dp(mac: MacAddress) -> anyhow::Result<()> {
    request_a2dp_proxy(ProxyJob::Connect(mac))
}

/// Find out whether the pods are already connected (e.g. when the service
/// starts after their connect broadcast). Answered by [`on_profile_proxy`].
pub(crate) fn check_airpods_connected() -> anyhow::Result<()> {
    request_a2dp_proxy(ProxyJob::ReportConnected)
}

fn request_a2dp_proxy(job: ProxyJob) -> anyhow::Result<()> {
    let service = SERVICE.lock().clone().context("service not running")?;
    PROXY_JOBS.lock().push(job);
    with_env(|env| {
        let context = app_context(env)?;
        let requested = BluetoothAdapter::default_adapter(env)?.get_profile_proxy(
            env,
            &context,
            &*service,
            Profile::A2dp,
        )?;
        anyhow::ensure!(requested, "getProfileProxy(A2DP) refused");
        Ok(())
    })
}

/// `ServiceListener.onServiceConnected`: run the queued jobs, release the
/// proxy. Returns whether a [`ProxyJob::ReportConnected`] found the pods.
pub(crate) fn on_profile_proxy(
    env: &mut Env,
    profile: i32,
    proxy: JObject,
) -> anyhow::Result<bool> {
    let Ok(Profile::A2dp) = Profile::try_from(profile) else {
        return Ok(false);
    };
    let jobs = std::mem::take(&mut *PROXY_JOBS.lock());
    let a2dp = BluetoothA2dp(proxy);
    let adapter = BluetoothAdapter::default_adapter(env)?;
    let connected = jobs
        .into_iter()
        .map(|job| -> anyhow::Result<bool> {
            match job {
                ProxyJob::Connect(mac) => {
                    let device = adapter.get_remote_device(env, &address(mac))?;
                    let connected = a2dp.connect(env, &device)?;
                    tracing::info!(%mac, connected, "A2DP connect");
                    Ok(false)
                }
                ProxyJob::ReportConnected => {
                    let devices = a2dp.get_connected_devices(env)?;
                    airpods_among(env, &devices)
                }
            }
        })
        .collect::<Vec<_>>();
    adapter.close_profile_proxy(env, Profile::A2dp, &a2dp.0)?;
    connected
        .into_iter()
        .try_fold(false, |any, found| Ok(any | found?))
}

/// Whether a `List<BluetoothDevice>` holds AirPods (by name, as elsewhere).
fn airpods_among<'local>(
    env: &mut Env<'local>,
    devices: &List<JObject<'local>>,
) -> anyhow::Result<bool> {
    (0..devices.size(env)?).try_fold(false, |found, i| {
        let name = BluetoothDevice(devices.get(env, i)?).get_name(env)?;
        Ok(found || name.is_some_and(|name| is_airpods_name(&name)))
    })
}
