//! API maturity: rust-url-shaped getters, traits, `query_pairs_mut`, `Host::parse`.

use std::collections::{BTreeSet, HashSet};

use sorug::{Host, Origin, Url};

#[test]
fn url_hash_ord_in_collections() {
    let a = Url::parse("https://example.com/a").unwrap().into_owned();
    let b = Url::parse("https://example.com/b").unwrap().into_owned();
    let mut set = HashSet::new();
    set.insert(a.clone());
    assert!(set.contains(&a));
    let mut ordered = BTreeSet::new();
    ordered.insert(b.clone());
    ordered.insert(a.clone());
    assert_eq!(ordered.iter().next().unwrap().as_str(), a.as_str());
}

#[test]
fn getters_authority_domain_port_special() {
    let url = Url::parse("https://user:pass@example.com:8443/p?q=1#f").unwrap();
    assert!(url.has_authority());
    assert_eq!(url.authority(), Some("user:pass@example.com:8443"));
    assert_eq!(url.domain(), Some("example.com"));
    assert_eq!(url.host_str(), Some("example.com"));
    assert!(url.is_special());
    assert_eq!(url.port(), Some(8443));
    assert_eq!(url.port_or_known_default(), Some(8443));
    assert_eq!(url.password_opt(), Some("pass"));

    let def = Url::parse("https://example.com/").unwrap();
    assert_eq!(def.port(), None);
    assert_eq!(def.port_or_known_default(), Some(443));
    assert_eq!(def.password_opt(), None);

    let ip = Url::parse("http://127.0.0.1/").unwrap();
    assert!(ip.domain().is_none());
    assert_eq!(ip.host_str(), Some("127.0.0.1"));
}

#[test]
fn set_port_option_u16() {
    let mut url = Url::parse("https://example.com:8443/").unwrap();
    url.set_port(Some(9000)).unwrap();
    assert_eq!(url.port(), Some(9000));
    url.set_port(Some(443)).unwrap(); // default → omitted
    assert_eq!(url.port(), None);
    assert_eq!(url.as_str(), "https://example.com/");
    url.set_port(None).unwrap();
    assert_eq!(url.port(), None);

    let mut data = Url::parse("data:text/plain,hi").unwrap();
    assert!(data.set_port(Some(80)).is_err());
}

#[test]
fn set_port_str_quirks() {
    let mut url = Url::parse("http://example.com:8080/").unwrap();
    url.set_port_str("9090").unwrap();
    assert_eq!(url.port(), Some(9090));
    url.set_port_str("").unwrap();
    assert_eq!(url.port(), None);
}

#[test]
fn query_pairs_and_mut() {
    let url = Url::parse("https://example.com/?a=1&b=2").unwrap();
    let pairs: Vec<_> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    assert_eq!(
        pairs,
        vec![("a".into(), "1".into()), ("b".into(), "2".into())]
    );

    let mut url = url.into_owned();
    url.query_pairs_mut().append("c", "3").set("a", "9");
    assert_eq!(url.query(), Some("a=9&b=2&c=3"));

    url.query_pairs_mut().clear();
    assert!(url.query().is_none());
}

#[test]
fn host_parse_and_hash() {
    let h = Host::parse("example.com").unwrap();
    assert!(matches!(h, Host::Domain(_)));
    let v4 = Host::parse("127.0.0.1").unwrap();
    assert!(matches!(v4, Host::Ipv4(_)));
    let v6 = Host::parse("[::1]").unwrap();
    assert!(matches!(v6, Host::Ipv6(_)));
    assert!(Host::parse("exa mple").is_err());

    let opaque = Host::parse_opaque("example.com").unwrap();
    assert!(matches!(opaque, Host::Domain(_)));

    let mut set = HashSet::new();
    set.insert(h.to_owned());
    assert_eq!(set.len(), 1);
}

#[test]
fn origin_display_and_ascii_alias() {
    let url = Url::parse("https://example.com/x").unwrap();
    let origin = url.origin();
    assert_eq!(origin.serialized(), "https://example.com");
    assert_eq!(origin.ascii_serialization(), origin.serialized());
    assert_eq!(origin.to_string(), "https://example.com");
    assert_eq!(Origin::new_opaque().to_string(), "null");
    assert_ne!(Origin::new_opaque(), Origin::new_opaque());
}

#[test]
fn try_from_str_and_string() {
    let url = Url::try_from("https://example.com/").unwrap();
    assert_eq!(url.as_str(), "https://example.com/");
    let url2 = Url::try_from(String::from("http://a/")).unwrap();
    assert_eq!(url2.scheme(), "http");
    assert!(Url::try_from("not a url").is_err());
}
