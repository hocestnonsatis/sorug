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
//!
//! # Crate features
//!
//! - **`std`** (default): enables [`std::error::Error`] for [`ParseError`] and
//!   `memchr`'s std backend.
//! - **`serde`**: serialize / deserialize [`Url`] as an href string.
//! - **`http`**: convert between [`Url`] and [`http::Uri`] (implies `std`).
//!
//! # Migration from 0.2
//!
//! - [`Url::join`], [`Url::make_relative`], [`Url::path_segments`],
//!   [`Url::path_segments_mut`], [`Url::query_pairs`], [`Url::query_pairs_mut`],
//!   [`Url::host_parsed`], [`AsRef<str>`], and [`core::str::FromStr`].
//! - Optional features: `serde`, `http`; disable `std` with
//!   `default-features = false` for `no_std` + `alloc`.
//! - Public [`Host`] enum (`Domain` / `Ipv4` / `Ipv6`) with [`Host::parse`].
//! - [`UrlFlags::HOST_IDNA`] is set when the serialized host contains an ACE
//!   label (`xn--`).
//! - Port API: [`Url::set_port`] takes `Option<u16>` (rust-url shape);
//!   quirks/WPT string setter is [`Url::set_port_str`].

#![cfg_attr(not(any(feature = "std", test)), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]
#![allow(
    clippy::cast_possible_truncation,
    clippy::if_not_else,
    clippy::missing_fields_in_debug,
    clippy::range_plus_one
)]

extern crate alloc;

mod parser;
mod path_segments;
mod query_pairs;
mod search_params;

#[cfg(feature = "http")]
#[cfg_attr(docsrs, doc(cfg(feature = "http")))]
mod http_impl;
#[cfg(feature = "serde")]
#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
mod serde_impl;

pub use parser::host::Host;
pub use path_segments::PathSegmentsMut;
pub use query_pairs::QueryPairsMut;
pub use search_params::{SearchParams, SearchParamsIter, parse_urlencoded, serialize_urlencoded};

#[cfg(feature = "http")]
#[cfg_attr(docsrs, doc(cfg(feature = "http")))]
pub use http_impl::uri_to_url;

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::{String, ToString};
use core::fmt;
use core::ops::Range;

use parser::ParsedUrl;

// `to_owned` / `to_string` on `str` require these traits under `no_std` + `alloc`.
#[allow(unused_imports)]
use alloc::borrow::ToOwned;

// ---------------------------------------------------------------------------
// WHATWG basic URL parser states
// ---------------------------------------------------------------------------

/// States of the WHATWG [basic URL parser](https://url.spec.whatwg.org/#url-parsing)
/// state machine.
///
/// Exposed for debugging / advanced tooling; most consumers should not need this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[doc(hidden)]
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

#[cfg(feature = "std")]
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
// Origin
// ---------------------------------------------------------------------------

/// A URL [origin](https://url.spec.whatwg.org/#concept-url-origin): either an
/// opaque origin or a `(scheme, host, port)` tuple.
///
/// # Opaque equality
///
/// All [`Origin::Opaque`] values compare equal. The HTML / WHATWG model treats
/// each opaque origin as unique; use this type for ASCII serialization and
/// coarse checks, not fine-grained same-origin policy for opaque URLs.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum Origin {
    /// Globally unique / non-tuple origin (file, non-special, failed blob, …).
    Opaque,
    /// Tuple origin of scheme, host serialization, and port (default applied).
    Tuple {
        scheme: String,
        host: String,
        port: u16,
    },
}

impl Origin {
    /// ASCII serialization of an origin (`"null"` or `"scheme://host[:port]"`).
    ///
    /// Default ports are omitted per the HTML ASCII serialization of an origin.
    #[must_use]
    pub fn serialized(&self) -> String {
        match self {
            Self::Opaque => String::from("null"),
            Self::Tuple { scheme, host, port } => {
                if parser::default_port_for_scheme(scheme) == Some(*port) {
                    format!("{scheme}://{host}")
                } else {
                    format!("{scheme}://{host}:{port}")
                }
            }
        }
    }

    /// Alias for [`Self::serialized`] (rust-url naming).
    #[inline]
    #[must_use]
    pub fn ascii_serialization(&self) -> String {
        self.serialized()
    }

    /// Whether this is a tuple origin.
    #[inline]
    #[must_use]
    pub const fn is_tuple(&self) -> bool {
        matches!(self, Self::Tuple { .. })
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.serialized())
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
///
/// # Advanced mutation
///
/// [`ensure_owned`](Backing::ensure_owned) / [`as_mut_string`](Backing::as_mut_string)
/// are crate-internal helpers. Mutating the buffer without updating `Url`
/// component offsets will corrupt the record — use public setters instead.
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

