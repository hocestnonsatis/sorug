//! Delimiter / byte scanning: SWAR for short inputs, [`memchr`] for long.

#![allow(clippy::match_same_arms)]

use memchr::{memchr, memchr2, memchr3, memrchr};

/// Inputs shorter than this use register-level SWAR instead of `memchr`.
///
/// Tuned for URL component scans (host/path/query): most authority and path
/// segments are well under 64 bytes; `memchr`'s setup cost wins only on longer
/// buffers. Keep in sync with Criterion `url_parse` / mutation benches — do not
/// raise without evidence on Fast_Path_ASCII and Complex_Query_Fragment.
const SWAR_THRESHOLD: usize = 64;

const ONES: u64 = 0x0101_0101_0101_0101;
const HIGHS: u64 = 0x8080_8080_8080_8080;

/// Load 8 bytes at `bytes[i..]` as a little-endian `u64` (caller guarantees length).
#[inline(always)]
fn load_u64_le(bytes: &[u8], i: usize) -> u64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&bytes[i..i + 8]);
    u64::from_le_bytes(buf)
}

/// Classic SWAR zero-byte detector: true if any lane of `x` is `0`.
#[inline(always)]
const fn has_zero_byte(x: u64) -> u64 {
    x.wrapping_sub(ONES) & !x & HIGHS
}

/// Index of the first matching byte within a SWAR match mask, or `8` if empty.
#[inline(always)]
const fn first_match_index(mask: u64) -> u32 {
    mask.trailing_zeros() >> 3
}

/// Index of the last matching byte within a SWAR match mask, or `None`.
#[inline(always)]
const fn last_match_index(mask: u64) -> Option<u32> {
    if mask == 0 {
        None
    } else {
        Some(63u32.saturating_sub(mask.leading_zeros()) >> 3)
    }
}

#[inline(always)]
fn splat(b: u8) -> u64 {
    u64::from(b) * ONES
}

