//! WHATWG [basic URL parser](https://url.spec.whatwg.org/#url-parsing).
//!
//! Builds a CoW serialization (`Backing`) with rust-url-style component offsets.

#![allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::doc_markdown,
    clippy::elidable_lifetime_names,
    clippy::if_not_else,
    clippy::manual_let_else,
    clippy::match_like_matches_macro,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::redundant_closure,
    clippy::redundant_closure_for_method_calls,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::unnecessary_wraps,
    clippy::unwrap_used
)]

pub(crate) mod fast;
pub(crate) mod host;
pub(crate) mod percent;
pub(crate) mod punycode;
pub(crate) mod scan;
pub(crate) mod serialization;
pub(crate) mod unicode_ranges;

use core::fmt::Write as _;
use std::str::Chars;

use self::host::{AppendedHost, Host, append_host, parse_host, parse_opaque_host};
use self::percent::{
    append_fragment, append_path_segment, append_query, in_c0_encode_set, in_path_encode_set,
    in_userinfo_encode_set, percent_encode_char, utf8_percent_encode,
};
use self::scan::{
    find_authority_end, find_file_host_end, find_hash, find_host_end, find_last_at,
    find_path_delim, find_query_or_hash, has_ascii_tab_or_newline,
};
use crate::{Backing, ParseError};

use self::serialization::SerializationBuf;

// ---------------------------------------------------------------------------
// Flag bits (must match `crate::UrlFlags`)
// ---------------------------------------------------------------------------

pub(crate) const FLAG_SPECIAL: u8 = 1 << 0;
pub(crate) const FLAG_HAS_CREDENTIALS: u8 = 1 << 1;
pub(crate) const FLAG_HAS_EMPTY_HOST: u8 = 1 << 2;
pub(crate) const FLAG_OPAQUE_PATH: u8 = 1 << 3;
pub(crate) const FLAG_HAS_PASSWORD: u8 = 1 << 4;
pub(crate) const FLAG_HOST_IPV4: u8 = 1 << 5;
pub(crate) const FLAG_HOST_IPV6: u8 = 1 << 6;

/// Parsed URL record with CoW serialization (rust-url layout).
#[derive(Clone, Debug, Eq)]
pub(crate) struct ParsedUrl<'a> {
    pub serialization: Backing<'a>,
    /// Exclusive end of scheme (before `:`).
    pub scheme_end: u32,
    /// Exclusive end of username in serialization.
    pub username_end: u32,
    pub host_start: u32,
    pub host_end: u32,
    /// `None` = null port (default or absent).
    pub port: Option<u16>,
    pub path_start: u32,
    /// Index of `?`, or `None`.
    pub query_start: Option<u32>,
    /// Index of `#`, or `None`.
    pub fragment_start: Option<u32>,
    pub flags: u8,
}

impl PartialEq for ParsedUrl<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.serialization.as_str() == other.serialization.as_str()
            && self.scheme_end == other.scheme_end
            && self.username_end == other.username_end
            && self.host_start == other.host_start
            && self.host_end == other.host_end
            && self.port == other.port
            && self.path_start == other.path_start
            && self.query_start == other.query_start
            && self.fragment_start == other.fragment_start
            && self.flags == other.flags
    }
}

impl ParsedUrl<'_> {
    #[inline]
    pub(crate) fn scheme(&self) -> &str {
        &self.serialization.as_str()[..self.scheme_end as usize]
    }

    #[inline]
    #[allow(dead_code)]
    pub(crate) fn is_special(&self) -> bool {
        self.flags & FLAG_SPECIAL != 0
    }

    #[inline]
    pub(crate) fn has_opaque_path(&self) -> bool {
        self.flags & FLAG_OPAQUE_PATH != 0
    }

    #[inline]
    fn slice(&self, range: core::ops::RangeTo<u32>) -> &str {
        &self.serialization.as_str()[..range.end as usize]
    }

    #[inline]
    fn host_str(&self) -> Option<&str> {
        if self.host_start == self.host_end {
            if self.flags & FLAG_HAS_EMPTY_HOST != 0 {
                Some("")
            } else {
                // Null host (no authority / opaque path without host).
                None
            }
        } else {
            Some(&self.serialization.as_str()[self.host_start as usize..self.host_end as usize])
        }
    }

    fn byte_at(&self, i: u32) -> u8 {
        self.serialization.as_bytes()[i as usize]
    }

    #[inline]
    #[allow(dead_code)]
    fn as_str(&self) -> &str {
        self.serialization.as_str()
    }
}

/// Parse `input` per the WHATWG basic URL parser.
pub(crate) fn parse<'i>(
    input: &'i str,
    base: Option<&ParsedUrl<'_>>,
) -> Result<ParsedUrl<'i>, ParseError> {
    let trimmed = input.trim_matches(|ch: char| ch <= ' ');
    if base.is_none() {
        if let Some(fast) = fast::try_fast_special_absolute(trimmed) {
            return Ok(fast);
        }
    }
    Parser {
        serialization: SerializationBuf::new(trimmed),
        base_url: base,
    }
    .parse_url(trimmed)
}

// ---------------------------------------------------------------------------
// Scheme helpers
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) enum SchemeType {
    File,
    SpecialNotFile,
    NotSpecial,
}

impl SchemeType {
    #[inline]
    pub(crate) fn is_special(self) -> bool {
        !matches!(self, Self::NotSpecial)
    }

    #[inline]
    pub(crate) fn is_file(self) -> bool {
        matches!(self, Self::File)
    }
}

impl From<&str> for SchemeType {
    fn from(s: &str) -> Self {
        match s {
            "http" | "https" | "ws" | "wss" | "ftp" => Self::SpecialNotFile,
            "file" => Self::File,
            _ => Self::NotSpecial,
        }
    }
}

fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "http" | "ws" => Some(80),
        "https" | "wss" => Some(443),
        "ftp" => Some(21),
        _ => None,
    }
}

fn scheme_flags(scheme_type: SchemeType) -> u8 {
    if scheme_type.is_special() {
        FLAG_SPECIAL
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// Input (skips ASCII tab / LF / CR)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Input<'i> {
    chars: Chars<'i>,
}

impl<'i> Input<'i> {
    fn new(input: &'i str) -> Self {
        Self {
            chars: input.chars(),
        }
    }

    #[allow(dead_code)]
    fn new_trim_c0_control_and_space(original: &'i str) -> Self {
        let input = original.trim_matches(|ch: char| ch <= ' ');
        Self::new(input)
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.clone().next().is_none()
    }

    #[inline]
    fn starts_with_char(&self, p: char) -> bool {
        self.clone().next() == Some(p)
    }

    #[inline]
    fn starts_with_ascii_alpha(&self) -> bool {
        matches!(self.clone().next(), Some(c) if c.is_ascii_alphabetic())
    }

    #[inline]
    fn split_prefix_str(&self, s: &str) -> Option<Self> {
        let mut remaining = self.clone();
        for c in s.chars() {
            if remaining.next() != Some(c) {
                return None;
            }
        }
        Some(remaining)
    }

    #[inline]
    fn split_prefix_char(&self, c: char) -> Option<Self> {
        let mut remaining = self.clone();
        if remaining.next() == Some(c) {
            Some(remaining)
        } else {
            None
        }
    }

    #[inline]
    fn split_first(&self) -> (Option<char>, Self) {
        let mut remaining = self.clone();
        (remaining.next(), remaining)
    }

    #[inline]
    fn peek_char(&self) -> Option<char> {
        self.clone().next()
    }

    #[inline]
    fn count_matching(&self, mut f: impl FnMut(char) -> bool) -> (u32, Self) {
        let mut count = 0;
        let mut remaining = self.clone();
        loop {
            let mut input = remaining.clone();
            match input.next() {
                Some(c) if f(c) => {
                    remaining = input;
                    count += 1;
                }
                _ => return (count, remaining),
            }
        }
    }

    #[inline]
    fn next_utf8(&mut self) -> Option<(char, &'i str)> {
        loop {
            let utf8 = self.chars.as_str();
            let c = self.chars.next()?;
            if !is_ascii_tab_or_newline(c) {
                return Some((c, &utf8[..c.len_utf8()]));
            }
        }
    }

    #[inline]
    fn as_str(&self) -> &'i str {
        self.chars.as_str()
    }

    /// Advance the cursor by `nbytes` raw bytes (must be a char boundary).
    #[inline]
    fn skip_bytes(&mut self, nbytes: usize) {
        let s = self.chars.as_str();
        debug_assert!(nbytes <= s.len());
        debug_assert!(s.is_char_boundary(nbytes));
        self.chars = s[nbytes..].chars();
    }

    /// Remaining bytes without allocating.
    #[inline]
    fn as_bytes(&self) -> &'i [u8] {
        self.chars.as_str().as_bytes()
    }
}

