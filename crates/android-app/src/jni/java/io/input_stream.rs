use jni::{Env, JValue, jni_sig, jni_str, objects::JObject, refs::Reference};

pub struct InputStream<R>(pub(crate) R);
impl<'object_scope, R: Reference + AsRef<JObject<'object_scope>>> InputStream<R> {
    pub fn read(&self, env: &mut Env) -> anyhow::Result<Vec<u8>> {
        let buffer = env.new_byte_array(4096)?;

        let bytes_read = env
            .call_method(
                &self.0,
                jni_str!("read"),
                jni_sig!("([B)I"),
                &[JValue::Object(&buffer)],
            )?
            .i()?;

        match bytes_read {
            ..=-1 => anyhow::bail!("stream closed"),
            0 => Ok(vec![]),
            1.. => {
                let mut data = vec![0i8; bytes_read as usize];
                buffer.get_region(env, 0, data.as_mut_slice())?;

                Ok(data.into_iter().map(|b| b as u8).collect())
            }
        }
    }
}
