use jni::{Env, jni_sig, jni_str, objects::JObject, refs::Reference};

use crate::jni::string;

/// `java.io.File`.
pub struct File<R>(pub R);

impl<'object_scope, R: Reference + AsRef<JObject<'object_scope>>> File<R> {
    /// `getAbsolutePath()`.
    pub fn get_absolute_path(&self, env: &mut Env) -> anyhow::Result<String> {
        let path = env.call_method(
            &self.0,
            jni_str!("getAbsolutePath"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?;
        string(env, path)?.ok_or_else(|| anyhow::anyhow!("getAbsolutePath() returned null"))
    }
}
