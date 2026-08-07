//! `std` file-path ↔ `file:` URL bridges (rust-url parity).

#![cfg(all(
    feature = "std",
    any(
        unix,
        windows,
        target_os = "redox",
        target_os = "wasi",
        target_os = "hermit"
    )
))]

use std::path::{Path, PathBuf};

use alloc::string::String;
use alloc::vec::Vec;

use crate::Url;
use crate::parser::percent::{
    in_special_path_segment_encode_set, percent_decode, percent_encode_bytes,
};

impl Url<'static> {
    /// Convert an absolute file path into a `file:` URL.
    ///
    /// Returns `Err` if the path is not absolute, or (on Windows) if the prefix
    /// is not a disk (`C:`) or UNC (`\\server\share`) prefix.
    #[allow(clippy::result_unit_err)]
    pub fn from_file_path<P: AsRef<Path>>(path: P) -> Result<Self, ()> {
        let mut serialization = String::from("file://");
        path_to_file_url_segments(path.as_ref(), &mut serialization)?;
        Url::parse(&serialization)
            .map(Url::into_owned)
            .map_err(|_| ())
    }

    /// Like [`Self::from_file_path`], but ensures a trailing `/` so the URL is a
    /// useful directory base for [`Url::join`].
    #[allow(clippy::result_unit_err)]
    pub fn from_directory_path<P: AsRef<Path>>(path: P) -> Result<Self, ()> {
        let mut url = Self::from_file_path(path)?;
        if !url.as_str().ends_with('/') {
            let mut s = url.as_str().to_owned();
            s.push('/');
            url = Url::parse(&s).map(Url::into_owned).map_err(|_| ())?;
        }
        Ok(url)
    }
}

impl Url<'_> {
    /// Convert a `file:` (or similar) URL path to an absolute [`PathBuf`].
    ///
    /// Does **not** check the scheme — callers should verify `scheme() == "file"`
    /// when that matters. Returns `Err` if the host is neither empty nor
    /// `"localhost"` (except Windows `file:` UNC hosts), or if decoding fails.
    #[allow(clippy::result_unit_err)]
    pub fn to_file_path(&self) -> Result<PathBuf, ()> {
        let Some(segments) = self.path_segments() else {
            return Err(());
        };
        let host = match self.host() {
            None | Some("") | Some("localhost") => None,
            Some(h) if cfg!(windows) && self.scheme() == "file" => Some(h),
            _ => return Err(()),
        };
        file_url_segments_to_pathbuf(host, segments)
    }
}

#[cfg(any(unix, target_os = "redox", target_os = "wasi", target_os = "hermit"))]
fn path_to_file_url_segments(path: &Path, serialization: &mut String) -> Result<(), ()> {
    #[cfg(target_os = "hermit")]
    use std::os::hermit::ffi::OsStrExt;
    #[cfg(any(unix, target_os = "redox"))]
    use std::os::unix::prelude::OsStrExt;

    if !path.is_absolute() {
        return Err(());
    }
    let mut empty = true;
    for component in path.components().skip(1) {
        empty = false;
        serialization.push('/');
        #[cfg(not(target_os = "wasi"))]
        percent_encode_bytes(
            component.as_os_str().as_bytes(),
            in_special_path_segment_encode_set,
            serialization,
        );
        #[cfg(target_os = "wasi")]
        percent_encode_bytes(
            component.as_os_str().to_string_lossy().as_bytes(),
            in_special_path_segment_encode_set,
            serialization,
        );
    }
    if empty {
        serialization.push('/');
    }
    Ok(())
}

