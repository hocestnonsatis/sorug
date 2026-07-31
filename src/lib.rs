//! `sorug` — a blazingly fast, zero-allocation-preferred, WHATWG-compliant URL parser.
//!
//! # Design
//!
//! - **Index-based record**: component boundaries live as `u32` offsets into the
//!   WHATWG `href` serialization, keeping the struct small and cache-friendly.
//! - **Lazy / CoW serialization**: when the input is already canonical ASCII,
//!   [`Url`] borrows it with zero heap allocations; the first required mutation
//!   upgrades to an owned buffer.
//! - **`Cow` on demand**: percent-decoding helpers allocate only when needed.
//! - **SIMD delimiter scans**: [`memchr`] on hot path segments.
//! - **Strict WHATWG state machine**: transitions follow the
//!   [URL Living Standard](https://url.spec.whatwg.org/#url-parsing).

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]
#![allow(
    clippy::cast_possible_truncation,
    clippy::if_not_else,
    clippy::missing_fields_in_debug,
    clippy::range_plus_one
)]

mod parser;

use core::fmt;
use core::ops::Range;
use std::borrow::Cow;

use parser::ParsedUrl;

// ---------------------------------------------------------------------------
// WHATWG basic URL parser states
// ---------------------------------------------------------------------------

/// States of the WHATWG [basic URL parser](https://url.spec.whatwg.org/#url-parsing)
/// state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum State {
    /// [scheme start state](https://url.spec.whatwg.org/#scheme-start-state)
    SchemeStart,
    /// [scheme state](https://url.spec.whatwg.org/#scheme-state)
    Scheme,
    /// [no scheme state](https://url.spec.whatwg.org/#no-scheme-state)
    NoScheme,
    /// [special relative or authority state](https://url.spec.whatwg.org/#special-relative-or-authority-state)
    SpecialRelativeOrAuthority,
    /// [path or authority state](https://url.spec.whatwg.org/#path-or-authority-state)
    PathOrAuthority,
    /// [relative state](https://url.spec.whatwg.org/#relative-state)
    Relative,
    /// [relative slash state](https://url.spec.whatwg.org/#relative-slash-state)
    RelativeSlash,
    /// [special authority slashes state](https://url.spec.whatwg.org/#special-authority-slashes-state)
    SpecialAuthoritySlashes,
    /// [special authority ignore slashes state](https://url.spec.whatwg.org/#special-authority-ignore-slashes-state)
    SpecialAuthorityIgnoreSlashes,
    /// [authority state](https://url.spec.whatwg.org/#authority-state)
    Authority,
    /// [host state](https://url.spec.whatwg.org/#host-state)
    Host,
    /// [hostname state](https://url.spec.whatwg.org/#hostname-state)
    Hostname,
    /// [port state](https://url.spec.whatwg.org/#port-state)
    Port,
    /// [file state](https://url.spec.whatwg.org/#file-state)
    File,
    /// [file slash state](https://url.spec.whatwg.org/#file-slash-state)
    FileSlash,
    /// [file host state](https://url.spec.whatwg.org/#file-host-state)
    FileHost,
    /// [path start state](https://url.spec.whatwg.org/#path-start-state)
    PathStart,
    /// [path state](https://url.spec.whatwg.org/#path-state)
    Path,
    /// [opaque path state](https://url.spec.whatwg.org/#cannot-be-a-base-url-path-state)
    OpaquePath,
    /// [query state](https://url.spec.whatwg.org/#query-state)
    Query,
    /// [fragment state](https://url.spec.whatwg.org/#fragment-state)
    Fragment,
}

// ---------------------------------------------------------------------------
// Parse errors
// ---------------------------------------------------------------------------

/// Failure modes of [`Url::parse`] / the basic URL parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParseError {
    /// Input could not be parsed into a valid URL record (WHATWG "failure").
    Failure,
    /// Input length exceeds `u32::MAX` bytes (index representation limit).
    InputTooLong,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failure => f.write_str("URL parse failure"),
            Self::InputTooLong => f.write_str("URL input exceeds u32 index range"),
        }
    }
}

