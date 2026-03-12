use jni::{Env, jni_sig, jni_str, objects::JObject, refs::Reference};

use crate::jni::android::bluetooth::device::BluetoothDevice;
use crate::jni::android::bluetooth::le::scan_record::ScanRecord;

/// `android.bluetooth.le.ScanResult`.
pub struct ScanResult<R>(pub(crate) R);

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>> ScanResult<R> {
    pub fn get_device(
        &self,
        env: &mut Env<'env_scope>,
    ) -> anyhow::Result<BluetoothDevice<JObject<'env_scope>>> {
        Ok(BluetoothDevice(
            env.call_method(
                &self.0,
                jni_str!("getDevice"),
                jni_sig!("()Landroid/bluetooth/BluetoothDevice;"),
                &[],
            )?
            .l()?,
        ))
    }

    pub fn get_scan_record(
        &self,
        env: &mut Env<'env_scope>,
    ) -> anyhow::Result<ScanRecord<JObject<'env_scope>>> {
        Ok(ScanRecord(
            env.call_method(
                &self.0,
                jni_str!("getScanRecord"),
                jni_sig!("()Landroid/bluetooth/le/ScanRecord;"),
                &[],
            )?
            .l()?,
        ))
    }
}
