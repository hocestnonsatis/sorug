//! Ultra-fast path for already-canonical absolute special URLs.
//!
//! Hot schemes are identified with a single 64-bit integer compare
//! (`https://`, `file:///`, …). Short inputs use SWAR delimiter scans from
//! [`super::scan`]; long inputs fall back to `memchr`. Successful parses return
//! [`Backing::Borrowed`] — zero heap allocations.

use super::scan::{find_authority_end, find_last_at, find_query_or_hash};
use super::{
    FLAG_HAS_CREDENTIALS, FLAG_HAS_EMPTY_HOST, FLAG_HAS_PASSWORD, FLAG_SPECIAL, ParsedUrl,
};
use crate::Backing;

// ---------------------------------------------------------------------------
// 64-bit little-endian scheme prefixes (one compare ≈ one cycle)
// ---------------------------------------------------------------------------

/// `https://`
const PREFIX_HTTPS: u64 = u64::from_le_bytes(*b"https://");
/// `http://` in the low 7 bytes (`\0` in lane 7).
const PREFIX_HTTP: u64 = u64::from_le_bytes(*b"http://\0");
const MASK_7: u64 = 0x00FF_FFFF_FFFF_FFFF;
/// `file:///`
const PREFIX_FILE: u64 = u64::from_le_bytes(*b"file:///");
/// `ftp://` in the low 6 bytes.
const PREFIX_FTP: u64 = u64::from_le_bytes(*b"ftp://\0\0");
const MASK_6: u64 = 0x0000_FFFF_FFFF_FFFF;
/// `wss://` in the low 6 bytes.
const PREFIX_WSS: u64 = u64::from_le_bytes(*b"wss://\0\0");
/// `ws://` in the low 5 bytes.
const PREFIX_WS: u64 = u64::from_le_bytes(*b"ws://\0\0\0");
const MASK_5: u64 = 0x0000_00FF_FFFF_FFFF;

#[inline(always)]
fn load_prefix(bytes: &[u8]) -> u64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&bytes[..8]);
    u64::from_le_bytes(buf)
}

#[cold]
#[inline(never)]
fn cold_none<'a>() -> Option<ParsedUrl<'a>> {
    None
}

/// Attempt a zero-copy parse of a special absolute URL.
///
/// Returns `None` when any mutation would be required (uppercase scheme/host,
/// IDNA, path shorten, percent-encoding, default-port elision, …).
#[inline(always)]
pub(crate) fn try_fast_special_absolute(input: &str) -> Option<ParsedUrl<'_>> {
    let bytes = input.as_bytes();
    if bytes.len() < 8 {
        return cold_none();
    }

    let prefix = load_prefix(bytes);

    // Hottest schemes first for branch prediction.
    if prefix == PREFIX_HTTPS {
        return try_fast_authority(input, bytes, 5, 8, 443);
    }
    if prefix & MASK_7 == PREFIX_HTTP {
        return try_fast_authority(input, bytes, 4, 7, 80);
    }
    if prefix == PREFIX_FILE {
        return try_fast_file(input, bytes);
    }
    if prefix & MASK_6 == PREFIX_FTP {
        return try_fast_authority(input, bytes, 3, 6, 21);
    }
    if prefix & MASK_6 == PREFIX_WSS {
        return try_fast_authority(input, bytes, 3, 6, 443);
    }
    if prefix & MASK_5 == PREFIX_WS {
        return try_fast_authority(input, bytes, 2, 5, 80);
    }

    cold_none()
}

/// `file:///` with empty host — path may contain uppercase (Windows drives).
#[inline(always)]
fn try_fast_file<'a>(input: &'a str, bytes: &'a [u8]) -> Option<ParsedUrl<'a>> {
    debug_assert_eq!(bytes.get(7), Some(&b'/'));
    let (query_start, fragment_start) = scan_path_query_fragment(7, bytes)?;

    Some(ParsedUrl {
        serialization: Backing::Borrowed(input),
        scheme_end: 4,
        username_end: 7,
        host_start: 7,
        host_end: 7,
        port: None,
        path_start: 7,
        query_start,
        fragment_start,
        flags: FLAG_SPECIAL | FLAG_HAS_EMPTY_HOST,
    })
}

