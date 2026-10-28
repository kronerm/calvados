use jni::{Env, jni_sig, jni_str, objects::JObject, refs::Reference};

use crate::jni::string;

pub struct BluetoothDevice<R>(pub(crate) R);
impl<'object_scope, R: Reference + AsRef<JObject<'object_scope>>> BluetoothDevice<R> {
    /// `getName()`; `None` while unknown.
    pub fn get_name(&self, env: &mut Env) -> anyhow::Result<Option<String>> {
        let name = env.call_method(
            &self.0,
            jni_str!("getName"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?;
        string(env, name)
    }

    /// `getAddress()`: `AA:BB:…`.
    pub fn get_address(&self, env: &mut Env) -> anyhow::Result<String> {
        let address = env.call_method(
            &self.0,
            jni_str!("getAddress"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?;
        string(env, address)?.ok_or_else(|| anyhow::anyhow!("getAddress() returned null"))
    }
}
