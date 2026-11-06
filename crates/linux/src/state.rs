//! `$XDG_STATE_HOME/calvados` (else `~/.local/state/calvados`): the AirPods'
//! identity keys (see `presence`), the remembered D-Bus settings, one
//! file each, and the pods' crash logs.

use std::path::PathBuf;

use protocol::MacAddress;
use protocol::aacp::crash_log;
use protocol::settings::Setting;

pub fn dir() -> Option<PathBuf> {
    xdg_dir("XDG_STATE_HOME", ".local/state")
}

/// `$variable/calvados`, else `~/fallback/calvados`.
pub fn xdg_dir(variable: &str, fallback: &str) -> Option<PathBuf> {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(fallback)))
        .map(|dir| dir.join("calvados"))
}

/// One bud's crash logs, as the pods sent them.
pub fn save_crash_log(pods: MacAddress, bud: u8, data: &[u8]) {
    let Some(dir) = dir() else {
        return;
    };
    let path = dir.join(crash_log::file_name(pods, bud, unix_now()));
    let saved = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&path, data));
    match saved {
        Ok(()) => tracing::info!(path = %path.display(), "kept the pods' crash logs"),
        Err(e) => tracing::warn!("keeping the pods' crash logs: {e}"),
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// Stored as `on` / `off`; anything else (or nothing) is on.
pub fn load(setting: Setting) -> bool {
    dir()
        .and_then(|dir| std::fs::read_to_string(dir.join(setting.key())).ok())
        .is_none_or(|stored| stored.trim() != "off")
}

pub fn save(setting: Setting, enabled: bool) {
    let Some(dir) = dir() else {
        return;
    };
    let value = if enabled { "on" } else { "off" };
    let saved =
        std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(dir.join(setting.key()), value));
    if let Err(e) = saved {
        tracing::warn!(?setting, "remembering the setting: {e}");
    }
}
