//! Thin integration locks for cookbook / DX surfaces that are not covered by WPT.
//!
//! Covers `make_relative`, value-aware `SearchParams`, and optional `serde` href
//! round-trips so README/cookbook snippets stay honest.

use sorug::{SearchParams, Url};

#[test]
fn make_relative_cookbook_examples() {
    let base = Url::parse("https://example.com/dir/page").unwrap();
    let target = Url::parse("https://example.com/dir/x").unwrap();
    assert_eq!(base.make_relative(&target).as_deref(), Some("x"));

    let joined = base.join("../other").unwrap();
    assert_eq!(joined.as_str(), "https://example.com/other");
    assert_eq!(base.make_relative(&joined).as_deref(), Some("../other"));

    let other_host = Url::parse("https://other.example/dir/x").unwrap();
    assert!(base.make_relative(&other_host).is_none());
}

#[test]
fn search_params_value_aware_has_delete_size() {
    let mut params = SearchParams::parse("q=1&lang=tr&q=2");
    assert_eq!(params.size(), 3);
    assert!(params.has("q"));
    assert!(params.has_value("q", "1"));
    assert!(!params.has_value("q", "missing"));

    assert!(params.delete_value("q", "1"));
    assert_eq!(params.size(), 2);
    assert!(params.has_value("q", "2"));
    assert!(!params.has_value("q", "1"));

    params.delete("lang");
    assert_eq!(params.size(), 1);
    assert_eq!(params.serialize(), "q=2");
}

#[test]
fn search_params_round_trip_via_url() {
    let mut url = Url::parse("https://example.com/old").unwrap().into_owned();
    url.set_pathname("/api/v1");
    url.set_search("?q=1");

    let mut params = url.search_params();
    params.append("lang", "tr");
    url.set_search_params(&params);
    assert_eq!(url.search(), "?q=1&lang=tr");
    assert_eq!(url.href(), "https://example.com/api/v1?q=1&lang=tr");
}

#[cfg(feature = "serde")]
#[test]
fn serde_href_round_trip() {
    let url = Url::parse("https://user:pass@example.com:8443/a?b=1#c")
        .unwrap()
        .into_owned();
    let json = serde_json::to_string(&url).expect("serialize");
    assert_eq!(json, "\"https://user:pass@example.com:8443/a?b=1#c\"");
    let back: Url<'static> = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.as_str(), url.as_str());
}
