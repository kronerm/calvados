use jni::{Env, JValue, jni_sig, jni_str, objects::JObject, refs::Reference};
use num_enum::{IntoPrimitive, TryFromPrimitive};

use crate::jni::android::bluetooth::device::BluetoothDevice;
use crate::jni::java::util::list::List;

/// `BluetoothProfile.*`, those used here.
#[derive(Debug, Clone, Copy, IntoPrimitive, TryFromPrimitive)]
#[repr(i32)]
pub enum Profile {
    A2dp = 2,
}

/// `android.bluetooth.BluetoothA2dp` (a `BluetoothProfile` proxy).
pub struct BluetoothA2dp<R>(pub R);

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>> BluetoothA2dp<R> {
    /// `getConnectedDevices()` — devices with A2DP connected right now.
    pub fn get_connected_devices(
        &self,
        env: &mut Env<'env_scope>,
    ) -> anyhow::Result<List<JObject<'env_scope>>> {
        Ok(List(
            env.call_method(
                &self.0,
                jni_str!("getConnectedDevices"),
                jni_sig!("()Ljava/util/List;"),
                &[],
            )?
            .l()?,
        ))
    }

    /// Hidden `connect(device)`; reachable like LibrePods' reflection call.
    pub fn connect<'d>(
        &self,
        env: &mut Env<'env_scope>,
        device: &BluetoothDevice<impl Reference + AsRef<JObject<'d>>>,
    ) -> anyhow::Result<bool> {
        Ok(env
            .call_method(
                &self.0,
                jni_str!("connect"),
                jni_sig!("(Landroid/bluetooth/BluetoothDevice;)Z"),
                &[JValue::Object(device.0.as_ref())],
            )?
            .z()?)
    }
}