/// SWAR: first index of `needle`, or `None`.
#[inline(always)]
fn swar_find(haystack: &[u8], needle: u8) -> Option<usize> {
    let n = splat(needle);
    let mut i = 0;
    let len = haystack.len();
    while i + 8 <= len {
        let word = load_u64_le(haystack, i);
        let mask = has_zero_byte(word ^ n);
        if mask != 0 {
            return Some(i + first_match_index(mask) as usize);
        }
        i += 8;
    }
    while i < len {
        if haystack[i] == needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// SWAR: last index of `needle` in `haystack[0..end]`.
///
/// Candidates from [`has_zero_byte`] are verified: a true zero-byte match can
/// borrow into later `0x01` lanes (e.g. `'A' ^ '@'`), producing false positives
/// at higher indices. Always confirm `haystack[idx] == needle`.
#[inline(always)]
fn swar_rfind(haystack: &[u8], end: usize, needle: u8) -> Option<usize> {
    let end = end.min(haystack.len());
    let n = splat(needle);
    let mut i = end;
    while i >= 8 {
        i -= 8;
        let word = load_u64_le(haystack, i);
        let mut mask = has_zero_byte(word ^ n);
        while let Some(lane) = last_match_index(mask) {
            let idx = i + lane as usize;
            if idx < end && haystack[idx] == needle {
                return Some(idx);
            }
            // Clear this lane's high bit; keep scanning earlier lanes.
            mask &= !(0x80u64 << (lane * 8));
        }
    }
    while i > 0 {
        i -= 1;
        if haystack[i] == needle {
            return Some(i);
        }
    }
    None
}

/// SWAR: first index of any of two needles.
#[inline(always)]
fn swar_find2(haystack: &[u8], n1: u8, n2: u8) -> Option<usize> {
    let v1 = splat(n1);
    let v2 = splat(n2);
    let mut i = 0;
    let len = haystack.len();
    while i + 8 <= len {
        let word = load_u64_le(haystack, i);
        let mask = has_zero_byte(word ^ v1) | has_zero_byte(word ^ v2);
        if mask != 0 {
            return Some(i + first_match_index(mask) as usize);
        }
        i += 8;
    }
    while i < len {
        let b = haystack[i];
        if b == n1 || b == n2 {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// SWAR: first index of any of three needles.
#[inline(always)]
fn swar_find3(haystack: &[u8], n1: u8, n2: u8, n3: u8) -> Option<usize> {
    let v1 = splat(n1);
    let v2 = splat(n2);
    let v3 = splat(n3);
    let mut i = 0;
    let len = haystack.len();
    while i + 8 <= len {
        let word = load_u64_le(haystack, i);
        let mask = has_zero_byte(word ^ v1) | has_zero_byte(word ^ v2) | has_zero_byte(word ^ v3);
        if mask != 0 {
            return Some(i + first_match_index(mask) as usize);
        }
        i += 8;
    }
    while i < len {
        let b = haystack[i];
        if b == n1 || b == n2 || b == n3 {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// SWAR: first index of any of four needles.
#[inline(always)]
fn swar_find4(haystack: &[u8], n1: u8, n2: u8, n3: u8, n4: u8) -> Option<usize> {
    let v1 = splat(n1);
    let v2 = splat(n2);
    let v3 = splat(n3);
    let v4 = splat(n4);
    let mut i = 0;
    let len = haystack.len();
    while i + 8 <= len {
        let word = load_u64_le(haystack, i);
        let mask = has_zero_byte(word ^ v1)
            | has_zero_byte(word ^ v2)
            | has_zero_byte(word ^ v3)
            | has_zero_byte(word ^ v4);
        if mask != 0 {
            return Some(i + first_match_index(mask) as usize);
        }
        i += 8;
    }
    while i < len {
        let b = haystack[i];
        if b == n1 || b == n2 || b == n3 || b == n4 {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// First of four needles, or `None`.
#[inline(always)]
fn find4(n1: u8, n2: u8, n3: u8, n4: u8, haystack: &[u8]) -> Option<usize> {
    if haystack.len() < SWAR_THRESHOLD {
        swar_find4(haystack, n1, n2, n3, n4)
    } else {
        match (memchr3(n1, n2, n3, haystack), memchr(n4, haystack)) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => None,
        }
    }
}

#[inline(always)]
fn find3(n1: u8, n2: u8, n3: u8, haystack: &[u8]) -> Option<usize> {
    if haystack.len() < SWAR_THRESHOLD {
        swar_find3(haystack, n1, n2, n3)
    } else {
        memchr3(n1, n2, n3, haystack)
    }
}

#[inline(always)]
fn find2(n1: u8, n2: u8, haystack: &[u8]) -> Option<usize> {
    if haystack.len() < SWAR_THRESHOLD {
        swar_find2(haystack, n1, n2)
    } else {
        memchr2(n1, n2, haystack)
    }
}

#[inline(always)]
fn find1(needle: u8, haystack: &[u8]) -> Option<usize> {
    if haystack.len() < SWAR_THRESHOLD {
        swar_find(haystack, needle)
    } else {
        memchr(needle, haystack)
    }
}

/// First authority-terminating delimiter: `/`, `?`, `#`, and `\` when `special`.
#[inline(always)]
pub(crate) fn find_authority_end(haystack: &[u8], special: bool) -> usize {
    if special {
        find4(b'/', b'?', b'#', b'\\', haystack).unwrap_or(haystack.len())
    } else {
        find3(b'/', b'?', b'#', haystack).unwrap_or(haystack.len())
    }
}

/// Byte offset of the last `@` in `haystack[0..end]`, if any.
#[inline(always)]
pub(crate) fn find_last_at(haystack: &[u8], end: usize) -> Option<usize> {
    let end = end.min(haystack.len());
    if end < SWAR_THRESHOLD {
        swar_rfind(haystack, end, b'@')
    } else {
        memrchr(b'@', &haystack[..end])
    }
}

/// First host-terminating delimiter outside IPv6 brackets.
///
/// Returns `(byte_end, saw_tab_or_newline)`. When tabs/newlines appear before the
/// delimiter, the caller should use the slow char-wise path.
#[inline]
pub(crate) fn find_host_end(haystack: &[u8], special: bool) -> (usize, bool) {
    // Leading tab/LF/CR are ignored; IPv6 still starts with `[` after them.
    let mut start = 0;
    let mut leading_ignored = false;
    while start < haystack.len() && matches!(haystack[start], b'\t' | b'\n' | b'\r') {
        leading_ignored = true;
        start += 1;
    }
    if haystack.get(start) == Some(&b'[') {
        let (end, ignored) = find_host_end_bracketed(&haystack[start..], special);
        return (start + end, leading_ignored || ignored);
    }

    let delim = if special {
        match (
            find4(b':', b'/', b'?', b'#', haystack),
            find1(b'\\', haystack),
        ) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => None,
        }
    } else {
        find4(b':', b'/', b'?', b'#', haystack)
    };
    let end = delim.unwrap_or(haystack.len());
    let ignored = leading_ignored || find3(b'\t', b'\n', b'\r', &haystack[..end]).is_some();
    (end, ignored)
}

fn find_host_end_bracketed(haystack: &[u8], special: bool) -> (usize, bool) {
    let mut inside = false;
    let mut ignored = false;
    let mut i = 0;
    while i < haystack.len() {
        match haystack[i] {
            b'[' => inside = true,
            b']' => inside = false,
            b':' if !inside => break,
            b'\\' if special && !inside => break,
            b'/' | b'?' | b'#' if !inside => break,
            b'\t' | b'\n' | b'\r' => ignored = true,
            _ => {}
        }
        i += 1;
    }
    (i, ignored)
}

/// First path-segment delimiter: `/`, `?`, `#`, and `\` when `special`.
#[inline(always)]
pub(crate) fn find_path_delim(haystack: &[u8], special: bool) -> Option<(usize, u8)> {
    let i = if special {
        find4(b'/', b'\\', b'?', b'#', haystack)?
    } else {
        find3(b'/', b'?', b'#', haystack)?
    };
    Some((i, haystack[i]))
}

/// Path delimiters for component setters: `?` / `#` are content, not terminators.
pub(crate) fn find_path_delim_setter(haystack: &[u8], special: bool) -> Option<(usize, u8)> {
    let i = if special {
        find2(b'/', b'\\', haystack)?
    } else {
        find1(b'/', haystack)?
    };
    Some((i, haystack[i]))
}

/// First `#` in the query (fragment start).
#[inline(always)]
pub(crate) fn find_hash(haystack: &[u8]) -> Option<usize> {
    find1(b'#', haystack)
}

/// First `?` or `#` (opaque-path / query boundary).
#[inline(always)]
pub(crate) fn find_query_or_hash(haystack: &[u8]) -> Option<(usize, u8)> {
    let i = find2(b'?', b'#', haystack)?;
    Some((i, haystack[i]))
}

/// Whether `bytes` contains ASCII tab / LF / CR.
#[inline(always)]
pub(crate) fn has_ascii_tab_or_newline(bytes: &[u8]) -> bool {
    find3(b'\t', b'\n', b'\r', bytes).is_some()
}

/// File-host terminator: `/`, `\`, `?`, `#`.
#[inline(always)]
pub(crate) fn find_file_host_end(haystack: &[u8]) -> (usize, bool) {
    let end = find4(b'/', b'\\', b'?', b'#', haystack).unwrap_or(haystack.len());
    let ignored = has_ascii_tab_or_newline(&haystack[..end]);
    (end, ignored)
}

/// Index of the first byte for which `needs_encode` is true, if any.
#[inline]
pub(crate) fn find_first_encode(bytes: &[u8], needs_encode: impl Fn(u8) -> bool) -> Option<usize> {
    bytes.iter().position(|&b| needs_encode(b))
}

#[cfg(test)]
mod swar_at_tests {
    use super::*;

    #[test]
    fn last_at_before_capital_a() {
        let s = b"r:pass@Api.example.com/";
        let end = find_authority_end(s, true);
        let at = find_last_at(s, end);
        assert_eq!(end, 22);
        assert_eq!(at, Some(6), "slice={:?}", core::str::from_utf8(&s[..end]));
    }

    #[test]
    fn swar_rfind_a_vs_at() {
        assert_eq!(swar_rfind(b"AAAAAAAA", 8, b'@'), None);
        assert_eq!(swar_rfind(b"AAA@AAAA", 8, b'@'), Some(3));
        assert_eq!(swar_rfind(b"r:pass@A", 8, b'@'), Some(6));
        assert_eq!(swar_rfind(b"pass@Api", 8, b'@'), Some(4));
        // Borrow false-positives must not win over an earlier real '@'.
        assert_eq!(swar_rfind(b"x@ABCDEF", 8, b'@'), Some(1));
    }

    #[test]
    fn credentials_before_capital_a_host() {
        let url = crate::Url::parse("https://r:pass@Api.example.com/").unwrap();
        assert_eq!(url.href(), "https://r:pass@api.example.com/");
    }
}
