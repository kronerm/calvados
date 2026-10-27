//! The pods' firmware: the running version (AACP `0x1D`, and exactly over
//! [`crate::uarp`]) and, with the `catalog` feature, the current release in
//! Apple's public catalog (fetched by the platform).

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
    /// reported one changed.
    pub fn reported(&mut self, model: Option<String>, reported: Option<String>) {
        let changed = self
            .reported
            .as_ref()
            .is_some_and(|known| reported.as_ref().is_some_and(|new| new != known));
        if changed {
            self.exact = None;
        }
        self.model = model.or(self.model.take());
        self.reported = reported.or(self.reported.take());
    }

    /// Fill in what this session doesn't know yet from what was stored,
    /// unless it already reported another version.
    pub fn or_stored(self, stored: Self) -> Self {
        let other = self
            .reported
            .as_ref()
            .is_some_and(|known| stored.reported.as_ref() != Some(known));
        match other {
            true => self,
            false => Self {
                model: self.model.or(stored.model),
                reported: self.reported.or(stored.reported),
                exact: self.exact.or(stored.exact),
            },
        }
    }

    /// The major.minor, exact or reported.
    pub fn version(&self) -> Option<Version> {
        self.exact
            .map(|exact| exact.version)
            .or_else(|| Version::of_running(self.reported.as_deref()?))
    }

    /// For storage: `key=value` lines, those known.
    pub fn to_stored(&self) -> String {
        [
            ("model", self.model.clone()),
            ("reported", self.reported.clone()),
            ("exact", self.exact.map(|exact| exact.to_string())),
        ]
        .into_iter()
        .filter_map(|(key, value)| Some(format!("{key}={}\n", value?)))
        .collect()
    }

    /// What [`Self::to_stored`] wrote; unknown lines are skipped.
    pub fn from_stored(stored: &str) -> Self {
        stored.lines().filter_map(|line| line.split_once('=')).fold(
            Self::default(),
            |firmware, (key, value)| match key {
                "model" => Self {
                    model: Some(value.to_owned()),
                    ..firmware
                },
                "reported" => Self {
                    reported: Some(value.to_owned()),
                    ..firmware
                },
                "exact" => Self {
                    exact: value.parse().ok(),
                    ..firmware
                },
                _ => firmware,
            },
        )
    }
}

/// The catalog's current release.
#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub struct Release {
    pub version: ExactVersion,
    /// E.g. `9A348`.
    pub build: String,
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

/// Models of one product: the catalog serves some and refuses others (403),
/// and the served ones carry the same build. Served first.
const PRODUCTS: &[&[&str]] = &[
    // AirPods Pro 2 (USB-C): right bud, case, left bud (403).
    &["A3048", "A2968", "A3047"],
];

/// The models to ask the catalog for, in order, for pods reporting `model`
/// (the primary bud's): its product's, or just itself when unknown.
pub fn catalog_models(model: &str) -> Vec<String> {
    PRODUCTS
        .iter()
        .find(|models| models.contains(&model))
        .map_or_else(
            || vec![model.to_owned()],
            |models| models.iter().map(|&m| m.to_owned()).collect(),
        )
}

pub fn catalog_url(model: &str) -> String {
    format!(
        "https://mesu.apple.com/assets/com_apple_MobileAsset_UARP_{model}/com_apple_MobileAsset_UARP_{model}.xml"
    )
}

/// The newest of the catalog's `Assets` (an XML plist; there's one).
#[cfg(feature = "catalog")]
pub fn parse_catalog(xml: &[u8]) -> Option<Release> {
    let catalog = plist::Value::from_reader_xml(xml).ok()?;
    catalog
        .as_dictionary()?
        .get("Assets")?
        .as_array()?
        .iter()
        .filter_map(|asset| {
            let asset = asset.as_dictionary()?;
            let number = |key| asset.get(key)?.as_unsigned_integer()?.try_into().ok();
            Some(Release {
                version: ExactVersion {
                    version: Version {
                        major: number("FirmwareVersionMajor")?,
                        minor: number("FirmwareVersionMinor")?,
                    },
                    release: number("FirmwareVersionRelease")?,
                },
                build: asset.get("Build")?.as_string()?.to_owned(),
            })
        })
        .max_by_key(|release| release.version)
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

    #[test]
    fn the_stored_firmware_fills_in_unless_another_is_reported() {
        let stored = Firmware {
            model: Some("A3048".into()),
            reported: Some("71.2256000021000000.6663".into()),
            exact: "7.1.21".parse().ok(),
        };
        assert_eq!(Firmware::from_stored(&stored.to_stored()), stored);
        assert_eq!(Firmware::from_stored("junk\nexact=7"), Firmware::default());
        assert_eq!(Firmware::default().or_stored(stored.clone()), stored);
        let newer = Firmware {
            reported: Some("90.3431000025000000.6807".into()),
            ..Firmware::default()
        };
        assert_eq!(newer.clone().or_stored(stored), newer);
    }

    #[test]
    fn a_product_asks_its_served_models_first() {
        assert_eq!(catalog_models("A3047"), ["A3048", "A2968", "A3047"]);
        assert_eq!(catalog_models("A9999"), ["A9999"]);
        assert_eq!(
            catalog_url("A3048"),
            "https://mesu.apple.com/assets/com_apple_MobileAsset_UARP_A3048/com_apple_MobileAsset_UARP_A3048.xml"
        );
    }

    #[cfg(feature = "catalog")]
    #[test]
    fn the_catalog_names_the_current_release() {
        // Trimmed from A3048's, Oct 27 2026.
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>AssetType</key>
	<string>com.apple.MobileAsset.UARP.A3048</string>
	<key>Assets</key>
	<array>
		<dict>
			<key>Build</key>
			<string>9A348</string>
			<key>DeviceName</key>
			<string>A3048</string>
			<key>FirmwareVersionBuild</key>
			<integer>0</integer>
			<key>FirmwareVersionMajor</key>
			<integer>9</integer>
			<key>FirmwareVersionMinor</key>
			<integer>0</integer>
			<key>FirmwareVersionRelease</key>
			<integer>348</integer>
			<key>_IsZipStreamable</key>
			<true/>
			<key>_Measurement</key>
			<data>
			H8K8Ux39hMcp8/BfHdUxejV7SV8=
			</data>
		</dict>
	</array>
</dict>
</plist>"#;
        let release = parse_catalog(xml).unwrap();
        assert_eq!(release.to_string(), "9.0.348 (9A348)");
        let reported = |raw: &str| Firmware {
            reported: Some(raw.into()),
            ..Firmware::default()
        };
        assert!(release.is_newer_than(&reported("71.2256000021000000.6663")));
        assert!(!release.is_newer_than(&reported("90.3431000025000000.6807")));
        assert_eq!(parse_catalog(b"<html>Forbidden</html>"), None);
        assert_eq!(
            parse_catalog(
                br#"<plist version="1.0"><dict><key>Assets</key><array/></dict></plist>"#
            ),
            None
        );
    }
}