impl Iterator for Input<'_> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        self.chars.by_ref().find(|&c| !is_ascii_tab_or_newline(c))
    }
}

#[inline]
fn is_ascii_tab_or_newline(ch: char) -> bool {
    matches!(ch, '\t' | '\n' | '\r')
}

#[inline]
fn to_u32(i: usize) -> Result<u32, ParseError> {
    u32::try_from(i).map_err(|_| ParseError::InputTooLong)
}

// ---------------------------------------------------------------------------
// Windows drive letter helpers
// ---------------------------------------------------------------------------

#[inline]
fn is_windows_drive_letter(segment: &str) -> bool {
    segment.len() == 2 && starts_with_windows_drive_letter(segment)
}

#[inline]
fn is_normalized_windows_drive_letter(segment: &str) -> bool {
    is_windows_drive_letter(segment) && segment.as_bytes().get(1) == Some(&b':')
}

fn path_starts_with_windows_drive_letter(s: &str) -> bool {
    match s.as_bytes().first() {
        Some(b'/' | b'\\' | b'?' | b'#') => starts_with_windows_drive_letter(&s[1..]),
        _ => false,
    }
}

fn starts_with_windows_drive_letter(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2
        && (b[0] as char).is_ascii_alphabetic()
        && matches!(b[1], b':' | b'|')
        && (b.len() == 2 || matches!(b[2], b'/' | b'\\' | b'?' | b'#'))
}

