use jni::{Env, JValue, jni_sig, jni_str, objects::JObject};

pub struct ParcelUuid<R>(pub(crate) R);

impl<'env_scope> ParcelUuid<JObject<'env_scope>> {
    pub fn from_string(env: &mut Env<'env_scope>, uuid: &str) -> anyhow::Result<Self> {
        // Create ParcelUuid for the AirPods service
        let parcel_uuid_class = env.find_class(jni_str!("android/os/ParcelUuid"))?;

        let uuid_string = env.new_string(uuid)?;

        let parcel_uuid = env
            .call_static_method(
                &parcel_uuid_class,
                jni_str!("fromString"),
                jni_sig!("(Ljava/lang/String;)Landroid/os/ParcelUuid;"),
                &[JValue::Object(&uuid_string)],
            )?
            .l()?;

        Ok(Self(parcel_uuid))
    }
}