    /// Upgrade to [`Owned`](Backing::Owned) in place if currently borrowed.
    pub(crate) fn ensure_owned(&mut self) {
        if let Self::Borrowed(s) = self {
            *self = Self::Owned((*s).to_owned());
        }
    }

    /// Mutable access to the owned serialization buffer (upgrades if needed).
    pub(crate) fn as_mut_string(&mut self) -> &mut String {
        self.ensure_owned();
        match self {
            Self::Owned(s) => s,
            Self::Borrowed(_) => unreachable!("ensure_owned left Borrowed"),
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

    /// Parse `input` with `self` as the base URL.
    ///
    /// The inverse of this is [`Url::make_relative`].
    ///
    /// # Notes
    ///
    /// - A trailing slash is significant. Without it, the last path component is
    ///   treated as a file name and removed to obtain the directory used as the base.
    /// - A scheme-relative special URL as input replaces everything in the base
    ///   after the scheme.
    /// - An absolute URL (with a scheme) as input replaces the whole base URL.
    ///
    /// The returned [`Url`] borrows from `input` when the serialization stays
    /// canonical; call [`Url::into_owned`] if it must outlive a temporary input.
    ///
    /// # Errors
    ///
    /// See [`Url::parse`].
    #[inline]
    pub fn join<'b>(&self, input: &'b str) -> Result<Url<'b>, ParseError> {
        Url::parse_with_base(input, Some(self))
    }

