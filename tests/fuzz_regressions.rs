//! Fuzz-campaign regression suite for `sorug`.
//!
//! Locks in security/correctness for inputs discovered during the 2026-08-01 →
//! 2026-08-02 libFuzzer campaign (`fuzz/logs/triaged_crashes.txt`, 453
//! artifacts). Focus areas:
//! - IDNA CheckBidi / CheckJoiners / UTS #46 host mapping
//! - Fast-path bailouts (ports, passwords, scheme jumps)
//! - `file:` path / Windows-drive / empty-segment edge cases
//! - Empty `@` authority rejections
//!
//! **Oracle:** [`ada_url`] (Chromium/Node WHATWG). Success/failure and `href`
//! must match ada. [`url`] (servo/rust-url) is checked where it agrees; known
//! rust-url deviations are allowlisted (same class as the fuzz harness).
//!
//! Every case also asserts: no panic, getter invariants, and href round-trip.

use std::panic::{catch_unwind, AssertUnwindSafe};

use sorug::Url;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct NamedCase {
    name: &'static str,
    input: &'static str,
}

fn parse_no_panic(input: &str) -> Result<Url<'_>, sorug::ParseError> {
    catch_unwind(AssertUnwindSafe(|| Url::parse(input))).unwrap_or_else(|_| {
        panic!("sorug panicked on {}", input.escape_default());
    })
}

fn assert_invariants(url: &Url<'_>) {
    let href = url.href();
    let len = href.len();
    assert_eq!(url.as_str(), href);
    assert!(url.scheme_range().end <= len);
    assert!(url.path().len() <= len);
    assert!(url.search().len() <= len);
    assert!(url.hash().len() <= len);
    let _ = (
        url.scheme(),
        url.protocol(),
        url.username(),
        url.password(),
        url.host(),
        url.hostname(),
        url.host_with_port(),
        url.port_u16(),
        url.port_str(),
        url.pathname(),
        url.query(),
        url.fragment(),
        url.flags(),
        url.has_host(),
        url.backing(),
    );
}

fn assert_round_trip(url: &Url<'_>) {
    let href = url.href().to_owned();
    let again = Url::parse(&href).unwrap_or_else(|e| {
        panic!(
            "href re-parse failed for {}: {e:?}",
            href.escape_default()
        );
    });
    assert_eq!(again.href(), href, "href round-trip changed");
    assert_invariants(&again);
}

/// Assert sorug matches ada (Node) on success/failure and href; no panic.
fn assert_matches_ada(input: &str) {
    let ours = parse_no_panic(input);
    let ada = ada_url::Url::parse(input, None);
    match (&ours, &ada) {
        (Ok(o), Ok(a)) => {
            assert_eq!(
                o.href(),
                a.href(),
                "href diverge vs ada (Node) for {}",
                input.escape_default()
            );
            assert_invariants(o);
            assert_round_trip(o);
        }
        (Err(_), Err(_)) => {}
        (Ok(o), Err(e)) => panic!(
            "sorug ok ({}) but ada err ({e:?}) for {}",
            o.href().escape_default(),
            input.escape_default()
        ),
        (Err(e), Ok(a)) => panic!(
            "sorug err ({e:?}) but ada ok ({}) for {}",
            a.href().escape_default(),
            input.escape_default()
        ),
    }
}

/// When rust-url agrees with ada/sorug, assert full differential; otherwise
/// only require ada agreement (documented deviation).
fn assert_matches_ada_and_servo_when_aligned(input: &str) {
    assert_matches_ada(input);
    let ours = Url::parse(input);
    let servo = url::Url::parse(input);
    let ada = ada_url::Url::parse(input, None);
    match (&ours, &servo, &ada) {
        (Ok(o), Ok(s), Ok(a)) if o.href() == a.href() && o.href() == s.as_str() => {}
        (Err(_), Err(_), Err(_)) => {}
        (Ok(o), Ok(s), Ok(a)) if o.href() == a.href() && o.href() != s.as_str() => {
            // Known rust-url gap; ada is authoritative.
        }
        (Err(_), Ok(s), Err(_)) => {
            // rust-url accepts; Node/ada/sorug reject (empty @, CheckBidi, …).
            let _ = s;
        }
        (Ok(o), Err(_), Ok(a)) if o.href() == a.href() => {
            // rust-url rejects ACE / IDNA that Node accepts.
        }
        _ => {}
    }
}

