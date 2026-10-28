//! `$XDG_STATE_HOME/calvados` (else `~/.local/state/calvados`): the AirPods'
//! identity keys (see `presence`) and the remembered D-Bus settings, one
//! file each.

use std::path::PathBuf;

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
