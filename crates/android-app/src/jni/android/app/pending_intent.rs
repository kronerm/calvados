use jni::{Env, JValue, jni_sig, jni_str, objects::JObject, refs::Reference, strings::JNIStr};

use crate::jni::android::content::context::Context;
use crate::jni::android::content::intent::Intent;

bitflags::bitflags! {
    /// `android.app.PendingIntent.FLAG_*`.
    #[derive(Debug, Clone, Copy)]
    pub struct Flags: i32 {
        const UPDATE_CURRENT = 0x0000_0004;
        /// The system adds extras.
        const MUTABLE = 0x0200_0000;
        const IMMUTABLE = 0x0400_0000;
    }
}

/// `android.app.PendingIntent`.
pub struct PendingIntent<R>(R);

impl<'env_scope> PendingIntent<JObject<'env_scope>> {
    /// `PendingIntent.getForegroundService(context, requestCode, intent, flags)`.
    pub fn get_foreground_service<'c, 'i>(
        env: &mut Env<'env_scope>,
        context: &Context<impl Reference + AsRef<JObject<'c>>>,
        request_code: i32,
        intent: &Intent<impl Reference + AsRef<JObject<'i>>>,
        flags: Flags,
    ) -> anyhow::Result<Self> {
        Self::factory(
            env,
            jni_str!("getForegroundService"),
            context,
            request_code,
            intent,
            flags,
        )
    }

    fn factory<'c, 'i>(
        env: &mut Env<'env_scope>,
        method: &'static JNIStr,
        context: &Context<impl Reference + AsRef<JObject<'c>>>,
        request_code: i32,
        intent: &Intent<impl Reference + AsRef<JObject<'i>>>,
        flags: Flags,
    ) -> anyhow::Result<Self> {
        let class = env.find_class(jni_str!("android/app/PendingIntent"))?;
        // Both factories share this signature.
        let sig = jni_sig!(
            "(Landroid/content/Context;ILandroid/content/Intent;I)Landroid/app/PendingIntent;"
        );
        let pending = env
            .call_static_method(
                &class,
                method,
                sig,
                &[
                    JValue::Object(context.as_obj()),
                    JValue::Int(request_code),
                    JValue::Object(intent.as_obj()),
                    JValue::Int(flags.bits()),
                ],
            )?
            .l()?;
        Ok(Self(pending))
    }
}

impl<'object_scope, R: Reference + AsRef<JObject<'object_scope>>> PendingIntent<R> {
    pub fn as_obj(&self) -> &JObject<'object_scope> {
        self.0.as_ref()
    }
}