#[cfg(windows)]
fn path_to_file_url_segments(path: &Path, serialization: &mut String) -> Result<(), ()> {
    use crate::parser::percent::{in_path_segment_encode_set, utf8_percent_encode};
    use std::path::{Component, Prefix};

    if !path.is_absolute() {
        return Err(());
    }
    let mut components = path.components();
    let host_start = serialization.len() + 1;

    match components.next() {
        Some(Component::Prefix(ref p)) => match p.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                serialization.push('/');
                serialization.push(letter as char);
                serialization.push(':');
            }
            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                let server = server.to_str().ok_or(())?;
                serialization.push_str(server);
                serialization.push('/');
                let share = share.to_str().ok_or(())?;
                utf8_percent_encode(share, in_path_segment_encode_set, serialization);
            }
            _ => return Err(()),
        },
        _ => return Err(()),
    }

    let mut path_only_has_prefix = true;
    for component in components {
        if component == Component::RootDir {
            continue;
        }
        path_only_has_prefix = false;
        let component = component.as_os_str().to_str().ok_or(())?;
        serialization.push('/');
        utf8_percent_encode(component, in_path_segment_encode_set, serialization);
    }

    if serialization.len() > host_start
        && is_windows_drive_letter(&serialization[host_start..])
        && path_only_has_prefix
    {
        serialization.push('/');
    }
    Ok(())
}

#[cfg(windows)]
fn is_windows_drive_letter(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

#[cfg(any(unix, target_os = "redox", target_os = "wasi", target_os = "hermit"))]
fn file_url_segments_to_pathbuf(
    host: Option<&str>,
    segments: core::str::Split<'_, char>,
) -> Result<PathBuf, ()> {
    #[cfg(not(target_os = "wasi"))]
    use std::ffi::OsStr;
    #[cfg(target_os = "hermit")]
    use std::os::hermit::ffi::OsStrExt;
    #[cfg(any(unix, target_os = "redox"))]
    use std::os::unix::prelude::OsStrExt;

    if host.is_some() {
        return Err(());
    }

    let mut bytes = Vec::new();
    if cfg!(target_os = "redox") {
        bytes.extend(b"file:");
    }

    for segment in segments {
        bytes.push(b'/');
        bytes.extend(percent_decode(segment.as_bytes()).as_ref());
    }

    if bytes.len() > 2
        && bytes[bytes.len() - 2].is_ascii_alphabetic()
        && matches!(bytes[bytes.len() - 1], b':' | b'|')
    {
        bytes.push(b'/');
    }

    #[cfg(not(target_os = "wasi"))]
    let path = PathBuf::from(OsStr::from_bytes(&bytes));
    #[cfg(target_os = "wasi")]
    let path = String::from_utf8(bytes)
        .map(PathBuf::from)
        .map_err(|_| ())?;

    debug_assert!(
        path.is_absolute(),
        "to_file_path() failed to produce an absolute Path"
    );
    Ok(path)
}

#[cfg(windows)]
fn file_url_segments_to_pathbuf(
    host: Option<&str>,
    mut segments: core::str::Split<'_, char>,
) -> Result<PathBuf, ()> {
    let mut string = String::new();
    if let Some(host) = host {
        string.push_str(r"\\");
        string.push_str(host);
    } else {
        let first = segments.next().ok_or(())?;
        match first.len() {
            2 => {
                if !first.as_bytes()[0].is_ascii_alphabetic() || first.as_bytes()[1] != b':' {
                    return Err(());
                }
                string.push_str(first);
            }
            4 => {
                let bytes = first.as_bytes();
                if !bytes[0].is_ascii_alphabetic()
                    || bytes[1] != b'%'
                    || bytes[2] != b'3'
                    || (bytes[3] != b'a' && bytes[3] != b'A')
                {
                    return Err(());
                }
                string.push(bytes[0] as char);
                string.push(':');
            }
            _ => return Err(()),
        }
    }

    for segment in segments {
        string.push('\\');
        let decoded = percent_decode(segment.as_bytes());
        let s = core::str::from_utf8(decoded.as_ref()).map_err(|_| ())?;
        if s.contains('\0') {
            return Err(());
        }
        string.push_str(s);
    }

    let path = PathBuf::from(string);
    if !path.is_absolute() {
        return Err(());
    }
    Ok(path)
}
