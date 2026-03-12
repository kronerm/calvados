use jni::{Env, JValue, jni_sig, jni_str, objects::JObject};

pub struct Integer<R>(pub(crate) R);
impl<'env_scope> Integer<JObject<'env_scope>> {
    pub fn value_of(env: &mut Env<'env_scope>, value: i32) -> anyhow::Result<Self> {
        let integer_class = env.load_class(jni_str!("java/lang/Integer"))?;

        let integer = env
            .call_static_method(
                &integer_class,
                jni_str!("valueOf"),
                jni_sig!("(I)Ljava/lang/Integer;"),
                &[JValue::Int(value)],
            )?
            .l()?;

        Ok(Self(integer))
    }
}
