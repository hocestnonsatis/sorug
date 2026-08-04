//! WHATWG / rust-url URL component setters.
//!
//! Semantics follow `url` 2.5 quirks + `Url::set_*` (authority surgery, port
//! splice, path/query/fragment replace).

#![allow(clippy::option_option, clippy::result_unit_err)]

use core::fmt::Write as _;

use alloc::borrow::{Cow, ToOwned};
use alloc::string::String;

use super::host::Host;
use super::percent::{in_userinfo_encode_set, utf8_percent_encode};
use super::serialization::SerializationBuf;
use super::{
    Input, Parser, SchemeType, default_port_for_scheme, parse_host_setter, parse_port_setter,
    to_u32,
};
use crate::{ParseError, Url, UrlFlags};

impl Url<'_> {
    /// Whether this URL has an opaque path (cannot-be-a-base).
    #[inline]
    #[must_use]
    pub fn cannot_be_a_base(&self) -> bool {
        self.flags.contains(UrlFlags::OPAQUE_PATH)
    }

    /// Re-parse `value` as an absolute URL and replace `self` on success.
    pub fn set_href(&mut self, value: &str) -> Result<(), ParseError> {
        let parsed = Url::parse(value)?.into_owned();
        self.serialization = parsed.serialization;
        self.scheme_end = parsed.scheme_end;
        self.username_end = parsed.username_end;
        self.host_start = parsed.host_start;
        self.host_end = parsed.host_end;
        self.port = parsed.port;
        self.path_start = parsed.path_start;
        self.query_start = parsed.query_start;
        self.fragment_start = parsed.fragment_start;
        self.flags = parsed.flags;
        Ok(())
    }

    /// Quirks `protocol` setter: strip after first `:`, then [`Self::set_scheme`].
    #[allow(clippy::result_unit_err)]
    pub fn set_protocol(&mut self, mut value: &str) -> Result<(), ()> {
        if let Some(position) = value.find(':') {
            value = &value[..position];
        }
        self.set_scheme(value)
    }

    /// Change the scheme (WHATWG scheme-state override rules).
    #[allow(clippy::result_unit_err, clippy::suspicious_operation_groupings)]
    pub fn set_scheme(&mut self, scheme: &str) -> Result<(), ()> {
        let mut parser = Parser::for_setter(SerializationBuf::new(""));
        let remaining = parser.parse_scheme_setter(Input::new_no_trim(scheme))?;
        let new_scheme = parser.serialization.as_str().to_owned();
        let new_scheme_type = SchemeType::from(new_scheme.as_str());
        let old_scheme_type = SchemeType::from(self.scheme());

        if (new_scheme_type.is_special() && !old_scheme_type.is_special())
            || (!new_scheme_type.is_special() && old_scheme_type.is_special())
        {
            return Err(());
        }

        // WHATWG: credentials or non-null port → cannot switch to file.
        // rust-url approximates with `has_authority()` (rejects all →file with `://`);
        // WPT only covers credential/port cases — follow the checklist.
        if new_scheme_type.is_file() && (self.includes_credentials() || self.port_u16().is_some()) {
            return Err(());
        }

        // WHATWG: file + empty/null host cannot switch scheme.
        if old_scheme_type.is_file() && self.host_is_null_or_empty() {
            return Err(());
        }

        if !remaining.is_empty() || (self.host_is_null_or_empty() && new_scheme_type.is_special()) {
            return Err(());
        }

        // Opaque path cannot switch to a special scheme (also covered by host check).
        if self.cannot_be_a_base() && new_scheme_type.is_special() {
            return Err(());
        }

        let old_scheme_end = self.scheme_end;
        let new_scheme_end = to_u32(new_scheme.len()).map_err(|_| ())?;
        let adjust = |index: &mut u32| {
            *index = index
                .wrapping_sub(old_scheme_end)
                .wrapping_add(new_scheme_end);
        };

        self.scheme_end = new_scheme_end;
        adjust(&mut self.username_end);
        adjust(&mut self.host_start);
        adjust(&mut self.host_end);
        adjust(&mut self.path_start);
        if self.query_start != Self::NONE {
            adjust(&mut self.query_start);
        }
        if self.fragment_start != Self::NONE {
            adjust(&mut self.fragment_start);
        }

        let suffix = self.as_str()[old_scheme_end as usize..].to_owned();
        let ser = self.serialization.as_mut_string();
        ser.clear();
        ser.push_str(&new_scheme);
        ser.push_str(&suffix);

        if new_scheme_type.is_special() {
            self.flags.insert(UrlFlags::SPECIAL);
        } else {
            self.flags.remove(UrlFlags::SPECIAL);
        }

        // Drop the port if it is the default for the new scheme.
        let previous_port = self.port_u16();
        let _ = self.set_port(previous_port);

        Ok(())
    }

    /// Change the username (percent-encoded). Fails without a non-empty non-file host.
    #[allow(clippy::result_unit_err)]
    pub fn set_username(&mut self, username: &str) -> Result<(), ()> {
        if self.cannot_have_username_password_port() {
            return Err(());
        }
        let username_start = self.scheme_end + 3;
        debug_assert!(self.slice(self.scheme_end..username_start) == "://");
        if self.slice(username_start..self.username_end) == username {
            return Ok(());
        }

        let after_username = self.as_str()[self.username_end as usize..].to_owned();
        {
            let ser = self.serialization.as_mut_string();
            ser.truncate(username_start as usize);
            utf8_percent_encode(username, in_userinfo_encode_set, ser);
        }

        let mut removed_bytes = self.username_end;
        self.username_end = to_u32(self.serialization.len()).map_err(|_| ())?;
        let mut added_bytes = self.username_end;

        let new_username_is_empty = self.username_end == username_start;
        let ser = self.serialization.as_mut_string();
        match (new_username_is_empty, after_username.chars().next()) {
            (true, Some('@')) => {
                removed_bytes += 1;
                ser.push_str(&after_username[1..]);
            }
            (false, Some('@')) | (_, Some(':')) | (true, _) => {
                ser.push_str(&after_username);
            }
            (false, _) => {
                added_bytes += 1;
                ser.push('@');
                ser.push_str(&after_username);
            }
        }

        let adjust = |index: &mut u32| {
            *index = index.wrapping_sub(removed_bytes).wrapping_add(added_bytes);
        };
        adjust(&mut self.host_start);
        adjust(&mut self.host_end);
        adjust(&mut self.path_start);
        if self.query_start != Self::NONE {
            adjust(&mut self.query_start);
        }
        if self.fragment_start != Self::NONE {
            adjust(&mut self.fragment_start);
        }

        self.sync_credential_flags();
        Ok(())
    }

    /// Change the password. Quirks: empty string clears. Fails without a usable host.
    #[allow(clippy::result_unit_err)]
    pub fn set_password(&mut self, password: &str) -> Result<(), ()> {
        if self.cannot_have_username_password_port() {
            return Err(());
        }
        if !password.is_empty() {
            let host_and_after = self.as_str()[self.host_start as usize..].to_owned();
            {
                let ser = self.serialization.as_mut_string();
                ser.truncate(self.username_end as usize);
                ser.push(':');
                utf8_percent_encode(password, in_userinfo_encode_set, ser);
                ser.push('@');
            }

            let old_host_start = self.host_start;
            let new_host_start = to_u32(self.serialization.len()).map_err(|_| ())?;
            let adjust = |index: &mut u32| {
                *index = index
                    .wrapping_sub(old_host_start)
                    .wrapping_add(new_host_start);
            };
            self.host_start = new_host_start;
            adjust(&mut self.host_end);
            adjust(&mut self.path_start);
            if self.query_start != Self::NONE {
                adjust(&mut self.query_start);
            }
            if self.fragment_start != Self::NONE {
                adjust(&mut self.fragment_start);
            }

            self.serialization.as_mut_string().push_str(&host_and_after);
        } else if (self.username_end as usize) < self.serialization.len()
            && self.byte_at(self.username_end) == b':'
        {
            let has_username_or_password = self.byte_at(self.host_start - 1) == b'@';
            debug_assert!(has_username_or_password);
            let username_start = self.scheme_end + 3;
            let empty_username = username_start == self.username_end;
            let start = self.username_end;
            let end = if empty_username {
                self.host_start
            } else {
                self.host_start - 1
            };
            self.serialization
                .as_mut_string()
                .drain(start as usize..end as usize);
            let offset = end - start;
            self.host_start -= offset;
            self.host_end -= offset;
            self.path_start -= offset;
            if self.query_start != Self::NONE {
                self.query_start -= offset;
            }
            if self.fragment_start != Self::NONE {
                self.fragment_start -= offset;
            }
        }
        self.sync_credential_flags();
        Ok(())
    }

    /// Change the host to an IP address, skipping the host parser.
    ///
    /// If this URL cannot-be-a-base, do nothing and return `Err`.
    #[cfg(feature = "std")]
    #[cfg_attr(docsrs, doc(cfg(feature = "std")))]
    #[allow(clippy::result_unit_err)]
    pub fn set_ip_host(&mut self, address: core::net::IpAddr) -> Result<(), ()> {
        if self.cannot_be_a_base() {
            return Err(());
        }
        let host = match address {
            core::net::IpAddr::V4(a) => Host::Ipv4(a),
            core::net::IpAddr::V6(a) => Host::Ipv6(a),
        };
        self.set_host_internal(host, None);
        Ok(())
    }

    /// Quirks `host` setter (host + optional port).
    #[allow(clippy::result_unit_err)]
    pub fn set_host(&mut self, host: &str) -> Result<(), ()> {
        if self.cannot_be_a_base() {
            return Err(());
        }
        let scheme = self.scheme().to_owned();
        let scheme_type = SchemeType::from(scheme.as_str());
        if scheme_type == SchemeType::File && host.is_empty() {
            self.set_host_internal(Host::Domain(Cow::Owned(String::new())), Some(None));
            return Ok(());
        }

        let input = Input::new_no_trim(host);
        let (parsed_host, is_empty, remaining) =
            parse_host_setter(input, scheme_type).map_err(|_| ())?;

        let opt_port = if let Some(remaining) = remaining.split_prefix_char(':') {
            if remaining.is_empty() {
                None
            } else {
                parse_port_setter(remaining, default_port_for_scheme(&scheme))
                    .ok()
                    .map(|(port, _)| port)
            }
        } else {
            None
        };

        if is_empty
            && (!self.username().is_empty()
                || matches!(opt_port, Some(Some(_)))
                || self.port_u16().is_some())
        {
            return Err(());
        }

        self.set_host_internal(parsed_host, opt_port);
        Ok(())
    }

    /// Quirks `hostname` setter (host only; rejects trailing `:`).
    #[allow(clippy::result_unit_err)]
    pub fn set_hostname(&mut self, hostname: &str) -> Result<(), ()> {
        if self.cannot_be_a_base() {
            return Err(());
        }
        let scheme_type = SchemeType::from(self.scheme());
        if scheme_type == SchemeType::File && hostname.is_empty() {
            self.set_host_internal(Host::Domain(Cow::Owned(String::new())), Some(None));
            return Ok(());
        }

        let input = Input::new_no_trim(hostname);
        let (host, is_empty, remaining) = parse_host_setter(input, scheme_type).map_err(|_| ())?;
        if remaining.starts_with_char(':') {
            return Err(());
        }
        if is_empty {
            if scheme_type == SchemeType::SpecialNotFile
                || self.port_u16().is_some()
                || !self.username().is_empty()
                || !self.password().is_empty()
            {
                return Err(());
            }
        }
        self.set_host_internal(host, None);
        Ok(())
    }

    /// Quirks string port setter (WPT `port` attribute).
    ///
    /// For typed ports, prefer [`Self::set_port`].
    #[allow(clippy::result_unit_err)]
    pub fn set_port_str(&mut self, port: &str) -> Result<(), ()> {
        if self.cannot_have_username_password_port() {
            return Err(());
        }
        // Empty string clears the port. Tab/LF/CR-only input is not empty and
        // yields no digits → failure / no-op (WPT / browsers; rust-url 2.5 clears).
        if port.is_empty() {
            self.set_port_internal(None);
            return Ok(());
        }
        let scheme = self.scheme().to_owned();
        let (new_port, _) =
            parse_port_setter(Input::new_no_trim(port), default_port_for_scheme(&scheme))
                .map_err(|_| ())?;
        self.set_port_internal(new_port);
        Ok(())
    }

    /// Change the port number (`None` clears). Same preconditions as quirks port.
    ///
    /// Default ports for the scheme are normalized to “null” (omitted in the href).
    #[allow(clippy::result_unit_err)]
    pub fn set_port(&mut self, mut port: Option<u16>) -> Result<(), ()> {
        if self.cannot_have_username_password_port() {
            return Err(());
        }
        if port.is_some() && port == default_port_for_scheme(self.scheme()) {
            port = None;
        }
        self.set_port_internal(port);
        Ok(())
    }

    /// Quirks `pathname` setter (no-op if opaque path).
    pub fn set_pathname(&mut self, new_pathname: &str) {
        if self.cannot_be_a_base() {
            return;
        }
        let special = SchemeType::from(self.scheme()).is_special();
        if new_pathname.starts_with('/') || (special && new_pathname.starts_with('\\')) {
            self.set_path(new_pathname);
        } else if special || !new_pathname.is_empty() || !self.has_authority() {
            // Non-special + authority + empty → truly empty path (`foo://host`).
            // Without authority (`unix:/…`), empty still becomes `/`.
            let mut path_to_set = String::from("/");
            path_to_set.push_str(new_pathname);
            self.set_path(&path_to_set);
        } else {
            self.set_path(new_pathname);
        }
    }

    /// Replace the path (or opaque path) serialization.
    pub fn set_path(&mut self, mut path: &str) {
        let after_path = self.take_after_path();
        let old_after_path_pos = to_u32(self.serialization.len()).unwrap_or(u32::MAX);
        let cannot_be_a_base = self.cannot_be_a_base();
        let scheme_type = SchemeType::from(self.scheme());
        let scheme_end = self.scheme_end;

        // Drop a stale anarchist `/.` marker so path replace starts at `:`.
        let mut truncate_at = self.path_start;
        if !cannot_be_a_base
            && !self.has_authority()
            && self.path_start == scheme_end + 3
            && self.as_str().as_bytes().get(scheme_end as usize + 1) == Some(&b'/')
            && self.as_str().as_bytes().get(scheme_end as usize + 2) == Some(&b'.')
        {
            truncate_at = scheme_end + 1;
            self.path_start = truncate_at;
        }

        let prefix = {
            let ser = self.serialization.as_mut_string();
            ser.truncate(truncate_at as usize);
            core::mem::take(ser)
        };

        let mut parser = Parser::for_setter(SerializationBuf::from_owned(prefix));
        if cannot_be_a_base {
            if path.starts_with('/') {
                parser.serialization.push_str("%2F");
                path = &path[1..];
            }
            let _ = parser.parse_opaque_path(Input::new_no_trim(path));
        } else {
            let mut has_host = true;
            let _ = parser.parse_path_start(scheme_type, &mut has_host, Input::new_no_trim(path));
        }

        let mut new_ser = parser.serialization.into_string();

        // Anarchist URL fix: path `//…` immediately after `:` needs `/.` inserted.
        let mut path_start = self.path_start;
        if !cannot_be_a_base
            && !scheme_type.is_special()
            && path_start == scheme_end + 1
            && new_ser.len() > path_start as usize
            && new_ser.as_bytes().get(path_start as usize) == Some(&b'/')
            && new_ser.as_bytes().get(path_start as usize + 1) == Some(&b'/')
        {
            new_ser.insert_str(path_start as usize, "/.");
            path_start += 2;
            self.path_start = path_start;
        }

        let new_after_path_pos = to_u32(new_ser.len()).unwrap_or(u32::MAX);
        new_ser.push_str(&after_path);
        *self.serialization.as_mut_string() = new_ser;

        let adjust = |index: &mut u32| {
            if *index != Self::NONE {
                *index = index
                    .wrapping_sub(old_after_path_pos)
                    .wrapping_add(new_after_path_pos);
            }
        };
        adjust(&mut self.query_start);
        adjust(&mut self.fragment_start);
    }

    /// Quirks `search` setter.
    pub fn set_search(&mut self, new_search: &str) {
        self.set_query(match new_search {
            "" => None,
            _ if new_search.starts_with('?') => Some(&new_search[1..]),
            _ => Some(new_search),
        });
    }

    /// Replace or clear the query (fragment preserved).
    pub fn set_query(&mut self, query: Option<&str>) {
        let fragment = self.take_fragment();

        if self.query_start != Self::NONE {
            debug_assert!(self.byte_at(self.query_start) == b'?');
            self.serialization
                .as_mut_string()
                .truncate(self.query_start as usize);
            self.query_start = Self::NONE;
        }

        if let Some(input) = query {
            let scheme_type = SchemeType::from(self.scheme());
            self.query_start = to_u32(self.serialization.len()).unwrap_or(Self::NONE);
            let mut prefix = core::mem::take(self.serialization.as_mut_string());
            prefix.push('?');
            let mut parser = Parser::for_setter(SerializationBuf::from_owned(prefix));
            let _ = parser.parse_query(scheme_type, Input::new_trim_tab_and_newlines(input));
            *self.serialization.as_mut_string() = parser.serialization.into_string();
        } else if fragment.is_none() {
            self.strip_trailing_spaces_from_opaque_path();
        }

        self.restore_fragment(fragment);
    }

    /// Quirks `hash` setter.
    pub fn set_hash(&mut self, new_hash: &str) {
        self.set_fragment(match new_hash {
            "" => None,
            _ if new_hash.starts_with('#') => Some(&new_hash[1..]),
            _ => Some(new_hash),
        });
    }

    /// Replace or clear the fragment.
    pub fn set_fragment(&mut self, fragment: Option<&str>) {
        if self.fragment_start != Self::NONE {
            debug_assert!(self.byte_at(self.fragment_start) == b'#');
            self.serialization
                .as_mut_string()
                .truncate(self.fragment_start as usize);
            self.fragment_start = Self::NONE;
        }
        if let Some(input) = fragment {
            self.fragment_start = to_u32(self.serialization.len()).unwrap_or(Self::NONE);
            let ser = self.serialization.as_mut_string();
            ser.push('#');
            let prefix = core::mem::take(ser);
            let mut parser = Parser::for_setter(SerializationBuf::from_owned(prefix));
            parser.parse_fragment(Input::new_no_trim(input));
            *ser = parser.serialization.into_string();
        } else {
            self.strip_trailing_spaces_from_opaque_path();
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

impl Url<'_> {
    #[inline]
    fn includes_credentials(&self) -> bool {
        !self.username().is_empty() || !self.password().is_empty()
    }

    #[inline]
    fn host_is_null_or_empty(&self) -> bool {
        match self.host() {
            None => true,
            Some(h) => h.is_empty(),
        }
    }

    #[inline]
    fn cannot_have_username_password_port(&self) -> bool {
        !self.has_host() || self.host() == Some("") || self.scheme() == "file"
    }

    fn sync_credential_flags(&mut self) {
        if self.username().is_empty() && self.password().is_empty() {
            self.flags.remove(UrlFlags::HAS_CREDENTIALS);
            self.flags.remove(UrlFlags::HAS_PASSWORD);
        } else {
            self.flags.insert(UrlFlags::HAS_CREDENTIALS);
            if self.password().is_empty() {
                self.flags.remove(UrlFlags::HAS_PASSWORD);
            } else {
                self.flags.insert(UrlFlags::HAS_PASSWORD);
            }
        }
    }

    fn take_fragment(&mut self) -> Option<String> {
        if self.fragment_start == Self::NONE {
            return None;
        }
        let start = self.fragment_start;
        debug_assert!(self.byte_at(start) == b'#');
        let fragment = self.as_str()[start as usize + 1..].to_owned();
        self.serialization.as_mut_string().truncate(start as usize);
        self.fragment_start = Self::NONE;
        Some(fragment)
    }

    fn restore_fragment(&mut self, fragment: Option<String>) {
        if let Some(fragment) = fragment {
            debug_assert_eq!(self.fragment_start, Self::NONE);
            self.fragment_start = to_u32(self.serialization.len()).unwrap_or(Self::NONE);
            let ser = self.serialization.as_mut_string();
            ser.push('#');
            ser.push_str(&fragment);
        }
    }

    pub(crate) fn take_after_path(&mut self) -> String {
        let i = if self.query_start != Self::NONE {
            self.query_start
        } else if self.fragment_start != Self::NONE {
            self.fragment_start
        } else {
            return String::new();
        };
        let after = self.as_str()[i as usize..].to_owned();
        self.serialization.as_mut_string().truncate(i as usize);
        after
    }

    pub(crate) fn restore_after_path(&mut self, old_after_path_position: u32, after_path: &str) {
        let new_after_path_position = to_u32(self.serialization.len()).unwrap_or(u32::MAX);
        let adjust = |index: &mut u32| {
            if *index != Self::NONE {
                *index = index
                    .wrapping_sub(old_after_path_position)
                    .wrapping_add(new_after_path_position);
            }
        };
        adjust(&mut self.query_start);
        adjust(&mut self.fragment_start);
        self.serialization.as_mut_string().push_str(after_path);
    }

    fn strip_trailing_spaces_from_opaque_path(&mut self) {
        if !self.cannot_be_a_base() {
            return;
        }
        if self.fragment_start != Self::NONE || self.query_start != Self::NONE {
            return;
        }
        let ser = self.serialization.as_mut_string();
        while ser.ends_with(' ') {
            ser.pop();
        }
    }

    fn set_port_internal(&mut self, port: Option<u16>) {
        let old_port = self.port_u16();
        match (old_port, port) {
            (None, None) => {}
            (Some(_), None) => {
                let offset = self.path_start - self.host_end;
                self.serialization
                    .as_mut_string()
                    .drain(self.host_end as usize..self.path_start as usize);
                self.path_start = self.host_end;
                if self.query_start != Self::NONE {
                    self.query_start -= offset;
                }
                if self.fragment_start != Self::NONE {
                    self.fragment_start -= offset;
                }
            }
            (Some(old), Some(new)) if old == new => {}
            (_, Some(new)) => {
                let path_and_after = self.as_str()[self.path_start as usize..].to_owned();
                {
                    let ser = self.serialization.as_mut_string();
                    ser.truncate(self.host_end as usize);
                    let _ = write!(ser, ":{new}");
                }
                let old_path_start = self.path_start;
                let new_path_start = to_u32(self.serialization.len()).unwrap_or(old_path_start);
                self.path_start = new_path_start;
                let adjust = |index: &mut u32| {
                    if *index != Self::NONE {
                        *index = index
                            .wrapping_sub(old_path_start)
                            .wrapping_add(new_path_start);
                    }
                };
                adjust(&mut self.query_start);
                adjust(&mut self.fragment_start);
                self.serialization.as_mut_string().push_str(&path_and_after);
            }
        }
        self.port = match port {
            Some(p) => u32::from(p),
            None => Self::NO_PORT,
        };
    }

    /// `opt_new_port`: `None` = leave port; `Some(None)` = remove; `Some(Some(p))` = set.
    fn set_host_internal(&mut self, host: Host<'_>, opt_new_port: Option<Option<u16>>) {
        let old_suffix_pos = if opt_new_port.is_some() {
            self.path_start
        } else {
            self.host_end
        };
        let mut suffix = self.as_str()[old_suffix_pos as usize..].to_owned();
        let needs_authority_slashes = !self.has_authority();
        // Undo anarchist `/.` once a real authority is present (`non-spec:/.//p` → `…://h//p`).
        let mut anarchist_skip = 0u32;
        if needs_authority_slashes && suffix.starts_with("/.//") {
            suffix.drain(..2);
            anarchist_skip = 2;
        }

        {
            let ser = self.serialization.as_mut_string();
            ser.truncate(self.host_start as usize);
            if needs_authority_slashes {
                ser.push('/');
                ser.push('/');
                self.username_end += 2;
                self.host_start += 2;
            }
            let _ = write!(ser, "{host}");
        }

        self.host_end = to_u32(self.serialization.len()).unwrap_or(self.host_start);

        // Host flags
        self.flags.remove(UrlFlags::HOST_IPV4);
        self.flags.remove(UrlFlags::HOST_IPV6);
        self.flags.remove(UrlFlags::HOST_IDNA);
        self.flags.remove(UrlFlags::HAS_EMPTY_HOST);
        match &host {
            Host::Domain(d) if d.is_empty() => {
                self.flags.insert(UrlFlags::HAS_EMPTY_HOST);
            }
            Host::Ipv4(_) => self.flags.insert(UrlFlags::HOST_IPV4),
            Host::Ipv6(_) => self.flags.insert(UrlFlags::HOST_IPV6),
            Host::Domain(d) => {
                if d.contains("xn--") {
                    self.flags.insert(UrlFlags::HOST_IDNA);
                }
            }
        }

        if let Some(new_port) = opt_new_port {
            self.port = match new_port {
                Some(p) => u32::from(p),
                None => Self::NO_PORT,
            };
            if let Some(port) = new_port {
                let _ = write!(self.serialization.as_mut_string(), ":{port}");
            }
        }

        let new_suffix_pos = to_u32(self.serialization.len()).unwrap_or(old_suffix_pos);
        self.serialization.as_mut_string().push_str(&suffix);

        let adjust = |index: &mut u32| {
            if *index != Self::NONE {
                *index = index
                    .wrapping_sub(old_suffix_pos)
                    .wrapping_add(new_suffix_pos)
                    .wrapping_sub(anarchist_skip);
            }
        };
        adjust(&mut self.path_start);
        adjust(&mut self.query_start);
        adjust(&mut self.fragment_start);
    }
}
