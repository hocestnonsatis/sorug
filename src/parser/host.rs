//! Host parsing: domain (IDNA), opaque host, IPv4, IPv6.

use std::borrow::Cow;
use std::fmt::{self, Write};
use std::net::{Ipv4Addr, Ipv6Addr};

use super::percent::{in_c0_encode_set, percent_decode, utf8_percent_encode};
use super::punycode;
use crate::ParseError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Host<'a> {
    Domain(Cow<'a, str>),
    Ipv4(Ipv4Addr),
    Ipv6(Ipv6Addr),
}

impl fmt::Display for Host<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Domain(d) => f.write_str(d),
            Self::Ipv4(a) => write!(f, "{a}"),
            Self::Ipv6(a) => {
                f.write_str("[")?;
                write_ipv6(a, f)?;
                f.write_str("]")
            }
        }
    }
}

/// Result of appending a special host directly into the serialization buffer.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum AppendedHost {
    EmptyDomain,
    Domain,
    Ipv4,
    Ipv6,
}

/// Parse a special-URL host (domain / IPv4 / IPv6).
pub(crate) fn parse_host(input: &str) -> Result<Host<'_>, ParseError> {
    if let Some(inner) = input.strip_prefix('[') {
        let Some(ipv6) = inner.strip_suffix(']') else {
            return Err(ParseError::Failure);
        };
        return parse_ipv6(ipv6).map(Host::Ipv6);
    }

    let decoded_cow = percent_decode(input.as_bytes());
    match decoded_cow {
        Cow::Borrowed(bytes) => {
            let decoded = std::str::from_utf8(bytes).map_err(|_| ParseError::Failure)?;
            domain_or_ipv4(decoded)
        }
        Cow::Owned(bytes) => {
            let decoded = std::str::from_utf8(&bytes).map_err(|_| ParseError::Failure)?;
            Ok(match domain_or_ipv4(decoded)? {
                Host::Domain(d) => Host::Domain(Cow::Owned(d.into_owned())),
                Host::Ipv4(a) => Host::Ipv4(a),
                Host::Ipv6(a) => Host::Ipv6(a),
            })
        }
    }
}

/// Parse a special host and append its href form to `out` (no intermediate ACE `String`
/// for the domain path — ACE is written once into the URL buffer).
pub(crate) fn append_host(
    input: &str,
    out: &mut super::serialization::SerializationBuf<'_>,
) -> Result<AppendedHost, ParseError> {
    if let Some(inner) = input.strip_prefix('[') {
        let Some(ipv6) = inner.strip_suffix(']') else {
            return Err(ParseError::Failure);
        };
        let addr = parse_ipv6(ipv6)?;
        out.push('[');
        write_ipv6_to(out, &addr).map_err(|_| ParseError::Failure)?;
        out.push(']');
        return Ok(AppendedHost::Ipv6);
    }

    let decoded_cow = percent_decode(input.as_bytes());
    let decoded = match &decoded_cow {
        Cow::Borrowed(bytes) => std::str::from_utf8(bytes).map_err(|_| ParseError::Failure)?,
        Cow::Owned(bytes) => std::str::from_utf8(bytes).map_err(|_| ParseError::Failure)?,
    };
    append_domain_or_ipv4(decoded, out)
}

fn domain_or_ipv4(decoded: &str) -> Result<Host<'_>, ParseError> {
    let ascii = punycode::to_ascii(decoded).map_err(|()| ParseError::Failure)?;
    if ascii.is_empty() {
        return Err(ParseError::Failure);
    }
    if ends_in_a_number(&ascii) {
        Ok(Host::Ipv4(parse_ipv4(&ascii)?))
    } else {
        Ok(Host::Domain(ascii))
    }
}

