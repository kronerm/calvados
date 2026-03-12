use std::{fmt, ops::Deref, str::FromStr};

/// [`eui48::MacAddress`] whose `Debug` is just the address, for packet logs.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct MacAddress(eui48::MacAddress);

impl MacAddress {
    pub const fn new(bytes: [u8; 6]) -> Self {
        Self(eui48::MacAddress::new(bytes))
    }
}

impl Deref for MacAddress {
    type Target = eui48::MacAddress;

    fn deref(&self) -> &eui48::MacAddress {
        &self.0
    }
}

impl FromStr for MacAddress {
    type Err = eui48::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}

impl fmt::Display for MacAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::Debug for MacAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_is_the_address() {
        let mac: MacAddress = "02:66:77:88:99:aa".parse().unwrap();
        assert_eq!(format!("{:?}", Some(mac)), "Some(02:66:77:88:99:aa)");
    }
}
