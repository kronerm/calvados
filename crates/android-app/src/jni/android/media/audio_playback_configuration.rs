use jni::{Env, jni_sig, jni_str, objects::JObject, refs::Reference};

use crate::jni::android::media::audio_attributes::AudioAttributes;

/// `android.media.AudioPlaybackConfiguration`: one active playback.
pub struct AudioPlaybackConfiguration<R>(pub R);

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>>
    AudioPlaybackConfiguration<R>
{
    /// `getAudioAttributes()`.
    pub fn get_audio_attributes(
        &self,
        env: &mut Env<'env_scope>,
    ) -> anyhow::Result<AudioAttributes<JObject<'env_scope>>> {
        Ok(AudioAttributes(
            env.call_method(
                &self.0,
                jni_str!("getAudioAttributes"),
                jni_sig!("()Landroid/media/AudioAttributes;"),
                &[],
            )?
            .l()?,
        ))
    }
}
