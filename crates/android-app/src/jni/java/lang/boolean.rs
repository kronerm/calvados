use jni::{Env, JValue, jni_sig, jni_str, objects::JObject};

pub struct Boolean<R>(pub(crate) R);
impl<'env_scope> Boolean<JObject<'env_scope>> {
    pub fn value_of(env: &mut Env<'env_scope>, value: bool) -> anyhow::Result<Self> {
        let boolean_class = env.load_class(jni_str!("java/lang/Boolean"))?;

        let boolean = env
            .call_static_method(
                &boolean_class,
                jni_str!("valueOf"),
                jni_sig!("(Z)Ljava/lang/Boolean;"),
                &[JValue::Bool(value)],
            )?
            .l()?;

        Ok(Self(boolean))
    }
}
