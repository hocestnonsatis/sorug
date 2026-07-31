//! Comprehensive correctness / security validation for `sorug`.
//!
//! Categories:
//! - Security & control-character injection
//! - Malformed schemes & authorities
//! - IPv4 / IPv6 edge cases
//! - Path normalization & encoded-dot traps
//! - Differential fuzz-check vs `url` (servo/rust-url)
//!
//! Notes on the differential suite: a small set of `file:` multi-slash inputs
//! are excluded because current WPT + ada-url preserve empty path segments
//! (`file:////foo` → `file:////foo`) while rust-url 2.5 collapses them. Those
//! cases are asserted separately against the WPT-expected serialization.

use sorug::Url;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn assert_href(input: &str, expected: &str) {
    let url = Url::parse(input).unwrap_or_else(|e| {
        panic!("expected success for {input:?}, got {e:?}");
    });
    assert_eq!(
        url.href(),
        expected,
        "href mismatch for input {}",
        input.escape_default()
    );
}

fn assert_fails(input: &str) {
    assert!(
        Url::parse(input).is_err(),
        "expected failure for {}",
        input.escape_default()
    );
}

fn assert_matches_servo(input: &str) {
    let servo = url::Url::parse(input);
    let ours = Url::parse(input);
    match (servo, ours) {
        (Ok(s), Ok(o)) => {
            assert_eq!(
                o.href(),
                s.as_str(),
                "href diverge for {}",
                input.escape_default()
            );
        }
        (Err(_), Err(_)) => {}
        (Ok(s), Err(e)) => panic!(
            "servo ok ({}) but sorug err ({e:?}) for {}",
            s.as_str(),
            input.escape_default()
        ),
        (Err(e), Ok(o)) => panic!(
            "sorug ok ({}) but servo err ({e:?}) for {}",
            o.href(),
            input.escape_default()
        ),
    }
}

/// rust-url collapses empty `file:` path segments; WHATWG/WPT/ada keep them.
fn is_known_rust_url_file_slash_deviation(input: &str, sorug_href: &str, servo_href: &str) -> bool {
    if !input.starts_with("file:") && !input.starts_with("FILE:") {
        return false;
    }
    sorug_href.starts_with("file:")
        && servo_href.starts_with("file:")
        && sorug_href != servo_href
        && sorug_href.matches('/').count() > servo_href.matches('/').count()
}

// ===========================================================================
// a. Security & control characters
// ===========================================================================

#[test]
fn security_null_bytes() {
    // Null in host → invalid host code point → failure.
    assert_fails("https://exam\0ple.com/");
    // Null in path is ignored as a C0 control (tab/LF/CR-class handling differs;
    // WHATWG strips C0-at-edges and ignores tab/LF/CR inside; other C0 in path
    // are percent-encoded or skipped per state). Match servo.
    assert_matches_servo("https://example.com/\0");
    assert_matches_servo("https://example.com/foo\0bar");
    assert_matches_servo("data:text/plain,\0hello");
}

#[test]
fn security_newlines_tabs_cr() {
    // Leading/trailing ASCII whitespace (≤ U+0020) is stripped.
    assert_href("\thttps://example.com/", "https://example.com/");
    assert_href("\nhttps://example.com/", "https://example.com/");
    assert_href("\rhttps://example.com/", "https://example.com/");
    assert_href("https://example.com/\n", "https://example.com/");
    assert_href("  https://example.com/  ", "https://example.com/");

    // Tabs/LF/CR inside the URL are ignored during parsing.
    assert_href("https://exam\tple.com/", "https://example.com/");
    assert_href("https://exam\nple.com/", "https://example.com/");
    assert_href("https://exam\rple.com/", "https://example.com/");
    assert_href("https://example.com/fo\no", "https://example.com/foo");
    assert_href("http://example.com/foo\tbar", "http://example.com/foobar");

    assert_matches_servo("ht\ttp://example.com/");
}

