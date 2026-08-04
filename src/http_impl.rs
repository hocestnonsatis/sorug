//! Optional [`http`] crate bridge: convert between [`Url`] and [`http::Uri`].
//!
//! Conversions go through the serialized href / URI string (same approach as
//! typical `url` + `http` callers). Fragments are dropped when converting to
//! [`http::Uri`] (HTTP URIs have no fragment). Schemes that `http::Uri` rejects
//! (notably `file:` and many opaque URLs such as `data:`) fail with
//! [`http::uri::InvalidUri`].

use core::convert::TryFrom;

use http::Uri;
use http::uri::InvalidUri;

use crate::{ParseError, Url};

impl TryFrom<&Url<'_>> for Uri {
    type Error = InvalidUri;

    fn try_from(url: &Url<'_>) -> Result<Self, Self::Error> {
        url.as_str().parse()
    }
}

impl TryFrom<Url<'_>> for Uri {
    type Error = InvalidUri;

    fn try_from(url: Url<'_>) -> Result<Self, Self::Error> {
        Uri::try_from(&url)
    }
}

impl TryFrom<&Uri> for Url<'static> {
    type Error = ParseError;

    fn try_from(uri: &Uri) -> Result<Self, Self::Error> {
        // `Uri`'s `Display` is the canonical HTTP serialization (no fragment).
        Url::parse(&uri.to_string()).map(Url::into_owned)
    }
}

impl TryFrom<Uri> for Url<'static> {
    type Error = ParseError;

    fn try_from(uri: Uri) -> Result<Self, Self::Error> {
        Url::try_from(&uri)
    }
}

impl Url<'_> {
    /// Convert this URL to an [`http::Uri`].
    ///
    /// Equivalent to `Uri::try_from(self)`. The fragment (if any) is dropped.
    /// Returns [`InvalidUri`] for schemes / forms that the `http` crate rejects
    /// (for example `file:` and most opaque `data:` URLs).
    #[inline]
    pub fn to_uri(&self) -> Result<Uri, InvalidUri> {
        Uri::try_from(self)
    }
}

/// Parse an [`http::Uri`] into an owned [`Url`].
///
/// Relative URIs such as `/path` fail because WHATWG parsing requires a base.
#[inline]
pub fn uri_to_url(uri: &Uri) -> Result<Url<'static>, ParseError> {
    Url::try_from(uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_roundtrip() {
        let url = Url::parse("https://example.com/a?b=1").unwrap();
        let uri = url.to_uri().unwrap();
        assert_eq!(uri.to_string(), "https://example.com/a?b=1");
        let back = uri_to_url(&uri).unwrap();
        assert_eq!(back.as_str(), "https://example.com/a?b=1");
    }

    #[test]
    fn fragment_dropped() {
        let url = Url::parse("https://example.com/a#frag").unwrap();
        let uri = Uri::try_from(&url).unwrap();
        assert_eq!(uri.to_string(), "https://example.com/a");
        assert!(uri_to_url(&uri).unwrap().fragment().is_none());
    }

    #[test]
    fn credentials_and_port() {
        let url = Url::parse("https://user:pass@example.com:8443/p").unwrap();
        let uri = url.to_uri().unwrap();
        assert_eq!(uri.to_string(), "https://user:pass@example.com:8443/p");
    }

    #[test]
    fn ipv6() {
        let url = Url::parse("http://[::1]/").unwrap();
        let uri = url.to_uri().unwrap();
        assert_eq!(uri.to_string(), "http://[::1]/");
    }

    #[test]
    fn file_and_data_rejected() {
        let file = Url::parse("file:///tmp/x").unwrap();
        assert!(file.to_uri().is_err());
        let data = Url::parse("data:text/plain,hi").unwrap();
        assert!(data.to_uri().is_err());
    }

    #[test]
    fn relative_uri_fails_url_parse() {
        let uri: Uri = "/only-path".parse().unwrap();
        assert!(uri_to_url(&uri).is_err());
    }

    #[test]
    fn owned_try_from() {
        let url = Url::parse("ws://example.com/socket").unwrap().into_owned();
        let uri = Uri::try_from(url).unwrap();
        let back = Url::try_from(uri).unwrap();
        assert_eq!(back.as_str(), "ws://example.com/socket");
    }
}
