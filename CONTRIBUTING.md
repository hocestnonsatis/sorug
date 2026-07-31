# Contributing to sorug

Thank you for your interest in contributing. sorug aims to be a reference-grade WHATWG URL parser: **correct first**, then relentlessly fast, without compromising memory safety.

By participating, you agree to uphold the [Code of Conduct](CODE_OF_CONDUCT.md).

## Project principles (non-negotiable)

1. **WHATWG compliance** — Behavior follows the [URL Living Standard](https://url.spec.whatwg.org/). WPT regressions are blockers.
2. **Zero-copy by default** — Prefer borrowing the input serialization. Allocate only when the algorithm requires mutation or non-ASCII processing.
3. **`forbid(unsafe_code)`** — Library code must not introduce `unsafe`. The crate enforces this via `[lints.rust] unsafe_code = "forbid"` in `Cargo.toml`. Do not add `#![allow(unsafe_code)]` or equivalent workarounds.
4. **No bloat** — Avoid heavy Unicode / IDNA crates when a focused in-tree implementation suffices. Dependencies must earn their weight on the hot path.

Pull requests that trade these principles for micro-benchmarks will be declined.

## Development setup

Requirements:

- Rust **1.85+** (see `rust-version` in `Cargo.toml`)
- A recent stable toolchain (`rustup update stable`)

Clone and build:

```bash
git clone https://github.com/hocestnonsatis/sorug.git
cd sorug
cargo build
```

## Tests

Run the full suite (unit tests, WPT, comprehensive validation):

```bash
cargo test
```

WPT only (`tests/urltestdata.json`):

```bash
cargo test --test wpt
```

Security / IP / path / differential checks:

```bash
cargo test --test comprehensive_validation
```

All of the above must pass before a PR is mergeable. If you intentionally diverge from rust-url on a WPT-correct edge case, document it in the differential allowlist (see `tests/comprehensive_validation.rs`) and explain why in the PR.

## Benchmarks

Criterion benches live in `benches/url_benchmark.rs` and compare sorug against ada-url and servo/`url`:

```bash
cargo bench --bench url_benchmark
```

Guidelines:

- Prefer reporting **relative** change on the same machine over absolute nanoseconds in PR descriptions.
- Do not regress Fast Path ASCII, Complex Query, or File Edge Case without a strong correctness rationale.
- IDNA is near-parity with peers; large swings deserve investigation.

## Coding standards

- Follow existing module layout under `src/parser/` (`fast`, `scan`, `host`, `percent`, `punycode`, `serialization`).
- Prefer clear state-machine transitions over clever one-liners on the parser path.
- Run Clippy locally; pedantic lints are enabled at `warn`:

  ```bash
  cargo clippy --all-targets -- -D warnings
  ```

- Format with `rustfmt`:

  ```bash
  cargo fmt --all
  ```

## Pull request checklist

- [ ] `cargo test` passes (including WPT).
- [ ] No new `unsafe` and no lint overrides that weaken `forbid(unsafe_code)`.
- [ ] Zero-copy / CoW invariants preserved for ASCII-canonical inputs.
- [ ] Benchmarks run if the change touches the hot path; summarize deltas.
- [ ] Public API changes are documented and called out (crate is **pre-release**; breaking changes are allowed but must be explicit).
- [ ] New behavior covered by tests (WPT case, unit test, or comprehensive validation).

## Issues and discussion

- Bug reports: include the input string, expected vs actual `href` / components, and whether rust-url / ada agree.
- Performance ideas: include a minimal repro and Criterion snippets when possible.
- Security-sensitive URL parsing bugs: prefer a private GitHub Security Advisory if disclosure could be harmful; otherwise open an issue with a clear repro.

## License of contributions

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in sorug is dual-licensed under **MIT OR Apache-2.0**, the same as the project.
