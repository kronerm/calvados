use jni::{Env, JValue, jni_sig, jni_str, objects::JObject, refs::Reference};

pub struct OutputStream<R>(pub(crate) R);
impl<'object_scope, R: Reference + AsRef<JObject<'object_scope>>> OutputStream<R> {
    pub fn write(&self, env: &mut Env, data: &[u8]) -> anyhow::Result<()> {
        let byte_array = env.byte_array_from_slice(data)?;

        env.call_method(
            &self.0,
            jni_str!("write"),
            jni_sig!("([B)V"),
            &[JValue::Object(&byte_array)],
        )?;

        env.call_method(&self.0, jni_str!("flush"), jni_sig!("()V"), &[])?;

        Ok(())
    }
}
