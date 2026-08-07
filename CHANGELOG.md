# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/hocestnonsatis/sorug/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/hocestnonsatis/sorug/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/hocestnonsatis/sorug/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/hocestnonsatis/sorug/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/hocestnonsatis/sorug/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/hocestnonsatis/sorug/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/hocestnonsatis/sorug/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/hocestnonsatis/sorug/releases/tag/v0.1.0
