//! The pods' firmware: what they last reported, stored per pair; Apple's
//! firmware catalog: the current release for the pods' product, asked at
//! most once a day, off the loop; and the release's package for an update,
//! fetched once and cached.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::Context as _;
use firmware_fetch::Client;
use protocol::MacAddress;
use protocol::firmware::{Firmware, Release};
use tokio::sync::mpsc;

use crate::{Input, state};

/// `<address>.firmware` in the state directory, like the keys.
fn path(pods: MacAddress) -> Option<PathBuf> {
    let pods = bluer::Address(pods.to_array());
    Some(state::dir()?.join(format!("{pods}.firmware")))
}

/// What was stored for `pods`; nothing when never stored.
pub fn stored(pods: MacAddress) -> Firmware {
    path(pods)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|stored| Firmware::from_stored(&stored))
        .unwrap_or_default()
}

pub fn save(pods: MacAddress, firmware: &Firmware) {
    let Some(path) = path(pods) else {
        return;
    };
    let saved = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&path, firmware.to_stored()));
    if let Err(e) = saved {
        tracing::warn!(%pods, "storing the firmware: {e}");
    }
}

/// Owned by the loop; fetches report back as [`Input::Catalog`] and
/// [`Input::Package`].
pub struct Catalog {
    client: Client,
    checked: firmware_fetch::Catalog,
}

impl Catalog {
    pub fn new() -> Self {
        Self {
            client: Client::default(),
            checked: firmware_fetch::Catalog::default(),
        }
    }

    /// Ask for `model`'s product unless that was done within a day.
    pub fn check(&mut self, model: &str, inputs: &mpsc::UnboundedSender<Input>, now: Instant) {
        let Some(models) = self.checked.due(model, now) else {
            return;
        };
        let (client, inputs) = (self.client.clone(), inputs.clone());
        tokio::task::spawn_blocking(move || {
            let release = client.release(&models);
            // The loop is gone only when the daemon stops.
            let _ = inputs.send(Input::Catalog(models, release));
        });
    }

    /// A check's result; `None` keeps what an earlier one found.
    pub fn found(&mut self, models: Vec<String>, release: Option<Release>) {
        self.checked.found(models, release);
    }

    pub fn latest(&self, model: &str) -> Option<&Release> {
        self.checked.latest(model)
    }

    /// `release`'s package, from the cache or downloaded, checked; reported
    /// as [`Input::Package`] (`None` when it couldn't be had).
    pub fn fetch_package(&self, release: Release, inputs: &mpsc::UnboundedSender<Input>) {
        let (client, inputs) = (self.client.clone(), inputs.clone());
        tokio::task::spawn_blocking(move || {
            let package = cache_dir()
                .context("no cache directory")
                .and_then(|cache| client.package(&release, &cache))
                .inspect(|package| tracing::info!(?package, "firmware package ready"))
                .inspect_err(|e| tracing::warn!(%release, "firmware package: {e:#}"))
                .ok();
            let _ = inputs.send(Input::Package(package));
        });
    }
}

/// `$XDG_CACHE_HOME/calvados` (else under `~/.cache`).
fn cache_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .map(|dir| dir.join("calvados"))
}
