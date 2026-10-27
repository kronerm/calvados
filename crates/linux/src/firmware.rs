//! Apple's firmware catalog: the current release for the pods' product,
//! asked at most once a day, off the loop. Only tells; never updates.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use protocol::firmware::{Release, catalog_models, catalog_url, parse_catalog};
use tokio::sync::mpsc;

use crate::Input;

const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const TIMEOUT: Duration = Duration::from_secs(30);

/// Owned by the loop; fetches report back as [`Input::Catalog`].
pub struct Catalog {
    /// `None` when the client couldn't be built: no checks.
    client: Option<reqwest::Client>,
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
        // Already installed is fine.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .build()
            .inspect_err(|e| tracing::info!("no firmware checks: {e}"))
            .ok();
        Self {
            client,
            checked: HashMap::new(),
        }
    }

    /// Ask for `model`'s product unless that was done within a day.
    pub fn check(&mut self, model: &str, inputs: &mpsc::UnboundedSender<Input>, now: Instant) {
        let Some(client) = &self.client else {
            return;
        };
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
        let (client, inputs) = (client.clone(), inputs.clone());
        tokio::spawn(async move {
            let release = fetch(&client, &models).await;
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
async fn fetch(client: &reqwest::Client, models: &[String]) -> Option<Release> {
    for model in models {
        match fetch_one(client, model).await {
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

async fn fetch_one(client: &reqwest::Client, model: &str) -> reqwest::Result<Option<Release>> {
    let body = client
        .get(catalog_url(model))
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    Ok(parse_catalog(&body))
}
