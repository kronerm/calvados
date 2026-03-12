use jni::{Env, JValue, jni_sig, jni_str, objects::JObject, refs::Reference};

/// `java.util.List`.
pub struct List<R>(pub(crate) R);

impl<'env_scope> List<JObject<'env_scope>> {
    /// `Collections.singletonList(item)`.
    pub fn singleton<'i>(
        env: &mut Env<'env_scope>,
        item: &(impl Reference + AsRef<JObject<'i>>),
    ) -> anyhow::Result<Self> {
        let class = env.load_class(jni_str!("java/util/Collections"))?;
        Ok(Self(
            env.call_static_method(
                &class,
                jni_str!("singletonList"),
                jni_sig!("(Ljava/lang/Object;)Ljava/util/List;"),
                &[JValue::Object(item.as_ref())],
            )?
            .l()?,
        ))
    }
}

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>> List<R> {
    pub fn size(&self, env: &mut Env<'env_scope>) -> anyhow::Result<i32> {
        Ok(env
            .call_method(&self.0, jni_str!("size"), jni_sig!("()I"), &[])?
            .i()?)
    }

    pub fn get(
        &self,
        env: &mut Env<'env_scope>,
        index: i32,
    ) -> anyhow::Result<JObject<'env_scope>> {
        Ok(env
            .call_method(
                &self.0,
                jni_str!("get"),
                jni_sig!("(I)Ljava/lang/Object;"),
                &[JValue::Int(index)],
            )?
            .l()?)
    }
}