impl std::error::Error for ParseError {}

// ---------------------------------------------------------------------------
// URL flags (packed)
// ---------------------------------------------------------------------------

/// Compact boolean attributes of a parsed URL record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(transparent)]
pub struct UrlFlags(u8);

impl UrlFlags {
    pub const SPECIAL: Self = Self(1 << 0);
    pub const HAS_CREDENTIALS: Self = Self(1 << 1);
    pub const HAS_EMPTY_HOST: Self = Self(1 << 2);
    pub const OPAQUE_PATH: Self = Self(1 << 3);
    pub const HAS_PASSWORD: Self = Self(1 << 4);
    pub const HOST_IPV4: Self = Self(1 << 5);
    pub const HOST_IPV6: Self = Self(1 << 6);
    pub const HOST_IDNA: Self = Self(1 << 7);

    #[inline]
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    #[inline]
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[inline]
    pub const fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    #[inline]
    pub const fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }

    #[inline]
    #[must_use]
    pub const fn is_special(self) -> bool {
        self.contains(Self::SPECIAL)
    }

    #[inline]
    #[must_use]
    pub const fn has_opaque_path(self) -> bool {
        self.contains(Self::OPAQUE_PATH)
    }
}

// ---------------------------------------------------------------------------
// Serialization backing (CoW)
// ---------------------------------------------------------------------------

/// Storage for the WHATWG `href` serialization.
///
/// [`Borrowed`](Backing::Borrowed) is used when the trimmed input was already
/// canonical and required no mutations. [`Owned`](Backing::Owned) is used after
/// the first normalization (lowercasing, path shorten, percent-encoding, IDNA, …).
#[derive(Clone, Debug, Eq)]
pub enum Backing<'a> {
    Borrowed(&'a str),
    Owned(String),
}

impl PartialEq for Backing<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Backing<'_> {
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Borrowed(s) => s,
            Self::Owned(s) => s.as_str(),
        }
    }

    #[inline]
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.as_str().as_bytes()
    }

    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.as_str().len()
    }

    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.as_str().is_empty()
    }

    #[inline]
    #[must_use]
    pub fn is_borrowed(&self) -> bool {
        matches!(self, Self::Borrowed(_))
    }

    /// Force an owned copy, detaching from any input lifetime.
    #[must_use]
    pub fn into_owned(self) -> Backing<'static> {
        match self {
            Self::Borrowed(s) => Backing::Owned(s.to_owned()),
            Self::Owned(s) => Backing::Owned(s),
        }
    }
}

// ---------------------------------------------------------------------------
// Url record
// ---------------------------------------------------------------------------

/// A parsed URL record backed by its WHATWG serialization (`href`).
///
/// Component boundaries are `u32` byte offsets into [`Self::as_str`].
/// Absent optional markers use the sentinel [`Url::NONE`] (`u32::MAX`).
///
/// When the input is already in canonical form, [`Backing::Borrowed`] avoids
/// heap allocation entirely.
#[derive(Clone)]
pub struct Url<'a> {
    serialization: Backing<'a>,
    scheme_end: u32,
    username_end: u32,
    host_start: u32,
    host_end: u32,
    /// Explicit port, or [`Url::NO_PORT`].
    port: u32,
    path_start: u32,
    /// Index of `?`, or [`Url::NONE`].
    query_start: u32,
    /// Index of `#`, or [`Url::NONE`].
    fragment_start: u32,
    flags: UrlFlags,
}

impl<'a> Url<'a> {
    /// Sentinel: component marker absent.
    pub const NONE: u32 = u32::MAX;
    /// Sentinel: no explicit port.
    pub const NO_PORT: u32 = u32::MAX;

