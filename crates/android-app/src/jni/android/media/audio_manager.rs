use jni::{
    Env, JValue, jni_sig, jni_str,
    objects::{JObject, JString},
    refs::Reference,
};

use crate::jni::android::view::key_event::KeyEvent;

/// `android.media.AudioManager`.
pub struct AudioManager<R>(pub(crate) R);

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>> AudioManager<R> {
    /// `dispatchMediaKeyEvent(KeyEvent)` — inject a media button press into the
    /// active media session.
    pub fn dispatch_media_key_event<'k>(
        &self,
        env: &mut Env<'env_scope>,
        key_event: &KeyEvent<impl Reference + AsRef<JObject<'k>>>,
    ) -> anyhow::Result<()> {
        env.call_method(
            &self.0,
            jni_str!("dispatchMediaKeyEvent"),
            jni_sig!("(Landroid/view/KeyEvent;)V"),
            &[JValue::Object(key_event.as_obj())],
        )?;
        Ok(())
    }

    /// `setParameters("key=value;…")`, passed on to the audio HAL.
    pub fn set_parameters(&self, env: &mut Env<'env_scope>, pairs: &str) -> anyhow::Result<()> {
        let pairs = JString::new(env, pairs)?;
        env.call_method(
            &self.0,
            jni_str!("setParameters"),
            jni_sig!("(Ljava/lang/String;)V"),
            &[JValue::Object(&pairs)],
        )?;
        Ok(())
    }
}
