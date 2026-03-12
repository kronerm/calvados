use jni::{
    Env, JValue, jni_sig, jni_str,
    objects::{JObject, JString},
    refs::Reference,
};
use num_enum::IntoPrimitive;

use crate::jni::android::app::pending_intent::PendingIntent;

/// `View.VISIBLE`, `View.GONE`…
#[derive(Debug, Clone, Copy, IntoPrimitive)]
#[repr(i32)]
pub enum Visibility {
    Visible = 0,
    Gone = 8,
}

/// `android.widget.RemoteViews`: a layout inflated by another process (the
/// notification shade), changed only through recorded setter calls.
pub struct RemoteViews<R>(pub(crate) R);

impl<'env_scope> RemoteViews<JObject<'env_scope>> {
    pub fn new(env: &mut Env<'env_scope>, package: &str, layout: i32) -> anyhow::Result<Self> {
        let package = JString::new(env, package)?;
        let class = env.find_class(jni_str!("android/widget/RemoteViews"))?;
        Ok(Self(env.new_object(
            &class,
            jni_sig!("(Ljava/lang/String;I)V"),
            &[JValue::Object(&package), JValue::Int(layout)],
        )?))
    }
}

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>> RemoteViews<R> {
    pub fn as_obj(&self) -> &JObject<'object_scope> {
        self.0.as_ref()
    }

    /// `setTextViewText(id, text)`; `text` is any `CharSequence` (a
    /// `Spanned` keeps its styling).
    pub fn set_text_view_text(
        &self,
        env: &mut Env<'env_scope>,
        id: i32,
        text: &JObject,
    ) -> anyhow::Result<()> {
        env.call_method(
            &self.0,
            jni_str!("setTextViewText"),
            jni_sig!("(ILjava/lang/CharSequence;)V"),
            &[JValue::Int(id), JValue::Object(text)],
        )?;
        Ok(())
    }

    pub fn set_image_view_resource(
        &self,
        env: &mut Env<'env_scope>,
        id: i32,
        drawable: i32,
    ) -> anyhow::Result<()> {
        env.call_method(
            &self.0,
            jni_str!("setImageViewResource"),
            jni_sig!("(II)V"),
            &[JValue::Int(id), JValue::Int(drawable)],
        )?;
        Ok(())
    }

    pub fn set_view_visibility(
        &self,
        env: &mut Env<'env_scope>,
        id: i32,
        visibility: Visibility,
    ) -> anyhow::Result<()> {
        env.call_method(
            &self.0,
            jni_str!("setViewVisibility"),
            jni_sig!("(II)V"),
            &[JValue::Int(id), JValue::Int(visibility.into())],
        )?;
        Ok(())
    }

    pub fn set_on_click_pending_intent<'p>(
        &self,
        env: &mut Env<'env_scope>,
        id: i32,
        pending: &PendingIntent<impl Reference + AsRef<JObject<'p>>>,
    ) -> anyhow::Result<()> {
        env.call_method(
            &self.0,
            jni_str!("setOnClickPendingIntent"),
            jni_sig!("(ILandroid/app/PendingIntent;)V"),
            &[JValue::Int(id), JValue::Object(pending.as_obj())],
        )?;
        Ok(())
    }

    /// Call the view's remotable `method(int)`, e.g. `setBackgroundResource`.
    pub fn set_int(
        &self,
        env: &mut Env<'env_scope>,
        id: i32,
        method: &str,
        value: i32,
    ) -> anyhow::Result<()> {
        let method = JString::new(env, method)?;
        env.call_method(
            &self.0,
            jni_str!("setInt"),
            jni_sig!("(ILjava/lang/String;I)V"),
            &[JValue::Int(id), JValue::Object(&method), JValue::Int(value)],
        )?;
        Ok(())
    }

    /// Call the view's remotable `method(float)`, e.g. `setAlpha`.
    pub fn set_float(
        &self,
        env: &mut Env<'env_scope>,
        id: i32,
        method: &str,
        value: f32,
    ) -> anyhow::Result<()> {
        let method = JString::new(env, method)?;
        env.call_method(
            &self.0,
            jni_str!("setFloat"),
            jni_sig!("(ILjava/lang/String;F)V"),
            &[
                JValue::Int(id),
                JValue::Object(&method),
                JValue::Float(value),
            ],
        )?;
        Ok(())
    }
}
