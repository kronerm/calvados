use jni::{Env, JValue, jni_sig, jni_str, objects::JObject, refs::Reference};

/// `android.media.ToneGenerator`.
pub struct ToneGenerator<R>(pub(crate) R);

/// `AudioManager.STREAM_MUSIC`: played where media goes.
const STREAM_MUSIC: i32 = 3;

/// `ToneGenerator.TONE_*`, the ones used.
#[derive(Debug, Clone, Copy, num_enum::IntoPrimitive)]
#[repr(i32)]
pub enum Tone {
    /// `TONE_PROP_BEEP2`: two short beeps.
    PropBeep2 = 28,
}

impl<'env_scope> ToneGenerator<JObject<'env_scope>> {
    /// `new ToneGenerator(STREAM_MUSIC, volume)`, volume 0-100.
    pub fn music(env: &mut Env<'env_scope>, volume: i32) -> anyhow::Result<Self> {
        Ok(Self(env.new_object(
            jni_str!("android/media/ToneGenerator"),
            jni_sig!("(II)V"),
            &[JValue::Int(STREAM_MUSIC), JValue::Int(volume)],
        )?))
    }
}

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>> ToneGenerator<R> {
    /// `startTone(tone, durationMs)`.
    pub fn start_tone(
        &self,
        env: &mut Env<'env_scope>,
        tone: Tone,
        millis: i32,
    ) -> anyhow::Result<()> {
        env.call_method(
            &self.0,
            jni_str!("startTone"),
            jni_sig!("(II)Z"),
            &[JValue::Int(tone.into()), JValue::Int(millis)],
        )?;
        Ok(())
    }

    /// `release()`.
    pub fn release(&self, env: &mut Env<'env_scope>) -> anyhow::Result<()> {
        env.call_method(&self.0, jni_str!("release"), jni_sig!("()V"), &[])?;
        Ok(())
    }
}
