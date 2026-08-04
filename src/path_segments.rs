//! Mutable path-segment API ([`PathSegmentsMut`]), rust-url compatible.

use alloc::string::String;

use crate::Url;
use crate::parser::percent::{
    in_path_segment_encode_set, in_special_path_segment_encode_set, utf8_percent_encode,
};
use crate::parser::{SchemeType, to_u32};

/// Exposes methods to manipulate the path of a URL that is not cannot-be-a-base.
///
/// The path is slash-separated. After [`Self::clear`], `url.path() == "/"`.
///
/// # Examples
///
/// ```
/// use sorug::Url;
///
/// let mut url = Url::parse("mailto:me@example.com").unwrap();
/// assert!(url.path_segments_mut().is_err());
///
/// let mut url = Url::parse("http://example.net/foo/index.html").unwrap();
/// url.path_segments_mut()
///     .expect("cannot be base")
///     .pop()
///     .push("img")
///     .push("2/100%.png");
/// assert_eq!(url.as_str(), "http://example.net/foo/img/2%2F100%25.png");
/// ```
#[derive(Debug)]
pub struct PathSegmentsMut<'m, 'u> {
    url: &'m mut Url<'u>,
    after_first_slash: usize,
    after_path: String,
    old_after_path_position: u32,
}

pub(crate) fn new<'m, 'u>(url: &'m mut Url<'u>) -> PathSegmentsMut<'m, 'u> {
    url.serialization.ensure_owned();
    let after_path = url.take_after_path();
    let old_after_path_position = to_u32(url.serialization.len()).unwrap_or(u32::MAX);

    let path_start = url.path_start as usize;
    let after_first_slash = if url.as_str().as_bytes().get(path_start) == Some(&b'/') {
        path_start + 1
    } else {
        path_start
    };

    PathSegmentsMut {
        url,
        after_first_slash,
        after_path,
        old_after_path_position,
    }
}

impl Drop for PathSegmentsMut<'_, '_> {
    fn drop(&mut self) {
        self.url
            .restore_after_path(self.old_after_path_position, &self.after_path);
    }
}

impl PathSegmentsMut<'_, '_> {
    /// Remove all segments, leaving the minimal `url.path() == "/"`.
    pub fn clear(&mut self) -> &mut Self {
        let path_start = self.url.path_start as usize;
        let ser = self.url.serialization.as_mut_string();
        if ser.as_bytes().get(path_start) == Some(&b'/') {
            ser.truncate(path_start + 1);
            self.after_first_slash = path_start + 1;
        } else {
            // Empty path → become `"/"`.
            ser.truncate(path_start);
            ser.push('/');
            self.after_first_slash = path_start + 1;
        }
        self
    }

    /// Remove a trailing empty segment (trailing slash), unless the path is `"/"`.
    pub fn pop_if_empty(&mut self) -> &mut Self {
        let ser = self.url.serialization.as_mut_string();
        if self.after_first_slash >= ser.len() {
            return self;
        }
        if ser[self.after_first_slash..].ends_with('/') {
            ser.pop();
        }
        self
    }

    /// Remove the last path segment (leaving `"/"` if it was the only one).
    pub fn pop(&mut self) -> &mut Self {
        let ser = self.url.serialization.as_mut_string();
        if self.after_first_slash >= ser.len() {
            return self;
        }
        let last_slash = ser[self.after_first_slash..].rfind('/').unwrap_or(0);
        ser.truncate(self.after_first_slash + last_slash);
        self
    }

    /// Append one segment (see [`Self::extend`]).
    pub fn push(&mut self, segment: &str) -> &mut Self {
        self.extend(Some(segment))
    }

    /// Append each segment from `segments`.
    ///
    /// Segments are percent-encoded with `/` and `%` also encoded (`%2F` / `%25`).
    /// `"."` and `".."` segments are ignored.
    pub fn extend<I>(&mut self, segments: I) -> &mut Self
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let path_start = self.url.path_start as usize;
        let special = SchemeType::from(self.url.scheme()).is_special();
        for segment in segments {
            let segment = segment.as_ref();
            if matches!(segment, "." | "..") {
                continue;
            }
            let ser = self.url.serialization.as_mut_string();
            // Add a slash before the new segment except when path is exactly `"/"`.
            if ser.len() > path_start + 1 || ser.len() == path_start {
                ser.push('/');
                if self.after_first_slash == path_start {
                    self.after_first_slash = path_start + 1;
                }
            }
            if special {
                utf8_percent_encode(segment, in_special_path_segment_encode_set, ser);
            } else {
                utf8_percent_encode(segment, in_path_segment_encode_set, ser);
            }
        }
        self
    }
}
