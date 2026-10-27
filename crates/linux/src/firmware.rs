//! The pods' firmware: what they last reported, stored per pair, and
//! Apple's firmware catalog: the current release for the pods' product,
//! asked at most once a day, off the loop. Only tells; never updates.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use protocol::MacAddress;
use protocol::firmware::{Firmware, Release, catalog_models, catalog_url, parse_catalog};
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

const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const TIMEOUT: Duration = Duration::from_secs(30);

/// Owned by the loop; fetches report back as [`Input::Catalog`].
pub struct Catalog {
    client: ureq::Agent,
    /// By product (its models, as asked).
    checked: HashMap<Vec<String>, Checked>,
}

struct Checked {
    at: Instant,
    /// Kept from an earlier check when the latest one failed.
    release: Option<Release>,
}

impl Catalog {
    pub fn new() -> Self {
        let client = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .build()
            .into();
        Self {
            client,
            checked: HashMap::new(),
        }
    }

    /// Ask for `model`'s product unless that was done within a day.
    pub fn check(&mut self, model: &str, inputs: &mpsc::UnboundedSender<Input>, now: Instant) {
        let models = catalog_models(model);
        match self.checked.get_mut(&models) {
            Some(checked) if now.duration_since(checked.at) < CHECK_INTERVAL => return,
            Some(checked) => checked.at = now,
            None => {
                let checked = Checked {
                    at: now,
                    release: None,
                };
                self.checked.insert(models.clone(), checked);
            }
        }
        let (client, inputs) = (self.client.clone(), inputs.clone());
        tokio::task::spawn_blocking(move || {
            let release = fetch(&client, &models);
            // The loop is gone only when the daemon stops.
            let _ = inputs.send(Input::Catalog(models, release));
        });
    }

    /// A check's result; `None` keeps what an earlier one found.
    pub fn found(&mut self, models: Vec<String>, release: Option<Release>) {
        if let Some(checked) = self.checked.get_mut(&models) {
            checked.release = release.or(checked.release.take());
        }
    }

    pub fn latest(&self, model: &str) -> Option<&Release> {
        self.checked.get(&catalog_models(model))?.release.as_ref()
    }
}

/// The first of `models` the catalog answers for (it refuses some).
fn fetch(client: &ureq::Agent, models: &[String]) -> Option<Release> {
    for model in models {
        match fetch_one(client, model) {
            Ok(Some(release)) => {
                tracing::info!(%model, %release, "firmware catalog");
                return Some(release);
            }
            Ok(None) => tracing::debug!(%model, "firmware catalog: no release in the answer"),
            Err(e) => tracing::debug!(%model, "firmware catalog: {e}"),
        }
    }
    tracing::info!(?models, "firmware catalog: no answer");
    None
}

fn fetch_one(client: &ureq::Agent, model: &str) -> Result<Option<Release>, ureq::Error> {
    let body = client
        .get(catalog_url(model))
        .call()?
        .body_mut()
        .read_to_vec()?;
    Ok(parse_catalog(&body))
}
