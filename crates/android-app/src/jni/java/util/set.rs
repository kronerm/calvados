use jni::{Env, jni_sig, jni_str, objects::JObject, refs::Reference};

pub struct Set<R>(pub(crate) R);
impl<'local, R: Reference + AsRef<JObject<'local>>> Set<R> {
    pub fn iterator(
        &self,
        env: &mut Env<'local>,
    ) -> anyhow::Result<super::iterator::Iterator<JObject<'local>>> {
        let iterator = env
            .call_method(
                &self.0,
                jni_str!("iterator"),
                jni_sig!("()Ljava/util/Iterator;"),
                &[],
            )?
            .l()?;

        Ok(super::iterator::Iterator(iterator))
    }
}
