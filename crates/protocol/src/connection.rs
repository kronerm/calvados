//! AACP L2CAP channel identifiers.

pub const AIRPODS_PSM: u16 = 0x1001;
pub const AIRPODS_UUID: &str = "74ec2172-0bad-4d01-8f77-997b2be0722a";

/// Whether a paired device's name is AirPods' (`AirPods Pro`, any case).
pub fn is_airpods_name(name: &str) -> bool {
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
    }
}
