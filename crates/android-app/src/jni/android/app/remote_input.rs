use jni::{
    Env, JValue, jni_sig, jni_str,
    objects::{JObject, JString},
    refs::Reference,
};

use crate::jni::android::content::intent::Intent;
use crate::jni::android::os::bundle::Bundle;

/// `android.app.RemoteInput`: text typed into a notification action.
pub struct RemoteInput<R>(pub(crate) R);

impl<'env_scope> RemoteInput<JObject<'env_scope>> {
    /// `new RemoteInput.Builder(key).setLabel(label).build()`.
    pub fn new(env: &mut Env<'env_scope>, key: &str, label: &str) -> anyhow::Result<Self> {
        let key = JString::new(env, key)?;
        let label = JString::new(env, label)?;
        let class = env.find_class(jni_str!("android/app/RemoteInput$Builder"))?;
        let builder = env.new_object(
            &class,
            jni_sig!("(Ljava/lang/String;)V"),
            &[JValue::Object(&key)],
        )?;
        env.call_method(
            &builder,
            jni_str!("setLabel"),
            jni_sig!("(Ljava/lang/CharSequence;)Landroid/app/RemoteInput$Builder;"),
            &[JValue::Object(&label)],
        )?;
        let input = env
            .call_method(
                &builder,
                jni_str!("build"),
                jni_sig!("()Landroid/app/RemoteInput;"),
                &[],
            )?
            .l()?;
        Ok(Self(input))
    }

    /// `RemoteInput.getResultsFromIntent(intent)`; `None` without any.
    pub fn get_results_from_intent<'i>(
        env: &mut Env<'env_scope>,
        intent: &Intent<impl Reference + AsRef<JObject<'i>>>,
    ) -> anyhow::Result<Option<Bundle<JObject<'env_scope>>>> {
        let class = env.find_class(jni_str!("android/app/RemoteInput"))?;
        let results = env
            .call_static_method(
                &class,
                jni_str!("getResultsFromIntent"),
                jni_sig!("(Landroid/content/Intent;)Landroid/os/Bundle;"),
                &[JValue::Object(intent.as_obj())],
            )?
            .l()?;
        Ok((!results.is_null()).then_some(Bundle(results)))
    }
}

impl<'object_scope, R: Reference + AsRef<JObject<'object_scope>>> RemoteInput<R> {
    pub fn as_obj(&self) -> &JObject<'object_scope> {
        self.0.as_ref()
    }
}
