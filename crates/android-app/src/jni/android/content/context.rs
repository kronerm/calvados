use jni::{
    Env, JValue, jni_sig, jni_str,
    objects::{JObject, JString},
    refs::Reference,
};

use crate::jni::android::app::notification::NotificationManager;
use crate::jni::android::content::res::Resources;
use crate::jni::android::content::shared_preferences::SharedPreferences;
use crate::jni::android::media::audio_manager::AudioManager;
use crate::jni::java::io::file::File;

/// `android.content.Context` — the entry point for system services,
/// preferences and the app class loader. Obtained from
/// [`ActivityThread::get_application`](crate::jni::android::app::activity_thread::ActivityThread::get_application).
pub struct Context<R>(pub R);

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>> Context<R> {
    /// The underlying `Context` object, for calls that take a `Context` arg.
    pub fn as_obj(&self) -> &JObject<'object_scope> {
        self.0.as_ref()
    }

    /// `getPackageName()`.
    pub fn get_package_name(&self, env: &mut Env<'env_scope>) -> anyhow::Result<String> {
        let name = env
            .call_method(
                &self.0,
                jni_str!("getPackageName"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        // SAFETY: the method returns a `String`.
        let name = unsafe { JString::from_raw(env, name.into_raw()) };
        Ok(name.try_to_string(env)?)
    }

    /// `getResources()`.
    pub fn get_resources(
        &self,
        env: &mut Env<'env_scope>,
    ) -> anyhow::Result<Resources<JObject<'env_scope>>> {
        Ok(Resources(
            env.call_method(
                &self.0,
                jni_str!("getResources"),
                jni_sig!("()Landroid/content/res/Resources;"),
                &[],
            )?
            .l()?,
        ))
    }

    /// `getFilesDir()`: the app's private files directory.
    pub fn get_files_dir(
        &self,
        env: &mut Env<'env_scope>,
    ) -> anyhow::Result<File<JObject<'env_scope>>> {
        Ok(File(
            env.call_method(
                &self.0,
                jni_str!("getFilesDir"),
                jni_sig!("()Ljava/io/File;"),
                &[],
            )?
            .l()?,
        ))
    }

    /// `getCacheDir()`: the app's private cache, which the system may clear.
    pub fn get_cache_dir(
        &self,
        env: &mut Env<'env_scope>,
    ) -> anyhow::Result<File<JObject<'env_scope>>> {
        Ok(File(
            env.call_method(
                &self.0,
                jni_str!("getCacheDir"),
                jni_sig!("()Ljava/io/File;"),
                &[],
            )?
            .l()?,
        ))
    }

    fn get_system_service(
        &self,
        env: &mut Env<'env_scope>,
        name: &str,
    ) -> anyhow::Result<JObject<'env_scope>> {
        let name = JString::new(env, name)?;
        Ok(env
            .call_method(
                &self.0,
                jni_str!("getSystemService"),
                jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                &[JValue::Object(&name)],
            )?
            .l()?)
    }

    pub fn audio_manager(
        &self,
        env: &mut Env<'env_scope>,
    ) -> anyhow::Result<AudioManager<JObject<'env_scope>>> {
        Ok(AudioManager(self.get_system_service(env, "audio")?))
    }

    pub fn notification_manager(
        &self,
        env: &mut Env<'env_scope>,
    ) -> anyhow::Result<NotificationManager<JObject<'env_scope>>> {
        Ok(NotificationManager(
            self.get_system_service(env, "notification")?,
        ))
    }

    pub fn get_shared_preferences(
        &self,
        env: &mut Env<'env_scope>,
        name: &str,
    ) -> anyhow::Result<SharedPreferences<JObject<'env_scope>>> {
        let name = JString::new(env, name)?;
        // MODE_PRIVATE = 0
        Ok(SharedPreferences(
            env.call_method(
                &self.0,
                jni_str!("getSharedPreferences"),
                jni_sig!("(Ljava/lang/String;I)Landroid/content/SharedPreferences;"),
                &[JValue::Object(&name), JValue::Int(0)],
            )?
            .l()?,
        ))
    }
}
