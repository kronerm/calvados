use jni::{
    Env, JValue, jni_sig, jni_str,
    objects::{JObject, JString},
    refs::Reference,
};
use num_enum::IntoPrimitive;

use crate::jni::android::widget::remote_views::RemoteViews;

pub struct Notification<R>(pub R);
impl<'env_scope> Notification<JObject<'env_scope>> {
    pub fn builder(
        env: &mut Env<'env_scope>,
        context: impl AsRef<JObject<'env_scope>>,
        channel_id: &str,
    ) -> anyhow::Result<Builder<JObject<'env_scope>>> {
        let channel_id = JString::new(env, channel_id)?;
        let builder_class = env.find_class(jni_str!("android/app/Notification$Builder"))?;

        let builder = env.new_object(
            &builder_class,
            jni_sig!("(Landroid/content/Context;Ljava/lang/String;)V"),
            &[
                JValue::Object(context.as_ref()),
                JValue::Object(&channel_id),
            ],
        )?;

        Ok(Builder(builder))
    }
}

pub struct Builder<R>(R);
impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>> Builder<R> {
    pub fn set_content_title(self, env: &mut Env<'env_scope>, title: &str) -> anyhow::Result<Self> {
        let title = JString::new(env, title)?;
        env.call_method(
            &self.0,
            jni_str!("setContentTitle"),
            jni_sig!("(Ljava/lang/CharSequence;)Landroid/app/Notification$Builder;"),
            &[JValue::Object(&title)],
        )?;
        Ok(self)
    }
    pub fn set_content_text(self, env: &mut Env<'env_scope>, text: &str) -> anyhow::Result<Self> {
        let text = JString::new(env, text)?;
        env.call_method(
            &self.0,
            jni_str!("setContentText"),
            jni_sig!("(Ljava/lang/CharSequence;)Landroid/app/Notification$Builder;"),
            &[JValue::Object(&text)],
        )?;
        Ok(self)
    }
    pub fn set_sub_text(self, env: &mut Env<'env_scope>, text: &str) -> anyhow::Result<Self> {
        let text = JString::new(env, text)?;
        env.call_method(
            &self.0,
            jni_str!("setSubText"),
            jni_sig!("(Ljava/lang/CharSequence;)Landroid/app/Notification$Builder;"),
            &[JValue::Object(&text)],
        )?;
        Ok(self)
    }
    pub fn set_small_icon(self, env: &mut Env<'env_scope>, res_id: i32) -> anyhow::Result<Self> {
        env.call_method(
            &self.0,
            jni_str!("setSmallIcon"),
            jni_sig!("(I)Landroid/app/Notification$Builder;"),
            &[JValue::Int(res_id)],
        )?;
        Ok(self)
    }
    pub fn set_show_when(self, env: &mut Env<'env_scope>, show: bool) -> anyhow::Result<Self> {
        env.call_method(
            &self.0,
            jni_str!("setShowWhen"),
            jni_sig!("(Z)Landroid/app/Notification$Builder;"),
            &[JValue::Bool(show)],
        )?;
        Ok(self)
    }
    pub fn set_foreground_service_behavior(
        self,
        env: &mut Env<'env_scope>,
        behavior: i32,
    ) -> anyhow::Result<Self> {
        env.call_method(
            &self.0,
            jni_str!("setForegroundServiceBehavior"),
            jni_sig!("(I)Landroid/app/Notification$Builder;"),
            &[JValue::Int(behavior)],
        )?;
        Ok(self)
    }
    pub fn set_ongoing(self, env: &mut Env<'env_scope>, ongoing: bool) -> anyhow::Result<Self> {
        env.call_method(
            &self.0,
            jni_str!("setOngoing"),
            jni_sig!("(Z)Landroid/app/Notification$Builder;"),
            &[JValue::Bool(ongoing)],
        )?;
        Ok(self)
    }
    pub fn set_style<'s>(
        self,
        env: &mut Env<'env_scope>,
        style: &DecoratedCustomViewStyle<impl Reference + AsRef<JObject<'s>>>,
    ) -> anyhow::Result<Self> {
        env.call_method(
            &self.0,
            jni_str!("setStyle"),
            jni_sig!("(Landroid/app/Notification$Style;)Landroid/app/Notification$Builder;"),
            &[JValue::Object(style.0.as_ref())],
        )?;
        Ok(self)
    }
    pub fn set_custom_content_view<'v>(
        self,
        env: &mut Env<'env_scope>,
        views: &RemoteViews<impl Reference + AsRef<JObject<'v>>>,
    ) -> anyhow::Result<Self> {
        env.call_method(
            &self.0,
            jni_str!("setCustomContentView"),
            jni_sig!("(Landroid/widget/RemoteViews;)Landroid/app/Notification$Builder;"),
            &[JValue::Object(views.as_obj())],
        )?;
        Ok(self)
    }
    pub fn set_custom_big_content_view<'v>(
        self,
        env: &mut Env<'env_scope>,
        views: &RemoteViews<impl Reference + AsRef<JObject<'v>>>,
    ) -> anyhow::Result<Self> {
        env.call_method(
            &self.0,
            jni_str!("setCustomBigContentView"),
            jni_sig!("(Landroid/widget/RemoteViews;)Landroid/app/Notification$Builder;"),
            &[JValue::Object(views.as_obj())],
        )?;
        Ok(self)
    }
}
impl<'env_scope> Builder<JObject<'env_scope>> {
    pub fn build(
        self,
        env: &mut Env<'env_scope>,
    ) -> anyhow::Result<Notification<JObject<'env_scope>>> {
        let notification = env
            .call_method(
                &self.0,
                jni_str!("build"),
                jni_sig!("()Landroid/app/Notification;"),
                &[],
            )?
            .l()?;

