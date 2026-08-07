# Public API audit notes (pre-1.0)

Living checklist toward a future `1.0` freeze. Not a freeze by itself — see README.

**Sign-off:** API shape decisions below are locked for the 0.5 → 1.0 path
(2026-08-07). Freeze still requires sustained low churn + remaining README
checklist items (WPT/fuzz green, migration docs, publish hygiene).

## Surface map

| Item | Stability intent |
| --- | --- |
| `Url<'a>` | Stable contract: borrow when canonical; `into_owned` for `'static` |
| Getters / setters / `join` / `make_relative` | Stable WHATWG / rust-url-shaped |
| `SearchParams`, `query_pairs(_mut)`, `path_segments(_mut)` | Stable; value-aware `has`/`delete` additive |
| `Host`, `Origin`, `OpaqueOrigin` | Stable; opaque uniqueness since 0.4 |
| `Backing` | **Public, advanced** — prefer `as_str` / `href` / `into_owned` |
| `UrlFlags` | Public bitflags; treat as opaque where possible |
| `ParseError` | `Failure` \| `InputTooLong` only through 1.0 |
| `State` | `doc(hidden)` + `non_exhaustive` — may change/remove |
| Features `std` / `serde` / `http` | Stable gates |
| `sorug-ffi` | Separate; not crates.io; ABI follows workspace tags |

## Locked decisions (1.0 path)

| Topic | Decision |
| --- | --- |
| `Backing` visibility | Stay **public**. Document as advanced; everyday code uses `as_str` / `href` / `into_owned`. Do not hide before 1.0. |
| Host naming | Keep current shape: `host` / `host_str` → serialized string; `host_parsed` → typed `Host`. Do not rename to match rust-url’s `host()` → typed. Migration: [cookbook](cookbook.md). |
| `ParseError` | Keep `Failure` \| `InputTooLong`. No IDNA/subtype variants in 1.0 (would be breaking). |
| `State` | Remain `doc(hidden)` + `non_exhaustive`. |
| FFI | Stay off crates.io (`publish = false`); consumers pin GitHub Release tags. |
| IDNA deps | No ICU / `idna` crate; UCD + `data/idna_overlay.txt` stay in-tree. |
| Safety | Main crate remains `forbid(unsafe_code)`. |

## Not goals for 1.0

- Historical non-WHATWG quirk parity
- Trading `forbid(unsafe_code)` for micro-wins
- Adding ICU / `idna` crates
- Expanding `ParseError` variants
- Hiding `Backing` or collapsing host naming
