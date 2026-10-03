//! Mutable path-segment API ([`PathSegmentsMut`]), rust-url compatible.

use alloc::string::String;

use crate::Url;
use crate::parser::percent::{
    in_path_segment_encode_set, in_special_path_segment_encode_set, utf8_percent_encode,
};
use crate::parser::{SchemeType, is_windows_drive_letter, to_u32};

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
        let scheme_end = self.url.scheme_end as usize;
        let ser = self.url.serialization.as_mut_string();
        // Non-special anarchist URLs store a `/.` marker before a `//…` path.
        // Clearing must drop that marker so the href round-trips (`gi:/` not
        // `gi:/./`, which re-parses as `gi:/`).
        let anarchist = path_start == scheme_end + 3
            && ser.as_bytes().get(scheme_end + 1) == Some(&b'/')
            && ser.as_bytes().get(scheme_end + 2) == Some(&b'.');
        if anarchist {
            let new_path_start = scheme_end + 1;
            ser.truncate(new_path_start);
            ser.push('/');
            self.url.path_start = to_u32(new_path_start).unwrap_or(self.url.path_start);
            self.after_first_slash = new_path_start + 1;
            return self;
        }
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
        self.drop_stale_anarchist_marker();
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
        self.drop_stale_anarchist_marker();
        self
    }

    /// Non-special anarchist URLs (`foo:/.//b`) carry a `/.` marker only while
    /// the path starts with `//`. After shrinking the path, remove the marker
    /// if it is no longer needed so the href round-trips (`foo:/` not `foo:/./`).
    fn drop_stale_anarchist_marker(&mut self) {
        let path_start = self.url.path_start as usize;
        let scheme_end = self.url.scheme_end as usize;
        let ser = self.url.serialization.as_mut_string();
        let b = ser.as_bytes();
        let anarchist = path_start == scheme_end + 3
            && b.get(scheme_end + 1) == Some(&b'/')
            && b.get(scheme_end + 2) == Some(&b'.');
        if !anarchist || ser[path_start..].starts_with("//") {
            return;
        }
        ser.replace_range(scheme_end + 1..scheme_end + 3, "");
        self.url.path_start = to_u32(scheme_end + 1).unwrap_or(self.url.path_start);
        self.after_first_slash -= 2;
    }

    /// Append one segment (see [`Self::extend`]).
    pub fn push(&mut self, segment: &str) -> &mut Self {
        self.extend(Some(segment))
    }

    /// Append each segment from `segments`.
    ///
    /// Segments are percent-encoded with `/` and `%` also encoded (`%2F` / `%25`).
    /// `"."` and `".."` segments are ignored.
    ///
    /// On `file:` URLs, a Windows drive letter pushed as the first path segment
    /// normalizes `|` → `:` (same as the basic URL parser), so the href
    /// round-trips.
    pub fn extend<I>(&mut self, segments: I) -> &mut Self
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let path_start = self.url.path_start as usize;
        let scheme_type = SchemeType::from(self.url.scheme());
        let special = scheme_type.is_special();
        let is_file = scheme_type.is_file();
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
            // Match parser `SegAction::WinDrive`: first file path segment only.
            let segment_start = ser.len();
            let only_slashes_before = ser[path_start..segment_start].bytes().all(|b| b == b'/');
            if is_file && only_slashes_before && is_windows_drive_letter(segment) {
                // `is_windows_drive_letter` guarantees length 2 + ASCII letter.
                let letter = segment.as_bytes()[0] as char;
                ser.push(letter);
                ser.push(':');
                continue;
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
