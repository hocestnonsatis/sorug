//! WHATWG Web Platform Tests `toascii.json` harness (domain to ASCII).
//!
//! Mirrors WPT `toascii.window.js`: each input is parsed as
//! `https://{input}/x`; `output: null` means the URL must fail to parse,
//! otherwise `hostname` must equal `output`. The same check runs through the
//! `host` / `hostname` setters.
//!
//! Refresh with `./scripts/refresh-wpt.sh`.

use serde::Deserialize;
use sorug::Url;

const JSON: &str = include_str!("toascii.json");

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Entry {
    #[allow(dead_code)] // payload only distinguishes the variant
    Comment(String),
    Case(Case),
}

#[derive(Debug, Deserialize)]
struct Case {
    input: String,
    output: Option<String>,
}

fn cases() -> Vec<Case> {
    let entries: Vec<Entry> = serde_json::from_str(JSON).expect("toascii.json");
    entries
        .into_iter()
        .filter_map(|e| match e {
            Entry::Case(c) => Some(c),
            Entry::Comment(_) => None,
        })
        .collect()
}

#[test]
fn wpt_toascii_parser() {
    let mut failures = Vec::new();
    let all = cases();
    for case in &all {
        let href = format!("https://{}/x", case.input);
        let got = Url::parse(&href).ok().map(|u| u.hostname().to_owned());
        if got != case.output {
            failures.push(format!(
                "{:?}: expected {:?}, got {got:?}",
                case.input, case.output
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} / {} toascii cases failed:\n{}",
        failures.len(),
        all.len(),
        failures.join("\n")
    );
}

#[test]
fn wpt_toascii_setters() {
    let mut failures = Vec::new();
    for case in &cases() {
        for setter in ["host", "hostname"] {
            let mut url = Url::parse("https://x/x").unwrap();
            let _ = match setter {
                "host" => url.set_host(&case.input),
                _ => url.set_hostname(&case.input),
            };
            let expected = case.output.as_deref().unwrap_or("x");
            if url.hostname() != expected {
                failures.push(format!(
                    "{setter} {:?}: expected {expected:?}, got {:?}",
                    case.input,
                    url.hostname()
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