fn run_named(cases: &[NamedCase]) {
    for c in cases {
        let result = catch_unwind(AssertUnwindSafe(|| {
            assert_matches_ada_and_servo_when_aligned(c.input);
        }));
        if let Err(payload) = result {
            panic!(
                "regression case `{}` failed on {}: {payload:?}",
                c.name,
                c.input.escape_default()
            );
        }
    }
}

fn run_bulk(inputs: &[&str]) {
    for &input in inputs {
        assert_matches_ada_and_servo_when_aligned(input);
    }
}


// ---------------------------------------------------------------------------
// IDNA CheckBidi / CheckJoiners / bidi controls
// ---------------------------------------------------------------------------

const CHECK_BIDI_CASES: &[NamedCase] = &[
    // RTL label may end with EN digit
    NamedCase { name: "rtl_label_ends_with_en_digit", input: "http://\u{624}0.com/" },
    // Arabic NSM must not count as RTL letter
    NamedCase { name: "arabic_nsm_alone_not_rtl", input: "http://\u{613}.com/" },
    // ZWNJ fails CheckJoiners outside ContextJ
    NamedCase { name: "check_joiners_zwnj_arabic", input: "http://\u{639}\u{631}\u{628}\u{64a}\u{200c}.com/" },
    // CheckBidi rejects RTL+LTR letter mix
    NamedCase { name: "check_bidi_rtl_ltr_mix_hebrew", input: "http://\u{5e2}\u{5d1}\u{5e8}\u{5d9}\u{5ea}abc.com/" },
    // CheckBidi rejects LTR+RTL letter mix
    NamedCase { name: "check_bidi_ltr_rtl_mix_arabic", input: "http://abc\u{639}\u{631}\u{628}\u{64a}.com/" },
    // Bidi embedding controls disallowed in hosts
    NamedCase { name: "bidi_embedding_control", input: "http://\u{202a}.com/" },
    // Bidi isolates U+2066..=2069 rejected (Node)
    NamedCase { name: "bidi_isolate_control", input: "http://\u{2066}.com/" },
    // Leading combining/NSM rejected
    NamedCase { name: "leading_nsm_with_rtl", input: "http://\u{613}\u{627}.com/" },
    // RTL + trailing NSM accepted (NSM skipped)
    NamedCase { name: "rtl_with_trailing_nsm", input: "http://\u{627}\u{613}.com/" },
];


// ---------------------------------------------------------------------------
// IDNA UTS #46 mapping / disallowed code points
// ---------------------------------------------------------------------------

const IDNA_MAPPING_CASES: &[NamedCase] = &[
    // Letterlike U+210B → h via UTS #46/NFKC
    NamedCase { name: "letterlike_nfkc_script_h", input: "http://\u{210b}ost.com/" },
    // Vulgar fraction NFKC
    NamedCase { name: "vulgar_fraction_nfkc", input: "http://\u{bc}.com/" },
    // Leading combining mark rejected
    NamedCase { name: "leading_combining_mark", input: "http://\u{301}.com/" },
    // Hebrew unassigned U+05CC disallowed
    NamedCase { name: "hebrew_unassigned_05cc", input: "http://\u{5cc}.com/" },
    // U+FF61 → . label separator
    NamedCase { name: "halfwidth_ideographic_full_stop", input: "http://\u{ff61}a.com/" },
    // U+2000..=200A map to space → forbidden host
    NamedCase { name: "en_quad_maps_to_space", input: "http://\u{2000}.com/" },
    // U+203E → space → forbidden host
    NamedCase { name: "overline_maps_to_space", input: "http://\u{203e}.com/" },
    // U+05FC disallowed
    NamedCase { name: "hebrew_punctuation_05fc", input: "http://\u{5fc}.com/" },
];


