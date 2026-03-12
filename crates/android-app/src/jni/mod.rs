pub mod android;
pub mod java;

use jni::{
    Env, JavaVM, jni_sig, jni_str,
    objects::{JClass, JObject, JObjectArray},
};

use crate::jni::android::app::activity_thread::ActivityThread;
use crate::jni::android::content::context::Context;
use crate::jni::java::lang::reflect::constructor::Constructor;

pub trait Class<'env_scope> {
    fn class(env: &mut Env<'env_scope>) -> anyhow::Result<JClass<'env_scope>>;
}

pub trait GetDeclaredConstructors {
    fn get_declared_constructors<'env_scope>(
        &self,
        env: &mut Env<'env_scope>,
    ) -> anyhow::Result<Vec<Constructor<JObject<'env_scope>>>>;
}

impl GetDeclaredConstructors for JClass<'_> {
    fn get_declared_constructors<'env_scope>(
        &self,
        env: &mut Env<'env_scope>,
    ) -> anyhow::Result<Vec<Constructor<JObject<'env_scope>>>> {
        let constructors = env
            .call_method(
                self,
                jni_str!("getDeclaredConstructors"),
                jni_sig!("()[Ljava/lang/reflect/Constructor;"),
                &[],
            )?
            .l()?;

        let constructors_array =
            unsafe { JObjectArray::<JObject<'_>>::from_raw(env, constructors.into_raw()) };

        Ok((0..constructors_array.len(env)?)
            .map(|index| constructors_array.get_element(env, index).map(Constructor))
            .collect::<Result<Vec<_>, _>>()?)
    }
}

pub(crate) fn with_env<T>(f: impl FnOnce(&mut Env) -> anyhow::Result<T>) -> anyhow::Result<T> {
    JavaVM::singleton()?.attach_current_thread(f)
}

pub(crate) fn app_context<'local>(
    env: &mut Env<'local>,
) -> anyhow::Result<Context<JObject<'local>>> {
    ActivityThread::current_activity_thread(env)?.get_application(env)
}
