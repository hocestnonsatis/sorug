//! Copy-on-write serialization buffer for the WHATWG URL parser.
//!
//! Starts as a borrow of the trimmed input. Every append that matches the next
//! bytes of the source advances the borrow cursor; the first mismatch (or an
//! in-place mutation that cannot be expressed as a prefix) upgrades to an
//! owned [`String`].

use core::fmt;
use core::ops::{Deref, Range, RangeFrom, RangeTo};

use alloc::string::String;

use super::percent::AppendBuf;
use crate::Backing;

/// Internal builder used by the state machine.
pub(crate) struct SerializationBuf<'a> {
    source: &'a str,
    state: State,
}

enum State {
    /// Serialization is exactly `source[..len]`.
    Borrowed {
        len: usize,
    },
    Owned(String),
}

impl<'a> SerializationBuf<'a> {
    #[inline]
    pub(crate) fn new(source: &'a str) -> Self {
        Self {
            source,
            state: State::Borrowed { len: 0 },
        }
    }

    /// Start from an already-owned serialization prefix (e.g. URL setters).
    ///
    /// The borrow source is unused (`""`); all further writes go into `s`.
    #[inline]
    pub(crate) fn from_owned(s: String) -> SerializationBuf<'static> {
        SerializationBuf {
            source: "",
            state: State::Owned(s),
        }
    }

    #[inline]
    pub(crate) fn as_str(&self) -> &str {
        match &self.state {
            State::Borrowed { len } => &self.source[..*len],
            State::Owned(s) => s.as_str(),
        }
    }

    #[inline]
    pub(crate) fn as_bytes(&self) -> &[u8] {
        self.as_str().as_bytes()
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        match &self.state {
            State::Borrowed { len } => *len,
            State::Owned(s) => s.len(),
        }
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub(crate) fn clear(&mut self) {
        match &mut self.state {
            State::Borrowed { len } => *len = 0,
            State::Owned(s) => s.clear(),
        }
    }

    /// Force an owned buffer, copying the borrowed prefix if needed.
    #[inline(never)]
    pub(crate) fn upgrade_to_owned(&mut self) {
        if let State::Borrowed { len } = self.state {
            let mut owned = String::with_capacity(self.source.len().max(len).saturating_add(8));
            owned.push_str(&self.source[..len]);
            self.state = State::Owned(owned);
        }
    }

    #[inline]
    pub(crate) fn push(&mut self, c: char) {
        match &mut self.state {
            State::Borrowed { len } => {
                let rest = &self.source[*len..];
                // Hot path: ASCII byte match without UTF-8 rescans.
                if c.is_ascii() {
                    let b = c as u8;
                    if rest.as_bytes().first() == Some(&b) {
                        *len += 1;
                        return;
                    }
                } else if rest.starts_with(c) {
                    *len += c.len_utf8();
                    return;
                }
                self.upgrade_to_owned();
                if let State::Owned(s) = &mut self.state {
                    s.push(c);
                }
            }
            State::Owned(s) => s.push(c),
        }
    }

    #[inline]
    pub(crate) fn push_str(&mut self, s: &str) {
        match &mut self.state {
            State::Borrowed { len } => {
                let rest = &self.source[*len..];
                // O(1) when `s` is the next contiguous slice of the source
                // (typical for host / path bulk copies from the input cursor).
                if !s.is_empty()
                    && s.len() <= rest.len()
                    && core::ptr::eq(s.as_ptr(), rest.as_ptr())
                {
                    *len += s.len();
                    return;
                }
                if rest.starts_with(s) {
                    *len += s.len();
                } else {
                    self.upgrade_to_owned();
                    if let State::Owned(owned) = &mut self.state {
                        owned.push_str(s);
                    }
                }
            }
            State::Owned(owned) => owned.push_str(s),
        }
    }

    #[inline]
    pub(crate) fn truncate(&mut self, new_len: usize) {
        debug_assert!(new_len <= self.len());
        match &mut self.state {
            State::Borrowed { len } => {
                debug_assert!(self.source.is_char_boundary(new_len));
                *len = new_len;
            }
            State::Owned(s) => s.truncate(new_len),
        }
    }

    #[inline]
    pub(crate) fn pop(&mut self) -> Option<char> {
        match &mut self.state {
            State::Borrowed { len } => {
                let s = &self.source[..*len];
                let c = s.chars().next_back()?;
                *len -= c.len_utf8();
                Some(c)
            }
            State::Owned(s) => s.pop(),
        }
    }

    #[inline]
    pub(crate) fn ends_with(&self, pat: char) -> bool {
        self.as_str().ends_with(pat)
    }

    /// In-place insertion always requires ownership.
    #[inline]
    pub(crate) fn insert_str(&mut self, idx: usize, s: &str) {
        self.upgrade_to_owned();
        if let State::Owned(owned) = &mut self.state {
            owned.insert_str(idx, s);
        }
    }

    #[inline]
    pub(crate) fn reserve(&mut self, additional: usize) {
        self.upgrade_to_owned();
        if let State::Owned(s) = &mut self.state {
            s.reserve(additional);
        }
    }

    #[inline]
    pub(crate) fn replace_range(&mut self, range: core::ops::Range<usize>, replace_with: &str) {
        self.upgrade_to_owned();
        if let State::Owned(s) = &mut self.state {
            s.replace_range(range, replace_with);
        }
    }

    #[inline]
    pub(crate) fn finish(self) -> Backing<'a> {
        match self.state {
            State::Borrowed { len } => Backing::Borrowed(&self.source[..len]),
            State::Owned(s) => Backing::Owned(s),
        }
    }

    /// Consume into an owned `String` (upgrades if still borrowed).
    #[inline]
    pub(crate) fn into_string(mut self) -> String {
        self.upgrade_to_owned();
        match self.state {
            State::Owned(s) => s,
            State::Borrowed { .. } => unreachable!("upgrade_to_owned left Borrowed"),
        }
    }
}

impl Deref for SerializationBuf<'_> {
    type Target = str;

    #[inline]
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Write for SerializationBuf<'_> {
    #[inline]
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.push_str(s);
        Ok(())
    }
}

// Allow `buf[a..b]` style indexing used throughout the parser.
impl core::ops::Index<Range<usize>> for SerializationBuf<'_> {
    type Output = str;
    #[inline]
    fn index(&self, range: Range<usize>) -> &str {
        &self.as_str()[range]
    }
}

impl core::ops::Index<RangeFrom<usize>> for SerializationBuf<'_> {
    type Output = str;
    #[inline]
    fn index(&self, range: RangeFrom<usize>) -> &str {
        &self.as_str()[range]
    }
}

impl core::ops::Index<RangeTo<usize>> for SerializationBuf<'_> {
    type Output = str;
    #[inline]
    fn index(&self, range: RangeTo<usize>) -> &str {
        &self.as_str()[range]
    }
}

impl core::ops::Index<core::ops::RangeFull> for SerializationBuf<'_> {
    type Output = str;
    #[inline]
    fn index(&self, _range: core::ops::RangeFull) -> &str {
        self.as_str()
    }
}

impl AppendBuf for SerializationBuf<'_> {
    #[inline]
    fn push(&mut self, c: char) {
        SerializationBuf::push(self, c);
    }

    #[inline]
    fn push_str(&mut self, s: &str) {
        SerializationBuf::push_str(self, s);
    }
}
