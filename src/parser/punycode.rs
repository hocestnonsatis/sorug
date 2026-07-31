//! Minimal IDNA / Punycode for WHATWG host parsing.
//!
//! Implements RFC 3492 Bootstring (Punycode) encoding plus a deliberately small
//! UTS #46 mapping surface — enough for the WPT `urltestdata` suite without
//! shipping megabytes of Unicode tables.
//!
//! WHATWG domain parser (`beStrict = false`):
//! - ASCII input → lowercase pass-through (even if ACE labels are invalid)
//! - Non-ASCII → map → split labels → Punycode-encode non-ASCII labels

use std::borrow::Cow;

/// Convert a domain to ASCII per WHATWG (non-strict).
pub(crate) fn to_ascii(domain: &str) -> Result<Cow<'_, str>, ()> {
    if domain.is_empty() {
        return Err(());
    }

    if domain.is_ascii() {
        let lower = lowercase_ascii(domain);
        if contains_forbidden_domain_code_point(lower.as_ref()) {
            return Err(());
        }
        return Ok(lower);
    }

    // Reject UTS #46 disallowed code points that appear in WPT before mapping.
    if domain.chars().any(is_disallowed_idna) {
        return Err(());
    }

    let mapped = uts46_map(domain);
    if mapped.is_empty() {
        return Err(());
    }
    if mapped.chars().any(is_disallowed_idna) {
        return Err(());
    }

    let mut out = String::with_capacity(mapped.len() + 16);
    for (i, label) in mapped.split('.').enumerate() {
        if i > 0 {
            out.push('.');
        }
        if label.is_empty() {
            continue;
        }
        if label.is_ascii() {
            out.push_str(label);
        } else {
            out.push_str("xn--");
            encode_punycode(label, &mut out)?;
        }
    }

    if out.is_empty() || contains_forbidden_domain_code_point(&out) {
        return Err(());
    }
    Ok(Cow::Owned(out))
}

/// Minimal UTS #46 disallowed set covering WPT failures (noncharacters, spaces,
/// replacement char, line/paragraph separators). Full UTS46 tables are
/// intentionally not embedded.
fn is_disallowed_idna(c: char) -> bool {
    match c {
        '\u{00A0}' | '\u{3000}' | '\u{FFFD}' => true,
        // UTS #46: LINE SEPARATOR / PARAGRAPH SEPARATOR are disallowed.
        '\u{2028}' | '\u{2029}' => true,
        // Noncharacters: U+FDD0..FDEF, and any plane's U+FFFE / U+FFFF.
        '\u{FDD0}'..='\u{FDEF}' => true,
        c => {
            let u = c as u32;
            (u & 0xFFFE) == 0xFFFE
        }
    }
}

fn lowercase_ascii(s: &str) -> Cow<'_, str> {
    if s.bytes().all(|b| !b.is_ascii_uppercase()) {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(s.to_ascii_lowercase())
    }
}

#[inline]
fn contains_forbidden_domain_code_point(s: &str) -> bool {
    s.bytes().any(|b| {
        // Forbidden host code point, C0 control, `%`, or DEL.
        matches!(
            b,
            0x00..=0x1F
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
                | b'%'
                | 0x7F
        )
    })
}

/// Apply a minimal UTS #46 mapping + ASCII/Unicode lowercasing.
fn uts46_map(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        map_char(c, &mut out);
    }
    out
}

