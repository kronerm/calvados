use jni::{
    Env, JValue, jni_sig, jni_str,
    objects::{JClass, JObject, JObjectArray},
    refs::Reference,
};

pub struct Constructor<R>(pub(crate) R);

impl<'env_scope> Constructor<JObject<'env_scope>> {
    /// `class.getDeclaredConstructors()`.
    pub fn declared_by(env: &mut Env<'env_scope>, class: &JClass) -> anyhow::Result<Vec<Self>> {
        let constructors = env
            .call_method(
                class,
                jni_str!("getDeclaredConstructors"),
                jni_sig!("()[Ljava/lang/reflect/Constructor;"),
                &[],
            )?
            .l()?;
        // SAFETY: the method returns a `Constructor[]`.
        let constructors =
            unsafe { JObjectArray::<JObject>::from_raw(env, constructors.into_raw()) };
        Ok((0..constructors.len(env)?)
            .map(|index| constructors.get_element(env, index).map(Constructor))
            .collect::<Result<Vec<_>, _>>()?)
    }
}

impl<'object_scope, R: Reference + AsRef<JObject<'object_scope>>> Constructor<R> {
    pub fn set_accessible(&self, env: &mut Env, accessible: bool) -> anyhow::Result<()> {
        env.call_method(
            &self.0,
            jni_str!("setAccessible"),
            jni_sig!("(Z)V"),
            &[JValue::Bool(accessible)],
        )?;

        Ok(())
    }

    pub fn get_parameter_count(&self, env: &mut Env) -> anyhow::Result<i32> {
        Ok(env
            .call_method(&self.0, jni_str!("getParameterCount"), jni_sig!("()I"), &[])?
            .i()?)
    }

    pub fn new_instance<'args_scope, 'env_scope, Arg>(
        &self,
        env: &mut Env<'env_scope>,
        args: &[Arg],
    ) -> anyhow::Result<JObject<'env_scope>>
    where
        Arg: Reference + AsRef<JObject<'args_scope>>,
    {
        let args_array = JObjectArray::<JObject<'_>>::new(env, args.len(), JObject::null())?;
        for (index, value) in args.iter().enumerate() {
            args_array.set_element(env, index, value.as_ref())?
        }

        Ok(env
            .call_method(
                &self.0,
                jni_str!("newInstance"),
                jni_sig!("([Ljava/lang/Object;)Ljava/lang/Object;"),
                &[JValue::Object(&args_array)],
            )?
            .l()?)
    }
}
