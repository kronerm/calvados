use jni::{Env, JValue, jni_sig, jni_str, objects::JObject, refs::Reference};

/// `android.bluetooth.le.ScanFilter`.
pub struct ScanFilter<R>(pub(crate) R);

impl<'env_scope> ScanFilter<JObject<'env_scope>> {
    pub fn builder(env: &mut Env<'env_scope>) -> anyhow::Result<Builder<JObject<'env_scope>>> {
        let class = env.find_class(jni_str!("android/bluetooth/le/ScanFilter$Builder"))?;
        Ok(Builder(env.new_object(&class, jni_sig!("()V"), &[])?))
    }
}

impl<'object_scope, R: Reference + AsRef<JObject<'object_scope>>> ScanFilter<R> {
    pub fn as_obj(&self) -> &JObject<'object_scope> {
        self.0.as_ref()
    }
}

/// `android.bluetooth.le.ScanFilter.Builder`.
pub struct Builder<R>(R);

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>> Builder<R> {
    /// `setManufacturerData(id, data, mask)` — match advertisements whose
    /// manufacturer data starts with `data` under `mask`.
    pub fn set_manufacturer_data(
        self,
        env: &mut Env<'env_scope>,
        manufacturer_id: i32,
        data: &[u8],
        mask: &[u8],
    ) -> anyhow::Result<Self> {
        let data = env.byte_array_from_slice(data)?;
        let mask = env.byte_array_from_slice(mask)?;
        env.call_method(
            &self.0,
            jni_str!("setManufacturerData"),
            jni_sig!("(I[B[B)Landroid/bluetooth/le/ScanFilter$Builder;"),
            &[
                JValue::Int(manufacturer_id),
                JValue::Object(&data),
                JValue::Object(&mask),
            ],
        )?;
        Ok(self)
    }

    pub fn build(
        self,
        env: &mut Env<'env_scope>,
    ) -> anyhow::Result<ScanFilter<JObject<'env_scope>>> {
        Ok(ScanFilter(
            env.call_method(
                &self.0,
                jni_str!("build"),
                jni_sig!("()Landroid/bluetooth/le/ScanFilter;"),
                &[],
            )?
            .l()?,
        ))
    }
}