    fn from_parsed(p: ParsedUrl<'a>) -> Self {
        Self {
            serialization: p.serialization,
            scheme_end: p.scheme_end,
            username_end: p.username_end,
            host_start: p.host_start,
            host_end: p.host_end,
            port: match p.port {
                Some(port) => u32::from(port),
                None => Self::NO_PORT,
            },
            path_start: p.path_start,
            query_start: p.query_start.unwrap_or(Self::NONE),
            fragment_start: p.fragment_start.unwrap_or(Self::NONE),
            flags: UrlFlags(p.flags),
        }
    }

    fn to_parsed(&self) -> ParsedUrl<'_> {
        ParsedUrl {
            serialization: match &self.serialization {
                Backing::Borrowed(s) => Backing::Borrowed(s),
                Backing::Owned(s) => Backing::Borrowed(s.as_str()),
            },
            scheme_end: self.scheme_end,
            username_end: self.username_end,
            host_start: self.host_start,
            host_end: self.host_end,
            port: self.port_u16(),
            path_start: self.path_start,
            query_start: (self.query_start != Self::NONE).then_some(self.query_start),
            fragment_start: (self.fragment_start != Self::NONE).then_some(self.fragment_start),
            flags: self.flags.0,
        }
    }

    /// Empty placeholder (mostly for tests / incremental builders).
    #[must_use]
    pub fn blank() -> Url<'static> {
        Url {
            serialization: Backing::Owned(String::new()),
            scheme_end: 0,
            username_end: 0,
            host_start: 0,
            host_end: 0,
            port: Self::NO_PORT,
            path_start: 0,
            query_start: Self::NONE,
            fragment_start: Self::NONE,
            flags: UrlFlags::empty(),
        }
    }

    /// Detach from the input lifetime by owning the serialization.
    #[must_use]
    pub fn into_owned(self) -> Url<'static> {
        Url {
            serialization: self.serialization.into_owned(),
            scheme_end: self.scheme_end,
            username_end: self.username_end,
            host_start: self.host_start,
            host_end: self.host_end,
            port: self.port,
            path_start: self.path_start,
            query_start: self.query_start,
            fragment_start: self.fragment_start,
            flags: self.flags,
        }
    }

    /// Parse `input` according to the WHATWG basic URL parser with no base URL.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError::Failure`] on WHATWG failure, or
    /// [`ParseError::InputTooLong`] when the input cannot be indexed by `u32`.
    pub fn parse(input: &'a str) -> Result<Self, ParseError> {
        Self::parse_with_base(input, None)
    }

    /// Parse `input` against an optional absolute `base` URL.
    ///
    /// # Errors
    ///
    /// See [`Url::parse`].
    pub fn parse_with_base(input: &'a str, base: Option<&Url<'_>>) -> Result<Self, ParseError> {
        if input.len() > u32::MAX as usize {
            return Err(ParseError::InputTooLong);
        }
        let base_parsed = base.map(Url::to_parsed);
        let parsed = parser::parse(input, base_parsed.as_ref())?;
        Ok(Self::from_parsed(parsed))
    }

    /// The serialization backing (borrowed or owned).
    #[inline]
    #[must_use]
    pub const fn backing(&self) -> &Backing<'a> {
        &self.serialization
    }

    /// The full serialization (WHATWG `href`).
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.serialization.as_str()
    }

    /// Parser flags for this record.
    #[inline]
    #[must_use]
    pub const fn flags(&self) -> UrlFlags {
        self.flags
    }

    #[inline]
    fn byte_at(&self, i: u32) -> u8 {
        self.serialization.as_bytes()[i as usize]
    }

    #[inline]
    fn slice(&self, range: Range<u32>) -> &str {
        &self.as_str()[range.start as usize..range.end as usize]
    }

    #[inline]
    fn has_authority(&self) -> bool {
        self.serialization.len() >= self.scheme_end as usize + 3
            && self.byte_at(self.scheme_end + 1) == b'/'
            && self.byte_at(self.scheme_end + 2) == b'/'
    }

    #[inline]
    #[must_use]
    pub const fn scheme_range(&self) -> Range<usize> {
        0..self.scheme_end as usize
    }

    /// Scheme without trailing `:`.
    #[inline]
    #[must_use]
    pub fn scheme(&self) -> &str {
        &self.as_str()[self.scheme_range()]
    }

    /// WHATWG / WPT `protocol` getter (`scheme + ':'`).
    #[inline]
    #[must_use]
    pub fn protocol(&self) -> &str {
        &self.as_str()[..self.scheme_end as usize + 1]
    }

    #[inline]
    #[must_use]
    pub fn has_host(&self) -> bool {
        self.host_start != self.host_end || self.flags.contains(UrlFlags::HAS_EMPTY_HOST)
    }

    /// Host serialization without port (`None` when host is null).
    #[inline]
    #[must_use]
    pub fn host(&self) -> Option<&str> {
        if !self.has_host() {
            return None;
        }
        Some(self.slice(self.host_start..self.host_end))
    }

    /// Username component (percent-encoded), or `""`.
    #[inline]
    #[must_use]
    pub fn username(&self) -> &str {
        let scheme_separator_len = 3; // "://"
        if self.has_authority()
            && self.username_end > self.scheme_end + scheme_separator_len
        {
            self.slice(self.scheme_end + scheme_separator_len..self.username_end)
        } else {
            ""
        }
    }

    /// Password component, or `""` when the password slot is absent.
    #[inline]
    #[must_use]
    pub fn password(&self) -> &str {
        if self.has_authority()
            && (self.username_end as usize) < self.serialization.len()
            && self.byte_at(self.username_end) == b':'
        {
            self.slice(self.username_end + 1..self.host_start - 1)
        } else {
            ""
        }
    }

    #[inline]
    #[must_use]
    pub const fn port_u16(&self) -> Option<u16> {
        if self.port == Self::NO_PORT || self.port > u16::MAX as u32 {
            None
        } else {
            #[allow(clippy::cast_possible_truncation)]
            {
                Some(self.port as u16)
            }
        }
    }

    /// WHATWG / WPT `port` getter: decimal string, or `""` when null.
    #[inline]
    #[must_use]
    pub fn port_str(&self) -> String {
        match self.port_u16() {
            Some(p) => p.to_string(),
            None => String::new(),
        }
    }

    /// WHATWG / WPT `hostname` getter (empty string when host is null).
    #[inline]
    #[must_use]
    pub fn hostname(&self) -> &str {
        self.host().unwrap_or("")
    }

    /// WHATWG / WPT `host` getter: hostname, plus `":" port` when port is non-null.
    #[inline]
    #[must_use]
    pub fn host_with_port(&self) -> &str {
        if !self.has_host() {
            return "";
        }
        let end = if self.port != Self::NO_PORT {
            self.path_start
        } else {
            self.host_end
        };
        self.slice(self.host_start..end)
    }

    /// Path / opaque path serialization.
    #[inline]
    #[must_use]
    pub fn path(&self) -> &str {
        let end = if self.query_start != Self::NONE {
            self.query_start
        } else if self.fragment_start != Self::NONE {
            self.fragment_start
        } else {
            self.serialization.len() as u32
        };
        self.slice(self.path_start..end)
    }

    /// Alias for [`Self::path`] matching the WPT `pathname` attribute.
    #[inline]
    #[must_use]
    pub fn pathname(&self) -> &str {
        self.path()
    }

    /// Query without leading `?`, or `None` when null.
    #[inline]
    #[must_use]
    pub fn query(&self) -> Option<&str> {
        if self.query_start == Self::NONE {
            return None;
        }
        let start = self.query_start + 1;
        let end = if self.fragment_start != Self::NONE {
            self.fragment_start
        } else {
            self.serialization.len() as u32
        };
        Some(self.slice(start..end))
    }

    /// WHATWG / WPT `search` getter (`""` or `"?" + query`).
    #[inline]
    #[must_use]
    pub fn search(&self) -> &str {
        if self.query_start == Self::NONE {
            return "";
        }
        let end = if self.fragment_start != Self::NONE {
            self.fragment_start
        } else {
            self.serialization.len() as u32
        };
        if end == self.query_start + 1 {
            return "";
        }
        self.slice(self.query_start..end)
    }

    /// Fragment without leading `#`, or `None` when null.
    #[inline]
    #[must_use]
    pub fn fragment(&self) -> Option<&str> {
        if self.fragment_start == Self::NONE {
            return None;
        }
        Some(&self.as_str()[self.fragment_start as usize + 1..])
    }

    /// WHATWG / WPT `hash` getter (`""` or `"#" + fragment`).
    #[inline]
    #[must_use]
    pub fn hash(&self) -> &str {
        if self.fragment_start == Self::NONE {
            return "";
        }
        if self.fragment_start as usize + 1 == self.serialization.len() {
            return "";
        }
        &self.as_str()[self.fragment_start as usize..]
    }

    /// Serialize this URL record (WHATWG / WPT `href`).
    #[must_use]
    pub fn href(&self) -> &str {
        self.as_str()
    }

    /// Percent-decode `raw`, allocating only when decoding changes the bytes.
    #[must_use]
    pub fn percent_decode(raw: &str) -> Cow<'_, str> {
        let decoded = parser::percent::percent_decode(raw.as_bytes());
        match decoded {
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
}

