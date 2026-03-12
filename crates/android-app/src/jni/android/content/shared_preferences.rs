use jni::{
    Env, JValue, jni_sig, jni_str,
    objects::{JObject, JString},
    refs::Reference,
};

/// `android.content.SharedPreferences`.
pub struct SharedPreferences<R>(pub(crate) R);

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>> SharedPreferences<R> {
    /// `getString(key, default)`. Returns the default when the key is absent.
    pub fn get_string(
        &self,
        env: &mut Env<'env_scope>,
        key: &str,
        default: &str,
    ) -> anyhow::Result<String> {
        let key = JString::new(env, key)?;
        let default = JString::new(env, default)?;
        let value = env
            .call_method(
                &self.0,
                jni_str!("getString"),
                jni_sig!("(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;"),
                &[JValue::Object(&key), JValue::Object(&default)],
            )?
            .l()?;
        if value.is_null() {
            return Ok(String::new());
        }
        let value = unsafe { JString::from_raw(env, value.as_raw()) };
        Ok(value.to_string())
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
