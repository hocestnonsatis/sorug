//! WHATWG WPT URL setters harness for `sorug`.
//!
//! Reads `tests/setters_tests.json` and exercises quirks-style setters.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::Deserialize;
use sorug::Url;

const WPT_JSON: &str = include_str!("setters_tests.json");

#[derive(Debug, Deserialize)]
struct SetterCase {
    href: String,
    new_value: String,
    expected: BTreeMap<String, String>,
    #[allow(dead_code)]
    comment: Option<String>,
}

#[test]
fn wpt_setters_tests() {
    let root: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(WPT_JSON).expect("setters_tests.json must be valid JSON");

    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for (attr, value) in &root {
        if attr == "comment" {
            skipped += 1;
            continue;
        }
        let cases: Vec<SetterCase> = serde_json::from_value(value.clone())
            .unwrap_or_else(|e| panic!("invalid cases for {attr}: {e}"));

        for case in &cases {
            match run_case(attr, case) {
                Ok(()) => passed += 1,
                Err(err) => {
                    failed += 1;
                    if failures.len() < 64 {
                        failures.push(format!(
                            "{attr}: <{}> := {:?} — {err}",
                            case.href.escape_default(),
                            case.new_value
                        ));
                    }
                }
            }
        }
    }

    eprintln!("WPT setters: {passed} passed, {failed} failed, {skipped} comment blocks skipped");

    if failed > 0 {
        let extra = failed.saturating_sub(failures.len());
        let mut msg = format!("{failed} WPT setter case(s) failed:\n");
        for line in &failures {
            msg.push_str("  • ");
            msg.push_str(line);
            msg.push('\n');
        }
        if extra > 0 {
            let _ = writeln!(msg, "  … and {extra} more");
        }
        panic!("{msg}");
    }
}

fn run_case(attr: &str, case: &SetterCase) -> Result<(), String> {
    let mut url = Url::parse(&case.href)
        .map_err(|e| format!("href failed to parse ({e}): {:?}", case.href))?
        .into_owned();

    apply_setter(&mut url, attr, &case.new_value)?;

    for (key, expected) in &case.expected {
        let actual = getter(&url, key)?;
        if actual != *expected {
            return Err(format!(
                "after set {attr}={:?}: {key} expected {expected:?}, got {actual:?}",
                case.new_value
            ));
        }
    }
    Ok(())
}

fn apply_setter(url: &mut Url<'static>, attr: &str, value: &str) -> Result<(), String> {
    match attr {
        "href" => {
            let _ = url.set_href(value);
        }
        "protocol" => {
            let _ = url.set_protocol(value);
        }
        "username" => {
            let _ = url.set_username(value);
        }
        "password" => {
            let _ = url.set_password(value);
        }
        "host" => {
            let _ = url.set_host(value);
        }
        "hostname" => {
            let _ = url.set_hostname(value);
        }
        "port" => {
            let _ = url.set_port(value);
        }
        "pathname" => url.set_pathname(value),
        "search" => url.set_search(value),
        "hash" => url.set_hash(value),
        other => return Err(format!("unknown setter attribute: {other}")),
    }
    Ok(())
}

fn getter(url: &Url<'_>, key: &str) -> Result<String, String> {
    Ok(match key {
        "href" => url.href().to_owned(),
        "protocol" => url.protocol().to_owned(),
        "username" => url.username().to_owned(),
        "password" => url.password().to_owned(),
        "host" => url.host_with_port().to_owned(),
        "hostname" => url.hostname().to_owned(),
        "port" => url.port_str(),
        "pathname" => url.pathname().to_owned(),
        "search" => url.search().to_owned(),
        "hash" => url.hash().to_owned(),
        other => return Err(format!("unknown getter attribute: {other}")),
    })
}