fn map_char(c: char, out: &mut String) {
    match c {
        // Ignored (UTS #46): soft hyphen, ZWSP, WJ, BOM — WPT host cases.
        '\u{00AD}' | '\u{200B}' | '\u{2060}' | '\u{FEFF}' => {}

        // Non-transitional: deviation char ß is kept (encodes as xn--fa-hia, not fass).
        // Ideographic full stop → ASCII dot.
        '\u{3002}' => out.push('.'),

        // Fullwidth ASCII (! through ~): U+FF01..=U+FF5E → U+0021..=U+007E
        '\u{FF01}'..='\u{FF5E}' => {
            let mapped = char::from_u32(u32::from(c) - 0xFEE0).unwrap_or(c);
            out.push(mapped.to_ascii_lowercase());
        }

        // Mathematical Bold Capitals A–Z (U+1D400..=U+1D419)
        c if (0x1D400..=0x1D419).contains(&(c as u32)) => {
            out.push(char::from(b'a' + (c as u32 - 0x1D400) as u8));
        }
        // Mathematical Bold Small a–z (U+1D41A..=U+1D433)
        c if (0x1D41A..=0x1D433).contains(&(c as u32)) => {
            out.push(char::from(b'a' + (c as u32 - 0x1D41A) as u8));
        }

        c if c.is_ascii() => out.push(c.to_ascii_lowercase()),

        c => {
            for ch in c.to_lowercase() {
                if ch.is_ascii() {
                    out.push(ch.to_ascii_lowercase());
                } else {
                    out.push(ch);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// RFC 3492 Punycode encode
// ---------------------------------------------------------------------------

const BASE: u32 = 36;
const TMIN: u32 = 1;
const TMAX: u32 = 26;
const SKEW: u32 = 38;
const DAMP: u32 = 700;
const INITIAL_BIAS: u32 = 72;
const INITIAL_N: u32 = 128;

fn encode_punycode(input: &str, out: &mut String) -> Result<(), ()> {
    let mut n = INITIAL_N;
    let mut delta: u32 = 0;
    let mut bias = INITIAL_BIAS;

    let chars: Vec<char> = input.chars().collect();
    let mut basic_count = 0u32;

    for &c in &chars {
        if c.is_ascii() {
            out.push(c.to_ascii_lowercase());
            basic_count += 1;
        }
    }

    let mut h = basic_count;
    let b = basic_count;
    if b > 0 {
        out.push('-');
    }

    while (h as usize) < chars.len() {
        let mut m = u32::MAX;
        for &c in &chars {
            let cp = c as u32;
            if cp >= n && cp < m {
                m = cp;
            }
        }
        if m == u32::MAX {
            return Err(());
        }

        let advance = m.checked_sub(n).ok_or(())?;
        delta = delta
            .checked_add(advance.checked_mul(h.checked_add(1).ok_or(())?).ok_or(())?)
            .ok_or(())?;
        n = m;

        for &c in &chars {
            let cp = c as u32;
            if cp < n {
                delta = delta.checked_add(1).ok_or(())?;
            } else if cp == n {
                let mut q = delta;
                let mut k = BASE;
                loop {
                    let t = if k <= bias {
                        TMIN
                    } else if k >= bias.saturating_add(TMAX) {
                        TMAX
                    } else {
                        k - bias
                    };
                    if q < t {
                        break;
                    }
                    let code = t + ((q - t) % (BASE - t));
                    out.push(digit_to_char(code)?);
                    q = (q - t) / (BASE - t);
                    k = k.checked_add(BASE).ok_or(())?;
                }
                out.push(digit_to_char(q)?);
                bias = adapt(delta, h + 1, h == b);
                delta = 0;
                h += 1;
            }
        }
        delta += 1;
        n += 1;
    }
    Ok(())
}

fn adapt(mut delta: u32, num_points: u32, first_time: bool) -> u32 {
    delta = if first_time { delta / DAMP } else { delta / 2 };
    delta += delta / num_points;
    let mut k = 0u32;
    while delta > ((BASE - TMIN) * TMAX) / 2 {
        delta /= BASE - TMIN;
        k += BASE;
    }
    k + (((BASE - TMIN + 1) * delta) / (delta + SKEW))
}

fn digit_to_char(d: u32) -> Result<char, ()> {
    match d {
        0..=25 => Ok(char::from(b'a' + d as u8)),
        26..=35 => Ok(char::from(b'0' + (d as u8 - 26))),
        _ => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_passthrough_invalid_punycode() {
        assert_eq!(to_ascii("a.b.c.xn--pokxncvks").unwrap(), "a.b.c.xn--pokxncvks");
        assert_eq!(to_ascii("XN--").unwrap(), "xn--");
    }

    #[test]
    fn punycode_examples() {
        assert_eq!(to_ascii("é").unwrap(), "xn--9ca");
        assert_eq!(to_ascii("你好你好").unwrap(), "xn--6qqa088eba");
        assert_eq!(to_ascii("faß.ExAmPlE").unwrap(), "xn--fa-hia.example");
        assert_eq!(to_ascii("☃").unwrap(), "xn--n3h");
        assert!(to_ascii("\u{fffd}").is_err());
        assert!(to_ascii("GOO\u{a0}goo.com").is_err());
    }

    #[test]
    fn mapping_examples() {
        assert_eq!(to_ascii("Ｇｏ.com").unwrap(), "go.com");
        assert_eq!(to_ascii("www.foo。bar.com").unwrap(), "www.foo.bar.com");
        assert_eq!(to_ascii("GOO\u{200b}\u{2060}\u{feff}goo.com").unwrap(), "googoo.com");
        assert_eq!(to_ascii("a\u{ad}b").unwrap(), "ab");
        assert_eq!(to_ascii("loC𝐀𝐋𝐇𝐨𝐬𝐭").unwrap(), "localhost");
    }
}
