//! Differential / invariant fuzz target for `sorug`.
//!
//! Goals under adversarial UTF-8 input (capped at [`MAX_INPUT_LEN`]):
//! 1. Never panic / never trigger out-of-bounds indexing in getters.
//! 2. Match `url` (servo/rust-url) success/failure and `href` serialization,
//!    with allowlists for documented rust-url deviations (file empty segments,
//!    file host drop, path `^`/`{`/`}`/`` ` `` encoding, opaque trailing space).
//! 3. Canonical `href()` must re-parse identically (round-trip).
//!
//! **Not** a Unicode range hunter: IDNA tables are derived from vendored UCD
//! (`data/ucd/`) plus `data/idna_overlay.txt`. Host ACE mismatches should be
//! treated as algorithm / overlay / UCD-version deltas — see `fuzz/README.md`.
//!
//! Panics are intentional crash signals for libFuzzer — do **not** wrap the
//! parse path in `catch_unwind` or install a panic hook that swallows failures.
//! `#[inline(always)]` on library code must not be used to hide logic errors;
//! this harness lets panics propagate to the fuzzer runtime unchanged.

#![no_main]

use libfuzzer_sys::fuzz_target;
use std::str;

/// Hard cap: reject oversized inputs early to keep RSS / corpus growth bounded.
const MAX_INPUT_LEN: usize = 4096;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_LEN {
        return;
    }

    let Ok(s) = str::from_utf8(data) else {
        return;
    };

    // --- sorug parse (any panic = fuzzer crash) ---
    let ours = sorug::Url::parse(s);

    // --- differential vs servo/rust-url ---
    let servo = url::Url::parse(s);
    match (&ours, &servo) {
        (Ok(o), Ok(r)) => {
            let oh = o.href();
            let rh = r.as_str();
            // Normalize path-encode + drive-letter gaps first so other allowlists
            // (file host drop, empty segments, opaque space) can stack with them.
            if oh != rh {
                let oh_e = encode_path_gap_chars(oh);
                let rh_e = encode_path_gap_chars(rh);
                // Host-drop / slash / opaque must run before drive/`|` stripping —
                // stripping `/f:` first turns `file://of2/f:` into `file://of2`
                // and breaks the host-drop equality check.
                let oh_drive = strip_pipe_path_segments(&strip_windows_drive_segments(&oh_e));
                let rh_drive = strip_pipe_path_segments(&strip_windows_drive_segments(&rh_e));
                // Slash-after-drive normalize must not be chained with drive-strip —
                // joining `A:/E:` → `A:E:` makes the next `/E:` eat an authority slash.
                let oh_slash = strip_pipe_path_segments(&strip_slash_after_windows_drive(&oh_e));
                let rh_slash = strip_pipe_path_segments(&strip_slash_after_windows_drive(&rh_e));
                if oh_e != rh_e
                    && !is_known_rust_url_file_slash_deviation(s, &oh_e, &rh_e)
                    && !is_known_rust_url_file_host_drop(&oh_e, &rh_e)
                    && !is_known_rust_url_opaque_trailing_space(&oh_e, &rh_e)
                    && oh_drive != rh_drive
                    && oh_slash != rh_slash
                    && !is_known_rust_url_sticky_drive_dotdot(&oh_e, &rh_e)
                    && !is_known_rust_url_relative_drive_collapse(&oh_e, &rh_e)
                {
                    panic!(
                        "href diverge\n  input: {}\n  sorug: {}\n  servo: {}",
                        s.escape_default(),
                        oh.escape_default(),
                        rh.escape_default()
                    );
                }
            }
        }
        (Err(_), Err(_)) => {}
        (Ok(o), Err(e)) => {
            // rust-url ICU rejects some ACE labels (empty `xn--`, decode→unassigned,
            // `_` in ACE, …) that sorug keeps under WHATWG beStrict=false / WPT.
            // Real non-ACE IDNA gaps must still panic (fixed in punycode mapping).
            if is_idna_error(&e) && host_has_ace_label(o.href()) {
                // fall through to invariants / round-trip
            } else {
                panic!(
                    "sorug ok / servo err\n  input: {}\n  sorug: {}\n  servo: {e}",
                    s.escape_default(),
                    o.href().escape_default()
                );
            }
        }
        (Err(e), Ok(r)) => {
            // rust-url accepts `scheme://@` (empty credentials + empty host) as
            // `scheme://`; ada / Chrome / sorug reject.
            // IDNA / file leniency: only when input actually looks like that class
            // (not a blanket catch-all on any servo href containing `xn--` / `file:`).
            if is_known_rust_url_empty_at_authority(s, r.as_str())
                || is_known_rust_url_idna_table_delta(s, r.as_str())
                || is_known_rust_url_file_parse_leniency(s, r.as_str())
                || is_known_rust_url_arabic_punct_idna(s, r.as_str())
                || is_known_rust_url_disallowed_idna(s, r.as_str())
                || is_known_rust_url_bidi_idna(s, r.as_str())
                || is_known_rust_url_zwnj_idna(s, r.as_str())
            {
                return;
            }
            panic!(
                "sorug err / servo ok\n  input: {}\n  sorug: {e:?}\n  servo: {}",
                s.escape_default(),
                r.as_str().escape_default()
            );
        }
    }

    let Ok(url) = ours else {
        return;
    };

    // --- component / offset invariants (exercise every getter) ---
    assert_invariants(&url);

    // --- round-trip: canonical href must re-parse without panic ---
    let href = url.href().to_owned();
    let again = sorug::Url::parse(&href).unwrap_or_else(|e| {
        panic!(
            "href re-parse failed\n  input: {}\n  href: {}\n  err: {e:?}",
            s.escape_default(),
            href.escape_default()
        );
    });
    assert_eq!(
        again.href(),
        href,
        "href round-trip changed serialization\n  input: {}\n  first: {}\n  second: {}",
        s.escape_default(),
        href.escape_default(),
        again.href().escape_default()
    );
    assert_invariants(&again);
});