#[test]
fn security_c0_and_del_injection() {
    // C0 controls other than tab/LF/CR in host → failure.
    for c in [0x01u8, 0x02, 0x07, 0x08, 0x0b, 0x0c, 0x0e, 0x1f, 0x7f] {
        let mut s = b"https://exam".to_vec();
        s.push(c);
        s.extend_from_slice(b"ple.com/");
        let input = String::from_utf8(s).unwrap();
        assert_fails(&input);
    }
}

#[test]
fn security_space_and_forbidden_host_chars() {
    // Forbidden host code points (WHATWG).
    assert_fails("https://exam ple.com/");
    assert_fails("https://exam<ple.com/");
    assert_fails("https://exam>ple.com/");
    assert_fails("https://exam^ple.com/");
    assert_fails("https://exam|ple.com/");

    // `"`, `{`, `}`, `` ` `` are NOT forbidden host code points — both parsers accept.
    assert_matches_servo("https://exam\"ple.com/");
    assert_matches_servo("https://exam{ple.com/");
    assert_matches_servo("https://exam}ple.com/");
    assert_matches_servo("https://exam`ple.com/");
}

#[test]
fn security_userinfo_injection() {
    assert_href(
        "https://user:pass@example.com/",
        "https://user:pass@example.com/",
    );
    assert_href("https://user@example.com/", "https://user@example.com/");
    assert_matches_servo("https://user@name:pass@example.com/");
    assert_matches_servo("https://user:p@ss@example.com/");
}

#[test]
fn security_backslash_as_slash_special() {
    assert_href(r"http:\\example.com\foo", "http://example.com/foo");
    assert_href(r"https://example.com\bar", "https://example.com/bar");
    assert_matches_servo(r"foo://example.com\bar");
}

#[test]
fn security_idna_line_separators_rejected() {
    // UTS #46 disallows U+2028 / U+2029 in domains (ada + rust-url agree).
    assert_fails("http://example.com\u{2028}/");
    assert_fails("http://example.com\u{2029}/");
    assert_fails("http://\u{2028}example.com/");
}

// ===========================================================================
// b. Malformed schemes & authorities
// ===========================================================================

#[test]
fn malformed_double_port_colons() {
    assert_matches_servo("http://example.com:80:80");
    assert_matches_servo("http://example.com:80:80/");
    assert_fails("http://example.com:65536/");
    assert_fails("http://example.com:99999/");
    assert_href("http://example.com:8080/", "http://example.com:8080/");
    assert_href("http://example.com:80/", "http://example.com/");
    assert_href("https://example.com:443/", "https://example.com/");
}

#[test]
fn malformed_missing_and_excess_slashes() {
    assert_matches_servo("http:example.com");
    assert_matches_servo("http:/example.com");
    assert_matches_servo("http:/\\example.com");
    assert_href("http:///////example.com/", "http://example.com/");
    assert_href("https://////example.com/x", "https://example.com/x");
    assert_matches_servo("http:///example.com");
    assert_matches_servo("http:////example.com");
}

#[test]
fn malformed_scheme_characters() {
    assert_fails("1http://example.com/");
    assert_fails("://example.com/");
    assert_fails("/path");
    assert_fails("//example.com/");
    assert_href("a+b://host/x", "a+b://host/x");
    assert_href("a.b://host/x", "a.b://host/x");
    assert_href("a-b://host/x", "a-b://host/x");
    assert_href("HTTP://EXAMPLE.COM/FOO", "http://example.com/FOO");
    assert_href("HtTpS://ExAmPle.CoM/", "https://example.com/");
}

#[test]
fn malformed_empty_and_weird_authority() {
    assert_fails("http://");
    assert_fails("http:///");
    assert_fails("https:///");
    assert_fails("http://?");
    assert_fails("http://#");
    assert_matches_servo("http://user@");
    assert_matches_servo("http://user:pass@");
    assert_href("http://example.com", "http://example.com/");
    assert_href("https://example.com", "https://example.com/");
}

