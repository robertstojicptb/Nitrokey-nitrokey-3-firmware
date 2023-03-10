use core::fmt::{self, Display, Formatter};

pub const VERSION: Version = Version::from_env();
pub const VERSION_STRING: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Version {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
    pub pre: Option<&'static str>,
}

impl Version {
    pub const fn from_env() -> Self {
        Self {
            major: parse_simple_u8(env!("CARGO_PKG_VERSION_MAJOR")),
            minor: parse_simple_u8(env!("CARGO_PKG_VERSION_MINOR")),
            patch: parse_simple_u8(env!("CARGO_PKG_VERSION_PATCH")),
            pre: optional_str(env!("CARGO_PKG_VERSION_PRE")),
        }
    }

    pub const fn encode(&self) -> u32 {
        if self.patch >= 64 {
            panic!("patch version must not be larger than 63");
        }
        ((self.major as u32) << 22) | ((self.minor as u32) << 6) | (self.patch as u32)
    }

    pub const fn usb_release(&self) -> u16 {
        u16::from_be_bytes([self.major, self.minor])
    }
}

impl Display for Version {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "v{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(pre) = self.pre {
            write!(f, "-{pre}")?;
        }
        Ok(())
    }
}

const fn parse_simple_u8(s: &str) -> u8 {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        panic!("number may not be empty");
    }
    let mut value = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] < b'0' || bytes[i] > b'9' {
            panic!("number must only contain ASCII digits");
        }
        value *= 10;
        value += bytes[i] - b'0';
        i += 1;
    }
    value
}

const fn optional_str(s: &str) -> Option<&str> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    quickcheck::quickcheck! {
        fn test_parse_simple_u8(value: u8) -> bool {
            parse_simple_u8(&value.to_string()) == value
        }
    }
}