/// rust-url 2.5 collapses empty `file:` path segments; WHATWG / WPT / ada / sorug
/// preserve them (`file:////foo` stays `file:////foo`).
///
/// Leading C0 / ASCII whitespace is stripped by the parser before scheme
/// detection, so we key off the resulting `href`s (not the raw input prefix).
fn is_known_rust_url_file_slash_deviation(
    _input: &str,
    sorug_href: &str,
    servo_href: &str,
) -> bool {
    sorug_href.starts_with("file:")
        && servo_href.starts_with("file:")
        && sorug_href != servo_href
        && sorug_href.matches('/').count() > servo_href.matches('/').count()
}

/// rust-url 2.5 sometimes drops a non-`localhost` file host while ada / sorug /
/// WPT keep it (`file://ofe/x` → rust-url `file:///x`).
fn is_known_rust_url_file_host_drop(sorug_href: &str, servo_href: &str) -> bool {
    let Some(s) = sorug_href.strip_prefix("file://") else {
        return false;
    };
    let Some(v) = servo_href.strip_prefix("file://") else {
        return false;
    };
    // Servo form has an empty host (path begins immediately with '/').
    if !v.starts_with('/') {
        return false;
    }
    let Some(slash) = s.find('/') else {
        return false;
    };
    let host = &s[..slash];
    if host.is_empty() || host.eq_ignore_ascii_case("localhost") {
        return false;
    }
    // `file://host/rest` vs `file:///rest`
    v == &s[slash..]
}

/// rust-url 2.5 leaves trailing U+0020 before `?`/`#` literal in opaque paths;
/// WHATWG / WPT / ada / sorug rewrite that final space as `%20`.
fn is_known_rust_url_opaque_trailing_space(sorug_href: &str, servo_href: &str) -> bool {
    if sorug_href == servo_href {
        return false;
    }
    // Cheap structural filter: only opaque (no `://` authority form) URLs.
    let Some(s_colon) = sorug_href.find(':') else {
        return false;
    };
    let Some(v_colon) = servo_href.find(':') else {
        return false;
    };
    if sorug_href[s_colon..].starts_with("://") || servo_href[v_colon..].starts_with("://") {
        return false;
    }
    normalize_opaque_trailing_space(sorug_href) == normalize_opaque_trailing_space(servo_href)
}

