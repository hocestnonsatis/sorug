//! C ABI for [`sorug`](https://crates.io/crates/sorug).
//!
//! The main `sorug` crate forbids `unsafe`. This crate is the intentional
//! boundary for C / other-language consumers (`cdylib` + `staticlib`).
//!
//! # Ownership
//!
//! - [`sorug_parse`] / [`sorug_parse_with_base`] return an owned handle.
//! - Call [`sorug_free`] exactly once when finished.
//! - Getter output pointers borrow the handle's serialization and are valid
//!   until the handle is freed (or mutated — this API is read-only).

use std::os::raw::c_char;
use std::ptr;
use std::slice;
use std::str;

use sorug::Url;

/// Opaque URL handle. Free with [`sorug_free`].
pub struct SorugUrl {
    inner: Url<'static>,
}

fn component_out(s: &str, out_ptr: *mut *const c_char, out_len: *mut usize) {
    // SAFETY: caller guarantees out_ptr/out_len are writable (or null).
    unsafe {
        if !out_ptr.is_null() {
            *out_ptr = s.as_ptr().cast::<c_char>();
        }
        if !out_len.is_null() {
            *out_len = s.len();
        }
    }
}

fn optional_component(
    value: Option<&str>,
    out_ptr: *mut *const c_char,
    out_len: *mut usize,
) -> i32 {
    match value {
        Some(s) => {
            component_out(s, out_ptr, out_len);
            1
        }
        None => {
            unsafe {
                if !out_ptr.is_null() {
                    *out_ptr = ptr::null();
                }
                if !out_len.is_null() {
                    *out_len = 0;
                }
            }
            0
        }
    }
}

/// Parse a UTF-8 URL string.
///
/// # Safety
///
/// - `input` must be non-null and point to at least `len` readable bytes.
/// - Those bytes must be valid UTF-8.
///
/// Returns a heap-allocated handle, or null on parse failure / invalid UTF-8
/// arguments (null pointer).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sorug_parse(input: *const c_char, len: usize) -> *mut SorugUrl {
    if input.is_null() {
        return ptr::null_mut();
    }
    let bytes = unsafe { slice::from_raw_parts(input.cast::<u8>(), len) };
    let Ok(text) = str::from_utf8(bytes) else {
        return ptr::null_mut();
    };
    match Url::parse(text) {
        Ok(url) => Box::into_raw(Box::new(SorugUrl {
            inner: url.into_owned(),
        })),
        Err(_) => ptr::null_mut(),
    }
}

/// Parse `input` against an optional absolute `base` URL.
///
/// `base` may be null (absolute-URL-only parse). On success returns a new
/// handle; `base` is left intact.
///
/// # Safety
///
/// Same UTF-8 requirements as [`sorug_parse`]. `base`, when non-null, must be
/// a valid handle from this library.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sorug_parse_with_base(
    input: *const c_char,
    len: usize,
    base: *const SorugUrl,
) -> *mut SorugUrl {
    if input.is_null() {
        return ptr::null_mut();
    }
    let bytes = unsafe { slice::from_raw_parts(input.cast::<u8>(), len) };
    let Ok(text) = str::from_utf8(bytes) else {
        return ptr::null_mut();
    };
    let base_ref = if base.is_null() {
        None
    } else {
        Some(&unsafe { &*base }.inner)
    };
    match Url::parse_with_base(text, base_ref) {
        Ok(url) => Box::into_raw(Box::new(SorugUrl {
            inner: url.into_owned(),
        })),
        Err(_) => ptr::null_mut(),
    }
}

/// Free a handle returned by [`sorug_parse`] / [`sorug_parse_with_base`].
///
/// Passing null is a no-op. Do not use the pointer afterward.
///
/// # Safety
///
/// `url` must be null or a unique handle previously returned by this library
/// and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sorug_free(url: *mut SorugUrl) {
    if !url.is_null() {
        drop(unsafe { Box::from_raw(url) });
    }
}

macro_rules! str_getter {
    ($name:ident, $method:ident) => {
        /// Write the component into `out_ptr` / `out_len` (not NUL-terminated).
        ///
        /// Returns `0` on success, `-1` if `url` is null.
        ///
        /// # Safety
        ///
        /// `url` must be null or a valid handle. `out_ptr` / `out_len` may be
        /// null; when non-null they must be writable.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(
            url: *const SorugUrl,
            out_ptr: *mut *const c_char,
            out_len: *mut usize,
        ) -> i32 {
            if url.is_null() {
                return -1;
            }
            let s = unsafe { &*url }.inner.$method();
            component_out(s, out_ptr, out_len);
            0
        }
    };
}

str_getter!(sorug_href, href);
str_getter!(sorug_scheme, scheme);
str_getter!(sorug_username, username);
str_getter!(sorug_password, password);
str_getter!(sorug_pathname, pathname);
str_getter!(sorug_search, search);
str_getter!(sorug_hash, hash);