impl fmt::Debug for Url<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Url")
            .field("href", &self.href())
            .field("borrowed", &self.serialization.is_borrowed())
            .field("scheme", &self.scheme())
            .field("username", &self.username())
            .field("password", &self.password())
            .field("host", &self.host())
            .field("port", &self.port_u16())
            .field("path", &self.path())
            .field("query", &self.query())
            .field("fragment", &self.fragment())
            .field("flags", &self.flags)
            .finish_non_exhaustive()
    }
}

impl PartialEq for Url<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for Url<'_> {}

impl fmt::Display for Url<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_url_has_no_host_or_query() {
        let url = Url::blank();
        assert_eq!(url.scheme(), "");
        assert!(url.host().is_none());
        assert!(url.query().is_none());
        assert!(url.fragment().is_none());
        assert_eq!(url.port_u16(), None);
    }

    #[test]
    fn parse_https_example() {
        let url = Url::parse("https://example.com/foo").unwrap();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.hostname(), "example.com");
        assert_eq!(url.pathname(), "/foo");
        assert_eq!(url.href(), "https://example.com/foo");
        assert!(url.backing().is_borrowed());
    }

    #[test]
    fn uppercase_scheme_owns() {
        let url = Url::parse("HTTPS://example.com/foo").unwrap();
        assert_eq!(url.href(), "https://example.com/foo");
        assert!(!url.backing().is_borrowed());
    }

    #[test]
    fn state_variants_cover_whatwg_set() {
        let states = [
            State::SchemeStart,
            State::Scheme,
            State::NoScheme,
            State::SpecialRelativeOrAuthority,
            State::PathOrAuthority,
            State::Relative,
            State::RelativeSlash,
            State::SpecialAuthoritySlashes,
            State::SpecialAuthorityIgnoreSlashes,
            State::Authority,
            State::Host,
            State::Hostname,
            State::Port,
            State::File,
            State::FileSlash,
            State::FileHost,
            State::PathStart,
            State::Path,
            State::OpaquePath,
            State::Query,
            State::Fragment,
        ];
        assert_eq!(states.len(), 21);
    }

    #[test]
    fn percent_decode_stub_borrows() {
        let cow = Url::percent_decode("abc");
        assert!(matches!(cow, Cow::Borrowed(_)));
    }
}
