# sorug

[![CI](https://github.com/hocestnonsatis/sorug/actions/workflows/ci.yml/badge.svg)](https://github.com/hocestnonsatis/sorug/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/sorug.svg)](https://crates.io/crates/sorug)
[![docs.rs](https://docs.rs/sorug/badge.svg)](https://docs.rs/sorug)
[![WPT](https://img.shields.io/badge/WPT-891%2F891-brightgreen)](https://github.com/hocestnonsatis/sorug)
[![WPT setters](https://img.shields.io/badge/WPT%20setters-278%2F278-brightgreen)](https://github.com/hocestnonsatis/sorug)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE-MIT)
[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success)](https://github.com/hocestnonsatis/sorug/blob/main/CONTRIBUTING.md)

**sorug** is an ultra-high-performance, zero-copy, [WHATWG URL Living Standard](https://url.spec.whatwg.org/)-compliant URL parser written in Rust. It targets production parsers that need correctness *and* nanosecond-scale throughput — and currently outperforms both [servo/rust-url](https://github.com/servo/rust-url) and [ada-url](https://github.com/ada-url/ada) on the hot paths that matter.

## Why sorug?

| Pillar | What it means |
| --- | --- |
| **Zero-Copy** | Canonical ASCII inputs stay borrowed (`Backing::Borrowed`); heap allocation only on first required mutation (CoW). |
| **SIMD / SWAR** | 64-bit scheme-prefix jumps + SWAR delimiter scans for short inputs; [`memchr`](https://crates.io/crates/memchr) for longer buffers. |
| **Custom Punycode** | In-crate Punycode + UTS #46 mapping; membership tables from vendored Unicode UCD (`data/ucd/`) + `idna_overlay.txt` — no ICU/`idna` runtime dep. |
| **891 / 891 WPT** | Full pass of the Web Platform Tests `urltestdata` suite shipped in-tree. |
| **278 / 278 setters** | Full pass of WPT `setters_tests.json` (component mutators). |
| **`forbid(unsafe_code)`** | Zero `unsafe` in library code. Correctness first; speed without memory-safety shortcuts. |
| **`no_std` + `alloc`** | Embedded / WASM friendly (`default-features = false`). |

## Benchmarks

Criterion, Linux, release profile (`lto = true`, `codegen-units = 1`). Lower is better (nanoseconds / parse). Measured **2026-08-07** (0.5/0.6 prep; Fast Path ASCII now leads ada).

| Workload | **sorug** | ada-url | servo/`url` |
| --- | ---: | ---: | ---: |
| Fast Path ASCII (`https://example.com/api/v1/users`) | **28.3 ns** | 31.9 ns | 97 ns |
| Complex Query / Fragment | **56.0 ns** | 140 ns | 216 ns |
| IDNA / Punycode | **196 ns** | 277 ns | 248 ns |
| File Edge Case | **31.1 ns** | 95.0 ns | 129 ns |

Reproduce locally:

```bash
cargo bench --bench url_benchmark
```

> Numbers are indicative. Absolute values vary by CPU; relative ordering is what we track. IDNA uses an in-tree UTS #46 path with a Latin-1/CJK/kana/Hangul identity fast path; membership tables are built from vendored Unicode UCD (no ICU/`idna` crate).

## Quick start

```bash
cargo add sorug
```

```toml
[dependencies]
sorug = "0.5"
```

```rust
use sorug::Url;

fn main() -> Result<(), sorug::ParseError> {
    let url = Url::parse("https://example.com/path?q=1#frag")?;
    assert_eq!(url.scheme(), "https");
    assert_eq!(url.host(), Some("example.com"));
    assert_eq!(url.as_str(), "https://example.com/path?q=1#frag");
    assert_eq!(url.origin().serialized(), "https://example.com");
    Ok(())
}
```

Relative resolution with `join` / `make_relative`:

```rust
use sorug::Url;

let base = Url::parse("https://example.com/dir/page")?;
let joined = base.join("../other")?;
assert_eq!(joined.as_str(), "https://example.com/other");

let target = Url::parse("https://example.com/dir/x")?;
assert_eq!(base.make_relative(&target).as_deref(), Some("x"));
```

Mutate components (WHATWG / WPT setters) and edit the query as form-urlencoded pairs:

```rust
use sorug::{SearchParams, Url};

let mut url = Url::parse("https://example.com/old")?;
url.set_pathname("/api/v1");
url.set_search("?q=1");
assert_eq!(url.href(), "https://example.com/api/v1?q=1");

let mut params = SearchParams::parse("q=1");
params.append("lang", "tr");
url.set_search_params(&params);
assert_eq!(url.search(), "?q=1&lang=tr");
```

### Features

| Feature | Default | Purpose |
| --- | --- | --- |
| `std` | yes | [`std::error::Error`] for `ParseError`; `memchr` std backend |
| `serde` | no | Serialize / deserialize `Url` as an href string |
| `http` | no | Convert between `Url` and [`http::Uri`](https://docs.rs/http) (implies `std`) |

```toml
# no_std + alloc
sorug = { version = "0.5", default-features = false }

# with serde
sorug = { version = "0.5", features = ["serde"] }

# with http::Uri bridge
sorug = { version = "0.5", features = ["http"] }
```

`http` feature example:

```rust
use http::Uri;
use sorug::Url;

let url = Url::parse("https://example.com/api").unwrap();
let uri: Uri = url.to_uri().unwrap();
let back = sorug::uri_to_url(&uri).unwrap();
assert_eq!(back.as_str(), "https://example.com/api");
```

Git dependency (tracking `main`):

```toml
[dependencies]
sorug = { git = "https://github.com/hocestnonsatis/sorug" }
```

## Current Status & Roadmap

**Today (0.5.0)**

- Relative URL ops: `join` / `make_relative` / `path_segments` / `path_segments_mut` / `query_pairs(_mut)`.
- Typed `Host` (+ `Host::parse`), rust-url-shaped getters, `Hash` / `Ord`, optional `serde` / `http`, `no_std` + `alloc`.
- File paths, unique opaque origins, `set_ip_host` / `socket_addrs`, `SearchParams` (incl. value-aware `has`/`delete`/`size`), `parse_with_params`.
- IDNA: in-tree Punycode + UTS #46; membership tables from vendored Unicode UCD + `data/idna_overlay.txt` (Node/WPT).
- WPT parser: **891 / 891**; WPT setters: **278 / 278**.
- Docs: [docs.rs/sorug](https://docs.rs/sorug); recipes in [docs/cookbook.md](docs/cookbook.md).

**Breaking (0.3 → 0.4)**

- [`Origin::Opaque`](https://docs.rs/sorug/latest/sorug/enum.Origin.html) is now `Opaque(OpaqueOrigin)` with unique nonces — distinct opaque origins no longer compare equal.

**Next** (toward `1.0`; no freeze yet)

- Keep WPT fixtures current via [`scripts/refresh-wpt.sh`](scripts/refresh-wpt.sh); differential fuzz vs rust-url with documented divergences; weekly long fuzz via [`.github/workflows/fuzz-long.yml`](.github/workflows/fuzz-long.yml).
- Unicode UCD refresh when a new major is ready (`./scripts/refresh-ucd.sh`); see [data/ucd/README.md](data/ucd/README.md).
- When API churn stays low: run the [1.0 freeze checklist](#10-freeze-checklist) below.

### Semver and MSRV policy

- **0.x:** Breaking changes allowed in minor bumps (`0.N → 0.N+1`) when called out in [CHANGELOG.md](CHANGELOG.md). Additive APIs may land in patch or minor.
- **MSRV:** Declared in `Cargo.toml` as `rust-version` (currently **1.85**). MSRV bumps are **minor** in 0.x (documented in the changelog); CI verifies the declared toolchain.
- **Features:** `std` (default), `serde`, `http` (implies `std`). Disabling `std` is supported (`no_std` + `alloc`).
- **FFI:** `sorug-ffi` is **not** on crates.io; ABI may change with the workspace version. Prefer GitHub Release binaries pinned to a tag.

### 1.0 freeze checklist

Do **not** cut 1.0 until all boxes are true for a **sustained** low-churn period (no rush — product work continues on 0.x). Gate notes: [docs/api-audit.md](docs/api-audit.md).

- [x] Public surface audit signed off: `Url<'a>` lifetime, `Backing` stays public/advanced, `State` stays `doc(hidden)`, `ParseError` stays two variants — [docs/api-audit.md](docs/api-audit.md) (2026-08-07).
- [x] rust-url migration notes complete (`host` vs `host_parsed`, port setters, origins, lifetimes) — see [docs/cookbook.md](docs/cookbook.md).
- [ ] WPT parser + setters green on current fixtures; fuzz smoke + weekly long fuzz green; no open `wpt-freshness` regressions — **sustained** period required (no rush).
- [ ] CHANGELOG + docs.rs + Trusted Publishing ready for the freeze cut; FFI stays `publish = false` — verify again at freeze time.

**Not goals for 1.0:** historical non-WHATWG quirk parity; trading `forbid(unsafe_code)` for micro-wins; adding ICU/`idna` crates; expanding `ParseError`; hiding `Backing`.

**C FFI**

Optional C bindings live in [`ffi/`](ffi/) (`sorug-ffi`, workspace member, **not** on crates.io). Prebuilt `cdylib` / `staticlib` + `sorug.h` ship on [GitHub Releases](https://github.com/hocestnonsatis/sorug/releases). The main crate stays `forbid(unsafe_code)`. See [`ffi/README.md`](ffi/README.md).

**Not goals (for now)**

- Matching every historical quirk of non-WHATWG parsers.
- Trading `forbid(unsafe_code)` for micro-wins.

## Design sketch

- **Index-based record** — component boundaries are `u32` offsets into the WHATWG `href` serialization.
- **Lazy / CoW serialization** — borrow when input is already canonical; upgrade to owned on mutation.
- **Strict state machine** — transitions follow the [URL Living Standard](https://url.spec.whatwg.org/#url-parsing) basic URL parser.

## Testing

```bash
cargo test                 # unit + integration (incl. WPT + comprehensive validation)
cargo test --all-features  # includes serde / http
cargo test --workspace     # includes sorug-ffi
cargo test --test api_maturity       # Hash/Ord, getters, query_pairs, set_port
cargo test --test path_segments_mut  # PathSegmentsMut vs rust-url
cargo check --no-default-features  # no_std + alloc
cargo test --test wpt      # WPT urltestdata only
cargo test --test wpt_setters  # WPT setters_tests only
cargo bench                # Criterion vs ada-url and servo/url
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for policies, style, and review expectations.

## Cookbook

Integration snippets and rust-url migration: [docs/cookbook.md](docs/cookbook.md).
1.0 freeze gate (no rush): [docs/1.0-gate.md](docs/1.0-gate.md).

## License

Licensed under either of

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT license](LICENSE-MIT)

at your option.

## Code of Conduct

Participation is governed by our [Code of Conduct](CODE_OF_CONDUCT.md).
