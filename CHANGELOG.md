# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.6.3] - 2026-10-04

### Fixed

- `PathSegmentsMut::pop` / `pop_if_empty` on non-special anarchist URLs now
  drop the `/.` marker once the path no longer starts with `//`. Previously
  `foo:/.//b` + `pop()` serialized `foo:/./`, which re-parses as `foo:/`.
- `Url::parse*` returns `ParseError::InputTooLong` when the *serialized* URL
  (after percent-encoding growth) would not fit the `u32` offset space,
  instead of truncating offsets and risking getter panics on multi-GiB input.
- FFI: `sorug_origin` no longer mutates the handle through a shared getter
  (data race when called concurrently with other getters). The origin cache
  is now a `OnceLock`; the C signature takes `const SorugUrl *` (source
  compatible).

### Docs

- FFI `sorug_parse*` safety docs match behavior (null / invalid UTF-8 return
  null); `sorug_hostname` simplified (behavior unchanged).

## [0.6.2] - 2026-08-09

### Fixed

- `PathSegmentsMut::push` / `extend` on `file:` now normalize a leading Windows
  drive letter `|` → `:` (same as the basic URL parser). Previously
  `file:` + `push("h|")` serialized `file:///h|`, which re-parsed as
  `file:///h:` and failed weekly `url_mutate_fuzz` round-trip (2026-08-08).

### Changed

- Idempotent fast paths on `set_host` / `set_hostname` (non-empty match) and a
  broader `set_pathname` skip when the leading `/` is omitted or empty→`/`.
- WPT parser harness asserts `relativeTo` (`non-opaque-path-base` / `any-base`)
  and absolute-failure cases against hierarchical bases.

### Added

- Integration DX tests (`make_relative`, value-aware `SearchParams`, serde href).
- Criterion benches: `set_hostname` idempotent, `path_segments` push.

### Docs

- Cookbook path / file drive note; differential allowlist + 1.0-gate progress;
  Unicode 18 gate unchanged (stay on UCD 17.0.0).

## [0.6.1] - 2026-08-08

### Fixed

- Punycode ACE encoding no longer fails when the ASCII form exceeds 128 octets
  or when a label has more than 256 code points. WHATWG `beStrict = false`
  (Node/ada) accepts long labels; the former stack / code-point caps caused
  daily fuzz-smoke differential panics on oversized IDN hosts. Typical labels
  still encode on the stack; longer forms spill to the heap.
- `Url::set_host("")` now rejects when a password is present (same as
  `set_hostname`), avoiding unparseable `scheme://:password@` serializations.
- `PathSegmentsMut::clear` drops the non-special anarchist `/.` marker so
  cleared hrefs round-trip (`gi:/` instead of `gi:/./`).

### Changed

- Idempotent fast paths on `set_pathname` / `set_search` / `set_hash` /
  `set_scheme` / `set_port` / `set_password` when the component is already
  equal (skip CoW + reparse / authority surgery).
- `SchemeType` / default-port helpers live in `parser/scheme.rs` (behavior unchanged).

### Added

- FFI: `sorug_make_relative` + `sorug_string_free` (heap relative ref; pin GitHub
  Release tags). SearchParams / file-path stay Rust-only.
- Criterion peer mutate group and batch parse throughput benches.
- CI: wasm32 lib `cfg(test)` compile smoke (no WASI / ada-url).
- Fuzz regression for long ACE host with percent-encoded non-ASCII (`%C3%A9`).

### Docs

- Differential allowlist inventory: `docs/differential-allowlist.md`.
- Cookbook: relative resolution, opaque origins, file URLs, lifetimes,
  reqwest/hyper notes + rust-url divergence pointer.
- Unicode 18 refresh gate documented in `data/ucd/README.md` (not ready yet).
- 1.0-gate progress + freeze readiness procedure (no 1.0 cut yet).

## [0.6.0] - 2026-08-07

### Changed

- IDNA membership tables refreshed to **Unicode UCD 17.0.0** (`./scripts/refresh-ucd.sh`).
- Node-aligned Arabic Extended-C mix checks for newly-valid U+10EC5..=U+10EC7 (reject mixes with classic Arabic / historic RTL).