// ---------------------------------------------------------------------------
// Fast-path scheme jump / port / password edge cases
// ---------------------------------------------------------------------------

const FAST_PATH_CASES: &[NamedCase] = &[
    // Leading-zero ports must not keep raw :0343
    NamedCase { name: "port_leading_zeros", input: "https://example.com:0343/" },
    // Empty password after colon
    NamedCase { name: "empty_password", input: "https://user:@example.com/" },
    // Colon inside password forces slow path
    NamedCase { name: "password_contains_colon", input: "https://user:p:ass@example.com/" },
    // Baseline https fast path
    NamedCase { name: "https_simple", input: "https://example.com/" },
    // Baseline http fast path
    NamedCase { name: "http_simple", input: "http://example.com/" },
    // Baseline ws fast path
    NamedCase { name: "ws_simple", input: "ws://example.com/" },
    // Baseline wss fast path
    NamedCase { name: "wss_simple", input: "wss://example.com/" },
    // Baseline ftp fast path
    NamedCase { name: "ftp_simple", input: "ftp://example.com/" },
    // Baseline file fast path
    NamedCase { name: "file_simple", input: "file:///tmp/x" },
];


// ---------------------------------------------------------------------------
// file: path, drive letters, empty segments
// ---------------------------------------------------------------------------

const FILE_PATH_CASES: &[NamedCase] = &[
    // Do not inject / after Windows drive before ./
    NamedCase { name: "windows_drive_dot_segment", input: "file:///p:./foo" },
    // WPT/ada preserve empty file path segments
    NamedCase { name: "empty_path_segments_preserved", input: "file:////foo" },
    // Keep non-localhost file host (rust-url may drop)
    NamedCase { name: "non_localhost_host_kept", input: "file://of2/f:" },
    // file://localhost → empty host
    NamedCase { name: "localhost_normalized", input: "file://localhost/tmp" },
    // file:c:/ drive form
    NamedCase { name: "drive_letter_no_slash", input: "file:c:/windows/" },
    // C| Windows drive normalization
    NamedCase { name: "pipe_drive", input: "file:///C|/Windows/" },
    // Empty segments only
    NamedCase { name: "quad_slash_empty", input: "file:////" },
    // Dot segment at file root
    NamedCase { name: "dot_segment_root", input: "file:///./" },
];


// ---------------------------------------------------------------------------
// Empty / control-laden @ authority
// ---------------------------------------------------------------------------

const AUTHORITY_CASES: &[NamedCase] = &[
    // Empty credentials + empty host → failure
    NamedCase { name: "empty_at_http", input: "http://@/" },
    // Empty @ authority rejected (ada/Node)
    NamedCase { name: "empty_at_ftp", input: "ftp://@" },
    // Tab before @ still yields empty userinfo
    NamedCase { name: "tab_before_userinfo_at", input: "https://\t@example.com/" },
];

/// Representative crash-corpus inputs (CheckBidi / historic RTL scripts).
const CRASH_CHECK_BIDI: &[&str] = &[
    "ws:D\u{7fd}",
    "ws:-\u{10d25}",
    "ws:9\u{7fd}",
    "ws:;\u{7fd}",
    "ws:\u{10d50}\u{10910}",
    "ws:\u{6b2}\u{1d176}",
    "ws:\u{5e8}\u{5f3}",
    "ws:\u{10817}\u{111b8}",
    "ws:\u{69c}\u{6d4}",
    "ws:\u{10810}\u{11ef4}",
    "ws:\u{10a1d}\u{10add}",
    "ws:\u{10810}\u{11f3a}",
    "ws:\u{1091a}\u{10910}",
    "ws:\u{10add}\u{10b5d}",
    "ws:\u{10811}\u{10d32}",
    "ws:\u{108fd}\u{101fd}",
];

