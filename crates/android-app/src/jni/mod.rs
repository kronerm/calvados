pub mod android;
pub mod java;

use jni::{
    Env, JValueOwned, JavaVM,
    objects::{JObject, JString},
};

use crate::jni::android::app::activity_thread::ActivityThread;
use crate::jni::android::content::context::Context;

/// A Java call's `String` result; `None` for null.
pub(crate) fn string(env: &mut Env, value: JValueOwned) -> anyhow::Result<Option<String>> {
    let value = env.cast_local::<JString>(value.l()?)?;
    if value.is_null() {
        return Ok(None);
    }
    Ok(Some(value.try_to_string(env)?))
}

pub(crate) fn with_env<T>(f: impl FnOnce(&mut Env) -> anyhow::Result<T>) -> anyhow::Result<T> {
    JavaVM::singleton()?.attach_current_thread(f)
}

pub(crate) fn app_context<'local>(
    env: &mut Env<'local>,
) -> anyhow::Result<Context<JObject<'local>>> {
    ActivityThread::current_activity_thread(env)?.get_application(env)
}