fn starts_with_windows_drive_letter_segment(input: &Input<'_>) -> bool {
    let mut input = input.clone();
    match (input.next(), input.next(), input.next()) {
        (Some(a), Some(b), Some(c))
            if a.is_ascii_alphabetic()
                && matches!(b, ':' | '|')
                && matches!(c, '/' | '\\' | '?' | '#') =>
        {
            true
        }
        (Some(a), Some(b), None) if a.is_ascii_alphabetic() && matches!(b, ':' | '|') => true,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Parser<'b, 'i> {
    serialization: SerializationBuf<'i>,
    base_url: Option<&'b ParsedUrl<'b>>,
}

impl<'b, 'i> Parser<'b, 'i> {
    fn parse_url(mut self, input: &'i str) -> Result<ParsedUrl<'i>, ParseError> {
        // `input` is already C0/space-trimmed by `parse`.
        let input = Input::new(input);
        if let Ok(remaining) = self.parse_scheme(input.clone()) {
            return self.parse_with_scheme(remaining);
        }

        // no scheme
        if let Some(base_url) = self.base_url {
            if input.starts_with_char('#') {
                return self.fragment_only(base_url, input);
            }
            if base_url.has_opaque_path() {
                return Err(ParseError::Failure);
            }
            let scheme_type = SchemeType::from(base_url.scheme());
            if scheme_type.is_file() {
                self.parse_file(input, scheme_type, Some(base_url))
            } else {
                self.parse_relative(input, scheme_type, base_url)
            }
        } else {
            Err(ParseError::Failure)
        }
    }

    fn parse_scheme<'s>(&mut self, mut input: Input<'s>) -> Result<Input<'s>, ()> {
        if !input.starts_with_ascii_alpha() {
            return Err(());
        }
        debug_assert!(self.serialization.is_empty());
        while let Some(c) = input.next() {
            match c {
                'a'..='z' | '0'..='9' | '+' | '-' | '.' => self.serialization.push(c),
                'A'..='Z' => self.serialization.push(c.to_ascii_lowercase()),
                ':' => return Ok(input),
                _ => {
                    self.serialization.clear();
                    return Err(());
                }
            }
        }
        self.serialization.clear();
        Err(())
    }

    fn parse_with_scheme(mut self, input: Input<'i>) -> Result<ParsedUrl<'i>, ParseError> {
        let scheme_end = to_u32(self.serialization.len())?;
        let scheme_type = SchemeType::from(self.serialization.as_str());
        self.serialization.push(':');
        match scheme_type {
            SchemeType::File => {
                let base_file_url = self.base_url.filter(|base| base.scheme() == "file");
                self.serialization.clear();
                self.parse_file(input, scheme_type, base_file_url)
            }
            SchemeType::SpecialNotFile => {
                let (slashes_count, remaining) = input.count_matching(|c| matches!(c, '/' | '\\'));
                if let Some(base_url) = self.base_url {
                    if slashes_count < 2
                        && base_url.scheme() == &self.serialization.as_str()[..scheme_end as usize]
                    {
                        self.serialization.clear();
                        return self.parse_relative(input, scheme_type, base_url);
                    }
                }
                self.after_double_slash(remaining, scheme_type, scheme_end)
            }
            SchemeType::NotSpecial => self.parse_non_special(input, scheme_type, scheme_end),
        }
    }

    fn parse_non_special(
        mut self,
        input: Input<'i>,
        scheme_type: SchemeType,
        scheme_end: u32,
    ) -> Result<ParsedUrl<'i>, ParseError> {
        if let Some(input) = input.split_prefix_str("//") {
            return self.after_double_slash(input, scheme_type, scheme_end);
        }
        // No authority — opaque or hierarchical path.
        let path_start = to_u32(self.serialization.len())?;
        let username_end = path_start;
        let host_start = path_start;
        let host_end = path_start;
        let port = None;
        let mut flags = scheme_flags(scheme_type);
        let remaining = if let Some(input) = input.split_prefix_char('/') {
            self.serialization.push('/');
            self.parse_path(scheme_type, &mut false, path_start as usize, input)
        } else {
            flags |= FLAG_OPAQUE_PATH;
            self.parse_opaque_path(input)
        };
        self.with_query_and_fragment(
            scheme_type,
            scheme_end,
            username_end,
            host_start,
            host_end,
            port,
            path_start,
            flags,
            remaining,
        )
    }

    fn parse_file(
        mut self,
        input: Input<'i>,
        scheme_type: SchemeType,
        base_file_url: Option<&ParsedUrl>,
    ) -> Result<ParsedUrl<'i>, ParseError> {
        debug_assert!(self.serialization.is_empty());
        let (first_char, input_after_first_char) = input.split_first();
        if matches!(first_char, Some('/' | '\\')) {
            let (next_char, input_after_next_char) = input_after_first_char.split_first();
            if matches!(next_char, Some('/' | '\\')) {
                // file host state
                self.serialization.push_str("file://");
                let scheme_end = "file".len() as u32;
                let host_start = "file://".len() as u32;
                let (path_start_flag, host_empty, remaining) =
                    self.parse_file_host(input_after_next_char)?;
                let host_end = to_u32(self.serialization.len())?;
                let mut has_host = !host_empty;
                let remaining = if path_start_flag {
                    self.parse_path_start(SchemeType::File, &mut has_host, remaining)
                } else {
                    let path_start = self.serialization.len();
                    self.serialization.push('/');
                    self.parse_path(SchemeType::File, &mut has_host, path_start, remaining)
                };

                let (query_start, fragment_start) =
                    self.parse_query_and_fragment(scheme_type, scheme_end, remaining)?;
                let mut flags = FLAG_SPECIAL;
                if !has_host {
                    flags |= FLAG_HAS_EMPTY_HOST;
                }
                return Ok(ParsedUrl {
                    serialization: self.serialization.finish(),
                    scheme_end,
                    username_end: host_start,
                    host_start,
                    host_end,
                    port: None,
                    path_start: host_end,
                    query_start,
                    fragment_start,
                    flags,
                });
            }

            // file slash (single slash)
            self.serialization.push_str("file://");
            let scheme_end = "file".len() as u32;
            let host_start = "file://".len();
            let mut host_end = host_start;
            let mut flags = FLAG_SPECIAL | FLAG_HAS_EMPTY_HOST;
            // WHATWG file slash: always copy base host; only the drive-letter
            // append is gated on the input not starting with a Windows drive.
            if let Some(base_url) = base_file_url {
                if let Some(host_str) = base_url.host_str() {
                    self.serialization.push_str(host_str);
                    host_end = self.serialization.len();
                    flags = FLAG_SPECIAL
                        | (base_url.flags
                            & (FLAG_HOST_IPV4 | FLAG_HOST_IPV6 | FLAG_HAS_EMPTY_HOST));
                    if host_end == host_start {
                        flags |= FLAG_HAS_EMPTY_HOST;
                    } else {
                        flags &= !FLAG_HAS_EMPTY_HOST;
                    }
                }
                if !starts_with_windows_drive_letter_segment(&input_after_first_char) {
                    if let Some(first_segment) = file_first_path_segment(base_url) {
                        if is_normalized_windows_drive_letter(first_segment) {
                            self.serialization.push('/');
                            self.serialization.push_str(first_segment);
                        }
                    }
                }
            }

            let parse_path_input = if let Some(c) = first_char {
                if matches!(c, '/' | '\\' | '?' | '#') {
                    input
                } else {
                    input_after_first_char
                }
            } else {
                input_after_first_char
            };

            let remaining =
                self.parse_path(SchemeType::File, &mut false, host_end, parse_path_input);
            let host_start = host_start as u32;
            let (query_start, fragment_start) =
                self.parse_query_and_fragment(scheme_type, scheme_end, remaining)?;
            let host_end = host_end as u32;
            return Ok(ParsedUrl {
                serialization: self.serialization.finish(),
                scheme_end,
                username_end: host_start,
                host_start,
                host_end,
                port: None,
                path_start: host_end,
                query_start,
                fragment_start,
                flags,
            });
        }

        if let Some(base_url) = base_file_url {
            match first_char {
                None => {
                    let before_fragment = match base_url.fragment_start {
                        Some(i) => &base_url.serialization.as_str()[..i as usize],
                        None => base_url.serialization.as_str(),
                    };
                    self.serialization.push_str(before_fragment);
                    Ok(ParsedUrl {
                        serialization: self.serialization.finish(),
                        fragment_start: None,
                        scheme_end: base_url.scheme_end,
                        username_end: base_url.username_end,
                        host_start: base_url.host_start,
                        host_end: base_url.host_end,
                        port: base_url.port,
                        path_start: base_url.path_start,
                        query_start: base_url.query_start,
                        flags: base_url.flags,
                    })
                }
                Some('?') => {
                    let before_query = match (base_url.query_start, base_url.fragment_start) {
                        (None, None) => base_url.serialization.as_str(),
                        (Some(i), _) | (None, Some(i)) => base_url.slice(..i),
                    };
                    self.serialization.push_str(before_query);
                    let (query_start, fragment_start) =
                        self.parse_query_and_fragment(scheme_type, base_url.scheme_end, input)?;
                    Ok(ParsedUrl {
                        serialization: self.serialization.finish(),
                        query_start,
                        fragment_start,
                        scheme_end: base_url.scheme_end,
                        username_end: base_url.username_end,
                        host_start: base_url.host_start,
                        host_end: base_url.host_end,
                        port: base_url.port,
                        path_start: base_url.path_start,
                        flags: base_url.flags,
                    })
                }
                Some('#') => self.fragment_only(base_url, input),
                _ => {
                    // Copy base host/path/query, then either shorten or reset path
                    // for a Windows drive letter quirk (host is preserved).
                    let before_query = match (base_url.query_start, base_url.fragment_start) {
                        (None, None) => base_url.serialization.as_str(),
                        (Some(i), _) | (None, Some(i)) => base_url.slice(..i),
                    };
                    self.serialization.push_str(before_query);
                    if starts_with_windows_drive_letter_segment(&input) {
                        self.serialization.truncate(base_url.path_start as usize);
                    } else {
                        self.shorten_path(SchemeType::File, base_url.path_start as usize);
                    }
                    ensure_path_segment_boundary(
                        &mut self.serialization,
                        base_url.path_start as usize,
                        &input,
                        true,
                    );
                    let remaining = self.parse_path(
                        SchemeType::File,
                        &mut true,
                        base_url.path_start as usize,
                        input,
                    );
                    self.with_query_and_fragment(
                        SchemeType::File,
                        base_url.scheme_end,
                        base_url.username_end,
                        base_url.host_start,
                        base_url.host_end,
                        base_url.port,
                        base_url.path_start,
                        base_url.flags,
                        remaining,
                    )
                }
            }
        } else {
            self.serialization.push_str("file:///");
            let scheme_end = "file".len() as u32;
            let path_start = "file://".len();
            let remaining = self.parse_path(SchemeType::File, &mut false, path_start, input);
            let (query_start, fragment_start) =
                self.parse_query_and_fragment(SchemeType::File, scheme_end, remaining)?;
            let path_start = path_start as u32;
            Ok(ParsedUrl {
                serialization: self.serialization.finish(),
                scheme_end,
                username_end: path_start,
                host_start: path_start,
                host_end: path_start,
                port: None,
                path_start,
                query_start,
                fragment_start,
                flags: FLAG_SPECIAL | FLAG_HAS_EMPTY_HOST,
            })
        }
    }

    fn parse_relative(
        mut self,
        input: Input<'i>,
        scheme_type: SchemeType,
        base_url: &ParsedUrl,
    ) -> Result<ParsedUrl<'i>, ParseError> {
        debug_assert!(self.serialization.is_empty());
        let (first_char, input_after_first_char) = input.split_first();
        match first_char {
            None => {
                let before_fragment = match base_url.fragment_start {
                    Some(i) => &base_url.serialization.as_str()[..i as usize],
                    None => base_url.serialization.as_str(),
                };
                self.serialization.push_str(before_fragment);
                Ok(ParsedUrl {
                    serialization: self.serialization.finish(),
                    fragment_start: None,
                    scheme_end: base_url.scheme_end,
                    username_end: base_url.username_end,
                    host_start: base_url.host_start,
                    host_end: base_url.host_end,
                    port: base_url.port,
                    path_start: base_url.path_start,
                    query_start: base_url.query_start,
                    flags: base_url.flags,
                })
            }
            Some('?') => {
                let before_query = match (base_url.query_start, base_url.fragment_start) {
                    (None, None) => base_url.serialization.as_str(),
                    (Some(i), _) | (None, Some(i)) => base_url.slice(..i),
                };
                self.serialization.push_str(before_query);
                let (query_start, fragment_start) =
                    self.parse_query_and_fragment(scheme_type, base_url.scheme_end, input)?;
                Ok(ParsedUrl {
                    serialization: self.serialization.finish(),
                    query_start,
                    fragment_start,
                    scheme_end: base_url.scheme_end,
                    username_end: base_url.username_end,
                    host_start: base_url.host_start,
                    host_end: base_url.host_end,
                    port: base_url.port,
                    path_start: base_url.path_start,
                    flags: base_url.flags,
                })
            }
            Some('#') => self.fragment_only(base_url, input),
            // Relative slash: only `/`, or `\` when the URL is special.
            Some('\\') if !scheme_type.is_special() => {
                // Non-special: `\` is a normal path code point (not a delimiter).
                self.parse_relative_path(input, scheme_type, base_url)
            }
            Some('/' | '\\') => {
                let slash_pred: fn(char) -> bool = if scheme_type.is_special() {
                    |c| matches!(c, '/' | '\\')
                } else {
                    |c| c == '/'
                };
                let (slashes_count, _) = input.count_matching(slash_pred);
                if slashes_count >= 2 {
                    let scheme_end = base_url.scheme_end;
                    debug_assert!(base_url.byte_at(scheme_end) == b':');
                    self.serialization
                        .push_str(base_url.slice(..scheme_end + 1));
                    // Consume exactly two authority slashes; leave the rest for
                    // special-authority-ignore (special) or path (non-special).
                    let mut after_two = input.clone();
                    let _ = after_two.next();
                    let _ = after_two.next();
                    return self.after_double_slash(after_two, scheme_type, scheme_end);
                }
                let path_start = base_url.path_start;
                self.serialization.push_str(base_url.slice(..path_start));
                self.serialization.push('/');
                let remaining = self.parse_path(
                    scheme_type,
                    &mut true,
                    path_start as usize,
                    input_after_first_char,
                );
                self.with_query_and_fragment(
                    scheme_type,
                    base_url.scheme_end,
                    base_url.username_end,
                    base_url.host_start,
                    base_url.host_end,
                    base_url.port,
                    base_url.path_start,
                    base_url.flags,
                    remaining,
                )
            }
            _ => self.parse_relative_path(input, scheme_type, base_url),
        }
    }

    /// Relative-state "otherwise" branch: copy base, shorten path, continue in path state.
    fn parse_relative_path(
        mut self,
        input: Input<'i>,
        scheme_type: SchemeType,
        base_url: &ParsedUrl<'_>,
    ) -> Result<ParsedUrl<'i>, ParseError> {
        let before_query = match (base_url.query_start, base_url.fragment_start) {
            (None, None) => base_url.serialization.as_str(),
            (Some(i), _) | (None, Some(i)) => base_url.slice(..i),
        };
        self.serialization.push_str(before_query);
        self.shorten_path(scheme_type, base_url.path_start as usize);
        ensure_path_segment_boundary(
            &mut self.serialization,
            base_url.path_start as usize,
            &input,
            scheme_type.is_special(),
        );
        let remaining = match input.split_first() {
            (Some('/'), remaining) => self.parse_path(
                scheme_type,
                &mut true,
                base_url.path_start as usize,
                remaining,
            ),
            _ => self.parse_path(scheme_type, &mut true, base_url.path_start as usize, input),
        };
        self.with_query_and_fragment(
            scheme_type,
            base_url.scheme_end,
            base_url.username_end,
            base_url.host_start,
            base_url.host_end,
            base_url.port,
            base_url.path_start,
            base_url.flags,
            remaining,
        )
    }

    fn after_double_slash(
        mut self,
        input: Input<'i>,
        scheme_type: SchemeType,
        scheme_end: u32,
    ) -> Result<ParsedUrl<'i>, ParseError> {
        self.serialization.push('/');
        self.serialization.push('/');
        // special authority ignore slashes state: consume extra `/` and `\`.
        let mut input = input;
        if scheme_type.is_special() {
            while matches!(input.peek_char(), Some('/' | '\\')) {
                let _ = input.next();
            }
        }
        let before_authority = self.serialization.len();
        let (username_end, remaining, cred_flags) = self.parse_userinfo(input, scheme_type)?;
        let has_authority = before_authority != self.serialization.len();
        let host_start = to_u32(self.serialization.len())?;
        let (host_end, host_kind, port, remaining, empty_host) =
            self.parse_host_and_port(remaining, scheme_end, scheme_type)?;
        if empty_host && has_authority {
            return Err(ParseError::Failure);
        }
        let path_start = to_u32(self.serialization.len())?;
        let mut has_host = !empty_host;
        let remaining = self.parse_path_start(scheme_type, &mut has_host, remaining);
        let mut flags = scheme_flags(scheme_type) | cred_flags;
        if empty_host || !has_host {
            if scheme_type.is_special() {
                flags |= FLAG_HAS_EMPTY_HOST;
            }
        } else {
            flags |= host_flags_from_kind(host_kind);
        }
        self.with_query_and_fragment(
            scheme_type,
            scheme_end,
            username_end,
            host_start,
            host_end,
            port,
            path_start,
            flags,
            remaining,
        )
    }

    /// Returns `(username_end, remaining, credential flags)`.
    fn parse_userinfo<'s>(
        &mut self,
        mut input: Input<'s>,
        scheme_type: SchemeType,
    ) -> Result<(u32, Input<'s>, u8), ParseError> {
        let bytes = input.as_bytes();
        let special = scheme_type.is_special();
        let authority_end = find_authority_end(bytes, special);

        let Some(at) = find_last_at(bytes, authority_end) else {
            return Ok((to_u32(self.serialization.len())?, input, 0));
        };

        // Empty credentials (`@host`) — no username/password written.
        // Tabs/LF/CR before `@` count as empty too (`\t@host`).
        let userinfo_prefix = &input.as_str()[..at];
        let userinfo_empty = userinfo_prefix
            .bytes()
            .all(|b| matches!(b, b'\t' | b'\n' | b'\r'));
        if at == 0 || userinfo_empty {
            input.skip_bytes(at + 1); // consume optional ws + '@'
            let (c, _) = input.split_first();
            if matches!(c, Some('/' | '?' | '#')) || (special && c == Some('\\')) || c.is_none()
            {
                return Err(ParseError::Failure);
            }
            return Ok((to_u32(self.serialization.len())?, input, 0));
        }

        // Encode userinfo bytes before `@`, skipping ASCII tab/LF/CR.
        let userinfo = userinfo_prefix;
        let mut username_end = None;
        let mut has_password = false;
        let mut has_username = false;
        let mut i = 0;
        let ub = userinfo.as_bytes();
        while i < ub.len() {
            let b = ub[i];
            if matches!(b, b'\t' | b'\n' | b'\r') {
                i += 1;
                continue;
            }
            if b == b':' && username_end.is_none() {
                username_end = Some(to_u32(self.serialization.len())?);
                // Peek whether any non-ignored byte remains after this `:`.
                let rest_has_char = ub[i + 1..]
                    .iter()
                    .any(|&c| !matches!(c, b'\t' | b'\n' | b'\r'));
                if rest_has_char {
                    self.serialization.push(':');
                    has_password = true;
                }
                i += 1;
                continue;
            }
            if !has_password {
                has_username = true;
            }
            // Multi-byte UTF-8: take full char for percent-encode.
            let ch = userinfo[i..].chars().next().unwrap();
            let len = ch.len_utf8();
            percent_encode_char(ch, in_userinfo_encode_set, &mut self.serialization);
            i += len;
        }

        let username_end = match username_end {
            Some(i) => i,
            None => to_u32(self.serialization.len())?,
        };
        let mut flags = 0u8;
        if has_username || has_password {
            self.serialization.push('@');
            flags |= FLAG_HAS_CREDENTIALS;
        }
        if has_password {
            flags |= FLAG_HAS_PASSWORD;
        }
        input.skip_bytes(at + 1); // past `@`
        Ok((username_end, input, flags))
    }

    fn parse_host_and_port<'s>(
        &mut self,
        input: Input<'s>,
        scheme_end: u32,
        scheme_type: SchemeType,
    ) -> Result<(u32, HostKind, Option<u16>, Input<'s>, bool), ParseError> {
        let (appended, remaining) = self.parse_host_input_append(input, scheme_type)?;
        let empty_host = matches!(appended, AppendedHost::EmptyDomain);
        if empty_host {
            if remaining.starts_with_char(':') {
                return Err(ParseError::Failure);
            }
            if scheme_type.is_special() {
                return Err(ParseError::Failure);
            }
        }
        let host_end = to_u32(self.serialization.len())?;
        let host_kind = HostKind::from(appended);

        let (port, remaining) = if let Some(remaining) = remaining.split_prefix_char(':') {
            let scheme = &self.serialization.as_str()[..scheme_end as usize];
            let default = default_port(scheme);
            let (port, remaining) = Self::parse_port(remaining, default)?;
            if let Some(port) = port {
                self.serialization.push(':');
                let buf = itoa_buf(port);
                self.serialization.push_str(buf.as_str());
            }
            (port, remaining)
        } else {
            (None, remaining)
        };
        Ok((host_end, host_kind, port, remaining, empty_host))
    }

    /// Parse host from `input` and append its serialization form (special hosts
    /// write ACE/IPv4/IPv6 once; opaque hosts still go through [`Host`] Display).
    fn parse_host_input_append<'h>(
        &mut self,
        mut input: Input<'h>,
        scheme_type: SchemeType,
    ) -> Result<(AppendedHost, Input<'h>), ParseError> {
        debug_assert!(!scheme_type.is_file());
        let input_str = input.as_str();
        let bytes = input_str.as_bytes();
        let (byte_end, has_ignored) = find_host_end(bytes, scheme_type.is_special());

        if has_ignored {
            // Slow path: rebuild host without tab/LF/CR (may include non-ASCII).
            let mut rebuilt = String::with_capacity(byte_end);
            for c in input_str[..byte_end].chars() {
                if !is_ascii_tab_or_newline(c) {
                    rebuilt.push(c);
                }
            }
            input.skip_bytes(byte_end);
            if scheme_type == SchemeType::SpecialNotFile && rebuilt.is_empty() {
                return Err(ParseError::Failure);
            }
            if !scheme_type.is_special() {
                return self.append_opaque_host(&rebuilt).map(|h| (h, input));
            }
            if rebuilt.is_empty() {
                return Ok((AppendedHost::EmptyDomain, input));
            }
            return append_host(&rebuilt, &mut self.serialization).map(|h| (h, input));
        }

        let host_str = &input_str[..byte_end];
        input.skip_bytes(byte_end);

        if scheme_type == SchemeType::SpecialNotFile && host_str.is_empty() {
            return Err(ParseError::Failure);
        }
        if !scheme_type.is_special() {
            return self.append_opaque_host(host_str).map(|h| (h, input));
        }
        if host_str.is_empty() {
            return Ok((AppendedHost::EmptyDomain, input));
        }
        append_host(host_str, &mut self.serialization).map(|h| (h, input))
    }

    fn append_opaque_host(&mut self, host_str: &str) -> Result<AppendedHost, ParseError> {
        let host = parse_opaque_host(host_str)?;
        match &host {
            Host::Domain(d) if d.is_empty() => Ok(AppendedHost::EmptyDomain),
            Host::Domain(d) => {
                self.serialization.push_str(d.as_ref());
                Ok(AppendedHost::Domain)
            }
            Host::Ipv4(ip) => {
                let _ = write!(&mut self.serialization, "{ip}");
                Ok(AppendedHost::Ipv4)
            }
            Host::Ipv6(_) => {
                let _ = write!(&mut self.serialization, "{host}");
                Ok(AppendedHost::Ipv6)
            }
        }
    }

    /// Returns `(use_path_start, host_is_empty, remaining)`.
    fn parse_file_host<'s>(
        &mut self,
        input: Input<'s>,
    ) -> Result<(bool, bool, Input<'s>), ParseError> {
        let (has_host_buffer, host_str, remaining) = Self::file_host(input)?;
        // Windows drive letter quirk: buffer is reused in path state (not path start).
        if !has_host_buffer {
            return Ok((false, true, remaining));
        }
        if host_str.is_empty() {
            // Empty host → path start state (preserves leading empty path segments).
            return Ok((true, true, remaining));
        }
        match parse_host(&host_str)? {
            Host::Domain(ref d) if d.as_ref() == "localhost" || d.is_empty() => {
                Ok((true, true, remaining))
            }
            host => {
                let _ = write!(&mut self.serialization, "{host}");
                Ok((true, false, remaining))
            }
        }
    }

    fn file_host(input: Input<'_>) -> Result<(bool, String, Input<'_>), ParseError> {
        let input_str = input.as_str();
        let bytes = input_str.as_bytes();
        let (byte_end, has_ignored) = find_file_host_end(bytes);

        let host_str = if has_ignored {
            let mut out = String::with_capacity(byte_end);
            for c in input_str[..byte_end].chars() {
                if !is_ascii_tab_or_newline(c) {
                    out.push(c);
                }
            }
            out
        } else {
            input_str[..byte_end].to_owned()
        };

        if is_windows_drive_letter(&host_str) {
            return Ok((false, String::new(), input));
        }
        let mut remaining = input;
        remaining.skip_bytes(byte_end);
        Ok((true, host_str, remaining))
    }

    fn parse_port(
        mut input: Input<'_>,
        default: Option<u16>,
    ) -> Result<(Option<u16>, Input<'_>), ParseError> {
        let mut port: u32 = 0;
        let mut has_any_digit = false;
        while let (Some(c), remaining) = input.split_first() {
            if let Some(digit) = c.to_digit(10) {
                port = port * 10 + digit;
                if port > u16::MAX as u32 {
                    return Err(ParseError::Failure);
                }
                has_any_digit = true;
                input = remaining;
            } else if !matches!(c, '/' | '\\' | '?' | '#') {
                return Err(ParseError::Failure);
            } else {
                break;
            }
        }
        let mut opt_port = Some(port as u16);
        if !has_any_digit || opt_port == default {
            opt_port = None;
        }
        Ok((opt_port, input))
    }

    fn parse_path_start<'s>(
        &mut self,
        scheme_type: SchemeType,
        has_host: &mut bool,
        input: Input<'s>,
    ) -> Input<'s> {
        let path_start = self.serialization.len();
        let (maybe_c, remaining) = input.split_first();
        if scheme_type.is_special() {
            // Special URLs always have a non-empty path. Note that `file://` /
            // `https://` already end with `/` from the authority delimiter —
            // that slash is *before* path_start, so we still append a path `/`.
            self.serialization.push('/');
            if matches!(maybe_c, Some('/' | '\\')) {
                return self.parse_path(scheme_type, has_host, path_start, remaining);
            }
            return self.parse_path(scheme_type, has_host, path_start, input);
        } else if matches!(maybe_c, Some('?' | '#')) {
            return input;
        }

        if maybe_c.is_some() && maybe_c != Some('/') {
            self.serialization.push('/');
        }
        self.parse_path(scheme_type, has_host, path_start, input)
    }

    fn parse_path<'s>(
        &mut self,
        scheme_type: SchemeType,
        _has_host: &mut bool,
        path_start: usize,
        mut input: Input<'s>,
    ) -> Input<'s> {
        let special = scheme_type.is_special();
        loop {
            let segment_start = self.serialization.len();
            let mut ends_with_slash = false;

            // SIMD fast path: no tab/LF/CR in the upcoming segment → bulk copy.
            let raw = input.as_str();
            let bytes = raw.as_bytes();
            let delim = find_path_delim(bytes, special);
            let seg_end = delim.map_or(bytes.len(), |(i, _)| i);
            let chunk = &bytes[..seg_end];

            if !(has_ascii_tab_or_newline(chunk)
                || (scheme_type.is_file()
                    && self.serialization.len() > path_start
                    && is_normalized_windows_drive_letter(&self.serialization[path_start + 1..])))
            {
                // Bulk-append the segment (percent-encode only when needed).
                let segment_str = &raw[..seg_end];
                append_path_segment(segment_str, &mut self.serialization);
                input.skip_bytes(seg_end);

                if let Some((_, d)) = delim {
                    match d {
                        b'/' => {
                            self.serialization.push('/');
                            ends_with_slash = true;
                            input.skip_bytes(1);
                        }
                        b'\\' if special => {
                            self.serialization.push('/');
                            ends_with_slash = true;
                            input.skip_bytes(1);
                        }
                        b'?' | b'#' => {
                            // Leave delimiter for the caller.
                        }
                        _ => unreachable!(),
                    }
                }
            } else {
                // Slow path: char-wise (tabs / Windows-drive mid-segment quirks).
                loop {
                    let input_before_c = input.clone();
                    let c = match input.next() {
                        Some(c) => c,
                        None => break,
                    };
                    match c {
                        '/' => {
                            self.serialization.push(c);
                            ends_with_slash = true;
                            break;
                        }
                        '\\' if special => {
                            self.serialization.push('/');
                            ends_with_slash = true;
                            break;
                        }
                        '?' | '#' => {
                            input = input_before_c;
                            break;
                        }
                        _ => {
                            // Do not inject `/` after a Windows drive letter. WHATWG /
                            // Chrome keep `file:///p:foo` and `file:///p:./x` without an
                            // extra slash (injecting one left a stale `/./` when tabs
                            // delayed the `.` segment recognition).
                            percent_encode_char(c, in_path_encode_set, &mut self.serialization);
                        }
                    }
                }
            }

            let seg_action = {
                let segment_before_slash = if ends_with_slash {
                    &self.serialization[segment_start..self.serialization.len() - 1]
                } else {
                    &self.serialization[segment_start..self.serialization.len()]
                };
                match segment_before_slash {
                    ".." | "%2e%2e" | "%2e%2E" | "%2E%2e" | "%2E%2E" | "%2e." | "%2E." | ".%2e"
                    | ".%2E" => SegAction::DotDot,
                    "." | "%2e" | "%2E" => SegAction::Dot,
                    s if scheme_type.is_file()
                        && is_windows_drive_letter(s)
                        && self.serialization[path_start..segment_start]
                            .bytes()
                            .all(|b| b == b'/') =>
                    {
                        SegAction::WinDrive(s.chars().next().unwrap())
                    }
                    _ => SegAction::Other,
                }
            };
            match seg_action {
                SegAction::DotDot => {
                    debug_assert!(
                        segment_start == 0
                            || self.serialization.as_bytes().get(segment_start - 1) == Some(&b'/')
                    );
                    self.serialization.truncate(segment_start);
                    if self.serialization.ends_with('/')
                        && last_slash_can_be_removed(
                            &self.serialization,
                            path_start,
                            scheme_type.is_file(),
                        )
                    {
                        self.serialization.pop();
                    }
                    self.shorten_path(scheme_type, path_start);
                    if !self.serialization.ends_with('/') {
                        self.serialization.push('/');
                    }
                }
                SegAction::Dot => {
                    self.serialization.truncate(segment_start);
                    if !self.serialization.ends_with('/') {
                        self.serialization.push('/');
                    }
                }
                SegAction::WinDrive(c) => {
                    self.serialization.truncate(segment_start);
                    self.serialization.push(c);
                    self.serialization.push(':');
                    if ends_with_slash {
                        self.serialization.push('/');
                    }
                }
                SegAction::Other => {}
            }
            if !ends_with_slash {
                break;
            }
        }

        // NOTE: Older parsers collapsed leading empty file path segments
        // (`trim_start_matches('/')`). Current WHATWG + WPT preserve them
        // (e.g. `file:////`, `file://spider///`).

        input
    }

    fn shorten_path(&mut self, scheme_type: SchemeType, path_start: usize) {
        if self.serialization.len() == path_start {
            return;
        }
        if scheme_type.is_file()
            && is_normalized_windows_drive_letter(&self.serialization[path_start..])
        {
            return;
        }
        self.pop_path(scheme_type, path_start);
    }

    fn pop_path(&mut self, scheme_type: SchemeType, path_start: usize) {
        if self.serialization.len() > path_start {
            let slash_position = self.serialization[path_start..].rfind('/').unwrap();
            let segment_start = path_start + slash_position + 1;
            // Don’t pop a Windows drive letter (file URLs only).
            if !(scheme_type.is_file()
                && is_normalized_windows_drive_letter(&self.serialization[segment_start..]))
            {
                self.serialization.truncate(segment_start);
            }
        }
    }

    fn parse_opaque_path<'s>(&mut self, mut input: Input<'s>) -> Input<'s> {
        let path_begin = self.serialization.len();
        loop {
            let raw = input.as_str();
            let bytes = raw.as_bytes();

            // Fast path when the next stretch has no tab/LF/CR.
            match find_query_or_hash(bytes) {
                Some((i, _)) if !has_ascii_tab_or_newline(&bytes[..i]) => {
                    // Trailing spaces before `?`/`#`: all but the last stay literal;
                    // the last becomes `%20` (WHATWG opaque-path round-trip rule).
                    let mut space_start = i;
                    while space_start > 0 && bytes[space_start - 1] == b' ' {
                        space_start -= 1;
                    }
                    if space_start > 0 {
                        utf8_percent_encode(
                            &raw[..space_start],
                            in_c0_encode_set,
                            &mut self.serialization,
                        );
                    }
                    if i > space_start {
                        let n = i - space_start;
                        for _ in 0..n.saturating_sub(1) {
                            self.serialization.push(' ');
                        }
                        self.serialization.push_str("%20");
                    }
                    input.skip_bytes(i);
                    return input;
                }
                None if !has_ascii_tab_or_newline(bytes) => {
                    // EOF: strip trailing spaces, bulk-encode the rest.
                    let mut end = bytes.len();
                    while end > 0 && bytes[end - 1] == b' ' {
                        end -= 1;
                    }
                    if end > 0 {
                        utf8_percent_encode(&raw[..end], in_c0_encode_set, &mut self.serialization);
                    }
                    input.skip_bytes(bytes.len());
                    return input;
                }
                _ => {}
            }

            // Slow path (tabs / mixed): char-wise with the trailing-space rule.
            let input_before_c = input.clone();
            match input.next_utf8() {
                Some(('?' | '#', _)) => return input_before_c,
                Some((' ', _)) => {
                    let remaining = input.clone();
                    if matches!(remaining.peek_char(), Some('?' | '#')) {
                        self.serialization.push_str("%20");
                    } else {
                        self.serialization.push(' ');
                    }
                }
                Some((_, utf8_c)) => {
                    utf8_percent_encode(utf8_c, in_c0_encode_set, &mut self.serialization);
                }
                None => {
                    while self.serialization.len() > path_begin && self.serialization.ends_with(' ')
                    {
                        self.serialization.pop();
                    }
                    return input;
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn with_query_and_fragment(
        mut self,
        scheme_type: SchemeType,
        scheme_end: u32,
        username_end: u32,
        host_start: u32,
        host_end: u32,
        port: Option<u16>,
        mut path_start: u32,
        flags: u8,
        remaining: Input<'i>,
    ) -> Result<ParsedUrl<'i>, ParseError> {
        // Anarchist URL empty-path-segment fix (rust-url / WHATWG).
        let scheme_end_usize = scheme_end as usize;
        let path_start_usize = path_start as usize;
        if path_start_usize == scheme_end_usize + 1 {
            if self.serialization[path_start_usize..].starts_with("//") {
                self.serialization.insert_str(path_start_usize, "/.");
                path_start += 2;
            }
        } else if path_start_usize == scheme_end_usize + 3
            && &self.serialization[scheme_end_usize..path_start_usize] == ":/."
        {
            debug_assert_eq!(self.serialization.as_bytes()[path_start_usize], b'/');
            if self
                .serialization
                .as_bytes()
                .get(path_start_usize + 1)
                .copied()
                != Some(b'/')
            {
                self.serialization
                    .replace_range(scheme_end_usize..path_start_usize, ":");
                path_start -= 2;
            }
        }

        let (query_start, fragment_start) =
            self.parse_query_and_fragment(scheme_type, scheme_end, remaining)?;
        Ok(ParsedUrl {
            serialization: self.serialization.finish(),
            scheme_end,
            username_end,
            host_start,
            host_end,
            port,
            path_start,
            query_start,
            fragment_start,
            flags,
        })
    }

    fn parse_query_and_fragment(
        &mut self,
        scheme_type: SchemeType,
        _scheme_end: u32,
        mut input: Input<'i>,
    ) -> Result<(Option<u32>, Option<u32>), ParseError> {
        let mut query_start = None;
        match input.next() {
            Some('#') => {}
            Some('?') => {
                query_start = Some(to_u32(self.serialization.len())?);
                self.serialization.push('?');
                if let Some(remaining) = self.parse_query(scheme_type, input) {
                    input = remaining;
                } else {
                    return Ok((query_start, None));
                }
            }
            None => return Ok((None, None)),
            _ => panic!("parse_query_and_fragment called without ? or #"),
        }

        let fragment_start = to_u32(self.serialization.len())?;
        self.serialization.push('#');
        self.parse_fragment(input);
        Ok((query_start, Some(fragment_start)))
    }

    fn parse_query<'s>(
        &mut self,
        scheme_type: SchemeType,
        mut input: Input<'s>,
    ) -> Option<Input<'s>> {
        let special = scheme_type.is_special();
        loop {
            let raw = input.as_str();
            let bytes = raw.as_bytes();
            match find_hash(bytes) {
                Some(i) if !has_ascii_tab_or_newline(&bytes[..i]) => {
                    append_query(&raw[..i], special, &mut self.serialization);
                    input.skip_bytes(i + 1); // consume `#`
                    return Some(input);
                }
                None if !has_ascii_tab_or_newline(bytes) => {
                    append_query(raw, special, &mut self.serialization);
                    input.skip_bytes(bytes.len());
                    return None;
                }
                _ => {
                    // Slow path with ignored chars.
                    let input_before = input.clone();
                    match input.next_utf8() {
                        Some(('#', _)) => {
                            let _ = input_before;
                            return Some(input);
                        }
                        Some((_, utf8_c)) => {
                            append_query(utf8_c, special, &mut self.serialization);
                        }
                        None => return None,
                    }
                }
            }
        }
    }

    fn fragment_only(
        mut self,
        base_url: &ParsedUrl,
        mut input: Input<'i>,
    ) -> Result<ParsedUrl<'i>, ParseError> {
        let before_fragment = match base_url.fragment_start {
            Some(i) => base_url.slice(..i),
            None => base_url.serialization.as_str(),
        };
        debug_assert!(self.serialization.is_empty());
        self.serialization
            .reserve(before_fragment.len() + input.as_str().len());
        self.serialization.push_str(before_fragment);
        self.serialization.push('#');
        let next = input.next();
        debug_assert_eq!(next, Some('#'));
        self.parse_fragment(input);
        Ok(ParsedUrl {
            serialization: self.serialization.finish(),
            fragment_start: Some(to_u32(before_fragment.len())?),
            scheme_end: base_url.scheme_end,
            username_end: base_url.username_end,
            host_start: base_url.host_start,
            host_end: base_url.host_end,
            port: base_url.port,
            path_start: base_url.path_start,
            query_start: base_url.query_start,
            flags: base_url.flags,
        })
    }

    fn parse_fragment(&mut self, mut input: Input<'i>) {
        loop {
            let raw = input.as_str();
            let bytes = raw.as_bytes();
            if !has_ascii_tab_or_newline(bytes) {
                append_fragment(raw, &mut self.serialization);
                input.skip_bytes(bytes.len());
                return;
            }
            match input.next_utf8() {
                Some((_, utf8_c)) => {
                    append_fragment(utf8_c, &mut self.serialization);
                }
                None => return,
            }
        }
    }
}