/// http(s)/ws(s)/ftp with `scheme://` already verified via 64-bit prefix.
#[inline(always)]
fn try_fast_authority<'a>(
    input: &'a str,
    bytes: &'a [u8],
    scheme_end: usize,
    auth_start: usize,
    default_port: u16,
) -> Option<ParsedUrl<'a>> {
    if auth_start >= bytes.len() {
        return cold_none();
    }

    let auth_rel_end = find_authority_end(&bytes[auth_start..], true);
    let auth_end = auth_start + auth_rel_end;
    if auth_rel_end == 0 {
        return cold_none();
    }
    // `\` as authority terminator ⇒ would normalize to `/` → owned.
    if auth_end < bytes.len() && bytes[auth_end] == b'\\' {
        return cold_none();
    }

    let mut flags = FLAG_SPECIAL;
    let auth_bytes = &bytes[auth_start..auth_end];

    let (username_end, host_region_start) = if let Some(at) = find_last_at(auth_bytes, auth_rel_end)
    {
        if at == 0 {
            return cold_none();
        }
        let userinfo = &auth_bytes[..at];
        if !userinfo_is_clean(userinfo) {
            return cold_none();
        }
        flags |= FLAG_HAS_CREDENTIALS;
        let username_end = if let Some(ui_colon) = find_byte(userinfo, b':') {
            // Empty password → href omits the trailing ':'; extra ':' in the
            // password must be percent-encoded. Both require owned serialization.
            if ui_colon + 1 >= userinfo.len() || userinfo[ui_colon + 1..].contains(&b':') {
                return cold_none();
            }
            flags |= FLAG_HAS_PASSWORD;
            auth_start + ui_colon
        } else {
            auth_start + at
        };
        (username_end, auth_start + at + 1)
    } else {
        (auth_start, auth_start)
    };

    if host_region_start >= auth_end {
        return cold_none();
    }
    let host_region = &bytes[host_region_start..auth_end];

    let (host_end_rel, port) = match rfind_byte(host_region, b':') {
        Some(pcolon) => {
            let port_bytes = &host_region[pcolon + 1..];
            if port_bytes.is_empty() || !is_ascii_digits(port_bytes) {
                return cold_none();
            }
            let port = parse_u16_digits(port_bytes)?;
            if port == default_port {
                return cold_none();
            }
            // Href must use the decimal form without leading zeros. Bail to the
            // slow path so serialization is rewritten via itoa.
            if port_bytes.len() > 1 && port_bytes[0] == b'0' {
                return cold_none();
            }
            (pcolon, Some(port))
        }
        None => (host_region.len(), None),
    };

    let host = &host_region[..host_end_rel];
    if host.is_empty() || !host_is_clean_domain(host) || host_ends_in_a_number(host) {
        return cold_none();
    }

    let host_start = host_region_start as u32;
    let host_end = (host_region_start + host_end_rel) as u32;

    if auth_end == bytes.len() || bytes[auth_end] != b'/' {
        return cold_none();
    }

    let (query_start, fragment_start) = scan_path_query_fragment(auth_end, bytes)?;

    Some(ParsedUrl {
        serialization: Backing::Borrowed(input),
        scheme_end: scheme_end as u32,
        username_end: username_end as u32,
        host_start,
        host_end,
        port,
        path_start: auth_end as u32,
        query_start,
        fragment_start,
        flags,
    })
}

#[inline(always)]
fn find_byte(bytes: &[u8], needle: u8) -> Option<usize> {
    bytes.iter().position(|&c| c == needle)
}

#[inline(always)]
fn rfind_byte(bytes: &[u8], needle: u8) -> Option<usize> {
    bytes.iter().rposition(|&c| c == needle)
}

#[inline(always)]
fn is_ascii_digits(bytes: &[u8]) -> bool {
    bytes.iter().all(|b| b.is_ascii_digit())
}

#[inline(always)]
fn parse_u16_digits(bytes: &[u8]) -> Option<u16> {
    let mut n: u32 = 0;
    for &b in bytes {
        n = n * 10 + u32::from(b - b'0');
        if n > u32::from(u16::MAX) {
            return None;
        }
    }
    Some(n as u16)
}

/// Domain host: lowercase ASCII alphanumerics, `.`, `-` only.
#[inline(always)]
fn host_is_clean_domain(host: &[u8]) -> bool {
    for &c in host {
        if !matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-') {
            return false;
        }
    }
    true
}