/// rust-url injects `/` after a Windows drive when more path follows (`e:/%20`);
/// ada / Chrome / sorug keep `e:%20`. Normalize by dropping that slash.
fn strip_slash_after_windows_drive(href: &str) -> String {
    let bytes = href.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        // Use `out` (not input) for segment boundary — otherwise a just-skipped
        // slash after `A:` still sits in the input and falsely starts `/E:/`.
        let at_seg = out.last() == Some(&b'/') || out.is_empty();
        if at_seg
            && i + 3 < bytes.len()
            && bytes[i].is_ascii_alphabetic()
            && bytes[i + 1] == b':'
            && bytes[i + 2] == b'/'
            && !matches!(bytes[i + 3], b'?' | b'#')
        {
            out.push(bytes[i]);
            out.push(b':');
            i += 3; // letter + ':' + injected '/'
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).expect("drive-slash strip preserves UTF-8")
}

/// rust-url sometimes treats `/X:` as a sticky Windows drive even on non-file
/// URLs, so `..` fails to pop it (and preceding segments). ada / sorug pop
/// normally. Same class: opaque paths where a `X:\..` segment survives one
/// `..` pop (`b:\\../]…` vs `]…`). Detect by: servo had a drive / `X:\…`
/// segment; after stripping bare `/X:` drives, sorug's path segments are an
/// ordered subsequence of servo's.
fn is_known_rust_url_sticky_drive_dotdot(sorug_href: &str, servo_href: &str) -> bool {
    if !path_has_sticky_dotdot_remnant(servo_href) {
        return false;
    }
    // Prefer authority-aligned compare when both have `://`.
    if let (Some((s_auth, s_rest)), Some((v_auth, v_rest))) =
        (split_authority(sorug_href), split_authority(servo_href))
    {
        if s_auth == v_auth {
            return sticky_segments_match(s_rest, v_rest);
        }
    }
    // Opaque / no-authority paths (e.g. `d00:/…/b:/…`).
    sticky_segments_match(sorug_href, servo_href)
}

fn sticky_segments_match(sorug_part: &str, servo_part: &str) -> bool {
    let s_stripped = strip_windows_drive_segments(sorug_part);
    let v_stripped = strip_windows_drive_segments(servo_part);
    let s_segs: Vec<&str> = path_segments(&s_stripped)
        .into_iter()
        .filter(|s| !s.is_empty() && *s != ".")
        .collect();
    let v_segs: Vec<&str> = path_segments(&v_stripped)
        .into_iter()
        .filter(|s| !s.is_empty() && *s != ".")
        .collect();
    if s_segs == v_segs {
        return true;
    }
    is_ordered_subsequence(&s_segs, &v_segs)
}

/// rust-url keeps extra segments through relative `C:/…/../..` where ada /
/// sorug collapse to `c:/`.
fn is_known_rust_url_relative_drive_collapse(sorug_href: &str, servo_href: &str) -> bool {
    if sorug_href.contains("://") || servo_href.contains("://") {
        return false;
    }
    let s = sorug_href.as_bytes();
    let v = servo_href.as_bytes();
    if s.len() < 2
        || !s[0].is_ascii_alphabetic()
        || s[1] != b':'
        || v.len() < 2
        || !v[0].eq_ignore_ascii_case(&s[0])
        || v[1] != b':'
    {
        return false;
    }
    // sorug is `X:` or `X:/`; servo starts with the same drive prefix.
    let s_l = sorug_href.to_ascii_lowercase();
    let v_l = servo_href.to_ascii_lowercase();
    v_l.starts_with(&s_l) || v_l.starts_with(s_l.trim_end_matches('/'))
}