#[test]
fn malformed_file_schemes() {
    assert_href("file:///foo/bar", "file:///foo/bar");
    assert_href("file://localhost/tmp", "file:///tmp");
    assert_href("FILE:///FOO", "file:///FOO");
    // WPT: empty path segments preserved (rust-url collapses these).
    assert_href("file:////", "file:////");
    assert_href("file:////foo", "file:////foo");
    assert_href("file://///foo", "file://///foo");
    assert_matches_servo("file://host/path");
    assert_matches_servo("file:c:/windows");
    assert_matches_servo("file:/c|/windows");
    assert_matches_servo(r"file:\\server\share");
}

// ===========================================================================
// c. IP address edge cases
// ===========================================================================

#[test]
fn ipv4_malformed_and_edge() {
    assert_fails("http://256.0.0.1/");
    assert_fails("http://1.2.3.256/");
    assert_fails("http://1.2.3.4.5/");
    // IPv4 with fewer than 4 parts is valid (WHATWG syntax compression).
    assert_href("http://1.2.3/", "http://1.2.0.3/");
    assert_matches_servo("http://1.2.3.4/");
    assert_matches_servo("http://0x7f.0.0.1/");
    assert_matches_servo("http://127.0.0.1/");
    assert_matches_servo("http://0177.0.0.1/");
    assert_matches_servo("http://0x7f000001/");
    assert_matches_servo("http://2130706433/");
    assert_matches_servo("http://127.1/");
    assert_matches_servo("http://127.0.1/");
    assert_fails("http://0x100000000/");
    assert_matches_servo("http://0xffffffff/");
    assert_fails("http://0x1000000000/");
}

#[test]
fn ipv4_trailing_dot_and_empty_parts() {
    assert_matches_servo("http://127.0.0.1./");
    assert_matches_servo("http://0..0x300/");
    // `.` alone is a domain label, not a failed IPv4 parse.
    assert_href("http://./", "http://./");
    assert_matches_servo("http://0.0.0.0/");
}

#[test]
fn ipv6_malformed() {
    assert_fails("http://[::1/");
    assert_fails("http://[::1::2]/");
    assert_fails("http://[gggg::1]/");
    assert_fails("http://[::1]:65536/");
    assert_href("http://[::1]/", "http://[::1]/");
    assert_href("http://[2001:db8::1]/", "http://[2001:db8::1]/");
    assert_matches_servo("http://[::ffff:127.0.0.1]/");
    assert_matches_servo("http://[0:0:0:0:0:0:0:1]/");
    assert_matches_servo("http://[::]/");
    assert_fails("http://[1:2:3:4:5:6:7:8:9]/");
    assert_fails("http://[]/");
    assert_fails("http://[:]/");
}

#[test]
fn ipv6_with_zone_and_port() {
    assert_fails("http://[fe80::1%25eth0]/");
    assert_fails("http://[fe80::1%eth0]/");
    assert_href("http://[::1]:8080/", "http://[::1]:8080/");
    assert_href("http://[::1]:80/", "http://[::1]/");
}

// ===========================================================================
// d. Path normalization & dot-segment traps
// ===========================================================================

#[test]
fn path_dot_segments() {
    assert_href("https://example.com/a/./b/../c", "https://example.com/a/c");
    assert_href("https://example.com/../", "https://example.com/");
    assert_href("https://example.com/..", "https://example.com/");
    assert_href("https://example.com/.", "https://example.com/");
    assert_href("https://example.com/./", "https://example.com/");
    assert_href(
        "https://example.com/a/b/../../../../c",
        "https://example.com/c",
    );
    assert_href("https://example.com/a/b/../../c", "https://example.com/c");
    assert_href(
        "https://example.com/a/b/c/./../../g",
        "https://example.com/a/g",
    );
}

