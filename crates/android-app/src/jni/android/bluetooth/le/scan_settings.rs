use jni::{Env, JValue, jni_sig, jni_str, objects::JObject, refs::Reference};
use num_enum::IntoPrimitive;

/// `ScanSettings.SCAN_MODE_*`.
#[derive(Debug, Clone, Copy, IntoPrimitive)]
#[repr(i32)]
pub enum ScanMode {
    LowPower = 0,
}

bitflags::bitflags! {
    /// `ScanSettings.CALLBACK_TYPE_*`; the default is every match.
    #[derive(Debug, Clone, Copy)]
    pub struct CallbackType: i32 {
        /// One result when a device is first found, filtered in the
        /// controller.
        const FIRST_MATCH = 2;
    }
}

/// `android.bluetooth.le.ScanSettings`.
pub struct ScanSettings<R>(pub(crate) R);

impl<'env_scope> ScanSettings<JObject<'env_scope>> {
    pub fn builder(env: &mut Env<'env_scope>) -> anyhow::Result<Builder<JObject<'env_scope>>> {
        let class = env.find_class(jni_str!("android/bluetooth/le/ScanSettings$Builder"))?;
        Ok(Builder(env.new_object(&class, jni_sig!("()V"), &[])?))
    }
}

/// `android.bluetooth.le.ScanSettings.Builder`.
pub struct Builder<R>(R);

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>> Builder<R> {
    pub fn set_scan_mode(self, env: &mut Env<'env_scope>, mode: ScanMode) -> anyhow::Result<Self> {
        env.call_method(
            &self.0,
            jni_str!("setScanMode"),
            jni_sig!("(I)Landroid/bluetooth/le/ScanSettings$Builder;"),
            &[JValue::Int(mode.into())],
        )?;
        Ok(self)
    }

    pub fn set_callback_type(
        self,
        env: &mut Env<'env_scope>,
        kind: CallbackType,
    ) -> anyhow::Result<Self> {
        env.call_method(
            &self.0,
            jni_str!("setCallbackType"),
            jni_sig!("(I)Landroid/bluetooth/le/ScanSettings$Builder;"),
            &[JValue::Int(kind.bits())],
        )?;
        Ok(self)
    }

    pub fn build(
        self,
        env: &mut Env<'env_scope>,
    ) -> anyhow::Result<ScanSettings<JObject<'env_scope>>> {
        Ok(ScanSettings(
            env.call_method(
                &self.0,
                jni_str!("build"),
                jni_sig!("()Landroid/bluetooth/le/ScanSettings;"),
                &[],
            )?
            .l()?,
        ))
    }
}
