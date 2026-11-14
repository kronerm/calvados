use jni::{
    Env, JValue, jni_sig, jni_str,
    objects::{JObject, JString},
    refs::Reference,
};

use crate::jni::string;

/// `android.os.Bundle`.
pub struct Bundle<R>(pub(crate) R);

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>> Bundle<R> {
    /// `getCharSequence(key)` as text; `None` when absent.
    pub fn get_char_sequence(
        &self,
        env: &mut Env<'env_scope>,
        key: &str,
    ) -> anyhow::Result<Option<String>> {
        let key = JString::new(env, key)?;
        let value = env
            .call_method(
                &self.0,
                jni_str!("getCharSequence"),
                jni_sig!("(Ljava/lang/String;)Ljava/lang/CharSequence;"),
                &[JValue::Object(&key)],
            )?
            .l()?;
        if value.is_null() {
            return Ok(None);
        }
        let text = env.call_method(
            &value,
            jni_str!("toString"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?;
        string(env, text)
    }
}