        Ok(Notification(notification))
    }
}

/// `android.app.Notification.DecoratedCustomViewStyle`: custom content views
/// inside the system's header and frame.
pub struct DecoratedCustomViewStyle<R>(R);
impl<'env_scope> DecoratedCustomViewStyle<JObject<'env_scope>> {
    pub fn new(env: &mut Env<'env_scope>) -> anyhow::Result<Self> {
        let class = env.find_class(jni_str!(
            "android/app/Notification$DecoratedCustomViewStyle"
        ))?;
        Ok(Self(env.new_object(&class, jni_sig!("()V"), &[])?))
    }
}

/// `android.app.NotificationManager`. Obtain via
/// [`Context::notification_manager`](crate::jni::android::content::context::Context::notification_manager).
pub struct NotificationManager<R>(pub(crate) R);
impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>>
    NotificationManager<R>
{
    pub fn create_notification_channel<'c>(
        &self,
        env: &mut Env<'env_scope>,
        channel: &NotificationChannel<impl Reference + AsRef<JObject<'c>>>,
    ) -> anyhow::Result<()> {
        env.call_method(
            &self.0,
            jni_str!("createNotificationChannel"),
            jni_sig!("(Landroid/app/NotificationChannel;)V"),
            &[JValue::Object(channel.0.as_ref())],
        )?;
        Ok(())
    }

    /// `notify(id, notification)` — post or update a notification.
    pub fn notify<'n>(
        &self,
        env: &mut Env<'env_scope>,
        id: i32,
        notification: &Notification<impl Reference + AsRef<JObject<'n>>>,
    ) -> anyhow::Result<()> {
        env.call_method(
            &self.0,
            jni_str!("notify"),
            jni_sig!("(ILandroid/app/Notification;)V"),
            &[JValue::Int(id), JValue::Object(notification.0.as_ref())],
        )?;
        Ok(())
    }
}

/// `NotificationManager.IMPORTANCE_*`.
#[derive(Debug, Clone, Copy, IntoPrimitive)]
#[repr(i32)]
pub enum Importance {
    Low = 2,
}

/// `android.app.NotificationChannel`.
pub struct NotificationChannel<R>(R);

impl<'env_scope> NotificationChannel<JObject<'env_scope>> {
    pub fn new(
        env: &mut Env<'env_scope>,
        id: &str,
        name: &str,
        importance: Importance,
    ) -> anyhow::Result<Self> {
        let id = JString::new(env, id)?;
        let name = JString::new(env, name)?;
        let class = env.find_class(jni_str!("android/app/NotificationChannel"))?;
        Ok(Self(env.new_object(
            &class,
            jni_sig!("(Ljava/lang/String;Ljava/lang/CharSequence;I)V"),
            &[
                JValue::Object(&id),
                JValue::Object(&name),
                JValue::Int(importance.into()),
            ],
        )?))
    }
}

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>>
    NotificationChannel<R>
{
    pub fn set_show_badge(self, env: &mut Env<'env_scope>, show: bool) -> anyhow::Result<Self> {
        env.call_method(
            &self.0,
            jni_str!("setShowBadge"),
            jni_sig!("(Z)V"),
            &[JValue::Bool(show)],
        )?;
        Ok(self)
    }
}
