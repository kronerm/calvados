use jni::{
    Env, JValue, jni_sig, jni_str,
    objects::{JObject, JString},
    refs::Reference,
};

use crate::jni::string;

/// `android.content.SharedPreferences`.
pub struct SharedPreferences<R>(pub(crate) R);

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>> SharedPreferences<R> {
    /// `getString(key, null)`; `None` when the key is absent.
    pub fn get_string(
        &self,
        env: &mut Env<'env_scope>,
        key: &str,
    ) -> anyhow::Result<Option<String>> {
        let key = JString::new(env, key)?;
        let value = env.call_method(
            &self.0,
            jni_str!("getString"),
            jni_sig!("(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;"),
            &[JValue::Object(&key), JValue::Object(&JObject::null())],
        )?;
        string(env, value)
    }

    /// `getBoolean(key, default)`.
    pub fn get_boolean(
        &self,
        env: &mut Env<'env_scope>,
        key: &str,
        default: bool,
    ) -> anyhow::Result<bool> {
        let key = JString::new(env, key)?;
        Ok(env
            .call_method(
                &self.0,
                jni_str!("getBoolean"),
                jni_sig!("(Ljava/lang/String;Z)Z"),
                &[JValue::Object(&key), JValue::Bool(default)],
            )?
            .z()?)
    }

    pub fn edit(&self, env: &mut Env<'env_scope>) -> anyhow::Result<Editor<JObject<'env_scope>>> {
        Ok(Editor(
            env.call_method(
                &self.0,
                jni_str!("edit"),
                jni_sig!("()Landroid/content/SharedPreferences$Editor;"),
                &[],
            )?
            .l()?,
        ))
    }
}

/// `android.content.SharedPreferences.Editor`.
pub struct Editor<R>(R);

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>> Editor<R> {
    pub fn put_string(
        self,
        env: &mut Env<'env_scope>,
        key: &str,
        value: &str,
    ) -> anyhow::Result<Self> {
        let key = JString::new(env, key)?;
        let value = JString::new(env, value)?;
        env.call_method(
            &self.0,
            jni_str!("putString"),
            jni_sig!(
                "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;"
            ),
            &[JValue::Object(&key), JValue::Object(&value)],
        )?;
        Ok(self)
    }

    pub fn put_boolean(
        self,
        env: &mut Env<'env_scope>,
        key: &str,
        value: bool,
    ) -> anyhow::Result<Self> {
        let key = JString::new(env, key)?;
        env.call_method(
            &self.0,
            jni_str!("putBoolean"),
            jni_sig!("(Ljava/lang/String;Z)Landroid/content/SharedPreferences$Editor;"),
            &[JValue::Object(&key), JValue::Bool(value)],
        )?;
        Ok(self)
    }

    pub fn apply(self, env: &mut Env<'env_scope>) -> anyhow::Result<()> {
        env.call_method(&self.0, jni_str!("apply"), jni_sig!("()V"), &[])?;
        Ok(())
    }
}
