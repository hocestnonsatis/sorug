# Fuzzing sorug

## Role

The fuzzer is an **invariant / differential tester**, not a Unicode range hunter.

| Does | Does not |
| --- | --- |
| Crash / panic / OOB under adversarial UTF-8 | Discover missing IDNA code-point ranges |
| Diff `sorug` vs `url` (rust-url) href success/failure | Suggest edits to `data/idna_overlay.txt` as the default fix |
| Round-trip `href()` re-parse | Replace UCD table generation |

IDNA membership comes from vendored UCD (`data/ucd/`) + `data/idna_overlay.txt` via `build.rs`. A mismatch that is purely “UTS #46 status wrong for U+XXXX” is fixed by refreshing UCD or adjusting the overlay — not by growing a hand-maintained range list from corpus hits.

## Run

Daily CI smoke: [`.github/workflows/fuzz-smoke.yml`](../.github/workflows/fuzz-smoke.yml) (60s each target).

Weekly long run: [`.github/workflows/fuzz-long.yml`](../.github/workflows/fuzz-long.yml) (30m each target; corpus artifacts uploaded).

Local:

```bash
# From repo root — cargo-fuzz expects to run inside fuzz/
cd fuzz
cargo +nightly fuzz run url_fuzz -- -max_total_time=120 -max_len=4096 -dict=url_dict.txt
cargo +nightly fuzz run url_mutate_fuzz -- -max_total_time=120 -max_len=4096

# Multi-hour / 24h campaign (restarts on crash by default):
# ./scripts/run_fuzz_24h.sh
# FUZZ_TARGET=url_mutate_fuzz MAX_TOTAL_TIME=1800 ./scripts/run_fuzz_24h.sh
```

`fuzz/` is its own Cargo workspace (`[workspace]` in `fuzz/Cargo.toml`); the parent crate `exclude`s it.

Mutation target (`url_mutate_fuzz`) applies setters / `join` / path+query mutators and checks href round-trip invariants.

## Hygiene

- New differential findings → minimize → add a regression under `tests/fuzz_regressions.rs` when the case is stable.
- Document rust-url divergences in the harness allowlist with a WPT/Node rationale — do not “fix” sorug to match rust-url when WPT disagrees.
- Keep corpus under `fuzz/corpus/` local/CI artifacts; do not commit huge corpora.

## On mismatch

1. Prefer algorithm / WHATWG rule gaps (document allowlists in the harness when rust-url is wrong vs WPT).
2. For IDNA table deltas vs Node/ICU: edit `data/idna_overlay.txt` or bump UCD with `./scripts/refresh-ucd.sh`.
3. Do not reintroduce `data/idna_ranges.txt` as a fuzzer victory file.
