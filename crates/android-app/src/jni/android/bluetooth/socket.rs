use jni::{
    Env, jni_sig, jni_str,
    objects::{JClass, JObject},
    refs::{Global, Reference},
};

use crate::jni::{
    Class,
    java::io::{input_stream::InputStream, output_stream::OutputStream},
};

pub struct BluetoothSocket<R>(pub(crate) R);
impl<'object_scope, 'env_scope, R: Reference + AsRef<JObject<'object_scope>>> BluetoothSocket<R> {
    pub fn connect(&self, env: &mut Env) -> anyhow::Result<()> {
        env.call_method(&self.0, jni_str!("connect"), jni_sig!("()V"), &[])?
            .v()?;

        Ok(())
    }

    pub fn close(&self, env: &mut Env) -> anyhow::Result<()> {
        env.call_method(&self.0, jni_str!("close"), jni_sig!("()V"), &[])?
            .v()?;

        Ok(())
    }

    pub fn get_input_stream(
        &self,
        env: &mut Env<'env_scope>,
    ) -> anyhow::Result<InputStream<JObject<'env_scope>>> {
        let input_stream = env
            .call_method(
                &self.0,
                jni_str!("getInputStream"),
                jni_sig!("()Ljava/io/InputStream;"),
                &[],
            )?
            .l()?;
        Ok(InputStream(input_stream))
    }

    pub fn get_output_stream(
        &self,
        env: &mut Env<'env_scope>,
    ) -> anyhow::Result<OutputStream<JObject<'env_scope>>> {
        Ok(OutputStream(
            env.call_method(
                &self.0,
                jni_str!("getOutputStream"),
                jni_sig!("()Ljava/io/OutputStream;"),
                &[],
            )?
            .l()?,
        ))
    }
}

impl BluetoothSocket<JObject<'_>> {
    pub fn into_global(
        self,
        env: &mut Env,
    ) -> anyhow::Result<BluetoothSocket<Global<JObject<'static>>>> {
        Ok(BluetoothSocket(env.new_global_ref(self.0)?))
    }
}

impl<'object_scope, 'env_scope, R> Class<'env_scope> for BluetoothSocket<R>
where
    R: Reference + AsRef<JObject<'object_scope>>,
{
    fn class(env: &mut Env<'env_scope>) -> anyhow::Result<JClass<'env_scope>> {
        Ok(env.load_class(jni_str!("android/bluetooth/BluetoothSocket"))?)
    }
}
