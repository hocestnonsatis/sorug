//! Special-scheme classification and default ports (WHATWG).

/// WHATWG [special scheme](https://url.spec.whatwg.org/#special-scheme) bucket.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) enum SchemeType {
    File,
    SpecialNotFile,
    NotSpecial,
}

impl SchemeType {
    #[inline]
    pub(crate) fn is_special(self) -> bool {
        !matches!(self, Self::NotSpecial)
    }

    #[inline]
    pub(crate) fn is_file(self) -> bool {
        matches!(self, Self::File)
    }
}

impl From<&str> for SchemeType {
    fn from(s: &str) -> Self {
        match s {
            "http" | "https" | "ws" | "wss" | "ftp" => Self::SpecialNotFile,
            "file" => Self::File,
            _ => Self::NotSpecial,
        }
    }
}

#[inline]
fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "http" | "ws" => Some(80),
        "https" | "wss" => Some(443),
        "ftp" => Some(21),
        _ => None,
    }
}

/// Public crate helper for origin serialization / setters.
#[inline]
pub(crate) fn default_port_for_scheme(scheme: &str) -> Option<u16> {
    default_port(scheme)
}
