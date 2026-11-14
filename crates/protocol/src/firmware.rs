//! The pods' firmware: the running version (AACP `0x1D`, and exactly over
//! [`crate::uarp`]) and the current release in Apple's catalog (fetched and
//! parsed by `firmware-fetch`).

use std::fmt;
use std::str::FromStr;

/// A firmware's major.minor: all the running version string tells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
}

impl Version {
    /// From the pods' `<major><minor>.<SourceVersion>.<BuildVersion>`
    /// (`71.2256000021000000.6663`: 7.1); the minor is the first field's
    /// last digit.
    pub fn of_running(raw: &str) -> Option<Self> {
        let first = raw.split('.').next()?;
        let (major, minor) = first.split_at_checked(first.len().checked_sub(1)?)?;
        Some(Self {
            major: digits(major)?,
            minor: digits(minor)?,
        })
    }
}

/// A build's identity, `<SourceVersion>.<BuildVersion>`: the pods' running
/// string past its first field (`3431046004000000.29` for 9B47), and what
/// the release's package says in its `version.plist`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildId(String);

impl BuildId {
    pub fn new(source: &str, build: &str) -> Self {
        Self(format!("{source}.{build}"))
    }

    /// From the pods' running string; `None` without both fields.
    pub fn of_running(raw: &str) -> Option<Self> {
        let (_, rest) = raw.split_once('.')?;
        rest.contains('.').then(|| Self(rest.to_owned()))
    }
}

/// A non-empty run of ASCII digits (`parse` alone takes a sign).
fn digits(s: &str) -> Option<u32> {
    s.bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| s.parse().ok())
        .flatten()
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// major.minor.release, e.g. `7.1.21` (7B21): what UARP reports and the
/// catalog names. Several releases share a major.minor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExactVersion {
    pub major: u32,
    pub minor: u32,
    pub release: u32,
}

impl ExactVersion {
    pub fn major_minor(self) -> Version {
        Version {
            major: self.major,
            minor: self.minor,
        }
    }

    /// Major, minor and release as u32 BE, as UARP and `News` carry it.
    pub fn to_be_bytes(self) -> [u8; 12] {
        let words = [self.major, self.minor, self.release].map(u32::to_be_bytes);
        let mut bytes = [0; 12];
        bytes.copy_from_slice(words.as_flattened());
        bytes
    }

    pub fn from_be_bytes(bytes: &[u8; 12]) -> Self {
        let [major, minor, release] =
            [0, 4, 8].map(|at| u32::from_be_bytes(*bytes[at..].first_chunk().expect("12 bytes")));
        Self {
            major,
            minor,
            release,
        }
    }
}

impl fmt::Display for ExactVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major_minor(), self.release)
    }
}

impl FromStr for ExactVersion {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        match s
            .split('.')
            .map(digits)
            .collect::<Option<Vec<_>>>()
            .ok_or(())?[..]
        {
            [major, minor, release] => Ok(Self {
                major,
                minor,
                release,
            }),
            _ => Err(()),
        }
    }
}

/// What the pods run, as far as known: remembered per pair, so a session
/// without `0x1D` (one joining while another host is connected often gets
/// none) still shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Firmware {
    /// The primary bud's model number, e.g. `A3048`: the catalog's key.
    pub model: Option<String>,
    /// As `0x1D` reports it: see [`Version::of_running`].
    pub reported: Option<String>,
    /// From UARP, or the release whose [`BuildId`] it reports.
    pub exact: Option<ExactVersion>,
}

impl Firmware {
    /// A `0x1D` report: it wins; the exact version holds unless the
    /// reported one changed (`true`: the pods were updated).
    pub fn reported(&mut self, model: Option<String>, reported: Option<String>) -> bool {
        let changed = self
            .reported
            .as_ref()
            .is_some_and(|known| reported.as_ref().is_some_and(|new| new != known));
        if changed {
            self.exact = None;
        }
        self.model = model.or(self.model.take());
        self.reported = reported.or(self.reported.take());
        changed
    }

    /// The major.minor, exact or reported.
    pub fn version(&self) -> Option<Version> {
        self.exact
            .map(ExactVersion::major_minor)
            .or_else(|| Version::of_running(self.reported.as_deref()?))
    }

    /// The reported build is `release`'s.
    pub fn runs(&self, release: &Release) -> bool {
        let running = self.reported.as_deref().and_then(BuildId::of_running);
        running.is_some() && running == release.id
    }

