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
    pub version: Version,
    pub release: u32,
}

impl fmt::Display for ExactVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.version, self.release)
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
                version: Version { major, minor },
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
    /// From UARP (Linux).
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
            .map(|exact| exact.version)
            .or_else(|| Version::of_running(self.reported.as_deref()?))
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
}

impl Release {
    /// Newer than what runs: exactly when UARP told, else by major.minor
    /// (the `0x1D` string carries no release number).
    pub fn is_newer_than(&self, running: &Firmware) -> bool {
        match (running.exact, running.version()) {
            (Some(exact), _) => self.version > exact,
            (None, Some(version)) => self.version.version > version,
            (None, None) => false,
        }
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