    /// Path segments after the leading `/`, or `None` for cannot-be-a-base URLs.
    ///
    /// Empty segments (for example in `file:////…`) are preserved.
    #[inline]
    #[must_use]
    pub fn path_segments(&self) -> Option<core::str::Split<'_, char>> {
        let path = self.path();
        if self.cannot_be_a_base() || !path.starts_with('/') {
            None
        } else {
            Some(path[1..].split('/'))
        }
    }

    /// Mutable view of path segments, or `Err(())` if cannot-be-a-base.
    ///
    /// See [`PathSegmentsMut`].
    #[allow(clippy::result_unit_err)]
    pub fn path_segments_mut(&mut self) -> Result<PathSegmentsMut<'_, 'a>, ()> {
        if self.cannot_be_a_base() {
            Err(())
        } else {
            Ok(path_segments::new(self))
        }
    }

    /// Create a relative reference from `self` (base) to `url`, if possible.
    ///
    /// Returns `None` when `self` cannot be a base, or when scheme, host, or
    /// port differ. Username and password are ignored. Query and fragment are
    /// taken only from `url`.
    ///
    /// This is the inverse of [`Url::join`].
    #[must_use]
    pub fn make_relative(&self, url: &Url<'_>) -> Option<String> {
        if self.cannot_be_a_base() {
            return None;
        }

        if self.scheme() != url.scheme()
            || self.host() != url.host()
            || self.port_u16() != url.port_u16()
        {
            return None;
        }

        let mut relative = String::new();

        fn extract_path_filename(s: &str) -> (&str, &str) {
            let last_slash_idx = s.rfind('/').unwrap_or(0);
            let (path, filename) = s.split_at(last_slash_idx);
            if filename.is_empty() {
                (path, "")
            } else {
                (path, &filename[1..])
            }
        }

        let (base_path, base_filename) = extract_path_filename(self.path());
        let (url_path, url_filename) = extract_path_filename(url.path());

        let mut base_path = base_path.split('/').peekable();
        let mut url_path = url_path.split('/').peekable();

        while base_path.peek().is_some() && base_path.peek() == url_path.peek() {
            base_path.next();
            url_path.next();
        }

        for base_path_segment in base_path {
            if base_path_segment.is_empty() {
                break;
            }
            if !relative.is_empty() {
                relative.push('/');
            }
            relative.push_str("..");
        }

        for url_path_segment in url_path {
            if !relative.is_empty() {
                relative.push('/');
            }
            relative.push_str(url_path_segment);
        }

        if !relative.is_empty() || base_filename != url_filename {
            if url_filename.is_empty() {
                relative.push('/');
            } else {
                if !relative.is_empty() {
                    relative.push('/');
                }
                relative.push_str(url_filename);
            }
        }

        if let Some(query) = url.query() {
            relative.push('?');
            relative.push_str(query);
        }

        if let Some(fragment) = url.fragment() {
            relative.push('#');
            relative.push_str(fragment);
        }

        Some(relative)
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
    pub(crate) fn byte_at(&self, i: u32) -> u8 {
        self.serialization.as_bytes()[i as usize]
    }

    #[inline]
    pub(crate) fn slice(&self, range: Range<u32>) -> &str {
        &self.as_str()[range.start as usize..range.end as usize]
    }

    /// Whether the URL has an authority (`//` after the scheme).
    #[inline]
    #[must_use]
    pub fn has_authority(&self) -> bool {
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
    ///
    /// Alias: [`Self::host_str`].
    #[inline]
    #[must_use]
    pub fn host(&self) -> Option<&str> {
        if !self.has_host() {
            return None;
        }
        Some(self.slice(self.host_start..self.host_end))
    }

    /// Alias for [`Self::host`] (rust-url naming).
    #[inline]
    #[must_use]
    pub fn host_str(&self) -> Option<&str> {
        self.host()
    }

    /// Domain name host, or `None` when host is null / IPv4 / IPv6.
    #[inline]
    #[must_use]
    pub fn domain(&self) -> Option<&str> {
        if self.flags.contains(UrlFlags::HOST_IPV4) || self.flags.contains(UrlFlags::HOST_IPV6) {
            return None;
        }
        self.host()
    }

    /// Whether this URL uses a special scheme (`http`, `https`, `ws`, `wss`, `ftp`, `file`).
    #[inline]
    #[must_use]
    pub const fn is_special(&self) -> bool {
        self.flags.is_special()
    }

    /// Authority substring (`userinfo@host:port`), or `None` when there is no `//`.
    #[inline]
    #[must_use]
    pub fn authority(&self) -> Option<&str> {
        if !self.has_authority() {
            return None;
        }
        Some(self.slice(self.scheme_end + 3..self.path_start))
    }

    /// Typed host (domain / IPv4 / IPv6), or `None` when host is null.
    #[must_use]
    pub fn host_parsed(&self) -> Option<Host<'_>> {
        let host = self.host()?;
        if self.flags.contains(UrlFlags::HOST_IPV6) {
            let inner = host.strip_prefix('[')?.strip_suffix(']')?;
            return Some(Host::Ipv6(inner.parse().ok()?));
        }
        if self.flags.contains(UrlFlags::HOST_IPV4) {
            return Some(Host::Ipv4(host.parse().ok()?));
        }
        Some(Host::Domain(Cow::Borrowed(host)))
    }

    /// Username component (percent-encoded), or `""`.
    #[inline]
    #[must_use]
    pub fn username(&self) -> &str {
        let scheme_separator_len = 3; // "://"
        if self.has_authority() && self.username_end > self.scheme_end + scheme_separator_len {
            self.slice(self.scheme_end + scheme_separator_len..self.username_end)
        } else {
            ""
        }
    }

    /// Password component, or `""` when the password slot is absent.
    ///
    /// Prefer [`Self::password_opt`] when you need to distinguish “no password”
    /// from an empty password.
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

    /// Password component if the URL has a password slot (`user:` / `user:pass`).
    #[inline]
    #[must_use]
    pub fn password_opt(&self) -> Option<&str> {
        if self.has_authority()
            && (self.username_end as usize) < self.serialization.len()
            && self.byte_at(self.username_end) == b':'
        {
            Some(self.slice(self.username_end + 1..self.host_start - 1))
        } else {
            None
        }
    }

    /// Explicit port, if any (rust-url [`Url::port`](https://docs.rs/url/latest/url/struct.Url.html#method.port) shape).
    #[inline]
    #[must_use]
    pub const fn port(&self) -> Option<u16> {
        self.port_u16()
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

    /// Explicit port, or the scheme’s known default (`http`→80, `https`→443, …).
    #[inline]
    #[must_use]
    pub fn port_or_known_default(&self) -> Option<u16> {
        self.port_u16()
            .or_else(|| parser::default_port_for_scheme(self.scheme()))
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

    /// WHATWG [origin](https://url.spec.whatwg.org/#concept-url-origin) of this URL.
    #[must_use]
    pub fn origin(&self) -> Origin {
        match self.scheme() {
            "blob" => {
                // Parse the path serialization; only http/https/file inner schemes
                // contribute a non-opaque origin (file itself is treated as opaque).
                match Url::parse(self.path()) {
                    Ok(inner) => match inner.scheme() {
                        "http" | "https" | "file" => inner.origin(),
                        _ => Origin::Opaque,
                    },
                    Err(_) => Origin::Opaque,
                }
            }
            "ftp" | "http" | "https" | "ws" | "wss" => {
                let host = match self.host() {
                    Some(h) => h.to_owned(),
                    None => return Origin::Opaque,
                };
                let port = self
                    .port_u16()
                    .or_else(|| parser::default_port_for_scheme(self.scheme()))
                    .unwrap_or(0);
                Origin::Tuple {
                    scheme: self.scheme().to_owned(),
                    host,
                    port,
                }
            }
            // file and everything else → opaque
            _ => Origin::Opaque,
        }
    }

    /// Parse this URL's query as [`SearchParams`] (`URL.searchParams` stringification).
    ///
    /// Empty / missing query yields an empty list.
    #[must_use]
    pub fn search_params(&self) -> SearchParams {
        match self.query() {
            Some(q) => SearchParams::parse(q),
            None => SearchParams::new(),
        }
    }

    /// Iterate decoded query name/value pairs (zero-copy when possible).
    ///
    /// Missing / empty query yields an empty iterator.
    #[must_use]
    pub fn query_pairs(&self) -> search_params::Parse<'_> {
        parse_urlencoded(self.query().unwrap_or("").as_bytes())
    }

    /// Mutate query pairs; on drop the URL query is rewritten.
    pub fn query_pairs_mut(&mut self) -> QueryPairsMut<'_, 'a> {
        query_pairs::new(self)
    }

    /// Replace the query from an iterator of name/value pairs (urlencoded).
    ///
    /// An empty iterator clears the query (`set_query(None)`).
    pub fn set_query_pairs<'b, I>(&mut self, pairs: I)
    where
        I: IntoIterator<Item = (&'b str, &'b str)>,
    {
        let serialized = serialize_urlencoded(pairs);
        if serialized.is_empty() {
            self.set_query(None);
        } else {
            self.set_query(Some(&serialized));
        }
    }

    /// Replace the query using a [`SearchParams`] list.
    ///
    /// An empty list clears the query.
    pub fn set_search_params(&mut self, params: &SearchParams) {
        if params.is_empty() {
            self.set_query(None);
        } else {
            let s = params.serialize();
            self.set_query(Some(&s));
        }
    }

    /// Percent-decode `raw`, allocating only when decoding changes the bytes.
    #[must_use]
    pub fn percent_decode(raw: &str) -> Cow<'_, str> {
        let decoded = parser::percent::percent_decode(raw.as_bytes());
        match decoded {
            Cow::Borrowed(b) => match core::str::from_utf8(b) {
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

impl core::hash::Hash for Url<'_> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl PartialOrd for Url<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Url<'_> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl fmt::Display for Url<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AsRef<str> for Url<'_> {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl core::str::FromStr for Url<'static> {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Url::parse(s).map(Url::into_owned)
    }
}

impl TryFrom<&str> for Url<'static> {
    type Error = ParseError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Url::parse(s).map(Url::into_owned)
    }
}

impl TryFrom<String> for Url<'static> {
    type Error = ParseError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Url::parse(&s).map(Url::into_owned)
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

    #[test]
    fn origin_https_omits_default_port() {
        let url = Url::parse("https://example.com:443/foo").unwrap();
        assert_eq!(url.origin().serialized(), "https://example.com");
    }

    #[test]
    fn origin_blob_https() {
        let url = Url::parse("blob:https://example.com:443/").unwrap();
        assert_eq!(url.origin().serialized(), "https://example.com");
    }

    #[test]
    fn origin_blob_ftp_is_opaque() {
        let url = Url::parse("blob:ftp://host/path").unwrap();
        assert_eq!(url.origin().serialized(), "null");
    }

    #[test]
    fn join_resolves_relative() {
        let base = Url::parse("https://example.com/dir/page").unwrap();
        let joined = base.join("../other").unwrap();
        assert_eq!(joined.as_str(), "https://example.com/other");
    }

    #[test]
    fn join_trailing_slash_matters() {
        let base = Url::parse("https://example.com/dir/page").unwrap();
        assert_eq!(
            base.join("x").unwrap().as_str(),
            "https://example.com/dir/x"
        );
        let base_dir = Url::parse("https://example.com/dir/page/").unwrap();
        assert_eq!(
            base_dir.join("x").unwrap().as_str(),
            "https://example.com/dir/page/x"
        );
    }

    #[test]
    fn path_segments_skips_leading_slash() {
        let url = Url::parse("https://example.com/a/b/").unwrap();
        let segs: Vec<_> = url.path_segments().unwrap().collect();
        assert_eq!(segs, ["a", "b", ""]);
    }

    #[test]
    fn path_segments_none_for_opaque() {
        let url = Url::parse("mailto:user@example.com").unwrap();
        assert!(url.cannot_be_a_base());
        assert!(url.path_segments().is_none());
    }

    #[test]
    fn make_relative_rust_url_examples() {
        let base = Url::parse("https://example.net/a/b.html").unwrap();
        let url = Url::parse("https://example.net/a/c.png").unwrap();
        assert_eq!(base.make_relative(&url).as_deref(), Some("c.png"));

        let base = Url::parse("https://example.net/a/b/").unwrap();
        let url = Url::parse("https://example.net/a/b/c.png").unwrap();
        assert_eq!(base.make_relative(&url).as_deref(), Some("c.png"));

        let base = Url::parse("https://example.net/a/b/").unwrap();
        let url = Url::parse("https://example.net/a/d/c.png").unwrap();
        assert_eq!(base.make_relative(&url).as_deref(), Some("../d/c.png"));

        let base = Url::parse("https://example.net/a/b.html?c=d").unwrap();
        let url = Url::parse("https://example.net/a/b.html?e=f").unwrap();
        assert_eq!(base.make_relative(&url).as_deref(), Some("?e=f"));
    }

    #[test]
    fn make_relative_none_different_host() {
        let base = Url::parse("https://a.example/x").unwrap();
        let url = Url::parse("https://b.example/x").unwrap();
        assert!(base.make_relative(&url).is_none());
    }

    #[test]
    fn make_relative_roundtrip_join() {
        let base = Url::parse("https://example.net/a/b/").unwrap();
        let url = Url::parse("https://example.net/a/d/c.png").unwrap();
        let relative = base.make_relative(&url).unwrap();
        assert_eq!(base.join(&relative).unwrap().as_str(), url.as_str());
    }

    #[test]
    fn path_segments_mut_rust_url_examples() {
        let mut url = Url::parse("mailto:me@example.com").unwrap();
        assert!(url.path_segments_mut().is_err());

        let mut url = Url::parse("http://example.net/foo/index.html").unwrap();
        url.path_segments_mut()
            .unwrap()
            .pop()
            .push("img")
            .push("2/100%.png");
        assert_eq!(url.as_str(), "http://example.net/foo/img/2%2F100%25.png");

        let mut url = Url::parse("https://github.com/servo/rust-url/").unwrap();
        url.path_segments_mut().unwrap().clear().push("logout");
        assert_eq!(url.as_str(), "https://github.com/logout");

        let mut url = Url::parse("https://github.com/servo/rust-url/").unwrap();
        url.path_segments_mut()
            .unwrap()
            .pop_if_empty()
            .push("pulls");
        assert_eq!(url.as_str(), "https://github.com/servo/rust-url/pulls");

        let mut url = Url::parse("https://github.com/servo").unwrap();
        url.path_segments_mut()
            .unwrap()
            .extend(["..", "rust-url", ".", "pulls"]);
        assert_eq!(url.as_str(), "https://github.com/servo/rust-url/pulls");
    }

    #[test]
    fn path_segments_mut_preserves_query() {
        let mut url = Url::parse("https://example.com/a/b?x=1#h").unwrap();
        url.path_segments_mut().unwrap().pop().push("c");
        assert_eq!(url.as_str(), "https://example.com/a/c?x=1#h");
    }

    #[test]
    fn from_str_and_as_ref() {
        let url: Url<'static> = "https://example.com/p".parse().unwrap();
        assert_eq!(url.as_ref(), "https://example.com/p");
    }

    #[test]
    fn host_parsed_variants() {
        let domain = Url::parse("https://example.com/").unwrap();
        assert!(matches!(
            domain.host_parsed(),
            Some(Host::Domain(Cow::Borrowed("example.com")))
        ));

        let v4 = Url::parse("http://127.0.0.1/").unwrap();
        assert!(matches!(v4.host_parsed(), Some(Host::Ipv4(_))));

        let v6 = Url::parse("http://[::1]/").unwrap();
        assert!(matches!(v6.host_parsed(), Some(Host::Ipv6(_))));
    }

    #[test]
    fn host_idna_flag_set_for_ace() {
        let url = Url::parse("https://xn--trke-2oa7j.com/").unwrap();
        assert!(url.flags().contains(UrlFlags::HOST_IDNA));
    }
}