fn split_authority(href: &str) -> Option<(&str, &str)> {
    let rest = href.split_once("://")?.1;
    // Authority ends at first path/query/fragment delimiter — ignore `@` in the path.
    let auth_len = rest
        .find(['/', '?', '#'])
        .unwrap_or(rest.len());
    let authority = &rest[..auth_len];
    let _host = match authority.rfind('@') {
        Some(i) => &authority[i + 1..],
        None => authority,
    };
    let auth_end = href.len() - rest.len() + auth_len;
    Some((&href[..auth_end], &href[auth_end..]))
}

fn path_has_windows_drive_segment(path_and_after: &str) -> bool {
    let path = path_and_after.split(['?', '#']).next().unwrap_or(path_and_after);
    path.split('/').any(|seg| {
        let b = seg.as_bytes();
        // Bare `X:` or `X:\…` / `X:..` sticky remnants rust-url keeps through `..`.
        b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
    })
}

/// rust-url sticky remnants through `..`: Windows `X:`/`X:\…`, pipe `X|`, or `..:`.
fn path_has_sticky_dotdot_remnant(path_and_after: &str) -> bool {
    if path_has_windows_drive_segment(path_and_after) {
        return true;
    }
    let path = path_and_after.split(['?', '#']).next().unwrap_or(path_and_after);
    path.split('/').any(|seg| {
        let b = seg.as_bytes();
        seg == "..:"
            || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b'|')
            || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && b.contains(&b'\\'))
    })
}

fn path_segments(path_and_after: &str) -> Vec<&str> {
    let path = path_and_after.split(['?', '#']).next().unwrap_or(path_and_after);
    path.split('/').collect()
}

fn is_ordered_subsequence(short: &[&str], long: &[&str]) -> bool {
    let mut i = 0;
    for &s in long {
        if i < short.len() && short[i] == s {
            i += 1;
        }
    }
    i == short.len()
}

/// Drop `/X:` path segments rust-url may keep through `..` where ada / sorug drop them.
fn strip_windows_drive_segments(href: &str) -> String {
    // Drop complete `/X:` path segments only (slash + drive letter + colon at
    // segment end). Do not strip a prefix of longer segments like `/I:b`.
    let bytes = href.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 2 < bytes.len()
            && bytes[i] == b'/'
            && bytes[i + 1].is_ascii_alphabetic()
            && bytes[i + 2] == b':'
            && (i + 3 >= bytes.len() || matches!(bytes[i + 3], b'/' | b'?' | b'#'))
        {
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).expect("ASCII drive strip preserves UTF-8")
}

fn normalize_opaque_trailing_space(s: &str) -> String {
    // Rewrite ` #` / ` ?` → `%20#` / `%20?` once at the path/query/fragment edge.
    s.replace(" #", "%20#").replace(" ?", "%20?")
}

/// rust-url 2.5 leaves `^` `` ` `` `{` `}` literal in paths; WPT / sorug
/// percent-encode them (`%5E` `%60` `%7B` `%7D`). Applied before other allowlists.
fn encode_path_gap_chars(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'^' => out.extend_from_slice(b"%5E"),
            b'`' => out.extend_from_slice(b"%60"),
            b'{' => out.extend_from_slice(b"%7B"),
            b'}' => out.extend_from_slice(b"%7D"),
            _ => out.push(b),
        }
    }
    // Only ASCII bytes are rewritten; remaining UTF-8 sequences are untouched.
    String::from_utf8(out).expect("path-gap encode preserves UTF-8")
}

fn is_idna_error(err: &url::ParseError) -> bool {
    // rust-url surfaces IDNA failures as `ParseError::IdnaError` (Display:
    // "invalid international domain name").
    err.to_string().contains("international domain")
}

/// rust-url/ICU accepts some hosts with Arabic end-of-text / triple-dot marks
/// (U+061D/U+061E) that Chrome/ada reject (`domainToASCII` → empty).
fn is_known_rust_url_arabic_punct_idna(input: &str, servo_href: &str) -> bool {
    input.chars().any(|c| matches!(c, '\u{061D}' | '\u{061E}'))
        && servo_href.contains("xn--")
}

