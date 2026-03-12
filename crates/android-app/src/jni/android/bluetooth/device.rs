use jni::{Env, jni_sig, jni_str, objects::JObject, refs::Reference};

pub struct BluetoothDevice<R>(pub(crate) R);
impl<'object_scope, R: Reference + AsRef<JObject<'object_scope>>> BluetoothDevice<R> {
    pub fn get_name(&self, env: &mut Env) -> anyhow::Result<String> {
        let name_obj = env
            .call_method(
                &self.0,
                jni_str!("getName"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;

        // SAFETY: getName() returns a String, which is a JString
        let jstr = unsafe { jni::objects::JString::from_raw(env, name_obj.as_raw()) };

        Ok(jstr.to_string())
    }

    pub fn get_address(&self, env: &mut Env) -> anyhow::Result<String> {
        let address_obj = env
            .call_method(
                &self.0,
                jni_str!("getAddress"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;

        // SAFETY: getAddress() returns a String, which is a JString
        let jstr = unsafe { jni::objects::JString::from_raw(env, address_obj.as_raw()) };

        Ok(jstr.to_string())
    }
}
