use jni::{Env, jni_sig, jni_str, objects::JObject, refs::Reference};
use num_enum::TryFromPrimitive;

/// `AudioAttributes.USAGE_*`, those that matter here.
#[derive(Debug, Clone, Copy, TryFromPrimitive)]
#[repr(i32)]
pub enum Usage {
    Media = 1,
    /// VoIP calls.
    VoiceCommunication = 2,
}

/// `android.media.AudioAttributes`.
pub struct AudioAttributes<R>(pub R);

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>> AudioAttributes<R> {
    /// `getUsage()`; `None` for the other usages.
    pub fn get_usage(&self, env: &mut Env<'env_scope>) -> anyhow::Result<Option<Usage>> {
        let usage = env
            .call_method(&self.0, jni_str!("getUsage"), jni_sig!("()I"), &[])?
            .i()?;
        Ok(Usage::try_from(usage).ok())
    }
}