#[test]
fn path_encoded_dots() {
    assert_href("https://example.com/%2e", "https://example.com/");
    assert_href("https://example.com/%2e/", "https://example.com/");
    assert_href("https://example.com/%2e%2e", "https://example.com/");
    assert_href("https://example.com/%2E%2e/", "https://example.com/");
    assert_href("https://example.com/a/%2e%2e/b", "https://example.com/b");
    assert_href("https://example.com/a/%2E/b", "https://example.com/a/b");
    assert_href(
        "https://example.com/%2e%2e%2f",
        "https://example.com/%2e%2e%2f",
    );
    assert_matches_servo("https://example.com/foo/%2e%2e%2fbar");
}

#[test]
fn path_empty_segments_and_slashes() {
    assert_href("https://example.com//", "https://example.com//");
    assert_href("https://example.com///a", "https://example.com///a");
    assert_href("https://example.com/a//b", "https://example.com/a//b");
    assert_href("file:////", "file:////");
    assert_href("file:////foo", "file:////foo");
}

#[test]
fn path_percent_and_unicode() {
    assert_matches_servo("https://example.com/%00");
    assert_matches_servo("https://example.com/%zz");
    assert_matches_servo("https://example.com/你好");
    assert_matches_servo("https://example.com/\u{1F600}");
    assert_matches_servo("https://example.com/foo bar");
    assert_matches_servo("https://example.com/foo\"bar");
}

#[test]
fn path_query_fragment_boundaries() {
    assert_href("https://ex.com/p?q=1#frag", "https://ex.com/p?q=1#frag");
    assert_href("https://ex.com/p?#", "https://ex.com/p?#");
    assert_href("https://ex.com/p#?notquery", "https://ex.com/p#?notquery");
    assert_matches_servo("https://ex.com/p?q=#f");
    assert_matches_servo("https://ex.com/?");
    assert_matches_servo("https://ex.com/#");
}

// ===========================================================================
// e. Differential fuzz-check vs servo/rust-url
// ===========================================================================