fn last_slash_can_be_removed(serialization: &str, path_start: usize, is_file: bool) -> bool {
    let url_before_segment = &serialization[..serialization.len() - 1];
    if let Some(segment_before_start) = url_before_segment.rfind('/') {
        // Only file URLs protect the slash after a Windows drive letter.
        segment_before_start >= path_start
            && !(is_file
                && path_starts_with_windows_drive_letter(&serialization[segment_before_start..]))
    } else {
        false
    }
}

fn file_first_path_segment<'a>(url: &'a ParsedUrl<'_>) -> Option<&'a str> {
    let s = url.serialization.as_str();
    let path = &s[url.path_start as usize..];
    let path = match (url.query_start, url.fragment_start) {
        (Some(q), _) => &s[url.path_start as usize..q as usize],
        (None, Some(f)) => &s[url.path_start as usize..f as usize],
        (None, None) => path,
    };
    let path = path.strip_prefix('/').unwrap_or(path);
    if path.is_empty() {
        return None;
    }
    Some(path.split('/').next().unwrap_or(path))
}

/// After shortening a path, ensure the next path-state segment starts on a
/// boundary (list-model append), not glued onto the previous segment.
fn ensure_path_segment_boundary(
    serialization: &mut SerializationBuf<'_>,
    path_start: usize,
    input: &Input<'_>,
    special_or_nonempty_ok: bool,
) {
    if input.is_empty() && !special_or_nonempty_ok {
        return;
    }
    if serialization.len() == path_start {
        if special_or_nonempty_ok || !input.is_empty() {
            serialization.push('/');
        }
        return;
    }
    // New path-state buffer must not concatenate onto the last segment.
    if !input.is_empty() && !serialization.ends_with('/') && !matches!(input.peek_char(), Some('/'))
    {
        serialization.push('/');
    }
}

