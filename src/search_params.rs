//! [`application/x-www-form-urlencoded`](https://url.spec.whatwg.org/#application/x-www-form-urlencoded)
//! parse / serialize and a WHATWG-style [`SearchParams`] collection.

use core::fmt;
use std::borrow::Cow;

use crate::parser::percent::percent_decode;

/// A list of name/value pairs in `application/x-www-form-urlencoded` syntax.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SearchParams {
    pairs: Vec<(String, String)>,
}

impl SearchParams {
    /// Empty list.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self { pairs: Vec::new() }
    }

    /// Parse a query string (without a leading `?`).
    #[must_use]
    pub fn parse(input: &str) -> Self {
        Self {
            pairs: parse_urlencoded(input.as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect(),
        }
    }

    /// Number of name/value pairs.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    /// Whether there are no pairs.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// First value associated with `name`, if any.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// All values associated with `name`.
    #[must_use]
    pub fn get_all(&self, name: &str) -> Vec<&str> {
        self.pairs
            .iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    /// Whether any pair has this name.
    #[must_use]
    pub fn has(&self, name: &str) -> bool {
        self.pairs.iter().any(|(k, _)| k == name)
    }

    /// Append a name/value pair (does not remove existing names).
    pub fn append(&mut self, name: &str, value: &str) {
        self.pairs.push((name.to_owned(), value.to_owned()));
    }

    /// Set `name` to a single `value`, removing prior pairs with that name.
    ///
    /// If the name existed, the new pair is inserted at the first old index;
    /// otherwise it is appended.
    pub fn set(&mut self, name: &str, value: &str) {
        if let Some(first) = self.pairs.iter().position(|(k, _)| k == name) {
            self.pairs[first].1.clear();
            self.pairs[first].1.push_str(value);
            let mut i = first + 1;
            while i < self.pairs.len() {
                if self.pairs[i].0 == name {
                    self.pairs.remove(i);
                } else {
                    i += 1;
                }
            }
        } else {
            self.append(name, value);
        }
    }

    /// Remove all pairs with this name. Returns whether any were removed.
    pub fn delete(&mut self, name: &str) -> bool {
        let before = self.pairs.len();
        self.pairs.retain(|(k, _)| k != name);
        self.pairs.len() != before
    }

    /// Remove all pairs.
    pub fn clear(&mut self) {
        self.pairs.clear();
    }

    /// Iterate over `(name, value)` references.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.pairs.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Owned pairs (clone).
    #[must_use]
    pub fn pairs(&self) -> &[(String, String)] {
        &self.pairs
    }

    /// Serialize to `application/x-www-form-urlencoded` (no leading `?`).
    #[must_use]
    pub fn serialize(&self) -> String {
        serialize_urlencoded(self.pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())))
    }
}

impl fmt::Display for SearchParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.serialize())
    }
}

impl FromIterator<(String, String)> for SearchParams {
    fn from_iter<T: IntoIterator<Item = (String, String)>>(iter: T) -> Self {
        Self {
            pairs: iter.into_iter().collect(),
        }
    }
}

impl<'a> FromIterator<(&'a str, &'a str)> for SearchParams {
    fn from_iter<T: IntoIterator<Item = (&'a str, &'a str)>>(iter: T) -> Self {
        Self {
            pairs: iter
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
        }
    }
}

/// Parse `application/x-www-form-urlencoded` bytes into decoded name/value pairs.
#[must_use]
pub fn parse_urlencoded(input: &[u8]) -> Parse<'_> {
    Parse { input }
}

/// Iterator yielded by [`parse_urlencoded`].
#[derive(Clone, Debug)]
pub struct Parse<'a> {
    input: &'a [u8],
}

impl<'a> Iterator for Parse<'a> {
    type Item = (Cow<'a, str>, Cow<'a, str>);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.input.is_empty() {
                return None;
            }
            let mut split = self.input.splitn(2, |&b| b == b'&');
            let sequence = split.next()?;
            self.input = split.next().unwrap_or(&[]);
            if sequence.is_empty() {
                continue;
            }
            let mut kv = sequence.splitn(2, |&b| b == b'=');
            let name = kv.next()?;
            let value = kv.next().unwrap_or(&[]);
            return Some((decode_urlencoded(name), decode_urlencoded(value)));
        }
    }
}

