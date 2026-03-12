//! JNI entry points of `PersistentService.java`.

use std::path::Path;

use jni::EnvUnowned;
use jni::errors::LogErrorAndDefault;
use jni::objects::{JClass, JObject};

use crate::jni::android::app::service::Service;
use crate::jni::android::content::intent::Intent;
use crate::jni::android::media::audio_attributes::Usage;
use crate::jni::java::util::list::List;
use crate::service::{self, Event, post};
use crate::{bluetooth, init_tracing, intents, media, notification};

#[unsafe(no_mangle)]
extern "system" fn Java_dev_kroner_calvados_PersistentService_nativeOnCreate<'local>(
    mut env: EnvUnowned<'local>,
    service: JObject<'local>,
) {
    let jvm = env
        .with_env(|env| -> jni::errors::Result<_> {
            let service = Service(service);
            let files = service.as_context().get_files_dir(env);
            let files = files.and_then(|dir| dir.get_absolute_path(env));
            init_tracing(
                files
                    .as_deref()
                    .ok()
                    .map(|dir| Path::new(dir).join("logs"))
                    .as_deref(),
            );
            tracing::info!("Service: onCreate");
            if let Err(e) = files {
                tracing::warn!("no files directory, logging to logcat only: {e:#}");
            }

            // Also registers the JavaVM singleton used by every later JNI call.
            let jvm = env.get_java_vm()?;
            if let Err(e) = notification::start_foreground(env, &service) {
                tracing::error!("startForeground failed: {e:#}");
            }
            bluetooth::set_service(Some(env.new_global_ref(&service.0)?));
            Ok(Some(jvm))
        })
        .resolve::<LogErrorAndDefault>();
    if let Some(jvm) = jvm {
        service::start(jvm);
    }
}

/// `onStartCommand` intents and the ACL broadcasts.
#[unsafe(no_mangle)]
extern "system" fn Java_dev_kroner_calvados_PersistentService_nativeOnIntent<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    intent: JObject<'local>,
) {
    env.with_env(|env| -> jni::errors::Result<()> {
        match intents::parse(env, Intent(intent)) {
            Ok(Some(event)) => post(event),
            Ok(None) => {}
            Err(e) => tracing::warn!("intent: {e:#}"),
        }
        Ok(())
    })
    .resolve::<LogErrorAndDefault>();
}

/// `BluetoothProfile.ServiceListener.onServiceConnected`.
#[unsafe(no_mangle)]
extern "system" fn Java_dev_kroner_calvados_PersistentService_nativeOnProfileProxy<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    profile: jni::sys::jint,
    proxy: JObject<'local>,
) {
    env.with_env(|env| -> jni::errors::Result<()> {
        match bluetooth::on_profile_proxy(env, profile, proxy) {
            Ok(true) => {
                tracing::info!("AirPods already connected");
                post(Event::Intent(intents::ACTION_AIRPODS_AVAILABLE.to_owned()));
            }
            Ok(false) => {}
            Err(e) => tracing::warn!("profile proxy: {e:#}"),
        }
        Ok(())
    })
    .resolve::<LogErrorAndDefault>();
}

/// `AudioPlaybackCallback.onPlaybackConfigChanged`: what plays now.
#[unsafe(no_mangle)]
extern "system" fn Java_dev_kroner_calvados_PersistentService_nativeOnPlayback<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    configs: JObject<'local>,
) {
    env.with_env(|env| -> jni::errors::Result<()> {
        match media::active_usages(env, List(configs)) {
            Ok(usages) => post(Event::Playback {
                media: usages.iter().any(|usage| matches!(usage, Usage::Media)),
                voip: usages
                    .iter()
                    .any(|usage| matches!(usage, Usage::VoiceCommunication)),
            }),
            Err(e) => tracing::warn!("playback callback: {e:#}"),
        }
        Ok(())
    })
    .resolve::<LogErrorAndDefault>();
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_kroner_calvados_PersistentService_nativeOnDestroy<'local>(
    _env: EnvUnowned<'local>,
    _class: JClass<'local>,
) {
    tracing::info!("Service: onDestroy");
    bluetooth::set_service(None);
    service::stop();
}