#[derive(Copy, Clone)]
enum SegAction {
    DotDot,
    Dot,
    WinDrive(char),
    Other,
}

#[derive(Copy, Clone)]
enum HostKind {
    Domain,
    Ipv4,
    Ipv6,
}

impl From<&Host<'_>> for HostKind {
    fn from(h: &Host) -> Self {
        match h {
            Host::Domain(_) => Self::Domain,
            Host::Ipv4(_) => Self::Ipv4,
            Host::Ipv6(_) => Self::Ipv6,
        }
    }
}

impl From<AppendedHost> for HostKind {
    fn from(h: AppendedHost) -> Self {
        match h {
            AppendedHost::EmptyDomain | AppendedHost::Domain => Self::Domain,
            AppendedHost::Ipv4 => Self::Ipv4,
            AppendedHost::Ipv6 => Self::Ipv6,
        }
    }
}

fn host_flags_from_kind(kind: HostKind) -> u8 {
    match kind {
        HostKind::Domain => 0,
        HostKind::Ipv4 => FLAG_HOST_IPV4,
        HostKind::Ipv6 => FLAG_HOST_IPV6,
    }
}

struct ItoaBuf {
    buf: [u8; 5],
    start: usize,
}

impl ItoaBuf {
    fn as_str(&self) -> &str {
        // Only ASCII digits.
        // SAFETY: buf is filled with ASCII digit bytes only.
        core::str::from_utf8(&self.buf[self.start..]).unwrap_or("0")
    }
}

