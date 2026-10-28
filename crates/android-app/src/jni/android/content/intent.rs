use jni::{
    Env, JValue, jni_sig, jni_str,
    objects::{JObject, JString},
    refs::Reference,
};

use crate::jni::java::util::list::List;
use crate::jni::string;

/// `android.content.Intent`.
pub struct Intent<R>(pub(crate) R);

impl<'env_scope> Intent<JObject<'env_scope>> {
    /// Construct an empty `Intent` (the `Intent()` constructor).
    pub fn new(env: &mut Env<'env_scope>) -> anyhow::Result<Self> {
        let class = env.find_class(jni_str!("android/content/Intent"))?;
        Ok(Self(env.new_object(&class, jni_sig!("()V"), &[])?))
    }
}

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>> Intent<R> {
    pub fn as_obj(&self) -> &JObject<'object_scope> {
        self.0.as_ref()
    }

    /// `setClassName(packageName, className)` — target a specific component.
    pub fn set_class_name(
        self,
        env: &mut Env<'env_scope>,
        package: &str,
        class: &str,
    ) -> anyhow::Result<Self> {
        let package = JString::new(env, package)?;
        let class = JString::new(env, class)?;
        env.call_method(
            &self.0,
            jni_str!("setClassName"),
            jni_sig!("(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;"),
            &[JValue::Object(&package), JValue::Object(&class)],
        )?;
        Ok(self)
    }

    pub fn set_action(self, env: &mut Env<'env_scope>, action: &str) -> anyhow::Result<Self> {
        let action = JString::new(env, action)?;
        env.call_method(
            &self.0,
            jni_str!("setAction"),
            jni_sig!("(Ljava/lang/String;)Landroid/content/Intent;"),
            &[JValue::Object(&action)],
        )?;
        Ok(self)
    }

    /// `getAction()`; `None` when unset.
    pub fn get_action(&self, env: &mut Env<'env_scope>) -> anyhow::Result<Option<String>> {
        let action = env.call_method(
            &self.0,
            jni_str!("getAction"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?;
        string(env, action)
    }

    /// `getStringExtra(name)`; `None` when absent.
    pub fn get_string_extra(
        &self,
        env: &mut Env<'env_scope>,
        name: &str,
    ) -> anyhow::Result<Option<String>> {
        let name = JString::new(env, name)?;
        let extra = env.call_method(
            &self.0,
            jni_str!("getStringExtra"),
            jni_sig!("(Ljava/lang/String;)Ljava/lang/String;"),
            &[JValue::Object(&name)],
        )?;
        string(env, extra)
    }

    /// `getIntExtra(name, default)`.
    pub fn get_int_extra(
        &self,
        env: &mut Env<'env_scope>,
        name: &str,
        default: i32,
    ) -> anyhow::Result<i32> {
        let name = JString::new(env, name)?;
        Ok(env
            .call_method(
                &self.0,
                jni_str!("getIntExtra"),
                jni_sig!("(Ljava/lang/String;I)I"),
                &[JValue::Object(&name), JValue::Int(default)],
            )?
            .i()?)
    }

    /// `getParcelableExtra(name)`; `None` when absent.
    pub fn get_parcelable_extra(
        &self,
        env: &mut Env<'env_scope>,
        name: &str,
    ) -> anyhow::Result<Option<JObject<'env_scope>>> {
        let name = JString::new(env, name)?;
        let extra = env
            .call_method(
                &self.0,
                jni_str!("getParcelableExtra"),
                jni_sig!("(Ljava/lang/String;)Landroid/os/Parcelable;"),
                &[JValue::Object(&name)],
            )?
            .l()?;
        Ok((!extra.is_null()).then_some(extra))
    }

    /// `getParcelableArrayListExtra(name)`; `None` when absent.
    pub fn get_parcelable_array_list_extra(
        &self,
        env: &mut Env<'env_scope>,
        name: &str,
    ) -> anyhow::Result<Option<List<JObject<'env_scope>>>> {
        let name = JString::new(env, name)?;
        let list = env
            .call_method(
                &self.0,
                jni_str!("getParcelableArrayListExtra"),
                jni_sig!("(Ljava/lang/String;)Ljava/util/ArrayList;"),
                &[JValue::Object(&name)],
            )?
            .l()?;
        Ok((!list.is_null()).then_some(List(list)))
    }
}
