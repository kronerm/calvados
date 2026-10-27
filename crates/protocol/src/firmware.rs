//! The pods' firmware: the running version (AACP `0x1D`) and, with the
//! `catalog` feature, the current release in Apple's public catalog
//! (fetched by the platform).

use std::fmt;

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

/// The catalog's current release.
#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub struct Release {
    pub version: Version,
    pub release: u32,
    /// E.g. `9A348`.
    pub build: String,
}

impl Release {
    /// Newer than the running version: the running string carries no
    /// release number, so only major.minor compare.
    pub fn is_newer_than(&self, running: Version) -> bool {
        self.version > running
    }
}

/// `9.0.348 (9A348)`.
impl fmt::Display for Release {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{} ({})", self.version, self.release, self.build)
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
                version: Version {
                    major: number("FirmwareVersionMajor")?,
                    minor: number("FirmwareVersionMinor")?,
                },
                release: number("FirmwareVersionRelease")?,
                build: asset.get("Build")?.as_string()?.to_owned(),
            })
        })
        .max_by_key(|release| (release.version, release.release))
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
        let running = |raw| Version::of_running(raw).unwrap();
        assert!(release.is_newer_than(running("71.2256000021000000.6663")));
        assert!(!release.is_newer_than(running("90.3431000025000000.6807")));
        assert_eq!(parse_catalog(b"<html>Forbidden</html>"), None);
        assert_eq!(
            parse_catalog(
                br#"<plist version="1.0"><dict><key>Assets</key><array/></dict></plist>"#
            ),
            None
        );
    }
}