fn itoa_buf(mut value: u16) -> ItoaBuf {
    let mut buf = [b'0'; 5];
    let mut i = 5;
    if value == 0 {
        return ItoaBuf { buf, start: 4 };
    }
    while value > 0 {
        i -= 1;
        buf[i] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    ItoaBuf { buf, start: i }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_https_absolute() {
        let url = parse("https://example.com/foo", None).unwrap();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.serialization.as_str(), "https://example.com/foo");
        assert_eq!(url.port, None);
        assert!(url.flags & FLAG_SPECIAL != 0);
        assert_eq!(
            &url.serialization.as_str()[url.host_start as usize..url.host_end as usize],
            "example.com"
        );
    }

    #[test]
    fn parse_default_port_null() {
        let url = parse("http://example.com:80/", None).unwrap();
        assert_eq!(url.port, None);
        assert_eq!(url.serialization.as_str(), "http://example.com/");
    }

    #[test]
    fn parse_explicit_port() {
        let url = parse("http://example.com:8080/", None).unwrap();
        assert_eq!(url.port, Some(8080));
        assert_eq!(url.serialization.as_str(), "http://example.com:8080/");
    }

    #[test]
    fn parse_no_scheme_without_base_fails() {
        assert!(parse("/path", None).is_err());
    }

    #[test]
    fn parse_file_simple() {
        let url = parse("file:///foo/bar", None).unwrap();
        assert_eq!(url.scheme(), "file");
        assert!(url.flags & FLAG_HAS_EMPTY_HOST != 0);
        assert_eq!(url.serialization.as_str(), "file:///foo/bar");
    }

    #[test]
    fn parse_opaque_path() {
        let url = parse("mailto:user@example.com", None).unwrap();
        assert!(url.flags & FLAG_OPAQUE_PATH != 0);
        assert_eq!(url.serialization.as_str(), "mailto:user@example.com");
    }

    #[test]
    fn parse_path_dot_segments() {
        let url = parse("https://example.com/a/./b/../c", None).unwrap();
        assert_eq!(url.serialization.as_str(), "https://example.com/a/c");
    }

    #[test]
    fn parse_ipv6() {
        let url = parse("http://[::1]/", None).unwrap();
        assert!(url.flags & FLAG_HOST_IPV6 != 0);
        assert_eq!(
            &url.serialization.as_str()[url.host_start as usize..url.host_end as usize],
            "[::1]"
        );
    }

    #[test]
    fn parse_userinfo() {
        let url = parse("http://user:pass@example.com/", None).unwrap();
        assert!(url.flags & FLAG_HAS_CREDENTIALS != 0);
        assert!(url.flags & FLAG_HAS_PASSWORD != 0);
        assert_eq!(url.serialization.as_str(), "http://user:pass@example.com/");
    }

    #[test]
    fn relative_against_base() {
        let base = parse("https://example.com/dir/page", None).unwrap();
        let url = parse("../x", Some(&base)).unwrap();
        assert_eq!(url.serialization.as_str(), "https://example.com/x");
    }

    #[test]
    fn parse_empty_host_fails() {
        assert!(parse("http://", None).is_err());
        assert!(parse("http:///", None).is_err());
        // Extra slashes are ignored; host becomes "path".
        let url = parse("http:///path", None).unwrap();
        assert_eq!(url.serialization.as_str(), "http://path/");
    }

    #[test]
    fn parse_query_and_fragment() {
        let url = parse("https://ex.com/p?q=1#frag", None).unwrap();
        assert_eq!(url.serialization.as_str(), "https://ex.com/p?q=1#frag");
        assert_eq!(url.query_start, Some("https://ex.com/p".len() as u32));
        assert!(url.fragment_start.is_some());
    }

    #[test]
    fn parse_non_special_with_authority() {
        let url = parse("foo://example.com:99/bar", None).unwrap();
        assert_eq!(url.serialization.as_str(), "foo://example.com:99/bar");
        assert_eq!(url.port, Some(99));
        assert_eq!(url.flags & FLAG_SPECIAL, 0);
        assert_eq!(url.flags & FLAG_OPAQUE_PATH, 0);
    }

    #[test]
    fn parse_special_backslash_as_slash() {
        let url = parse(r"http:\\example.com\foo", None).unwrap();
        assert_eq!(url.serialization.as_str(), "http://example.com/foo");
    }

    #[test]
    fn parse_file_localhost_empty_host() {
        let url = parse("file://localhost/tmp", None).unwrap();
        assert_eq!(url.serialization.as_str(), "file:///tmp");
        assert!(url.flags & FLAG_HAS_EMPTY_HOST != 0);
    }

    #[test]
    fn relative_dotdot_pops_nonfile_drive_segment() {
        let base = parse("abc://x/y/z/C:/", None).unwrap();
        let url = parse("..", Some(&base)).unwrap();
        assert_eq!(url.serialization.as_str(), "abc://x/y/z/");
    }
}