/// rust-url accepts some UTS #46-disallowed / reserved code points that
/// Node/ada / sorug reject (UCD `disallowed`). Input may be percent-encoded
/// and may contain ignored tab/LF/CR inside escapes.
fn is_known_rust_url_disallowed_idna(input: &str, servo_href: &str) -> bool {
    if !servo_href.contains("xn--") {
        return false;
    }
    let stripped: String = input
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    percent_decode_lossy(&stripped).chars().any(|c| {
        matches!(
            c,
            // Arabic Extended-B holes
            '\u{0890}'..='\u{0896}'
            // Sidetic / reserved historic (U+10940..=U+1097F)
                | '\u{10940}'..='\u{1097F}'
                // Arabic Extended-C reserved holes after U+10EC7 (U+10EC5..=U+10EC7
                // are valid since Unicode 17; mix rules live in CheckBidi/Ext-C).
                | '\u{10EC8}'..='\u{10EFB}'
                // Other common reserved holes rust-url still ACE-encodes
                | '\u{088E}'..='\u{088F}'
        )
    })
}

/// rust-url is looser on CheckBidi / Ext-B IDNA than Node/ada (e.g. Thaana or
/// Arabic labels combined with Arabic Extended-B U+0870..=U+089F or Ext-A
/// U+08C8..=U+08D2, or Combining Diacritical Marks Extended U+1AC1..=U+1AFF).
/// Strip ignored ASCII + percent-decode first.
fn is_known_rust_url_bidi_idna(input: &str, servo_href: &str) -> bool {
    if !servo_href.contains("xn--") {
        return false;
    }
    let stripped: String = input
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    let decoded = percent_decode_lossy(&stripped);
    let has_ext = decoded.chars().any(|c| {
        matches!(
            c,
            '\u{0870}'..='\u{089F}'
                | '\u{08C8}'..='\u{08D2}'
                | '\u{1AC1}'..='\u{1AFF}' // Node rejects these mixes; rust-url encodes
                // Sidetic (U+10940..=U+1095F): Node CheckBidi-rejects with RTL; rust-url encodes
                | '\u{10940}'..='\u{1095F}'
                // Arabic Extended-C (U+10EC0..=U+10EFF): Node rejects mixes with historic RTL
                | '\u{10EC0}'..='\u{10EFF}'
                // Garay (U+10D40..=U+10D8F): Node rejects mixes with other RTL scripts
                | '\u{10D40}'..='\u{10D8F}'
                // Kawi (U+11F00..=U+11F5F): Node rejects mixes with historic RTL
                | '\u{11F00}'..='\u{11F5F}'
                // Tulu-Tigalari (U+11380..=U+113FF): Node rejects mixes with historic RTL
                | '\u{11380}'..='\u{113FF}'
                // Old Uyghur (U+10F70..=U+10FAF): Node rejects mixes with other RTL scripts
                | '\u{10F70}'..='\u{10FAF}'
                // Gurung Khema (U+16100..=U+1613F): Node rejects mixes with RTL
                | '\u{16100}'..='\u{1613F}'
                // Unassigned gap before Sunuwar (U+11B0A..=U+11BBF): Node rejects mixes with RTL
                | '\u{11B0A}'..='\u{11BBF}'
                // Znamenny musical notation combining marks: Node rejects mixes with RTL
                | '\u{1CF00}'..='\u{1CF2D}'
                | '\u{1CF30}'..='\u{1CF46}'
                // Unassigned / Todhri-area gap (U+1E6C0..=U+1E6FF): Node rejects mixes with RTL
                | '\u{1E6C0}'..='\u{1E6FF}'
                // Combining Cyrillic Small Letter Byelorussian-Ukrainian I: Node CheckBidi-rejects with RTL
                | '\u{1E08F}'
                // Egyptian Hieroglyph format/modifiers: Node rejects mixes with historic RTL
                | '\u{13430}'..='\u{13455}'
        )
    });
    let has_classic_rtl = decoded.chars().any(|c| {
        matches!(
            c,
            '\u{0590}'..='\u{05FF}'
                | '\u{0600}'..='\u{06FF}'
                | '\u{0700}'..='\u{074F}'
                | '\u{0750}'..='\u{077F}'
                | '\u{0780}'..='\u{07BF}'
                | '\u{07C0}'..='\u{07FF}'
                | '\u{0800}'..='\u{085F}'
                | '\u{0860}'..='\u{086A}' // Syriac Supplement
                | '\u{08A0}'..='\u{08C7}'
                | '\u{08D3}'..='\u{08FF}'
                // Arabic presentation forms (e.g. U+FB9E → noon ghunna)
                | '\u{FB50}'..='\u{FDFF}'
                | '\u{FE70}'..='\u{FEFF}'
                // Historic SMP RTL (e.g. Old North Arabian + Sidetic)
                | '\u{10800}'..='\u{1083F}'
                | '\u{10840}'..='\u{1085F}'
                | '\u{10860}'..='\u{1087F}'
                | '\u{10880}'..='\u{108AF}'
                | '\u{108E0}'..='\u{108FF}'
                | '\u{10900}'..='\u{1091F}'
                | '\u{10920}'..='\u{1093F}'
                | '\u{10980}'..='\u{109FF}'
                | '\u{10A00}'..='\u{10A5F}'
                | '\u{10A60}'..='\u{10A7F}'
                | '\u{10A80}'..='\u{10A9F}'
                | '\u{10AC0}'..='\u{10CFF}'
                | '\u{10D00}'..='\u{10D3F}'
                | '\u{10E80}'..='\u{10EBF}'
                | '\u{10F00}'..='\u{10FFF}'
                | '\u{1E800}'..='\u{1E8DF}'
                | '\u{1E900}'..='\u{1E95F}'
                // Arabic Mathematical Alphabetic Symbols (Bidi=AL)
                | '\u{1EE00}'..='\u{1EEFF}'
                // Indic / Ottoman Siyaq (Bidi=AL); Node rejects some mixes with Ext-B
                | '\u{1EC71}'..='\u{1ECB4}'
                | '\u{1ED01}'..='\u{1ED3D}'
        )
    });
    has_ext && has_classic_rtl
}

