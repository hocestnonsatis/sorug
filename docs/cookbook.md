# sorug cookbook

Short recipes for common integrations and migrating from servo/`url`. Full API:
[docs.rs/sorug](https://docs.rs/sorug). Public-surface decisions for the 1.0 path:
[api-audit.md](api-audit.md).

## Feature matrix

| Feature | Default | Enables |
| --- | --- | --- |
| `std` | yes | `std::error::Error` for `ParseError`; `memchr` std backend; `from_file_path` / `to_file_path`; `socket_addrs` |
| `serde` | no | Serialize / deserialize `Url` as an href string (`alloc` only is enough) |
| `http` | no | `Url` ↔ `http::Uri` (implies `std`) |

```toml
# default (std)
sorug = "0.5"

# no_std + alloc
sorug = { version = "0.5", default-features = false }

# serde without std
sorug = { version = "0.5", default-features = false, features = ["serde"] }

# http::Uri bridge
sorug = { version = "0.5", features = ["http"] }
```

## Serde (`features = ["serde"]`)

`Url` serializes as its href string. Prefer `into_owned()` before storing so the
value is `'static`:

```rust
use serde_json::json;
use sorug::Url;

let url = Url::parse("https://example.com/a")?.into_owned();
let v = serde_json::to_value(&url)?;
assert_eq!(v, json!("https://example.com/a"));
let back: Url = serde_json::from_value(v)?;
assert_eq!(back.as_str(), "https://example.com/a");
```

## `http::Uri` bridge (`features = ["http"]`)

```rust
use http::Uri;
use sorug::{uri_to_url, Url};

let url = Url::parse("https://example.com/api")?;
let uri: Uri = url.to_uri()?;
let again = uri_to_url(&uri)?;
assert_eq!(again.as_str(), url.as_str());
```

`http` implies `std`. Opaque / non-`http(s)` schemes may fail `to_uri` when they
are not valid HTTP URIs.

## `no_std` + `alloc`

Parse, getters, setters, `SearchParams`, and `Origin` work without `std`.
File-path helpers and `socket_addrs` require `std` (and supported OS targets).

### Opaque origins

`Origin::new_opaque()` uses an `AtomicUsize` nonce so distinct opaque origins
compare unequal (rust-url-compatible). That atomics path is available on `no_std`
targets that provide `AtomicUsize`; ASCII serialization remains `"null"`.

## Lifetimes and `into_owned`

Canonical ASCII inputs stay borrowed (`Url<'a>` tied to the input `&str`). The
first required mutation upgrades the backing to owned. When you need a
self-contained value (store in a struct, return from a function, Serde round-trip):

```rust
use sorug::Url;

fn stash(input: &str) -> Result<Url<'static>, sorug::ParseError> {
    Ok(Url::parse(input)?.into_owned())
}
```

`Backing` is public for advanced inspection; prefer `as_str()` / `href()` /
`into_owned()` in application code.

## Errors

```rust
use sorug::{ParseError, Url};

match Url::parse(input) {
    Ok(url) => { /* … */ }
    Err(ParseError::InputTooLong) => { /* over the internal length cap */ }
    Err(ParseError::Failure) => { /* WHATWG parse failure (incl. IDNA) */ }
}
```

`ParseError` stays two variants through 1.0 — there is no IDNA subtype.

## Host typing

- `Url::host()` / `host_str()` → serialized host string (`Option<&str>`).
- `Url::host_parsed()` → typed [`Host`](https://docs.rs/sorug/latest/sorug/enum.Host.html)
  (`Domain` / `Ipv4` / `Ipv6`).

## rust-url migration

| servo / `url` | sorug |
| --- | --- |
| `Url::parse` | `Url::parse` (same idea; borrow when canonical) |
| `url.host()` (typed) | `url.host_parsed()` |
| `url.host_str()` | `url.host()` or `url.host_str()` |
| `url.set_port(Some(443))` / `None` | `url.set_port(Some(443))` / `None` |
| string port quirks | `url.set_port_str(...)` |
| `url.origin()` opaque equality | `Origin::Opaque(OpaqueOrigin)` — unique nonces since 0.4; ASCII still `"null"` |
| `url.join` / `make_relative` | same names |
| `url.path_segments_mut` / `query_pairs_mut` | same names |
| `url.to_file_path` / `from_file_path` | same (`std`, supported platforms) |
| owned `'static` URL | `url.into_owned()` |

Breaking note when coming from sorug 0.3: opaque origins no longer compare equal
to each other.

## File paths and sockets (`std`)

```rust
use std::path::Path;
use sorug::Url;

let url = Url::from_file_path(Path::new("/tmp/x"))?;
let path = url.to_file_path()?;

let mut url = Url::parse("https://127.0.0.1/")?;
url.set_ip_host(std::net::Ipv4Addr::LOCALHOST.into());
let _addrs = url.socket_addrs(|| Ok(443))?;
```

`from_directory_path` appends a trailing slash for directory semantics.

## SearchParams

```rust
use sorug::{SearchParams, Url};

let mut params = SearchParams::parse("q=1&lang=tr");
assert_eq!(params.size(), 2);
assert!(params.has("q"));
assert!(params.has_value("lang", "tr"));
params.delete_value("lang", "en"); // no-op if value differs
params.delete("q");
params.append("q", "2");
params.sort();

let mut url = Url::parse("https://example.com/")?.into_owned();
url.set_search_params(&params);
assert_eq!(url.search(), "?lang=tr&q=2");
```

Value-aware `has` / `delete` match the WHATWG `URLSearchParams` overloads; `size`
is the pair count.

## C FFI

See [`ffi/README.md`](../ffi/README.md). Credential and host setters
(`sorug_set_username`, `sorug_set_password`, `sorug_set_host`) invalidate prior
getter pointers — refetch after mutation. SearchParams and file-path helpers stay
Rust-only; pin GitHub Release binaries to a tag (`sorug-ffi` is not on crates.io).
