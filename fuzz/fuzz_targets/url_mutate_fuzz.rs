//! Mutation / setter fuzz target for `sorug`.
//!
//! Goals under adversarial UTF-8 payloads (capped at [`MAX_INPUT_LEN`]):
//! 1. Never panic / never OOB when applying setters, `join`, path/query mutators.
//! 2. After mutation, canonical `href()` must re-parse identically (round-trip).
//! 3. Getters remain consistent with the serialization (offset invariants).
//!
//! Input layout (when long enough):
//! - byte 0: op selector
//! - bytes 1..: base URL string (NUL-terminated or remainder)
//! - remaining after base: op argument string

#![no_main]

use libfuzzer_sys::fuzz_target;
use std::str;

const MAX_INPUT_LEN: usize = 4096;

fuzz_target!(|data: &[u8]| {
    if data.is_empty() || data.len() > MAX_INPUT_LEN {
        return;
    }

    let op = data[0];
    let rest = &data[1..];
    let (base_bytes, arg_bytes) = split_nul(rest);
    let Ok(base_s) = str::from_utf8(base_bytes) else {
        return;
    };
    let Ok(arg_s) = str::from_utf8(arg_bytes) else {
        return;
    };

    let Ok(url) = sorug::Url::parse(base_s) else {
        return;
    };
    // Owned so setters can freely mutate without lifetime issues.
    let mut url = url.into_owned();

    match op % 16 {
        0 => {
            let _ = url.set_href(arg_s);
        }
        1 => {
            let _ = url.set_protocol(arg_s);
        }
        2 => {
            let _ = url.set_username(arg_s);
        }
        3 => {
            let _ = url.set_password(arg_s);
        }
        4 => {
            let _ = url.set_host(arg_s);
        }
        5 => {
            let _ = url.set_hostname(arg_s);
        }
        6 => {
            let _ = url.set_port_str(arg_s);
        }
        7 => {
            url.set_pathname(arg_s);
        }
        8 => {
            url.set_search(arg_s);
        }
        9 => {
            url.set_hash(arg_s);
        }
        10 => {
            let _ = url.join(arg_s);
        }
        11 => {
            if let Ok(mut segs) = url.path_segments_mut() {
                if !arg_s.is_empty() {
                    segs.push(arg_s);
                } else {
                    segs.clear();
                }
            }
        }
        12 => {
            let mut q = url.query_pairs_mut();
            if arg_s.is_empty() {
                q.clear();
            } else if let Some((k, v)) = arg_s.split_once('=') {
                q.append(k, v);
            } else {
                q.append(arg_s, "");
            }
        }
        13 => {
            let mut params = url.search_params();
            params.sort();
            if params.has(arg_s) {
                let _ = params.delete(arg_s);
            } else if !arg_s.is_empty() {
                params.append(arg_s, "1");
            }
            url.set_search_params(&params);
        }
        14 => {
            if let Ok(ip) = arg_s.parse() {
                let _ = url.set_ip_host(ip);
            }
        }
        _ => {
            let _ = url.set_scheme(arg_s);
        }
    }

    assert_invariants(&url);

    let href = url.href().to_owned();
    let again = sorug::Url::parse(&href).unwrap_or_else(|e| {
        panic!(
            "post-mutation href re-parse failed\n  base: {}\n  arg: {}\n  href: {}\n  err: {e:?}",
            base_s.escape_default(),
            arg_s.escape_default(),
            href.escape_default()
        );
    });
    assert_eq!(
        again.href(),
        href,
        "post-mutation href round-trip changed\n  base: {}\n  arg: {}\n  first: {}\n  second: {}",
        base_s.escape_default(),
        arg_s.escape_default(),
        href.escape_default(),
        again.href().escape_default()
    );
});

fn split_nul(data: &[u8]) -> (&[u8], &[u8]) {
    match data.iter().position(|&b| b == 0) {
        Some(i) => (&data[..i], &data[i + 1..]),
        None => {
            let mid = data.len() / 2;
            (&data[..mid], &data[mid..])
        }
    }
}

fn assert_invariants(url: &sorug::Url<'_>) {
    let href = url.href();
    let len = href.len();
    assert_eq!(url.as_str(), href);

    let scheme = url.scheme();
    let protocol = url.protocol();
    assert!(protocol.ends_with(':') || protocol.is_empty());
    assert!(scheme.len() <= len);
    assert!(protocol.len() <= len);

    let _ = url.username();
    let _ = url.password();
    let _ = url.host();
    let _ = url.hostname();
    let _ = url.host_with_port();
    let _ = url.port();
    let _ = url.port_str();
    let _ = url.pathname();
    let _ = url.path();
    let _ = url.search();
    let _ = url.query();
    let _ = url.hash();
    let _ = url.fragment();
    let _ = url.cannot_be_a_base();
    let _ = url.is_special();
    let _ = url.origin().serialized();
}
