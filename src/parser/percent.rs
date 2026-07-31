//! Percent-encode sets from the WHATWG URL Standard.

use super::scan::find_first_encode;

/// Minimal append surface shared by [`String`] and the CoW serialization buffer.
pub(crate) trait AppendBuf {
    fn push(&mut self, c: char);
    fn push_str(&mut self, s: &str);
}

impl AppendBuf for String {
    #[inline]
    fn push(&mut self, c: char) {
        String::push(self, c);
    }

    #[inline]
    fn push_str(&mut self, s: &str) {
        String::push_str(self, s);
    }
}

/// C0 controls and space — used as the base of several encode sets.
#[inline]
#[allow(dead_code)]
pub(crate) fn is_c0_control_or_space(c: u8) -> bool {
    c <= 0x20
}

/// https://url.spec.whatwg.org/#c0-control-percent-encode-set
#[inline]
pub(crate) fn in_c0_encode_set(c: u8) -> bool {
    c <= 0x1f || c > 0x7e
}

/// https://url.spec.whatwg.org/#fragment-percent-encode-set
#[inline]
pub(crate) fn in_fragment_encode_set(c: u8) -> bool {
    in_c0_encode_set(c) || matches!(c, b' ' | b'"' | b'<' | b'>' | b'`')
}

/// https://url.spec.whatwg.org/#query-percent-encode-set
#[inline]
pub(crate) fn in_query_encode_set(c: u8) -> bool {
    in_c0_encode_set(c) || matches!(c, b' ' | b'"' | b'#' | b'<' | b'>')
}

/// https://url.spec.whatwg.org/#special-query-percent-encode-set
#[inline]
pub(crate) fn in_special_query_encode_set(c: u8) -> bool {
    in_query_encode_set(c) || c == b'\''
}

/// https://url.spec.whatwg.org/#path-percent-encode-set
#[inline]
pub(crate) fn in_path_encode_set(c: u8) -> bool {
    in_query_encode_set(c) || matches!(c, b'?' | b'^' | b'`' | b'{' | b'}')
}

/// https://url.spec.whatwg.org/#userinfo-percent-encode-set
#[inline]
pub(crate) fn in_userinfo_encode_set(c: u8) -> bool {
    in_path_encode_set(c)
        || matches!(
            c,
            b'/' | b':' | b';' | b'=' | b'@' | b'[' | b'\\' | b']' | b'|'
        )
}

/// Path segment encode set (+ `%` so existing sequences are re-encoded when needed by callers).
#[inline]
#[allow(dead_code)]
pub(crate) fn in_path_segment_encode_set(c: u8) -> bool {
    in_path_encode_set(c) || c == b'/' || c == b'%'
}

#[inline]
#[allow(dead_code)]
pub(crate) fn in_special_path_segment_encode_set(c: u8) -> bool {
    in_path_segment_encode_set(c) || c == b'\\'
}

#[inline]
fn append_percent(out: &mut impl AppendBuf, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push('%');
    out.push(HEX[(byte >> 4) as usize] as char);
    out.push(HEX[(byte & 0xf) as usize] as char);
}

/// UTF-8 percent-encode `input` with the given predicate; append to `out`.
///
/// Fast path: if no byte needs encoding, bulk-append via `push_str`.
/// Otherwise copy safe runs in bulk and percent-encode only the rest.
pub(crate) fn utf8_percent_encode(
    input: &str,
    encode: impl Fn(u8) -> bool,
    out: &mut impl AppendBuf,
) {
    let bytes = input.as_bytes();
    let Some(first) = find_first_encode(bytes, &encode) else {
        out.push_str(input);
        return;
    };

    // Encode-set members used by the URL parser are ASCII, so `first` is a char boundary.
    debug_assert!(input.is_char_boundary(first));
    if first > 0 {
        out.push_str(&input[..first]);
    }

    let mut i = first;
    while i < bytes.len() {
        let start = i;
        while i < bytes.len() && !encode(bytes[i]) {
            i += 1;
        }
        if i > start {
            debug_assert!(input.is_char_boundary(start) && input.is_char_boundary(i));
            out.push_str(&input[start..i]);
        }
        while i < bytes.len() && encode(bytes[i]) {
            append_percent(out, bytes[i]);
            i += 1;
        }
    }
}

/// Percent-encode a single Unicode code point (UTF-8 form) if any of its bytes
/// are in the encode set; otherwise push the char as-is when ASCII-safe.
pub(crate) fn percent_encode_char(
    c: char,
    encode: impl Fn(u8) -> bool,
    out: &mut impl AppendBuf,
) {
    let mut buf = [0u8; 4];
    let encoded = c.encode_utf8(&mut buf);
    if encoded.bytes().any(|b| encode(b)) {
        for &b in encoded.as_bytes() {
            append_percent(out, b);
        }
    } else {
        out.push_str(encoded);
    }
}

/// Append a path segment with bulk-copy fast path when no encoding is needed.
#[inline]
pub(crate) fn append_path_segment(segment: &str, out: &mut impl AppendBuf) {
    utf8_percent_encode(segment, in_path_encode_set, out);
}

/// Append query bytes with bulk-copy fast path.
#[inline]
pub(crate) fn append_query(segment: &str, special: bool, out: &mut impl AppendBuf) {
    if special {
        utf8_percent_encode(segment, in_special_query_encode_set, out);
    } else {
        utf8_percent_encode(segment, in_query_encode_set, out);
    }
}

/// Append fragment bytes with bulk-copy fast path.
#[inline]
pub(crate) fn append_fragment(segment: &str, out: &mut impl AppendBuf) {
    utf8_percent_encode(segment, in_fragment_encode_set, out);
}

/// Decode percent-encoded bytes; invalid sequences are left as literal bytes.
pub(crate) fn percent_decode(input: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    if memchr::memchr(b'%', input).is_none() {
        return std::borrow::Cow::Borrowed(input);
    }
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'%' && i + 2 < input.len() {
            if let (Some(h), Some(l)) = (from_hex(input[i + 1]), from_hex(input[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(input[i]);
        i += 1;
    }
    std::borrow::Cow::Owned(out)
}

#[inline]
fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
