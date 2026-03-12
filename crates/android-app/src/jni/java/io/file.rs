use jni::{Env, jni_sig, jni_str, objects::JObject, objects::JString, refs::Reference};

/// `java.io.File`.
pub struct File<R>(pub R);

impl<'object_scope, R: Reference + AsRef<JObject<'object_scope>>> File<R> {
    /// `getAbsolutePath()`.
    pub fn get_absolute_path(&self, env: &mut Env) -> anyhow::Result<String> {
        let path = env
            .call_method(
                &self.0,
                jni_str!("getAbsolutePath"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        // SAFETY: the method returns a `String`.
        let path = unsafe { JString::from_raw(env, path.into_raw()) };
        Ok(path.try_to_string(env)?)
    }
}