/// Deterministic corpus of weird / adversarial inputs (≥ 500 cases).
fn differential_corpus() -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(600);

    let seeds = [
        "",
        " ",
        "\t",
        "\n",
        "http://example.com/",
        "https://example.com/api/v1/users",
        "https://user:password@api.example.com:8443/v1/search?q=rust+performance&sort=desc#results",
        "https://türkçe.com/iletisim",
        "file:///C:/Windows/System32/drivers/etc/hosts",
        "http://[::1]/",
        "http://127.0.0.1/",
        "ftp://ftp.example.com/file",
        "ws://example.com/ws",
        "wss://example.com/ws",
        "mailto:user@example.com",
        "data:text/plain,hello",
        "blob:https://example.com/uuid",
        "about:blank",
        "javascript:alert(1)",
        "http://example.com:80:80/",
        "http:///////example.com/",
        "http:example.com",
        "1http://example.com/",
        "http://256.0.0.1/",
        "http://[::1/",
        "http://[::1::2]/",
        "https://example.com/a/./b/../c",
        "https://example.com/%2e%2e",
        "https://exam\tple.com/",
        "https://exam\0ple.com/",
        r"http:\\example.com\foo",
        "file://localhost/tmp",
        "HTTP://EXAMPLE.COM/FOO",
        "https://user@name:pass@example.com/",
        "http://0x7f000001/",
        "http://[::ffff:127.0.0.1]/",
        "foo://example.com:99/bar",
        "https://example.com/a/b/../../../../c",
        "https://example.com/foo bar",
        "https://example.com/\u{1F4A9}",
        "http://.",
        "http://../",
        "http://?",
        "http://#",
        "http://@",
        "http://:@",
        "http://example.com:/",
        "http://example.com:00080/",
        "https://example.com?",
        "https://example.com#",
        "https://example.com?#",
        "file:",
        "file:/",
        "file://",
        "file:///",
        "file:c:",
        "file:c|",
        "file:/C|/",
        "file://localhost",
        "file://localhost/",
        "http://a.b.c.d.e.f.g.h.i.j.k.l.m.n.o.p.q.r.s.t.u.v.w.x.y.z/",
        "http://-example.com/",
        "http://example-.com/",
        "http://ex--ample.com/",
        "http://.example.com/",
        "http://example.com./",
        "http://192.168.0.257/",
        "http://192.168.0.1.1/",
        "http://0300.0250.0.1/",
        "http://0xC0.0xA8.0.1/",
        "http://[https://example.com]/",
        "http://example.com/%2f%2fevil.com/",
        "https://example.com/..;/..;/etc/passwd",
        "https://example.com/foo/bar;jsessionid=123",
        "https://example.com/foo?bar=1&baz=2#qux",
        "https://example.com/?q=%00%01%02",
        "https://example.com/#%00frag",
        "http://[v1.something]/",
        "http://[v0.0]/",
        "not-a-url",
        "//example.com/path",
        "/relative",
        "?query",
        "#hash",
        "http://example.com\u{2028}/",
        "http://example.com\u{2029}/",
        "http://\u{FF0E}example.com/",
    ];
    for s in seeds {
        out.push(s.to_owned());
    }

    let schemes = [
        "http", "https", "ftp", "ws", "wss", "file", "foo", "a+b", "HTTP", "HtTpS",
    ];
    let hosts = [
        "example.com",
        "EXAMPLE.COM",
        "127.0.0.1",
        "0x7f.1",
        "[::1]",
        "[::ffff:1.2.3.4]",
        "xn--e1afmkfd.xn--p1ai",
        "localhost",
        "a.b",
        "ex ample.com",
        "exam\tple.com",
        "256.0.0.1",
        "[::1",
        "[::1::2]",
        "",
        "user@example.com",
    ];
    let ports = [":80", ":443", ":8080", ":65535", ":65536"];
    let paths = [
        "", "/", "/.", "/..", "/./", "/../", "/%2e", "/%2e%2e", "/a/b/../c", "//",
        "/foo bar", "/\0", "/fo\to", r"\foo", "/%00", "/你好",
    ];
    let queries = ["", "?", "?q=1", "?q=1#f", "?q=\0", "?a=b&c=d"];
    let frags = ["", "#", "#x", "#\n", "#%00"];

    for scheme in schemes {
        for host in hosts {
            for port in ports {
                for path in &paths[..8] {
                    let url = if scheme == "file" {
                        format!("{scheme}:///{host}{path}")
                    } else {
                        format!("{scheme}://{host}{port}{path}")
                    };
                    out.push(url);
                }
            }
        }
    }

    for path in paths {
        for q in queries {
            for f in frags {
                out.push(format!("https://example.com{path}{q}{f}"));
            }
        }
    }

    let template = b"https://example.com/path?q=1#frag";
    for pos in 0..template.len() {
        for &ctrl in &[0u8, 1, 7, 8, 9, 10, 11, 12, 13, 0x1f, 0x7f] {
            let mut v = template.to_vec();
            v[pos] = ctrl;
            if let Ok(s) = String::from_utf8(v) {
                out.push(s);
            }
        }
    }

    for n in 0..12 {
        let slashes = "/".repeat(n);
        out.push(format!("http:{slashes}example.com/"));
        out.push(format!("https:{slashes}example.com/x"));
        out.push(format!("file:{slashes}foo"));
    }

    for enc in [
        "%", "%2", "%2e", "%2E", "%2e%2e", "%2e%2e%2f", "%00", "%ff", "%u002e", "%%30%30",
        "%2%65", "%c0%ae",
    ] {
        out.push(format!("https://example.com/{enc}"));
        out.push(format!("https://example.com/a/{enc}/b"));
    }

    let mut seen = std::collections::HashSet::new();
    out.retain(|s| seen.insert(s.clone()));
    assert!(
        out.len() >= 500,
        "differential corpus too small: {}",
        out.len()
    );
    out
}