/// Host without port. Returns `1` if present, `0` if absent, `-1` if `url` is null.
///
/// # Safety
///
/// Same as other getters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sorug_host(
    url: *const SorugUrl,
    out_ptr: *mut *const c_char,
    out_len: *mut usize,
) -> i32 {
    if url.is_null() {
        return -1;
    }
    optional_component(unsafe { &*url }.inner.host(), out_ptr, out_len)
}

/// Query without leading `?`. Returns `1` if present, `0` if absent, `-1` if null.
///
/// # Safety
///
/// Same as other getters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sorug_query(
    url: *const SorugUrl,
    out_ptr: *mut *const c_char,
    out_len: *mut usize,
) -> i32 {
    if url.is_null() {
        return -1;
    }
    optional_component(unsafe { &*url }.inner.query(), out_ptr, out_len)
}

/// Fragment without leading `#`. Returns `1` if present, `0` if absent, `-1` if null.
///
/// # Safety
///
/// Same as other getters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sorug_fragment(
    url: *const SorugUrl,
    out_ptr: *mut *const c_char,
    out_len: *mut usize,
) -> i32 {
    if url.is_null() {
        return -1;
    }
    optional_component(unsafe { &*url }.inner.fragment(), out_ptr, out_len)
}

/// Port as `u16`. Writes through `out_port` when present.
///
/// Returns `1` if a port is set, `0` if absent, `-1` if `url` is null.
///
/// # Safety
///
/// `out_port` may be null; when non-null it must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sorug_port(url: *const SorugUrl, out_port: *mut u16) -> i32 {
    if url.is_null() {
        return -1;
    }
    match unsafe { &*url }.inner.port_u16() {
        Some(p) => {
            if !out_port.is_null() {
                unsafe { *out_port = p };
            }
            1
        }
        None => 0,
    }
}

/// Returns `1` if the URL cannot be a base URL, `0` otherwise, `-1` if null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sorug_cannot_be_a_base(url: *const SorugUrl) -> i32 {
    if url.is_null() {
        return -1;
    }
    i32::from(unsafe { &*url }.inner.cannot_be_a_base())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    unsafe fn parse_str(s: &str) -> *mut SorugUrl {
        unsafe { sorug_parse(s.as_ptr().cast(), s.len()) }
    }

    unsafe fn read_str(
        f: unsafe extern "C" fn(*const SorugUrl, *mut *const c_char, *mut usize) -> i32,
        url: *const SorugUrl,
    ) -> String {
        let mut ptr: *const c_char = ptr::null();
        let mut len = 0usize;
        assert_eq!(unsafe { f(url, &raw mut ptr, &raw mut len) }, 0);
        let bytes = unsafe { slice::from_raw_parts(ptr.cast::<u8>(), len) };
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[test]
    fn parse_and_components() {
        unsafe {
            let url = parse_str("https://user:pass@example.com:8443/a/b?q=1#frag");
            assert!(!url.is_null());
            assert_eq!(read_str(sorug_scheme, url), "https");
            assert_eq!(read_str(sorug_username, url), "user");
            assert_eq!(read_str(sorug_password, url), "pass");
            assert_eq!(read_str(sorug_pathname, url), "/a/b");
            assert_eq!(read_str(sorug_search, url), "?q=1");
            assert_eq!(read_str(sorug_hash, url), "#frag");

            let mut host_ptr = ptr::null();
            let mut host_len = 0usize;
            assert_eq!(sorug_host(url, &raw mut host_ptr, &raw mut host_len), 1);
            let host = slice::from_raw_parts(host_ptr.cast::<u8>(), host_len);
            assert_eq!(host, b"example.com");

            let mut port = 0u16;
            assert_eq!(sorug_port(url, &raw mut port), 1);
            assert_eq!(port, 8443);

            assert_eq!(sorug_cannot_be_a_base(url), 0);
            sorug_free(url);
        }
    }

    #[test]
    fn parse_failure_and_null() {
        unsafe {
            assert!(sorug_parse(ptr::null(), 0).is_null());
            let bad = CString::new("not a url").unwrap();
            assert!(sorug_parse(bad.as_ptr(), bad.as_bytes().len()).is_null());
            assert_eq!(
                sorug_href(ptr::null(), ptr::null_mut(), ptr::null_mut()),
                -1
            );
            sorug_free(ptr::null_mut());
        }
    }

    #[test]
    fn parse_with_base_relative() {
        unsafe {
            let base = parse_str("https://example.com/dir/page");
            let rel = b"../other";
            let joined = sorug_parse_with_base(rel.as_ptr().cast(), rel.len(), base);
            assert!(!joined.is_null());
            assert_eq!(read_str(sorug_href, joined), "https://example.com/other");
            sorug_free(joined);
            sorug_free(base);
        }
    }

    #[test]
    fn opaque_cannot_be_base() {
        unsafe {
            let url = parse_str("mailto:a@b.com");
            assert_eq!(sorug_cannot_be_a_base(url), 1);
            assert_eq!(sorug_host(url, ptr::null_mut(), ptr::null_mut()), 0);
            sorug_free(url);
        }
    }
}
