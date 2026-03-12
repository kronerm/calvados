use jni::{
    Env, JValue, jni_sig, jni_str,
    objects::{JByteArray, JObject},
    refs::Reference,
};

/// `android.bluetooth.le.ScanRecord`: a scan result's advertisement.
pub struct ScanRecord<R>(pub(crate) R);

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>> ScanRecord<R> {
    /// `getManufacturerSpecificData(id)`: the data after the company id, if
    /// the advertisement has it.
    pub fn get_manufacturer_specific_data(
        &self,
        env: &mut Env<'env_scope>,
        manufacturer_id: i32,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let data = env
            .call_method(
                &self.0,
                jni_str!("getManufacturerSpecificData"),
                jni_sig!("(I)[B"),
                &[JValue::Int(manufacturer_id)],
            )?
            .l()?;
        if data.is_null() {
            return Ok(None);
        }
        // SAFETY: the method returns a `byte[]`.
        let data = unsafe { JByteArray::from_raw(env, data.into_raw()) };
        Ok(Some(env.convert_byte_array(&data)?))
    }
}
