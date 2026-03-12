use jni::{Env, JValue, jni_sig, jni_str, objects::JObject, refs::Reference};
use num_enum::IntoPrimitive;

/// `android.view.KeyEvent.ACTION_*`.
#[derive(Debug, Clone, Copy, IntoPrimitive)]
#[repr(i32)]
pub enum Action {
    Down = 0,
    Up = 1,
}

/// `android.view.KeyEvent.KEYCODE_*`.
#[derive(Debug, Clone, Copy, IntoPrimitive)]
#[repr(i32)]
pub enum KeyCode {
    MediaPlay = 126,
    MediaPause = 127,
}

/// `android.view.KeyEvent`.
pub struct KeyEvent<R>(R);

impl<'env_scope> KeyEvent<JObject<'env_scope>> {
    /// `KeyEvent(action, code)`.
    pub fn new(env: &mut Env<'env_scope>, action: Action, code: KeyCode) -> anyhow::Result<Self> {
        let class = env.find_class(jni_str!("android/view/KeyEvent"))?;
        Ok(Self(env.new_object(
            &class,
            jni_sig!("(II)V"),
            &[JValue::Int(action.into()), JValue::Int(code.into())],
        )?))
    }
}

impl<'object_scope, R: Reference + AsRef<JObject<'object_scope>>> KeyEvent<R> {
    pub fn as_obj(&self) -> &JObject<'object_scope> {
        self.0.as_ref()
    }
}
