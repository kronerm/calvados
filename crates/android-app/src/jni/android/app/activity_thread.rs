use jni::{Env, jni_sig, jni_str, objects::JObject, refs::Reference};

use crate::jni::android::content::context::Context;

pub struct ActivityThread<R>(R);
impl<'env_scope> ActivityThread<JObject<'env_scope>> {
    pub fn current_activity_thread(env: &mut Env<'env_scope>) -> anyhow::Result<Self> {
        let class = env.load_class(jni_str!("android/app/ActivityThread"))?;
        let current_activity_thread = env
            .call_static_method(
                &class,
                jni_str!("currentActivityThread"),
                jni_sig!("()Landroid/app/ActivityThread;"),
                &[],
            )?
            .l()?;
        Ok(Self(current_activity_thread))
    }
}

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>> ActivityThread<R> {
    /// The application `Context` (`ActivityThread.getApplication()`).
    pub fn get_application(
        &self,
        env: &mut Env<'env_scope>,
    ) -> anyhow::Result<Context<JObject<'env_scope>>> {
        Ok(Context(
            env.call_method(
                &self.0,
                jni_str!("getApplication"),
                jni_sig!("()Landroid/app/Application;"),
                &[],
            )?
            .l()?,
        ))
    }
}
