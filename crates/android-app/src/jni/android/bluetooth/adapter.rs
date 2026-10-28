use jni::{Env, JValue, jni_sig, jni_str, objects::JObject, refs::Reference};

use crate::jni::android::bluetooth::a2dp::Profile;
use crate::jni::android::bluetooth::device::BluetoothDevice;
use crate::jni::android::bluetooth::le::scanner::BluetoothLeScanner;
use crate::jni::android::content::context::Context;

pub struct BluetoothAdapter<R>(pub(crate) R);
impl<'env_scope> BluetoothAdapter<JObject<'env_scope>> {
    pub fn default_adapter(env: &mut Env<'env_scope>) -> anyhow::Result<Self> {
        let class = env.load_class(jni_str!("android/bluetooth/BluetoothAdapter"))?;
        let adapter = env
            .call_static_method(
                class,
                jni_str!("getDefaultAdapter"),
                jni_sig!("()Landroid/bluetooth/BluetoothAdapter;"),
                &[],
            )?
            .l()?;
        Ok(Self(adapter))
    }
}

impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>> BluetoothAdapter<R> {
    pub fn get_bonded_devices(
        &self,
        env: &mut Env<'env_scope>,
    ) -> anyhow::Result<crate::jni::java::util::set::Set<JObject<'env_scope>>> {
        let bonded_set = env
            .call_method(
                &self.0,
                jni_str!("getBondedDevices"),
                jni_sig!("()Ljava/util/Set;"),
                &[],
            )?
            .l()?;
        Ok(crate::jni::java::util::set::Set(bonded_set))
    }

    pub fn get_remote_device(
        &self,
        env: &mut Env<'env_scope>,
        mac_address: &str,
    ) -> anyhow::Result<BluetoothDevice<JObject<'env_scope>>> {
        let mac_address = env.new_string(mac_address)?;
        let device = env
            .call_method(
                &self.0,
                jni_str!("getRemoteDevice"),
                jni_sig!("(Ljava/lang/String;)Landroid/bluetooth/BluetoothDevice;"),
                &[JValue::Object(&mac_address)],
            )?
            .l()?;

        Ok(BluetoothDevice(device))
    }

    /// `getProfileProxy(context, listener, profile)`; the proxy arrives in
    /// `listener.onServiceConnected`.
    pub fn get_profile_proxy<'c, 'l>(
        &self,
        env: &mut Env<'env_scope>,
        context: &Context<impl Reference + AsRef<JObject<'c>>>,
        listener: &(impl Reference + AsRef<JObject<'l>>),
        profile: Profile,
    ) -> anyhow::Result<bool> {
        Ok(env
            .call_method(
                &self.0,
                jni_str!("getProfileProxy"),
                jni_sig!(
                    "(Landroid/content/Context;Landroid/bluetooth/BluetoothProfile$ServiceListener;I)Z"
                ),
                &[
                    JValue::Object(context.as_obj()),
                    JValue::Object(listener.as_ref()),
                    JValue::Int(profile.into()),
                ],
            )?
            .z()?)
    }

    /// `closeProfileProxy(profile, proxy)`.
    pub fn close_profile_proxy<'p>(
        &self,
        env: &mut Env<'env_scope>,
        profile: Profile,
        proxy: &(impl Reference + AsRef<JObject<'p>>),
    ) -> anyhow::Result<()> {
        env.call_method(
            &self.0,
            jni_str!("closeProfileProxy"),
            jni_sig!("(ILandroid/bluetooth/BluetoothProfile;)V"),
            &[JValue::Int(profile.into()), JValue::Object(proxy.as_ref())],
        )?;
        Ok(())
    }

    pub fn get_bluetooth_le_scanner(
        &self,
        env: &mut Env<'env_scope>,
    ) -> anyhow::Result<BluetoothLeScanner<JObject<'env_scope>>> {
        Ok(BluetoothLeScanner(
            env.call_method(
                &self.0,
                jni_str!("getBluetoothLeScanner"),
                jni_sig!("()Landroid/bluetooth/le/BluetoothLeScanner;"),
                &[],
            )?
            .l()?,
        ))
    }
}