#[cfg(test)]
mod fuzz_regressions {
    use super::*;
    #[test]
    fn opaque_ipv6_leading_newlines() {
        let url = parse("fihttp://\n\n[2001:db8::1]\n\n", None).unwrap();
        assert_eq!(url.serialization.as_str(), "fihttp://[2001:db8::1]");
    }
}

#[cfg(test)]
mod triage_tick2 {
    use super::*;
    #[test]
    fn at_then_nonascii_host() {
        let url = parse("https://@\u{624}0", None).unwrap();
        assert_eq!(url.serialization.as_str(), "https://xn--0-smc/");
    }
    #[test]
    fn scheme_with_tab() {
        let url = parse("w\ts://example.com/", None).unwrap();
        assert_eq!(url.serialization.as_str(), "ws://example.com/");
    }
    #[test]
    fn file_dot_roundtrip() {
        let input = "file:/p:\n\n\n./%2e0/";
        let url = parse(input, None).unwrap();
        assert_eq!(url.serialization.as_str(), "file:///p:./%2e0/");
        let href = url.serialization.as_str().to_owned();
        let again = parse(&href, None).unwrap();
        assert_eq!(again.serialization.as_str(), href);
    }
    #[test]
    fn ws_arabic_path() {
        let url = parse("w\ts:/\u{688}0/\u{688}0.0.0.0~/../)C", None).unwrap();
        assert_eq!(url.serialization.as_str(), "ws://xn--0-isc/)C");
    }
    #[test]
    fn https_backslash_host() {
        let url = parse("https:///\\\\\\\u{7e3}2\\\\\\\\\\\u{0}\\]\\", None).unwrap();
        assert!(url.serialization.as_str().starts_with("https://xn--2-cdd"));
    }
    #[test]
    fn superscript_ipv4_rejected() {
        assert!(parse("http:255.255.255.2\u{2075}85", None).is_err());
    }
}
