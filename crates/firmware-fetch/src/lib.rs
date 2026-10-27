//! Apple's firmware catalog and the release's package, shared by both
//! platforms (`protocol` stays sans-IO): [`Catalog`] remembers what each
//! product's last check found and when, [`Client`] does the blocking
//! requests, on whatever thread the platform runs them.

use std::collections::HashMap;
use std::io::Read as _;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use protocol::firmware::{Release, catalog_models, catalog_url, parse_catalog};
use protocol::superbinary::Package;

const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const TIMEOUT: Duration = Duration::from_secs(30);
/// For the package: 11 MB zipped for 9A348.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const DOWNLOAD_LIMIT: u64 = 256 * 1024 * 1024;
/// The SuperBinary inside the asset's zip.
const PACKAGE_NAME: &str = "AssetData/Firmware.acsw/firmware.uarp";

/// The checks so far, by product (its models, as asked); owned by the
/// platform's loop.
#[derive(Default)]
pub struct Catalog {
    checked: HashMap<Vec<String>, Checked>,
}

struct Checked {
    at: Instant,
    /// Kept from an earlier check when the latest one failed.
    release: Option<Release>,
}

impl Catalog {
    /// The models to ask for (with [`Client::release`]) when `model`'s
    /// product wasn't checked within a day; counted as checked from `now`.
    pub fn due(&mut self, model: &str, now: Instant) -> Option<Vec<String>> {
        let models = catalog_models(model);
        match self.checked.get_mut(&models) {
            Some(checked) if now.duration_since(checked.at) < CHECK_INTERVAL => return None,
            Some(checked) => checked.at = now,
            None => {
                let checked = Checked {
                    at: now,
                    release: None,
                };
                self.checked.insert(models.clone(), checked);
            }
        }
        Some(models)
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

/// Blocking HTTPS (rustls, ring, bundled roots); cheap to clone into a
/// worker thread.
#[derive(Clone)]
pub struct Client(ureq::Agent);

impl Default for Client {
    fn default() -> Self {
        Self(
            ureq::Agent::config_builder()
                .timeout_global(Some(TIMEOUT))
                .build()
                .into(),
        )
    }
}

impl Client {
    /// The current release from the first of `models` the catalog answers
    /// for (it refuses some).
    pub fn release(&self, models: &[String]) -> Option<Release> {
        for model in models {
            match self.release_of(model) {
                Ok(Some(release)) => {
                    tracing::info!(%model, %release, "firmware catalog");
                    return Some(release);
                }
                Ok(None) => tracing::info!(%model, "firmware catalog: no release in the answer"),
                Err(e) => tracing::info!(%model, "firmware catalog: {e:#}"),
            }
        }
        tracing::info!(?models, "firmware catalog: no answer");
        None
    }

    fn release_of(&self, model: &str) -> Result<Option<Release>, ureq::Error> {
        let body = self
            .0
            .get(catalog_url(model))
            .call()?
            .body_mut()
            .read_to_vec()?;
        Ok(parse_catalog(&body))
    }

    /// `release`'s package: `<cache>/<build>.uarp` if it still checks out,
    /// else downloaded, checked and cached there.
    pub fn package(&self, release: &Release, cache: &Path) -> anyhow::Result<Package> {
        let path = cache.join(format!("{}.uarp", release.build));
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
        let zip = self
            .0
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
}

fn unzip(zip: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip))?;
    let mut file = archive.by_name(PACKAGE_NAME)?;
    let mut bytes = Vec::with_capacity(usize::try_from(file.size()).unwrap_or(0));
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Written aside, then renamed: never a half package under the name.
fn store(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let partial = path.with_extension("part");
    path.parent().map_or(Ok(()), std::fs::create_dir_all)?;
    std::fs::write(&partial, bytes)?;
    std::fs::rename(&partial, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_product_is_checked_once_a_day() {
        let mut catalog = Catalog::default();
        let now = Instant::now();
        let models = catalog.due("A3047", now).unwrap();
        assert_eq!(models, ["A3048", "A2968", "A3047"]);
        // Same product, other model.
        assert!(
            catalog
                .due("A3048", now + Duration::from_secs(60))
                .is_none()
        );
        assert!(catalog.latest("A3048").is_none());
        assert!(catalog.due("A3048", now + CHECK_INTERVAL).is_some());
    }
}