fn append_domain_or_ipv4(
    decoded: &str,
    out: &mut super::serialization::SerializationBuf<'_>,
) -> Result<AppendedHost, ParseError> {
    let start = out.len();
    punycode::to_ascii_append_validated(decoded, out).map_err(|()| ParseError::Failure)?;
    let ascii = &out.as_str()[start..];
    if ascii.is_empty() {
        out.truncate(start);
        return Err(ParseError::Failure);
    }
    if ends_in_a_number(ascii) {
        let ip = parse_ipv4(ascii)?;
        out.truncate(start);
        write_ipv4_to(out, ip);
        Ok(AppendedHost::Ipv4)
    } else {
        Ok(AppendedHost::Domain)
    }
}

fn write_ipv4_to(out: &mut super::serialization::SerializationBuf<'_>, ip: Ipv4Addr) {
    let octets = ip.octets();
    let mut first = true;
    for o in octets {
        if !first {
            out.push('.');
        }
        first = false;
        // Tiny itoa for 0..=255.
        if o >= 100 {
            out.push(char::from(b'0' + o / 100));
            out.push(char::from(b'0' + (o / 10) % 10));
            out.push(char::from(b'0' + o % 10));
        } else if o >= 10 {
            out.push(char::from(b'0' + o / 10));
            out.push(char::from(b'0' + o % 10));
        } else {
            out.push(char::from(b'0' + o));
        }
    }
}

fn write_ipv6_to(
    out: &mut super::serialization::SerializationBuf<'_>,
    addr: &Ipv6Addr,
) -> Result<(), fmt::Error> {
    let segments = addr.segments();
    let (compress_start, compress_end) = longest_zero_sequence(&segments);
    let mut i = 0isize;
    while i < 8 {
        if i == compress_start {
            out.push(':');
            if i == 0 {
                out.push(':');
            }
            if compress_end < 8 {
                i = compress_end;
            } else {
                break;
            }
        }
        let _ = write!(out, "{:x}", segments[i as usize]);
        if i < 7 {
            out.push(':');
        }
        i += 1;
    }
    Ok(())
}

/// Parse a non-special (opaque) host.
pub(crate) fn parse_opaque_host(input: &str) -> Result<Host<'static>, ParseError> {
    // WHATWG host state ignores ASCII tab/LF/CR before other processing.
    let cleaned: Cow<'_, str> = if input.bytes().any(|b| matches!(b, b'\t' | b'\n' | b'\r')) {
        Cow::Owned(
            input
                .bytes()
                .filter(|&b| !matches!(b, b'\t' | b'\n' | b'\r'))
                .map(char::from)
                .collect(),
        )
    } else {
        Cow::Borrowed(input)
    };

    if let Some(inner) = cleaned.strip_prefix('[') {
        let Some(ipv6) = inner.strip_suffix(']') else {
            return Err(ParseError::Failure);
        };
        return parse_ipv6(ipv6).map(Host::Ipv6);
    }

    if cleaned.bytes().any(|c| is_forbidden_host_code_point(c)) {
        return Err(ParseError::Failure);
    }

    let mut out = String::new();
    utf8_percent_encode(&cleaned, in_c0_encode_set, &mut out);
    Ok(Host::Domain(Cow::Owned(out)))
}

#[inline]
fn is_forbidden_host_code_point(c: u8) -> bool {
    matches!(
        c,
        0x00 | b'\t'
            | b'\n'
            | b'\r'
            | b' '
            | b'#'
            | b'/'
            | b':'
            | b'<'
            | b'>'
            | b'?'
            | b'@'
            | b'['
            | b'\\'
            | b']'
            | b'^'
            | b'|'
    )
}

pub(crate) fn ends_in_a_number(input: &str) -> bool {
    let mut parts = input.rsplit('.');
    let last = parts.next().unwrap_or("");
    let last = if last.is_empty() {
        parts.next().unwrap_or("")
    } else {
        last
    };
    if last.is_empty() {
        return false;
    }
    if last.bytes().all(|c| c.is_ascii_digit()) {
        return true;
    }
    parse_ipv4_number(last).is_ok()
}