fn decode_urlencoded(input: &[u8]) -> Cow<'_, str> {
    let bytes: Cow<'_, [u8]> = match replace_plus(input) {
        Cow::Borrowed(b) => percent_decode(b),
        Cow::Owned(b) => match percent_decode(&b) {
            Cow::Borrowed(d) => Cow::Owned(d.to_owned()),
            Cow::Owned(v) => Cow::Owned(v),
        },
    };
    match bytes {
        Cow::Borrowed(b) => match std::str::from_utf8(b) {
            Ok(s) => Cow::Borrowed(s),
            Err(_) => Cow::Owned(String::from_utf8_lossy(b).into_owned()),
        },
        Cow::Owned(v) => match String::from_utf8(v) {
            Ok(s) => Cow::Owned(s),
            Err(e) => Cow::Owned(String::from_utf8_lossy(&e.into_bytes()).into_owned()),
        },
    }
}

fn replace_plus(input: &[u8]) -> Cow<'_, [u8]> {
    match input.iter().position(|&b| b == b'+') {
        None => Cow::Borrowed(input),
        Some(first) => {
            let mut owned = input.to_owned();
            owned[first] = b' ';
            for b in &mut owned[first + 1..] {
                if *b == b'+' {
                    *b = b' ';
                }
            }
            Cow::Owned(owned)
        }
    }
}

/// Serialize pairs as `application/x-www-form-urlencoded`.
#[must_use]
pub fn serialize_urlencoded<'a, I>(pairs: I) -> String
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut out = String::new();
    for (i, (name, value)) in pairs.into_iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        append_urlencoded(name.as_bytes(), &mut out);
        out.push('=');
        append_urlencoded(value.as_bytes(), &mut out);
    }
    out
}

fn byte_serialized_unchanged(byte: u8) -> bool {
    matches!(byte, b'*' | b'-' | b'.' | b'0'..=b'9' | b'A'..=b'Z' | b'_' | b'a'..=b'z')
}

fn append_urlencoded(bytes: &[u8], out: &mut String) {
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if byte_serialized_unchanged(b) {
            let start = i;
            i += 1;
            while i < bytes.len() && byte_serialized_unchanged(bytes[i]) {
                i += 1;
            }
            // Unchanged bytes are a subset of ASCII (checked above).
            if let Ok(s) = std::str::from_utf8(&bytes[start..i]) {
                out.push_str(s);
            }
        } else if b == b' ' {
            out.push('+');
            i += 1;
        } else {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0xf) as usize] as char);
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple() {
        let sp = SearchParams::parse("a=b&c=d");
        assert_eq!(sp.get("a"), Some("b"));
        assert_eq!(sp.get("c"), Some("d"));
        assert_eq!(sp.serialize(), "a=b&c=d");
    }

    #[test]
    fn parse_lone_name() {
        let sp = SearchParams::parse("qux");
        assert_eq!(sp.get("qux"), Some(""));
        assert_eq!(sp.serialize(), "qux=");
    }

    #[test]
    fn plus_and_percent() {
        let sp = SearchParams::parse("a=b+c&x=%23");
        assert_eq!(sp.get("a"), Some("b c"));
        assert_eq!(sp.get("x"), Some("#"));
    }

    #[test]
    fn set_replaces_and_keeps_order() {
        let mut sp = SearchParams::parse("a=1&b=2&a=3");
        sp.set("a", "9");
        assert_eq!(sp.serialize(), "a=9&b=2");
    }

    #[test]
    fn append_delete() {
        let mut sp = SearchParams::new();
        sp.append("a", "1");
        sp.append("a", "2");
        assert!(sp.delete("a"));
        assert!(sp.is_empty());
    }

    #[test]
    fn leading_question_in_name() {
        let sp = SearchParams::parse("?a=b&c=d");
        assert_eq!(sp.serialize(), "%3Fa=b&c=d");
    }
}