## [0.5.0] - 2026-08-07

### Added

- SearchParams value-aware `has` / `delete` and WHATWG-aligned `size` accessor.
- WPT harness asserts for `origin` and `searchParams` when present in fixtures.
- Mutation/setter fuzz target, daily fuzz smoke, and weekly long fuzz workflow.
- FFI `set_username` / `set_password` / `set_host`.
- Ecosystem cookbook (`serde`, `http::Uri`, `no_std`, rust-url migration, file paths).
- Semver and MSRV policy documented in README; public API audit decisions locked for the 1.0 path.
- CI: MSRV 1.85, wasm `no_std` check, `cargo test --no-default-features`, clippy `-D warnings`, cargo-deny, informational Criterion job.
- `scripts/refresh-wpt.sh` and weekly WPT freshness workflow.
- `scripts/refresh-ucd.sh` supports Unicode 17+ IdnaMappingTable layout (`Public/<ver>/idna/`).

### Changed

- Fast-path ASCII parse tuned toward ada parity (no `unsafe`).
- Criterion benches cover setters, `join`, SearchParams mutation, and href round-trip.

## [0.4.0] - 2026-08-04

### Added

- `Url::from_file_path` / `from_directory_path` / `to_file_path` (`std`, supported platforms).
- Unique opaque origins: `Origin::Opaque(OpaqueOrigin)`, `Origin::new_opaque()`.
- `Url::set_ip_host` / `Url::socket_addrs`.
- `SearchParams::sort`, `Url::parse_with_params`.
- FFI: `join`, origin getter, core setters; GitHub Release binaries for linux-x86_64, macos-aarch64, windows-x86_64.
- crates.io Trusted Publishing (OIDC) via `.github/workflows/publish.yml`.

### Changed

- **Breaking:** `Origin::Opaque` is now `Opaque(OpaqueOrigin)` with unique nonces — distinct opaque origins no longer compare equal. ASCII serialization remains `"null"`.

## [0.3.0] - 2026-08-03

### Added

- `Url::join` / `make_relative` / `path_segments` / `path_segments_mut` / `query_pairs` / `query_pairs_mut`.
- Public `Host` (`Domain` / `Ipv4` / `Ipv6`) with `Host::parse`.
- Optional features: `serde` (href string), `http` (`http::Uri` bridge; implies `std`).
- `no_std` + `alloc` via `default-features = false`.
- IDNA membership tables from vendored Unicode UCD + `data/idna_overlay.txt`.

### Changed

- Port API: `Url::set_port` takes `Option<u16>` (rust-url shape); quirks string setter is `set_port_str`.

## [0.2.0] - 2026-08-02

### Added

- WHATWG / WPT component setters (`set_href`, `set_protocol`, host/path/query/fragment, credentials).
- `SearchParams` and form-urlencoded helpers.
- `Origin` (tuple / opaque) with ASCII serialization.
- WPT setters suite: **278 / 278**.

## [0.1.1] - 2026-08-01

### Added

- IDNA range tables derived at build time; fuzz campaign lock-in.

## [0.1.0] - 2026-08-01

### Added

- Initial WHATWG URL parser with zero-copy CoW serialization.
- WPT parser suite: **891 / 891**.
- In-crate Punycode / UTS #46; SWAR + `memchr` delimiter scans.
- `forbid(unsafe_code)` on the main crate.

[Unreleased]: https://github.com/hocestnonsatis/sorug/compare/v0.6.3...HEAD
[0.6.3]: https://github.com/hocestnonsatis/sorug/compare/v0.6.2...v0.6.3
[0.6.2]: https://github.com/hocestnonsatis/sorug/compare/v0.6.1...v0.6.2
[0.6.1]: https://github.com/hocestnonsatis/sorug/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/hocestnonsatis/sorug/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/hocestnonsatis/sorug/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/hocestnonsatis/sorug/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/hocestnonsatis/sorug/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/hocestnonsatis/sorug/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/hocestnonsatis/sorug/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/hocestnonsatis/sorug/releases/tag/v0.1.0
