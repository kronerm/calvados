use jni::{Env, JValue, jni_sig, jni_str, objects::JObject, refs::Reference};

use crate::jni::android::app::pending_intent::PendingIntent;
use crate::jni::android::bluetooth::le::scan_settings::ScanSettings;
use crate::jni::java::util::list::List;

/// `android.bluetooth.le.BluetoothLeScanner`.
pub struct BluetoothLeScanner<R>(pub(crate) R);

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>>
    BluetoothLeScanner<R>
{
    /// `startScan(filters, settings, callbackIntent)` — results are delivered
    /// to `callback_intent`. Returns 0 on success.
    pub fn start_scan<'f, 's, 'p>(
        &self,
        env: &mut Env<'env_scope>,
        filters: &List<impl Reference + AsRef<JObject<'f>>>,
        settings: &ScanSettings<impl Reference + AsRef<JObject<'s>>>,
        callback_intent: &PendingIntent<impl Reference + AsRef<JObject<'p>>>,
    ) -> anyhow::Result<i32> {
        Ok(env
            .call_method(
                &self.0,
                jni_str!("startScan"),
                jni_sig!(
                    "(Ljava/util/List;Landroid/bluetooth/le/ScanSettings;Landroid/app/PendingIntent;)I"
                ),
                &[
                    JValue::Object(filters.0.as_ref()),
                    JValue::Object(settings.0.as_ref()),
                    JValue::Object(callback_intent.as_obj()),
                ],
            )?
            .i()?)
    }

    /// `stopScan(callbackIntent)`.
    pub fn stop_scan<'p>(
        &self,
        env: &mut Env<'env_scope>,
        callback_intent: &PendingIntent<impl Reference + AsRef<JObject<'p>>>,
    ) -> anyhow::Result<()> {
        env.call_method(
            &self.0,
            jni_str!("stopScan"),
            jni_sig!("(Landroid/app/PendingIntent;)V"),
            &[JValue::Object(callback_intent.as_obj())],
        )?;
        Ok(())
    }
}
