//! WHATWG Web Platform Tests harness for `sorug`.
//!
//! Reads `tests/urltestdata.json` (official WPT URL parser cases) and asserts
//! [`sorug::Url`] against every success / failure expectation.
//!
//! This is the WPT correctness suite. Phase 2 lands the state machine; remaining
//! failures are mostly file-host edge cases, `///` empty-host relatives, and a
//! few IDNA labels that current `idna` rejects (also rejected by rust-url 2.5).
//!
//! Fetch / refresh the fixture (also done automatically by `build.rs` when missing):
//!
//! ```bash
//! curl -fsSL -o tests/urltestdata.json \
//!   https://raw.githubusercontent.com/web-platform-tests/wpt/master/url/resources/urltestdata.json
//! ```

use std::fmt::Write as _;

use serde::Deserialize;
use sorug::Url;

const WPT_JSON: &str = include_str!("urltestdata.json");

/// Top-level `urltestdata.json` entry: comment string or test object.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
enum Entry {
    #[allow(dead_code)] // payload only distinguishes the variant
    Comment(String),
    Case(TestCase),
}

/// A single WPT URL constructor / parser case.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // `comment` / `origin` / `relative_to` / `search_params` are format noise
struct TestCase {
    input: String,
    /// Absolute base URL serialization, or `null` when parsing without a base.
    base: Option<String>,
    /// When `true`, parsing `input` against `base` must return failure.
    #[serde(default)]
    failure: Option<bool>,

    // --- expected URL API getters (present on success cases) ----------------
    href: Option<String>,
    protocol: Option<String>,
    username: Option<String>,
    password: Option<String>,
    /// Hostname + optional `":" port` (WPT `host` attribute).
    host: Option<String>,
    hostname: Option<String>,
    port: Option<String>,
    pathname: Option<String>,
    search: Option<String>,
    hash: Option<String>,

    // --- ignored / optional WPT metadata ------------------------------------
    origin: Option<String>,
    comment: Option<String>,
    #[serde(default, rename = "relativeTo")]
    relative_to: Option<String>,
    #[serde(default, rename = "searchParams")]
    search_params: Option<serde_json::Value>,
}

impl TestCase {
    fn expects_failure(&self) -> bool {
        self.failure == Some(true)
    }

    fn name(&self) -> String {
        match &self.base {
            Some(base) => format!(
                "<{}> against <{}>",
                self.input.escape_default(),
                base.escape_default()
            ),
            None => format!("<{}>", self.input.escape_default()),
        }
    }
}

#[test]
fn wpt_urltestdata() {
    let entries: Vec<Entry> =
        serde_json::from_str(WPT_JSON).expect("urltestdata.json must be valid JSON");

    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut ignored_comments = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for entry in &entries {
        let case = match entry {
            Entry::Comment(_) => {
                ignored_comments += 1;
                continue;
            }
            Entry::Case(case) => case,
        };

        match run_case(case) {
            Ok(()) => passed += 1,
            Err(err) => {
                failed += 1;
                // Cap stored details so a fully-red suite stays readable.
                if failures.len() < 64 {
                    failures.push(format!("{} — {err}", case.name()));
                }
            }
        }
    }

    eprintln!(
        "WPT urltestdata: {passed} passed, {failed} failed, {ignored_comments} comments skipped ({} entries)",
        entries.len()
    );

    if failed > 0 {
        let extra = failed.saturating_sub(failures.len());
        let mut msg = format!("{failed} WPT case(s) failed:\n");
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

fn run_case(case: &TestCase) -> Result<(), String> {
    let base = match &case.base {
        Some(base_str) => {
            let parsed = Url::parse(base_str)
                .map_err(|e| format!("base URL failed to parse ({e}): {base_str:?}"))?;
            Some(parsed)
        }
        None => None,
    };

    let result = Url::parse_with_base(&case.input, base.as_ref());

    if case.expects_failure() {
        return match result {
            Err(_) => Ok(()),
            Ok(url) => Err(format!("expected parse failure, got href {:?}", url.href())),
        };
    }

    let url = result.map_err(|e| format!("expected success, got {e}"))?;
    assert_success_components(&url, case)
}

fn assert_success_components(url: &Url, case: &TestCase) -> Result<(), String> {
    let href = require(case.href.as_deref(), "href")?;
    let protocol = require(case.protocol.as_deref(), "protocol")?;
    let username = require(case.username.as_deref(), "username")?;
    let password = require(case.password.as_deref(), "password")?;
    let host = require(case.host.as_deref(), "host")?;
    let hostname = require(case.hostname.as_deref(), "hostname")?;
    let port = require(case.port.as_deref(), "port")?;
    let pathname = require(case.pathname.as_deref(), "pathname")?;
    let search = require(case.search.as_deref(), "search")?;
    let hash = require(case.hash.as_deref(), "hash")?;

    eq("href", url.href(), href)?;
    eq("protocol", url.protocol(), protocol)?;
    eq("username", url.username(), username)?;
    eq("password", url.password(), password)?;
    eq("host", url.host_with_port(), host)?;
    eq("hostname", url.hostname(), hostname)?;
    eq("port", &url.port_str(), port)?;
    eq("pathname", url.pathname(), pathname)?;
    eq("path", url.path(), pathname)?;
    eq("search", url.search(), search)?;
    eq("hash", url.hash(), hash)?;

    // Cross-check scheme() against WPT protocol (strip trailing ':').
    let scheme = protocol
        .strip_suffix(':')
        .ok_or_else(|| format!("WPT protocol missing trailing ':': {protocol:?}"))?;
    eq("scheme", url.scheme(), scheme)?;

    Ok(())
}

fn require<'a>(value: Option<&'a str>, field: &str) -> Result<&'a str, String> {
    value.ok_or_else(|| format!("success case missing expected field `{field}`"))
}

fn eq(field: &str, actual: &str, expected: &str) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!("{field}: expected {expected:?}, got {actual:?}"))
    }
}
