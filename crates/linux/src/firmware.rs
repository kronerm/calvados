//! The pods' firmware: what they last reported, stored per pair; Apple's
//! firmware catalog: the current release for the pods' product, asked at
//! most once a day, off the loop; and the release's package for an update,
//! fetched once and cached.

use std::collections::HashMap;
use std::io::Read as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use protocol::MacAddress;
use protocol::firmware::{Firmware, Release, catalog_models, catalog_url, parse_catalog};
use protocol::superbinary::Package;
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
/// For the package: 11 MB zipped for 9A348.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const DOWNLOAD_LIMIT: u64 = 256 * 1024 * 1024;
/// The SuperBinary inside the asset's zip.
const PACKAGE_NAME: &str = "AssetData/Firmware.acsw/firmware.uarp";

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

    /// `release`'s package, from the cache or downloaded, checked; reported
    /// as [`Input::Package`] (`None` when it couldn't be had).
    pub fn fetch_package(&self, release: Release, inputs: &mpsc::UnboundedSender<Input>) {
        let (client, inputs) = (self.client.clone(), inputs.clone());
        tokio::task::spawn_blocking(move || {
            let package = package(&client, &release)
                .inspect(|package| tracing::info!(?package, "firmware package ready"))
                .inspect_err(|e| tracing::warn!(%release, "firmware package: {e:#}"))
                .ok();
            let _ = inputs.send(Input::Package(package));
        });
    }
}

/// `$XDG_CACHE_HOME/calvados/<build>.uarp` (else under `~/.cache`).
fn cached(build: &str) -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .map(|dir| dir.join("calvados").join(format!("{build}.uarp")))
}

/// The cached package if it still checks out, else downloaded and cached.
fn package(client: &ureq::Agent, release: &Release) -> anyhow::Result<Package> {
    let path = cached(&release.build).context("no cache directory")?;
    let from_cache = std::fs::read(&path)
        .ok()
        .and_then(|bytes| Package::parse(bytes).ok())
        .filter(|package| package.version() == release.version);
    if let Some(package) = from_cache {
        tracing::info!(path = %path.display(), "firmware package from the cache");
        return Ok(package);
    }
    let url = release
        .package
        .as_deref()
        .context("the catalog names no package")?;
    tracing::info!(%url, "downloading the firmware package");
    let zip = client
        .get(url)
        .config()
        .timeout_global(Some(DOWNLOAD_TIMEOUT))
        .build()
        .call()?
        .body_mut()
        .with_config()
        .limit(DOWNLOAD_LIMIT)
        .read_to_vec()?;
    let bytes = unzip(&zip)?;
    let package = Package::parse(bytes.clone())?;
    anyhow::ensure!(
        package.version() == release.version,
        "the package is {}, not {}",
        package.version(),
        release.version
    );
    if let Err(e) = store(&path, &bytes) {
        tracing::warn!(path = %path.display(), "caching the firmware package: {e}");
    }
    Ok(package)
}

fn unzip(zip: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip))?;
    let mut file = archive.by_name(PACKAGE_NAME)?;
    let mut bytes = Vec::with_capacity(usize::try_from(file.size()).unwrap_or(0));
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Written aside, then renamed: never a half package under the name.
fn store(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let partial = path.with_extension("part");
    path.parent().map_or(Ok(()), std::fs::create_dir_all)?;
    std::fs::write(&partial, bytes)?;
    std::fs::rename(&partial, path)
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