#[test]
fn differential_vs_servo_full_corpus() {
    let corpus = differential_corpus();
    let mut matched = 0usize;
    let mut both_ok = 0usize;
    let mut both_err = 0usize;
    let mut skipped_known = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for input in &corpus {
        let servo = url::Url::parse(input);
        let ours = Url::parse(input);
        match (&servo, &ours) {
            (Ok(s), Ok(o)) => {
                both_ok += 1;
                if o.href() == s.as_str() {
                    matched += 1;
                } else if is_known_rust_url_file_slash_deviation(input, o.href(), s.as_str()) {
                    skipped_known += 1;
                    matched += 1;
                } else {
                    failures.push(format!(
                        "HREF {} → sorug={:?} servo={:?}",
                        input.escape_default(),
                        o.href(),
                        s.as_str()
                    ));
                }
            }
            (Err(_), Err(_)) => {
                both_err += 1;
                matched += 1;
            }
            (Ok(s), Err(e)) => failures.push(format!(
                "SORUG_ERR {} → servo={:?} sorug={e:?}",
                input.escape_default(),
                s.as_str()
            )),
            (Err(e), Ok(o)) => failures.push(format!(
                "SERVO_ERR {} → sorug={:?} servo={e:?}",
                input.escape_default(),
                o.href()
            )),
        }
    }

    if !failures.is_empty() {
        let show = failures.len().min(40);
        panic!(
            "differential: {}/{} matched (ok={both_ok} err={both_err} known_file_skip={skipped_known}); {} failures, first {show}:\n{}",
            matched,
            corpus.len(),
            failures.len(),
            failures[..show].join("\n")
        );
    }

    eprintln!(
        "differential OK: {}/{} (both_ok={both_ok} both_err={both_err} known_file_skip={skipped_known})",
        matched,
        corpus.len()
    );
}

#[test]
fn differential_smoke_known_pairs() {
    for input in [
        "https://example.com/api/v1/users",
        "http://user:pass@host:8080/p?q=1#f",
        "file:///C:/Windows/System32/drivers/etc/hosts",
        "http://[::1]/",
        "https://example.com/a/./b/../c",
        "https://example.com/%2e%2e/x",
        "HTTP://EXAMPLE.COM/",
        r"http:\\example.com\a\b",
        "http:///////example.com/",
        "foo://bar/baz",
        "mailto:a@b.c",
        "data:,hello",
        "http://127.0.0.1/",
        "http://0x7f000001/",
        "https://exam\tple.com/",
        "  https://example.com/  ",
        "http://1.2.3/",
        "http://example.com\u{2028}/",
    ] {
        assert_matches_servo(input);
    }
}

#[test]
fn file_empty_segments_match_wpt_not_rust_url() {
    // Documented deviation: sorug + ada + WPT preserve empty segments.
    let cases = [
        ("file:////", "file:////"),
        ("file:////foo", "file:////foo"),
        ("file://///foo", "file://///foo"),
        ("file://////foo", "file://////foo"),
    ];
    for (input, expected) in cases {
        assert_href(input, expected);
        let ada = ada_url::Url::parse(input, None).expect("ada");
        assert_eq!(ada.href(), expected, "ada href for {input}");
        // rust-url disagrees — that is expected.
        let servo = url::Url::parse(input).expect("servo parses");
        assert_ne!(
            servo.as_str(),
            expected,
            "test assumption broken: rust-url unexpectedly matches WPT for {input}"
        );
    }
}

// ===========================================================================
// Fast-path / CoW invariants
// ===========================================================================

#[test]
fn cow_borrowed_for_canonical_ascii() {
    let url = Url::parse("https://example.com/api/v1/users").unwrap();
    assert!(url.backing().is_borrowed());
    let url = Url::parse("file:///C:/Windows/System32/drivers/etc/hosts").unwrap();
    assert!(url.backing().is_borrowed());
}

#[test]
fn cow_owned_when_normalization_required() {
    let url = Url::parse("HTTP://EXAMPLE.COM/FOO").unwrap();
    assert!(!url.backing().is_borrowed());
    let url = Url::parse("https://example.com/a/../b").unwrap();
    assert!(!url.backing().is_borrowed());
    let url = Url::parse(r"http:\\example.com\foo").unwrap();
    assert!(!url.backing().is_borrowed());
}
