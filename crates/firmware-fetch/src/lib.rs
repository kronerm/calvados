//! Apple's firmware catalog and the release's package, shared by both
//! platforms (`protocol` stays sans-IO): [`Catalog`] remembers what each
//! product's last check found and when, [`Client`] does the blocking
//! requests, on whatever thread the platform runs them.

use std::collections::HashMap;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use protocol::firmware::{BuildId, Release};
use protocol::superbinary::Package;

mod catalog;

const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const TIMEOUT: Duration = Duration::from_secs(30);
/// For the package: 11 MB zipped for 9A348.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const DOWNLOAD_LIMIT: u64 = 256 * 1024 * 1024;
/// The SuperBinary inside the asset's zip.
const PACKAGE_NAME: &str = "AssetData/Firmware.acsw/firmware.uarp";
/// Its build's `SourceVersion` and `BuildVersion`, as the pods report them.
const VERSION_NAME: &str = "AssetData/Firmware.acsw/version.plist";

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
        let models = catalog::models(model);
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
        self.checked.get(&catalog::models(model))?.release.as_ref()
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
            .get(catalog::url(model))
            .call()?
            .body_mut()
            .read_to_vec()?;
        Ok(catalog::parse(&body).map(|mut release| {
            release.id = release.package.as_deref().and_then(|url| {
                self.build_id(url)
                    .inspect_err(|e| tracing::info!(%url, "the package's build: {e:#}"))
                    .ok()
            });
            release
        }))
    }

    /// The package's build from its `version.plist`, read with range
    /// requests: the zip's directory and that file, a few KB of 11 MB.
    fn build_id(&self, url: &str) -> anyhow::Result<BuildId> {
        let length = self
            .0
            .head(url)
            .call()?
            .headers()
            .get("content-length")
            .and_then(|length| length.to_str().ok()?.parse().ok())
            .context("no length")?;
        let zip = io::BufReader::with_capacity(
            64 * 1024,
            Ranges {
                agent: &self.0,
                url,
                length,
                at: 0,
            },
        );
        let mut archive = zip::ZipArchive::new(zip)?;
        let mut xml = Vec::new();
        archive
            .by_name(VERSION_NAME)?
            .take(64 * 1024)
            .read_to_end(&mut xml)?;
        let plist = plist::Value::from_reader_xml(xml.as_slice())?;
        let text = |key| plist.as_dictionary()?.get(key)?.as_string();
        let (source, build) = text("SourceVersion")
            .zip(text("BuildVersion"))
            .context("no SourceVersion or BuildVersion")?;
        Ok(BuildId::new(source, build))
    }

    /// `release`'s package from [`Self::package`], logged; `None` when it
    /// couldn't be had.
    pub fn fetch(&self, release: &Release, cache: Option<&Path>) -> Option<Package> {
        cache
            .context("no cache directory")
            .and_then(|cache| self.package(release, cache))
            .inspect(|package| tracing::info!(?package, "firmware package ready"))
            .inspect_err(|e| tracing::warn!(%release, "firmware package: {e:#}"))
            .ok()
    }

    /// `release`'s package: `<cache>/<build>.uarp` if it still checks out,
    /// else downloaded, checked and cached there.
    fn package(&self, release: &Release, cache: &Path) -> anyhow::Result<Package> {
        let path = cache.join(format!("{}.uarp", release.build));
        let from_cache = std::fs::read(&path)
            .ok()
            .and_then(|bytes| checked(bytes, release).ok());
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
        let package = checked(unzip(&zip)?, release)?;
        if let Err(e) = store(&path, package.bytes()) {
            tracing::warn!(path = %path.display(), "caching the firmware package: {e}");
        }
        Ok(package)
    }
}

/// A remote file read with HTTP range requests.
struct Ranges<'a> {
    agent: &'a ureq::Agent,
    url: &'a str,
    length: u64,
    at: u64,
}

impl Read for Ranges<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.at >= self.length || buf.is_empty() {
            return Ok(0);
        }
        let end = (self.at + buf.len() as u64).min(self.length) - 1;
        let bytes = self
            .agent
            .get(self.url)
            .header("Range", &format!("bytes={}-{end}", self.at))
            .call()
            .and_then(|mut response| {
                partial(response.status().as_u16())?;
                response.body_mut().read_to_vec()
            })
            .map_err(io::Error::other)?;
        let read = bytes.len().min(buf.len());
        buf[..read].copy_from_slice(&bytes[..read]);
        self.at += read as u64;
        Ok(read)
    }
}

/// Only a partial answer: a whole file would be read into one buffer.
fn partial(status: u16) -> Result<(), ureq::Error> {
    match status {
        206 => Ok(()),
        _ => Err(ureq::Error::StatusCode(status)),
    }
}

impl Seek for Ranges<'_> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let at = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(by) => self.length.checked_add_signed(by),
            SeekFrom::Current(by) => self.at.checked_add_signed(by),
        };
        self.at = at.ok_or_else(|| io::Error::other("seek out of range"))?;
        Ok(self.at)
    }
}

/// `bytes` as `release`'s package, checked.
fn checked(bytes: Vec<u8>, release: &Release) -> anyhow::Result<Package> {
    let package = Package::parse(bytes)?;
    anyhow::ensure!(
        package.version() == release.version,
        "the package is {}, not {}",
        package.version(),
        release.version
    );
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
fn store(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let partial = path.with_extension("part");
    path.parent().map_or(Ok(()), std::fs::create_dir_all)?;
    std::fs::write(&partial, bytes)?;
    std::fs::rename(&partial, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against Apple's servers: `cargo test -p firmware-fetch -- --ignored`.
    #[test]
    #[ignore]
    fn the_live_release_names_its_build() {
        let release = Client::default().release(&["A3048".into()]).unwrap();
        let id = release.id.clone().expect("read from version.plist");
        println!("{release}: {id:?}");
    }

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
