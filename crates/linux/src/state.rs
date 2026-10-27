//! `$XDG_STATE_HOME/calvados` (else `~/.local/state/calvados`): the AirPods'
//! identity keys (see `presence`), the remembered D-Bus settings, and the
//! press-and-hold cycle last set, one file each.

use std::path::PathBuf;

use protocol::MacAddress;
use protocol::aacp::Cycle;
use protocol::display;
use protocol::settings::Setting;

pub fn dir() -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
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

/// `<address>.cycle`: the modes' names, comma-separated; empty while unknown.
fn cycle_path(pods: MacAddress) -> Option<PathBuf> {
    let pods = bluer::Address(pods.to_array());
    Some(dir()?.join(format!("{pods}.cycle")))
}

pub fn stored_cycle(pods: MacAddress) -> Option<Cycle> {
    let stored = std::fs::read_to_string(cycle_path(pods)?).ok()?;
    display::parse_cycle(stored.trim().split(','))
}

pub fn save_cycle(pods: MacAddress, cycle: Option<Cycle>) {
    let Some(path) = cycle_path(pods) else {
        return;
    };
    let saved = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&path, display::cycle_names(cycle).join(",")));
    if let Err(e) = saved {
        tracing::warn!(%pods, "storing the press-and-hold cycle: {e}");
    }
}