/// Fast `ends_in_a_number` for already-validated lowercase ASCII domains.
#[inline(always)]
fn host_ends_in_a_number(host: &[u8]) -> bool {
    let mut end = host.len();
    if end == 0 {
        return false;
    }
    if host[end - 1] == b'.' {
        end -= 1;
        if end == 0 {
            return false;
        }
    }
    let start = match memchr::memrchr(b'.', &host[..end]) {
        Some(i) => i + 1,
        None => 0,
    };
    let last = &host[start..end];
    if last.is_empty() {
        return false;
    }
    // Letters outside `a-f`/`x` cannot appear in decimal/octal/hex IPv4 labels.
    // Note: `x` must be allowed for `0x…` hex forms.
    if last.iter().any(|&c| matches!(c, b'g'..=b'w' | b'y'..=b'z')) {
        return false;
    }
    if last.iter().all(|&c| c.is_ascii_digit()) {
        return true;
    }
    super::host::ends_in_a_number(core::str::from_utf8(host).unwrap_or(""))
}

/// Single-pass path / query / fragment scan + validation (no double `memchr`).
#[inline(always)]
fn scan_path_query_fragment(path_base: usize, bytes: &[u8]) -> Option<(Option<u32>, Option<u32>)> {
    let rest = &bytes[path_base..];
    debug_assert_eq!(rest.first(), Some(&b'/'));

    match find_query_or_hash(rest) {
        Some((i, b'?')) => {
            validate_path_only(&rest[..i])?;
            let q = path_base + i;
            let after_q = &bytes[q + 1..];
            match find_byte(after_q, b'#') {
                Some(j) => {
                    validate_query(&after_q[..j])?;
                    validate_fragment(&after_q[j + 1..])?;
                    Some((Some(q as u32), Some((q + 1 + j) as u32)))
                }
                None => {
                    validate_query(after_q)?;
                    Some((Some(q as u32), None))
                }
            }
        }
        Some((i, b'#')) => {
            validate_path_only(&rest[..i])?;
            let f = path_base + i;
            validate_fragment(&bytes[f + 1..])?;
            Some((None, Some(f as u32)))
        }
        Some(_) => None,
        None => {
            validate_path_only(rest)?;
            Some((None, None))
        }
    }
}

#[inline(always)]
fn userinfo_is_clean(userinfo: &[u8]) -> bool {
    for &c in userinfo {
        if matches!(
            c,
            0x00..=0x1f
                | 0x7f..=0xff
                | b' '
                | b'"'
                | b'#'
                | b'<'
                | b'>'
                | b'?'
                | b'^'
                | b'`'
                | b'{'
                | b'}'
                | b'/'
                | b';'
                | b'='
                | b'@'
                | b'['
                | b'\\'
                | b']'
                | b'|'
        ) {
            return false;
        }
    }
    true
}

#[inline(always)]
fn validate_path_only(path: &[u8]) -> Option<()> {
    let mut i = 0;
    while i < path.len() {
        if path[i] != b'/' {
            return None;
        }
        i += 1;
        let start = i;
        while i < path.len() && path[i] != b'/' {
            let c = path[i];
            if matches!(
                c,
                0x00..=0x1f
                    | 0x7f..=0xff
                    | b' '
                    | b'"'
                    | b'#'
                    | b'<'
                    | b'>'
                    | b'?'
                    | b'^'
                    | b'`'
                    | b'{'
                    | b'}'
                    | b'\\'
                    | b'|' // file Windows drive `w|` → `w:` needs owned
            ) {
                return None;
            }
            i += 1;
        }
        match &path[start..i] {
            b"." | b".." | b"%2e" | b"%2E" | b"%2e%2e" | b"%2e%2E" | b"%2E%2e" | b"%2E%2E"
            | b"%2e." | b"%2E." | b".%2e" | b".%2E" => return None,
            _ => {}
        }
    }
    Some(())
}

#[inline(always)]
fn validate_query(q: &[u8]) -> Option<()> {
    for &c in q {
        if matches!(
            c,
            0x00..=0x1f | 0x7f..=0xff | b' ' | b'"' | b'#' | b'<' | b'>' | b'\''
        ) {
            return None;
        }
    }
    Some(())
}

#[inline(always)]
fn validate_fragment(f: &[u8]) -> Option<()> {
    for &c in f {
        if matches!(
            c,
            0x00..=0x1f | 0x7f..=0xff | b' ' | b'"' | b'<' | b'>' | b'`'
        ) {
            return None;
        }
    }
    Some(())
}
