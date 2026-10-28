//! Apple's firmware catalog: where each model's is, and its current release.

use protocol::firmware::{ExactVersion, Release};

/// Models of one product: the catalog serves some and refuses others (403),
/// and the served ones carry the same build. Served first.
const PRODUCTS: &[&[&str]] = &[
    // AirPods Pro 2 (USB-C): right bud, case, left bud (403).
    &["A3048", "A2968", "A3047"],
];

/// The models to ask the catalog for, in order, for pods reporting `model`
/// (the primary bud's): its product's, or just itself when unknown.
pub fn models(model: &str) -> Vec<String> {
    PRODUCTS
        .iter()
        .find(|models| models.contains(&model))
        .map_or_else(
            || vec![model.to_owned()],
            |models| models.iter().map(|&m| m.to_owned()).collect(),
        )
}

pub fn url(model: &str) -> String {
    format!(
        "https://mesu.apple.com/assets/com_apple_MobileAsset_UARP_{model}/com_apple_MobileAsset_UARP_{model}.xml"
    )
}

/// The newest of the catalog's `Assets` (an XML plist; there's one).
pub fn parse(xml: &[u8]) -> Option<Release> {
    let catalog = plist::Value::from_reader_xml(xml).ok()?;
    catalog
        .as_dictionary()?
        .get("Assets")?
        .as_array()?
        .iter()
        .filter_map(|asset| {
            let asset = asset.as_dictionary()?;
            let number = |key| asset.get(key)?.as_unsigned_integer()?.try_into().ok();
            let text = |key| asset.get(key)?.as_string();
            Some(Release {
                package: text("__BaseURL")
                    .zip(text("__RelativePath"))
                    .map(|(base, path)| format!("{base}{path}")),
                version: ExactVersion {
                    major: number("FirmwareVersionMajor")?,
                    minor: number("FirmwareVersionMinor")?,
                    release: number("FirmwareVersionRelease")?,
                },
                build: text("Build")?.to_owned(),
            })
        })
        .max_by_key(|release| release.version)
}

#[cfg(test)]
mod tests {
    use protocol::firmware::Firmware;

    use super::*;

    #[test]
    fn a_product_asks_its_served_models_first() {
        assert_eq!(models("A3047"), ["A3048", "A2968", "A3047"]);
        assert_eq!(models("A9999"), ["A9999"]);
        assert_eq!(
            url("A3048"),
            "https://mesu.apple.com/assets/com_apple_MobileAsset_UARP_A3048/com_apple_MobileAsset_UARP_A3048.xml"
        );
    }

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
			<key>__BaseURL</key>
			<string>https://updates.cdn-apple.com/MobileAssets2026/patches/082-16653/28D7B0C9-CA9C-4B3C-A8EE-41715E2763E6/</string>
			<key>__RelativePath</key>
			<string>com_apple_MobileAsset_UARP_A3048/5d0896d881bbc7e321f89b4ada11b31cf8b5569c.zip</string>
			<key>_Measurement</key>
			<data>
			H8K8Ux39hMcp8/BfHdUxejV7SV8=
			</data>
		</dict>
	</array>
</dict>
</plist>"#;
        let release = parse(xml).unwrap();
        assert_eq!(release.to_string(), "9.0.348 (9A348)");
        assert_eq!(
            release.package.as_deref(),
            Some(
                "https://updates.cdn-apple.com/MobileAssets2026/patches/082-16653/28D7B0C9-CA9C-4B3C-A8EE-41715E2763E6/com_apple_MobileAsset_UARP_A3048/5d0896d881bbc7e321f89b4ada11b31cf8b5569c.zip"
            )
        );
        let reported = |raw: &str| Firmware {
            reported: Some(raw.into()),
            ..Firmware::default()
        };
        assert!(release.is_newer_than(&reported("71.2256000021000000.6663")));
        assert!(!release.is_newer_than(&reported("90.3431000025000000.6807")));
        assert!(parse(b"<html>Forbidden</html>").is_none());
        assert!(
            parse(br#"<plist version="1.0"><dict><key>Assets</key><array/></dict></plist>"#)
                .is_none()
        );
    }
}