    /// Whether `release` is newer: exactly when known; else another build
    /// of the same or an older major.minor (`0x1D` carries no release
    /// number; the pods refuse what isn't newer anyway); else by
    /// major.minor. `None` when unknown.
    pub fn older_than(&self, release: &Release) -> Option<bool> {
        let latest = release.version.major_minor();
        let reported = self.reported.as_deref().and_then(BuildId::of_running);
        match (self.exact, reported, &release.id) {
            (Some(exact), ..) => Some(exact < release.version),
            (None, Some(running), Some(id)) => {
                Some(running != *id && self.version().is_some_and(|version| version <= latest))
            }
            (None, ..) => self.version().map(|running| running < latest),
        }
    }
}

/// The catalog's current release.
#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub struct Release {
    pub version: ExactVersion,
    /// E.g. `9A348`.
    pub build: String,
    /// The asset's zip (`AssetData/Firmware.acsw/firmware.uarp` inside).
    pub package: Option<String>,
    /// From the package's `version.plist`, when it could be read.
    pub id: Option<BuildId>,
}

impl Release {
    /// Newer than what runs ([`Firmware::older_than`]); not while unknown.
    pub fn is_newer_than(&self, running: &Firmware) -> bool {
        running.older_than(self).unwrap_or(false)
    }
}

/// `9.0.348 (9A348)`.
impl fmt::Display for Release {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.version, self.build)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_versions() {
        let version = |raw| Version::of_running(raw).map(|v| v.to_string());
        assert_eq!(version("71.2256000021000000.6663").as_deref(), Some("7.1"));
        assert_eq!(version("90.3431000025000000.6807").as_deref(), Some("9.0"));
        assert_eq!(version("100.1.2").as_deref(), Some("10.0"));
        assert_eq!(version("7"), None);
        assert_eq!(version(""), None);
        assert_eq!(version("+1.2"), None);
        assert_eq!(version("6A300"), None);
    }

    #[test]
    fn exact_versions() {
        let exact = "7.1.21".parse::<ExactVersion>().unwrap();
        assert_eq!(exact.to_string(), "7.1.21");
        assert!(exact < "7.1.22".parse().unwrap() && exact > "7.0.99".parse().unwrap());
        assert_eq!("7.1".parse::<ExactVersion>(), Err(()));
        assert_eq!("7.1.x".parse::<ExactVersion>(), Err(()));
    }

    #[test]
    fn updates_compare_exactly_when_known() {
        let release = Release {
            version: "7.1.22".parse().unwrap(),
            build: "7B22".into(),
            package: None,
            id: None,
        };
        let mut running = Firmware {
            reported: Some("71.2256000021000000.6663".into()),
            ..Firmware::default()
        };
        assert!(!release.is_newer_than(&running), "7.1 is all 0x1D tells");
        running.exact = "7.1.21".parse().ok();
        assert!(release.is_newer_than(&running));
        running.exact = "7.1.22".parse().ok();
        assert!(!release.is_newer_than(&running));
        assert!(!release.is_newer_than(&Firmware::default()));
    }

    #[test]
    fn the_reported_build_identifies_the_release() {
        let release = Release {
            version: "9.1.47".parse().unwrap(),
            build: "9B47".into(),
            package: None,
            id: Some(BuildId::new("3431046004000000", "29")),
        };
        let running = |raw: &str| Firmware {
            reported: Some(raw.into()),
            ..Firmware::default()
        };
        let latest = running("91.3431046004000000.29");
        assert!(latest.runs(&release));
        assert_eq!(latest.older_than(&release), Some(false));
        let earlier = running("91.3431040000000000.12");
        assert!(!earlier.runs(&release));
        assert_eq!(
            earlier.older_than(&release),
            Some(true),
            "same 9.1, other build"
        );
        assert_eq!(
            running("90.3431000025000000.6807").older_than(&release),
            Some(true)
        );
        assert_eq!(
            running("100.1.2").older_than(&release),
            Some(false),
            "a newer major"
        );
        assert_eq!(BuildId::of_running("91"), None);
        assert_eq!(BuildId::of_running("91.3431"), None);
    }

    #[test]
    fn a_report_drops_the_exact_version_only_when_it_changed() {
        let raw = "71.2256000021000000.6663";
        let mut firmware = Firmware {
            exact: "7.1.21".parse().ok(),
            ..Firmware::default()
        };
        firmware.reported(Some("A3048".into()), Some(raw.into()));
        assert_eq!(firmware.exact, "7.1.21".parse().ok());
        firmware.reported(None, None);
        assert_eq!(firmware.reported.as_deref(), Some(raw));
        assert_eq!(firmware.model.as_deref(), Some("A3048"));
        firmware.reported(None, Some("90.3431000025000000.6807".into()));
        assert_eq!(firmware.exact, None);
    }
}