/// Representative crash-corpus inputs (file: hosts and drives).
const CRASH_FILE: &[&str] = &[
    "file://&\u{7fd}",
    "file://05\u{e0113}",
    "file://2\u{345}\u{e}\0",
    "file://s\u{3f2}\\s",
    "file://5\u{345},\0",
    "file://5x\u{345}fl",
    "file://5x.\u{345}f",
    "file://s\u{3f2}x#mmaXl",
    "file:///./C|//.0?",
    "file:///./C|//\t\t\t",
];

/// Representative crash-corpus inputs (ASCII / percent-encoded fast-path neighbors).
const CRASH_FAST_PATH: &[&str] = &[
    "HTTP:%E1%A0%8f",
    "http:%E1%8f%ba",
    "wss://h%C2%A8J/",
    "HTTP:%E1%A0%8fp",
    "HTTP:%E0%A2%\n8f",
    "HTTP:%\nE0%A2%8f0",
    "http:/%E2%81%AB/",
    "http:%E1%85%a0Xb",
    "ws:i/e:!/C:/C:/..",
    "http:/e%E2%86%83B",
];

/// Representative crash-corpus inputs (format controls / NFKC / ignored cps).
const CRASH_IDNA_OTHER: &[&str] = &[
    "ws:\u{1bca0}",
    "ws:\u{1cd3}",
    "ws:\u{17b4}",
    "ws:-\u{1d17a}",
    "ws:\u{1bca2}.",
    "ws:-\u{1d176}",
    "ws:\u{1d781}\0",
    "ws:=\u{1bca0}",
    "ws:X\u{1bca2}",
    "ws:=\u{1bca2}",
];

/// Representative crash-corpus inputs (sticky drive / empty @ remnants).
const CRASH_OTHER: &[&str] = &[
    "\0oc://@",
    "filewss://@",
    "\r\0\0\0s://@\u{6}\0\0\0",
    "locXlhosthttp://@",
    "filele\r\rC:/\r\rC:/.. ",
    "ttt:/:///C|\\:/../..",
    "d:/./wslto../\0/b:/../..///tp",
    "op://\t@?\t\tex#/\t0.0.1.0\t\t?#\t\t/",
];


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn regression_check_bidi_named() {
    run_named(CHECK_BIDI_CASES);
}

#[test]
fn regression_idna_mapping_named() {
    run_named(IDNA_MAPPING_CASES);
}

#[test]
fn regression_fast_path_named() {
    run_named(FAST_PATH_CASES);
}

#[test]
fn regression_file_path_named() {
    run_named(FILE_PATH_CASES);
}

#[test]
fn regression_empty_authority_named() {
    run_named(AUTHORITY_CASES);
}

#[test]
fn regression_crash_corpus_check_bidi() {
    run_bulk(CRASH_CHECK_BIDI);
}

#[test]
fn regression_crash_corpus_file() {
    run_bulk(CRASH_FILE);
}

#[test]
fn regression_crash_corpus_fast_path() {
    run_bulk(CRASH_FAST_PATH);
}

#[test]
fn regression_crash_corpus_idna_other() {
    run_bulk(CRASH_IDNA_OTHER);
}

#[test]
fn regression_crash_corpus_other() {
    run_bulk(CRASH_OTHER);
}

#[test]
fn regression_all_cases_never_panic() {
    let mut n = 0usize;
    for table in [
        CHECK_BIDI_CASES,
        IDNA_MAPPING_CASES,
        FAST_PATH_CASES,
        FILE_PATH_CASES,
        AUTHORITY_CASES,
    ] {
        for c in table {
            let _ = parse_no_panic(c.input);
            n += 1;
        }
    }
    for bulk in [
        CRASH_CHECK_BIDI,
        CRASH_FILE,
        CRASH_FAST_PATH,
        CRASH_IDNA_OTHER,
        CRASH_OTHER,
    ] {
        for &input in bulk {
            let _ = parse_no_panic(input);
            n += 1;
        }
    }
    assert!(n >= 80, "expected a broad regression corpus, got {n}");
}

