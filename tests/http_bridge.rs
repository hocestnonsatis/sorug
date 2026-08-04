//! Integration tests for the optional `http` feature (`Url` ↔ `http::Uri`).

#![cfg(feature = "http")]

use http::Uri;
use sorug::{Url, uri_to_url};

#[test]
fn positive_https_with_query() {
    let url = Url::parse("https://example.com/api/v1?q=1").unwrap();
    let uri = url.to_uri().unwrap();
    assert_eq!(uri.scheme_str(), Some("https"));
    assert_eq!(uri.host(), Some("example.com"));
    assert_eq!(uri.path(), "/api/v1");
    assert_eq!(uri.query(), Some("q=1"));
    assert_eq!(uri_to_url(&uri).unwrap().as_str(), url.as_str());
}

#[test]
fn positive_credentials_ipv6_idna_ws() {
    let creds = Url::parse("https://u:p@example.com:9443/x").unwrap();
    assert_eq!(
        creds.to_uri().unwrap().to_string(),
        "https://u:p@example.com:9443/x"
    );

    let v6 = Url::parse("http://[2001:db8::1]/").unwrap();
    let uri = v6.to_uri().unwrap();
    assert_eq!(uri.to_string(), "http://[2001:db8::1]/");
    assert_eq!(uri.host(), Some("[2001:db8::1]"));

    let idna = Url::parse("https://bücher.example/").unwrap();
    let uri = idna.to_uri().unwrap();
    assert!(uri.host().unwrap().starts_with("xn--"));

    let ws = Url::parse("wss://example.com/socket").unwrap();
    assert_eq!(ws.to_uri().unwrap().scheme_str(), Some("wss"));
}

#[test]
fn positive_try_from_both_directions() {
    let url = Url::parse("http://example.net/").unwrap();
    let uri = Uri::try_from(&url).unwrap();
    let owned = Url::try_from(uri.clone()).unwrap();
    assert_eq!(owned.as_str(), "http://example.net/");
    let uri2 = Uri::try_from(url.into_owned()).unwrap();
    assert_eq!(uri2, uri);
}

#[test]
fn positive_fragment_stripped_roundtrip_without_hash() {
    let url = Url::parse("https://example.com/path#section").unwrap();
    assert_eq!(url.fragment(), Some("section"));
    let uri = url.to_uri().unwrap();
    assert_eq!(uri.to_string(), "https://example.com/path");
    let back = uri_to_url(&uri).unwrap();
    assert_eq!(back.as_str(), "https://example.com/path");
    assert!(back.fragment().is_none());
}

#[test]
fn negative_opaque_and_file() {
    assert!(
        Url::parse("data:text/plain,hello")
            .unwrap()
            .to_uri()
            .is_err()
    );
    assert!(Url::parse("file:///etc/passwd").unwrap().to_uri().is_err());
}

#[test]
fn negative_relative_uri_to_url() {
    let uri: Uri = "/relative/path".parse().unwrap();
    assert!(Url::try_from(&uri).is_err());
    assert!(uri_to_url(&uri).is_err());
}

#[test]
fn negative_star_uri() {
    let uri: Uri = "*".parse().unwrap();
    assert!(uri_to_url(&uri).is_err());
}

#[test]
fn mailto_accepted_by_http_uri() {
    // `http::Uri` accepts mailto; round-trip through sorug stays a valid URL.
    let url = Url::parse("mailto:user@example.com").unwrap();
    let uri = url.to_uri().unwrap();
    assert_eq!(uri_to_url(&uri).unwrap().scheme(), "mailto");
}
