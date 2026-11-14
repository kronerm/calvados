use jni::{
    Env, jni_sig, jni_str,
    objects::{JObject, JObjectArray},
    refs::Reference,
};

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

    /// `getUuids()` as strings: the services found by SDP, empty while
    /// unknown.
    pub fn get_uuids(&self, env: &mut Env) -> anyhow::Result<Vec<String>> {
        let uuids = env
            .call_method(
                &self.0,
                jni_str!("getUuids"),
                jni_sig!("()[Landroid/os/ParcelUuid;"),
                &[],
            )?
            .l()?;
        if uuids.is_null() {
            return Ok(Vec::new());
        }
        // SAFETY: the method returns a `ParcelUuid[]`.
        let uuids = unsafe { JObjectArray::<JObject>::from_raw(env, uuids.into_raw()) };
        (0..uuids.len(env)?)
            .map(|index| {
                let uuid = uuids.get_element(env, index)?;
                let text = env.call_method(
                    &uuid,
                    jni_str!("toString"),
                    jni_sig!("()Ljava/lang/String;"),
                    &[],
                )?;
                Ok(string(env, text)?.unwrap_or_default())
            })
            .collect()
    }

    /// Whether this is AirPods ([`crate::proto::connection::is_airpods`]).
    pub fn is_airpods(&self, env: &mut Env) -> anyhow::Result<bool> {
        let name = self.get_name(env)?;
        let uuids = self.get_uuids(env)?;
        Ok(crate::proto::connection::is_airpods(
            name.as_deref(),
            &uuids,
        ))
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
