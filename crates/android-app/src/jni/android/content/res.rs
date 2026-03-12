use jni::{
    Env, JValue, jni_sig, jni_str,
    objects::{JObject, JString},
    refs::Reference,
};

/// `android.content.res.Resources`. Obtain via
/// [`Context::get_resources`](crate::jni::android::content::context::Context::get_resources).
pub struct Resources<R>(pub(crate) R);

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>> Resources<R> {
    /// `getIdentifier(name, type, package)`: a resource's id by name (there's
    /// no generated `R` class); 0 if there's none.
    pub fn get_identifier(
        &self,
        env: &mut Env<'env_scope>,
        name: &str,
        kind: &str,
        package: &str,
    ) -> anyhow::Result<i32> {
        let name = JString::new(env, name)?;
        let kind = JString::new(env, kind)?;
        let package = JString::new(env, package)?;
        Ok(env
            .call_method(
                &self.0,
                jni_str!("getIdentifier"),
                jni_sig!("(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)I"),
                &[
                    JValue::Object(&name),
                    JValue::Object(&kind),
                    JValue::Object(&package),
                ],
            )?
            .i()?)
    }
}
