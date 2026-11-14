//! Local media playback control, as a media button press.

use std::time::Duration;

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

/// How long [`toggle_a2dp_output`] holds the suspend: long enough for the
/// output to be torn down.
const HOLD: Duration = Duration::from_millis(200);

/// Suspend the A2DP output for [`HOLD`], then resume it, so Android rebuilds
/// it. After a take, that clears the pods' remote suspend from when another
/// host had the audio (otherwise playback stays silent until a seek). The audio
/// HAL must honour `A2dpSuspended` (verified on a Pixel 9). Not during a
/// call: Android keeps A2DP suspended itself then.
pub(crate) fn toggle_a2dp_output() -> anyhow::Result<()> {
    let set = |value: &str| {
        with_env(|env| {
            app_context(env)?
                .audio_manager(env)?
                .set_parameters(env, &format!("A2dpSuspended={value}"))
        })
    };
    set("true")?;
    std::thread::sleep(HOLD);
    set("false")
}

/// How far a duck lowers the media volume (LibrePods: ~40 %).
const DUCKED: f64 = 0.4;
/// Steps of a volume change, so it fades.
const FADE_STEP: Duration = Duration::from_millis(50);

/// The media volume before [`duck`].
static DUCKED_FROM: std::sync::Mutex<Option<i32>> = std::sync::Mutex::new(None);

/// Lower the media volume to [`DUCKED`] of itself, fading (Conversation
/// Awareness), or restore it; on its own thread, for the fade.
pub(crate) fn duck(down: bool) {
    std::thread::spawn(move || {
        if let Err(e) = fade(down) {
            tracing::warn!(down, "ducking the media volume: {e:#}");
        }
    });
}

fn fade(down: bool) -> anyhow::Result<()> {
    let volume = |set: Option<i32>| {
        with_env(|env| {
            let audio = app_context(env)?.audio_manager(env)?;
            match set {
                Some(index) => audio.set_music_volume(env, index).map(|()| index),
                None => audio.get_music_volume(env),
            }
        })
    };
    let current = volume(None)?;
    let mut from = DUCKED_FROM.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let target = match (down, *from) {
        (true, None) => {
            *from = Some(current);
            (f64::from(current) * DUCKED).round() as i32
        }
        (false, Some(before)) => {
            *from = None;
            before
        }
        _ => return Ok(()),
    };
    drop(from);
    let step = if target > current { 1 } else { -1 };
    let mut index = current;
    while index != target {
        index += step;
        volume(Some(index))?;
        std::thread::sleep(FADE_STEP);
    }
    tracing::info!(down, current, target, "media volume");
    Ok(())
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