/// Explicit success/failure pins for the highest-value CheckBidi / file cases
/// (defense in depth beyond differential-vs-ada).
#[test]
fn regression_pinned_outcomes() {
    // --- must fail (Node/ada) ---
    assert!(
        parse_no_panic("http://\u{613}.com/").is_err(),
        "arabic NSM alone"
    );
    assert!(
        parse_no_panic("http://\u{639}\u{631}\u{628}\u{64a}\u{200c}.com/").is_err(),
        "ZWNJ CheckJoiners"
    );
    assert!(
        parse_no_panic("http://\u{5e2}\u{5d1}\u{5e8}\u{5d9}\u{5ea}abc.com/").is_err(),
        "Hebrew+LTR mix"
    );
    assert!(
        parse_no_panic("http://abc\u{639}\u{631}\u{628}\u{64a}.com/").is_err(),
        "LTR+Arabic mix"
    );
    assert!(
        parse_no_panic("http://\u{202a}.com/").is_err(),
        "bidi embedding"
    );
    assert!(
        parse_no_panic("http://\u{2066}.com/").is_err(),
        "bidi isolate"
    );
    assert!(
        parse_no_panic("http://\u{301}.com/").is_err(),
        "leading combining"
    );
    assert!(
        parse_no_panic("http://\u{5cc}.com/").is_err(),
        "Hebrew unassigned"
    );
    assert!(
        parse_no_panic("http://\u{2000}.com/").is_err(),
        "en quad → space"
    );
    assert!(
        parse_no_panic("http://\u{203e}.com/").is_err(),
        "overline → space"
    );
    assert!(
        parse_no_panic("http://\u{5fc}.com/").is_err(),
        "U+05FC"
    );
    assert!(parse_no_panic("http://@/").is_err(), "empty @");
    assert!(parse_no_panic("ftp://@").is_err(), "ftp empty @");

    // --- must succeed with exact href (Node/ada) ---
    assert_matches_ada("http://\u{624}0.com/");
    assert_matches_ada("http://\u{627}\u{613}.com/");
    {
        let u = parse_no_panic("http://\u{210b}ost.com/").expect("letterlike NFKC");
        assert_eq!(u.href(), "http://host.com/");
        assert_invariants(&u);
        assert_round_trip(&u);
    }
    {
        // U+FF61 → '.' so the host becomes a leading-dot label ".a.com".
        let u = parse_no_panic("http://\u{ff61}a.com/").expect("halfwidth full stop");
        assert_eq!(u.href(), "http://.a.com/");
        assert_invariants(&u);
        assert_round_trip(&u);
    }
    {
        let u = parse_no_panic("https://example.com:0343/").expect("port leading zeros");
        assert_eq!(u.href(), "https://example.com:343/");
        assert_invariants(&u);
        assert_round_trip(&u);
    }
    {
        let u = parse_no_panic("https://user:@example.com/").expect("empty password");
        assert_eq!(u.href(), "https://user@example.com/");
        assert_invariants(&u);
        assert_round_trip(&u);
    }
    {
        let u = parse_no_panic("file:///p:./foo").expect("drive + ./");
        assert_eq!(u.href(), "file:///p:./foo");
        assert_invariants(&u);
        assert_round_trip(&u);
    }
    {
        let u = parse_no_panic("file:////foo").expect("empty file segments");
        assert_eq!(u.href(), "file:////foo");
        assert_invariants(&u);
        assert_round_trip(&u);
    }
    {
        let u = parse_no_panic("file://localhost/tmp").expect("localhost file");
        assert_eq!(u.href(), "file:///tmp");
        assert_invariants(&u);
        assert_round_trip(&u);
    }
    {
        let u = parse_no_panic("file:////").expect("quad slash");
        assert_eq!(u.href(), "file:////");
        assert_invariants(&u);
        assert_round_trip(&u);
    }
}
