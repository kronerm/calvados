//! Apple's firmware catalog: the current release for the pods' product,
//! asked at most once a day, off the loop; and the release's package for an
//! update, fetched once and cached.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::Context as _;
use firmware_fetch::Client;
use protocol::firmware::Release;
use tokio::sync::mpsc;

use crate::Input;

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
