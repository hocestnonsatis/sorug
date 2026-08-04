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

```bash
cargo +nightly fuzz run url_fuzz -max_total_time=120 -dict=fuzz/url_dict.txt
```

`fuzz/` is its own Cargo workspace (`[workspace]` in `fuzz/Cargo.toml`); the parent crate `exclude`s it.

## On mismatch

1. Prefer algorithm / WHATWG rule gaps (document allowlists in the harness when rust-url is wrong vs WPT).
2. For IDNA table deltas vs Node/ICU: edit `data/idna_overlay.txt` or bump UCD with `./scripts/refresh-ucd.sh`.
3. Do not reintroduce `data/idna_ranges.txt` as a fuzzer victory file.