/// rust-url accepts ZWNJ (U+200C) in labels that fail UTS #46 CheckJoiners /
/// ContextJ (e.g. NKo + Arabic). Node/ada / sorug reject; Arabic↔Arabic and
/// Arabic↔Syriac remain accepted on both sides.
fn is_known_rust_url_zwnj_idna(input: &str, servo_href: &str) -> bool {
    if !servo_href.contains("xn--") {
        return false;
    }
    let stripped: String = input
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    percent_decode_lossy(&stripped).contains('\u{200C}')
}

/// Best-effort percent-decode for allowlist checks (invalid escapes kept literal).
fn percent_decode_lossy(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut i = 0usize;
    let mut out = Vec::with_capacity(bytes.len());
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            out.push((hex_nibble(bytes[i + 1]) << 4) | hex_nibble(bytes[i + 2]));
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_nibble(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

/// rust-url keeps `foo://@` → `foo://`; ada / sorug return failure.
/// Also covers ignored tab/LF/CR/C0 between scheme and `@`.
fn is_known_rust_url_empty_at_authority(input: &str, servo_href: &str) -> bool {
    // Strip C0 controls + tab/LF/CR so `ftp2://@` still matches with leading NULs.
    let cleaned: String = input
        .chars()
        .filter(|c| !matches!(c, '\0'..='\u{20}'))
        .collect();
    let Some((scheme, rest)) = cleaned.split_once("://") else {
        return false;
    };
    if !scheme
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
    {
        return false;
    }
    let after_at = match rest.find('@') {
        Some(i) => &rest[i + 1..],
        None => return false,
    };
    // Host empty (or only controls/space) before path/query/fragment.
    let after_hostish: String = after_at
        .chars()
        .filter(|c| !matches!(c, '\0'..='\u{20}'))
        .collect();
    if !after_hostish.is_empty()
        && !matches!(
            after_hostish.as_bytes().first(),
            Some(b'/' | b'?' | b'#' | b'\\')
        )
    {
        return false;
    }
    let expected = format!("{}://", scheme.to_ascii_lowercase());
    let servo_l = servo_href.to_ascii_lowercase();
    servo_l == expected || servo_l.starts_with(&expected)
}

/// True when the hostname contains any ACE label (`xn--…`).
fn host_has_ace_label(href: &str) -> bool {
    let Some(rest) = href.split_once("://").map(|(_, r)| r) else {
        return false;
    };
    // Authority only (before path/query/fragment) — do not use `@` from the path.
    let authority = rest
        .split_once(['/', '?', '#'])
        .map(|(a, _)| a)
        .unwrap_or(rest);
    let host = match authority.rfind('@') {
        Some(i) => &authority[i + 1..],
        None => authority,
    };
    let host = host
        .split_once(':')
        .map(|(h, _)| h)
        .unwrap_or(host);
    host.split('.')
        .any(|label| label.len() >= 4 && label[..4].eq_ignore_ascii_case("xn--"))
}

/// rust-url / ICU ACE-encodes some hosts that Node-aligned sorug rejects.
/// Require ACE in the *host* (not path) and an IDNA-looking input.
fn is_known_rust_url_idna_table_delta(input: &str, servo_href: &str) -> bool {
    if !host_has_ace_label(servo_href) {
        return false;
    }
    input_suggests_idna(input)
}

fn input_suggests_idna(input: &str) -> bool {
    if input.to_ascii_lowercase().contains("xn--") {
        return true;
    }
    // Non-ASCII in the input is the common IDNA trigger (percent-encoded hosts
    // are covered by the specific disallowed/bidi/zwnj allowlists).
    input.chars().any(|c| !c.is_ascii())
}

/// rust-url is looser on some `file:` hosts / paths; only allow when *input*
/// is also a `file:` URL (case-insensitive), not any servo `file:` href.
fn is_known_rust_url_file_parse_leniency(input: &str, servo_href: &str) -> bool {
    if !servo_href.as_bytes().starts_with(b"file:") {
        return false;
    }
    let trimmed: String = input
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    trimmed.len() >= 5 && trimmed[..5].eq_ignore_ascii_case("file:")
}

/// rust-url sometimes keeps a path segment containing `|` through `..`
/// normalization where ada / sorug / WHATWG drop it (`…/b|/../c` → `…/c`).
fn strip_pipe_path_segments(href: &str) -> String {
    let bytes = href.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && !matches!(bytes[end], b'/' | b'?' | b'#') {
                end += 1;
            }
            if bytes[start..end].contains(&b'|') {
                i = end;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).expect("pipe-segment strip preserves UTF-8")
}

/// Touch all public getters so offset math panics on adversarial parses.
fn assert_invariants(url: &sorug::Url<'_>) {
    let href = url.href();
    let len = href.len();
    assert_eq!(url.as_str(), href);

    // Scheme / protocol always present for a successful parse.
    let scheme = url.scheme();
    let protocol = url.protocol();
    assert!(!scheme.is_empty() || protocol == ":");
    assert!(protocol.ends_with(':') || protocol.is_empty());
    assert!(scheme.len() <= len);
    assert!(protocol.len() <= len);

    let _ = url.username();
    let _ = url.password();
    let _ = url.host();
    let _ = url.hostname();
    let _ = url.host_with_port();
    let _ = url.port_u16();
    let _ = url.port_str();
    let path = url.path();
    let _ = url.pathname();
    let _ = url.query();
    let _ = url.search();
    let _ = url.fragment();
    let _ = url.hash();
    let _ = url.flags();
    let _ = url.has_host();
    let _ = url.backing();

    // Path / search / hash slices must lie inside the serialization.
    assert!(path.len() <= len);
    assert!(url.search().len() <= len);
    assert!(url.hash().len() <= len);

    // Offset markers must be consistent with href length (no OOB when slicing).
    let scheme_end = url.scheme_range().end;
    assert!(scheme_end <= len);
}
