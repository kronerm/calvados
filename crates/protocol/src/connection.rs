//! AACP L2CAP channel identifiers.

pub const AIRPODS_PSM: u16 = 0x1001;
pub const AIRPODS_UUID: &str = "74ec2172-0bad-4d01-8f77-997b2be0722a";

/// Whether a paired device is AirPods: it offers [`AIRPODS_UUID`], which
/// survives a rename; by its name (`AirPods…`, any case) only while its
/// services aren't known.
pub fn is_airpods<S: AsRef<str>>(name: Option<&str>, uuids: &[S]) -> bool {
    match uuids {
        [] => name.is_some_and(is_airpods_name),
        uuids => uuids
            .iter()
            .any(|uuid| uuid.as_ref().eq_ignore_ascii_case(AIRPODS_UUID)),
    }
}

fn is_airpods_name(name: &str) -> bool {
    name.get(..6)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("airpod"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn airpods_by_name() {
        assert!(is_airpods_name("AirPods Pro"));
        assert!(is_airpods_name("airpods"));
        assert!(!is_airpods_name("Martin's AirPods"));
        assert!(!is_airpods_name("Air"));
        assert!(!is_airpods_name("Beats"));
        let none: [&str; 0] = [];
        assert!(is_airpods(Some("AirPods Pro"), &none));
        assert!(is_airpods(
            Some("Martin's"),
            &[
                "0000110b-0000-1000-8000-00805f9b34fb",
                "74EC2172-0BAD-4D01-8F77-997B2BE0722A"
            ]
        ));
        assert!(!is_airpods(
            Some("AirPods (fake)"),
            &["0000110b-0000-1000-8000-00805f9b34fb"]
        ));
    }
}
