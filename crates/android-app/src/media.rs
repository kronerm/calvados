//! Local media playback control, as a media button press.

use jni::Env;
use jni::objects::JObject;

use crate::jni::android::media::audio_attributes::Usage;
use crate::jni::android::media::audio_playback_configuration::AudioPlaybackConfiguration;
use crate::jni::android::view::key_event::{Action, KeyCode, KeyEvent};
use crate::jni::java::util::list::List;
use crate::jni::{app_context, with_env};

pub(crate) fn pause() -> anyhow::Result<()> {
    press(KeyCode::MediaPause)
}

pub(crate) fn resume() -> anyhow::Result<()> {
    press(KeyCode::MediaPlay)
}

/// The usages (`AudioAttributes.USAGE_*`) of the active playbacks, from the
/// playback callback's `List<AudioPlaybackConfiguration>`.
pub(crate) fn active_usages<'local>(
    env: &mut Env<'local>,
    configs: List<JObject<'local>>,
) -> anyhow::Result<Vec<Usage>> {
    (0..configs.size(env)?)
        .map(|i| {
            let config = AudioPlaybackConfiguration(configs.get(env, i)?);
            config.get_audio_attributes(env)?.get_usage(env)
        })
        .filter_map(Result::transpose)
        .collect()
}

/// Suspend the A2DP output for `hold`, then resume it, so Android rebuilds
/// it. After a take, that clears the pods' remote suspend from when another
/// host had the audio (otherwise playback stays silent until a seek). The audio
/// HAL must honour `A2dpSuspended` (verified on a Pixel 9). Not during a
/// call: Android keeps A2DP suspended itself then.
pub(crate) fn toggle_a2dp_output(hold: std::time::Duration) -> anyhow::Result<()> {
    let set = |value: &str| {
        with_env(|env| {
            app_context(env)?
                .audio_manager(env)?
                .set_parameters(env, &format!("A2dpSuspended={value}"))
        })
    };
    set("true")?;
    std::thread::sleep(hold);
    set("false")
}

/// Dispatch a key down + up to the active media session.
fn press(code: KeyCode) -> anyhow::Result<()> {
    with_env(|env| {
        let audio = app_context(env)?.audio_manager(env)?;
        [Action::Down, Action::Up]
            .into_iter()
            .try_for_each(|action| {
                let event = KeyEvent::new(env, action, code)?;
                audio.dispatch_media_key_event(env, &event)
            })
    })
}