/// Ok(None) means valid syntax but u32 overflow.
fn parse_ipv4_number(mut input: &str) -> Result<Option<u32>, ()> {
    if input.is_empty() {
        return Err(());
    }
    let mut radix = 10u32;
    if input.starts_with("0x") || input.starts_with("0X") {
        input = &input[2..];
        radix = 16;
    } else if input.len() >= 2 && input.starts_with('0') {
        input = &input[1..];
        radix = 8;
    }
    if input.is_empty() {
        return Ok(Some(0));
    }
    let valid = match radix {
        8 => input.bytes().all(|c| (b'0'..=b'7').contains(&c)),
        10 => input.bytes().all(|c| c.is_ascii_digit()),
        16 => input.bytes().all(|c| c.is_ascii_hexdigit()),
        _ => false,
    };
    if !valid {
        return Err(());
    }
    match u32::from_str_radix(input, radix) {
        Ok(n) => Ok(Some(n)),
        Err(_) => Ok(None),
    }
}

fn parse_ipv4(input: &str) -> Result<Ipv4Addr, ParseError> {
    let mut parts: Vec<&str> = input.split('.').collect();
    if parts.last() == Some(&"") {
        parts.pop();
    }
    if parts.len() > 4 {
        return Err(ParseError::Failure);
    }
    let mut numbers = Vec::with_capacity(parts.len());
    for part in parts {
        match parse_ipv4_number(part) {
            Ok(Some(n)) => numbers.push(n),
            Ok(None) | Err(()) => return Err(ParseError::Failure),
        }
    }
    if numbers.is_empty() {
        return Err(ParseError::Failure);
    }
    let mut ipv4 = numbers.pop().unwrap();
    if ipv4 > u32::MAX >> (8 * numbers.len() as u32) {
        return Err(ParseError::Failure);
    }
    if numbers.iter().any(|&x| x > 255) {
        return Err(ParseError::Failure);
    }
    for (counter, n) in numbers.iter().enumerate() {
        ipv4 += n << (8 * (3 - counter as u32));
    }
    Ok(Ipv4Addr::from(ipv4))
}

