# sorug

> **Status: Early release (`0.1.x`) on [crates.io](https://crates.io/crates/sorug).**
>
> APIs may still change before `1.0`. Pin a version and watch release notes.

[![CI](https://github.com/hocestnonsatis/sorug/actions/workflows/ci.yml/badge.svg)](https://github.com/hocestnonsatis/sorug/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/sorug.svg)](https://crates.io/crates/sorug)
[![docs.rs](https://docs.rs/sorug/badge.svg)](https://docs.rs/sorug)
[![WPT](https://img.shields.io/badge/WPT-891%2F891-brightgreen)](https://github.com/hocestnonsatis/sorug)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE-MIT)
[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success)](https://github.com/hocestnonsatis/sorug/blob/main/CONTRIBUTING.md)

**sorug** is an ultra-high-performance, zero-copy, [WHATWG URL Living Standard](https://url.spec.whatwg.org/)-compliant URL parser written in Rust. It targets production parsers that need correctness *and* nanosecond-scale throughput — and currently outperforms both [servo/rust-url](https://github.com/servo/rust-url) and [ada-url](https://github.com/ada-url/ada) on the hot paths that matter.

## Why sorug?

| Pillar | What it means |
| --- | --- |
| **Zero-Copy** | Canonical ASCII inputs stay borrowed (`Backing::Borrowed`); heap allocation only on first required mutation (CoW). |
| **SIMD / SWAR** | 64-bit scheme-prefix jumps + SWAR delimiter scans for short inputs; [`memchr`](https://crates.io/crates/memchr) for longer buffers. |
| **Custom Punycode** | Lightweight in-crate Punycode / minimal UTS #46 — no heavy `idna` dependency. |
| **891 / 891 WPT** | Full pass of the Web Platform Tests `urltestdata` suite shipped in-tree. |
| **`forbid(unsafe_code)`** | Zero `unsafe` in library code. Correctness first; speed without memory-safety shortcuts. |

## Benchmarks

Criterion, Linux, release profile (`lto = true`, `codegen-units = 1`). Lower is better (nanoseconds / parse).

| Workload | **sorug** | ada-url | servo/`url` |
| --- | ---: | ---: | ---: |
| Fast Path ASCII (`https://example.com/api/v1/users`) | **30.9 ns** | 31.5 ns | 97.6 ns |
| Complex Query / Fragment | **53.1 ns** | 141 ns | 193 ns |
| IDNA / Punycode | **171 ns** | 239 ns | 243 ns |
| File Edge Case | **30.8 ns** | 91.6 ns | 129 ns |

Reproduce locally:

```bash
cargo bench --bench url_benchmark
```

> Numbers are indicative. Absolute values vary by CPU; relative ordering is what we track.

## Quick start

```bash
cargo add sorug
```

```toml
[dependencies]
sorug = "0.1"
```

```rust
use sorug::Url;

fn main() -> Result<(), sorug::ParseError> {
    let url = Url::parse("https://example.com/path?q=1#frag")?;
    assert_eq!(url.scheme(), "https");
    assert_eq!(url.host(), Some("example.com"));
    assert_eq!(url.as_str(), "https://example.com/path?q=1#frag");
    Ok(())
}
```

Relative resolution with a base URL:

```rust
use sorug::Url;

let base = Url::parse("https://example.com/dir/page")?;
let joined = Url::parse_with_base("../other", Some(&base))?;
assert_eq!(joined.as_str(), "https://example.com/other");
```

Git dependency (tracking `main`):

```toml
[dependencies]
sorug = { git = "https://github.com/hocestnonsatis/sorug" }
```

## Current Status & Roadmap

**Today**

- Published on [crates.io](https://crates.io/crates/sorug) as **`0.1.1`** (early release).
- WPT: **891 / 891**. Core ASCII / file / complex-query / IDNA paths lead ada-url.
- Docs: [docs.rs/sorug](https://docs.rs/sorug).

**Next**

- Stabilize public API toward `1.0`.
- Expanded examples and docs.
- Continued differential testing against rust-url / ada where intentional divergences are documented.
- Optional `no_std` (+ `alloc`) exploration without sacrificing the zero-copy fast path.

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
cargo test --test wpt      # WPT urltestdata only
cargo bench                # Criterion vs ada-url and servo/url
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for policies, style, and review expectations.

## License

Licensed under either of

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT license](LICENSE-MIT)

at your option.

## Code of Conduct

Participation is governed by our [Code of Conduct](CODE_OF_CONDUCT.md).
