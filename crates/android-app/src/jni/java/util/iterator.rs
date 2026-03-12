use jni::{Env, jni_sig, jni_str, objects::JObject, refs::Reference};

pub struct Iterator<R>(pub(crate) R);
impl<'local, R: Reference + AsRef<JObject<'local>>> Iterator<R> {
    pub fn has_next(&self, env: &mut Env) -> anyhow::Result<bool> {
        Ok(env
            .call_method(&self.0, jni_str!("hasNext"), jni_sig!("()Z"), &[])?
            .z()?)
    }

    pub fn next(&self, env: &mut Env<'local>) -> anyhow::Result<JObject<'local>> {
        let obj = env
            .call_method(
                &self.0,
                jni_str!("next"),
                jni_sig!("()Ljava/lang/Object;"),
                &[],
            )?
            .l()?;

        Ok(obj)
    }
}