fn parse_ipv6(input: &str) -> Result<Ipv6Addr, ParseError> {
    let input = input.as_bytes();
    let len = input.len();
    let mut is_ip_v4 = false;
    let mut pieces = [0u16; 8];
    let mut piece_pointer = 0usize;
    let mut compress_pointer = None;
    let mut i = 0usize;

    if len < 2 {
        return Err(ParseError::Failure);
    }

    if input[0] == b':' {
        if input[1] != b':' {
            return Err(ParseError::Failure);
        }
        i = 2;
        piece_pointer = 1;
        compress_pointer = Some(1);
    }

    while i < len {
        if piece_pointer == 8 {
            return Err(ParseError::Failure);
        }
        if input[i] == b':' {
            if compress_pointer.is_some() {
                return Err(ParseError::Failure);
            }
            i += 1;
            piece_pointer += 1;
            compress_pointer = Some(piece_pointer);
            continue;
        }
        let start = i;
        let end = core::cmp::min(len, start + 4);
        let mut value = 0u16;
        while i < end {
            match (input[i] as char).to_digit(16) {
                Some(digit) => {
                    value = value * 0x10 + digit as u16;
                    i += 1;
                }
                None => break,
            }
        }
        if i < len {
            match input[i] {
                b'.' => {
                    if i == start {
                        return Err(ParseError::Failure);
                    }
                    i = start;
                    if piece_pointer > 6 {
                        return Err(ParseError::Failure);
                    }
                    is_ip_v4 = true;
                }
                b':' => {
                    i += 1;
                    if i == len {
                        return Err(ParseError::Failure);
                    }
                }
                _ => return Err(ParseError::Failure),
            }
        }
        if is_ip_v4 {
            break;
        }
        pieces[piece_pointer] = value;
        piece_pointer += 1;
    }

    if is_ip_v4 {
        if piece_pointer > 6 {
            return Err(ParseError::Failure);
        }
        let mut numbers_seen = 0;
        while i < len {
            if numbers_seen > 0 {
                if numbers_seen < 4 && i < len && input[i] == b'.' {
                    i += 1;
                } else {
                    return Err(ParseError::Failure);
                }
            }
            let mut ipv4_piece = None;
            while i < len {
                let digit = match input[i] {
                    c @ b'0'..=b'9' => c - b'0',
                    _ => break,
                };
                match ipv4_piece {
                    None => ipv4_piece = Some(u16::from(digit)),
                    Some(0) => return Err(ParseError::Failure),
                    Some(ref mut v) => {
                        *v = *v * 10 + u16::from(digit);
                        if *v > 255 {
                            return Err(ParseError::Failure);
                        }
                    }
                }
                i += 1;
            }
            pieces[piece_pointer] = if let Some(v) = ipv4_piece {
                pieces[piece_pointer] * 0x100 + v
            } else {
                return Err(ParseError::Failure);
            };
            numbers_seen += 1;
            if numbers_seen == 2 || numbers_seen == 4 {
                piece_pointer += 1;
            }
        }
        if numbers_seen != 4 {
            return Err(ParseError::Failure);
        }
    }

    if i < len {
        return Err(ParseError::Failure);
    }

    match compress_pointer {
        Some(compress_pointer) => {
            let mut swaps = piece_pointer - compress_pointer;
            piece_pointer = 7;
            while swaps > 0 {
                pieces.swap(piece_pointer, compress_pointer + swaps - 1);
                swaps -= 1;
                piece_pointer -= 1;
            }
        }
        None => {
            if piece_pointer != 8 {
                return Err(ParseError::Failure);
            }
        }
    }

    Ok(Ipv6Addr::new(
        pieces[0], pieces[1], pieces[2], pieces[3], pieces[4], pieces[5], pieces[6], pieces[7],
    ))
}

fn write_ipv6(addr: &Ipv6Addr, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let segments = addr.segments();
    let (compress_start, compress_end) = longest_zero_sequence(&segments);
    let mut i = 0isize;
    while i < 8 {
        if i == compress_start {
            f.write_str(":")?;
            if i == 0 {
                f.write_str(":")?;
            }
            if compress_end < 8 {
                i = compress_end;
            } else {
                break;
            }
        }
        write!(f, "{:x}", segments[i as usize])?;
        if i < 7 {
            f.write_str(":")?;
        }
        i += 1;
    }
    Ok(())
}

fn longest_zero_sequence(pieces: &[u16; 8]) -> (isize, isize) {
    let mut longest = -1;
    let mut longest_length = -1;
    let mut start = -1isize;
    macro_rules! finish_sequence {
        ($end:expr) => {{
            if start >= 0 {
                let length = $end - start;
                if length > longest_length {
                    longest = start;
                    longest_length = length;
                }
            }
        }};
    }
    for i in 0..8isize {
        if pieces[i as usize] == 0 {
            if start < 0 {
                start = i;
            }
        } else {
            finish_sequence!(i);
            start = -1;
        }
    }
    finish_sequence!(8);
    if longest_length < 2 {
        (-1, -2)
    } else {
        (longest, longest + longest_length)
    }
}

#[allow(dead_code)]
pub(crate) fn host_to_cow<'a>(host: &'a Host<'_>) -> Cow<'a, str> {
    match host {
        Host::Domain(d) => Cow::Borrowed(d.as_ref()),
        other => Cow::Owned(other.to_string()),
    }
}

#[cfg(test)]
mod opaque_ws {
    use super::*;
    #[test]
    fn ipv6_with_newlines() {
        let h = parse_opaque_host("\n\n[2001:db8::1]\n\n").unwrap();
        assert!(matches!(h, Host::Ipv6(_)));
    }
}
