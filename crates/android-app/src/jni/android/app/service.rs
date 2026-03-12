use jni::{Env, JValue, jni_sig, jni_str, objects::JObject, refs::Reference};

use crate::jni::android::app::notification::Notification;
use crate::jni::android::content::context::Context;

/// `android.app.Service`.
pub struct Service<R>(pub R);

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>> Service<R> {
    /// A service is a `Context`.
    pub fn as_context(&self) -> Context<&JObject<'object_scope>> {
        Context(self.0.as_ref())
    }

    /// `startForeground(id, notification)`.
    pub fn start_foreground<'n>(
        &self,
        env: &mut Env<'env_scope>,
        id: i32,
        notification: &Notification<impl Reference + AsRef<JObject<'n>>>,
    ) -> anyhow::Result<()> {
        env.call_method(
            &self.0,
            jni_str!("startForeground"),
            jni_sig!("(ILandroid/app/Notification;)V"),
            &[JValue::Int(id), JValue::Object(notification.0.as_ref())],
        )?;
        Ok(())
    }
}
