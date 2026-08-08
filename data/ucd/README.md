# Vendored Unicode UCD (pinned)

Offline source for `build.rs` IDNA membership tables. **Do not fetch at compile time.**

| File | Upstream |
| --- | --- |
| `UNICODE_VERSION` | Pin (currently **17.0.0**; bump via `refresh-ucd.sh`) |
| `IdnaMappingTable.txt` | `https://www.unicode.org/Public/idna/<ver>/IdnaMappingTable.txt` |
| `DerivedBidiClass.txt` | `https://www.unicode.org/Public/<ver>/ucd/extracted/DerivedBidiClass.txt` |
| `Scripts.txt` | `https://www.unicode.org/Public/<ver>/ucd/Scripts.txt` |
| `DerivedJoiningType.txt` | `https://www.unicode.org/Public/<ver>/ucd/extracted/DerivedJoiningType.txt` |

Refresh:

```bash
./scripts/refresh-ucd.sh 17.0.0   # or another Unicode major
cargo test
cargo test --test wpt --test wpt_setters
```

When bumping the major Unicode version, review `../idna_overlay.txt` for deltas
that Node/WPT still need, then cut a **minor** crate release (e.g. 0.6) with a
CHANGELOG note. Do not refresh at compile time — only via this script.

## Unicode 18 gate (not ready as of 2026-08-08)

Do **not** bump until all of the following are true:

1. Final (non-draft) `IdnaMappingTable.txt` published for 18.x under
   `https://www.unicode.org/Public/idna/18.0.0/` **or**
   `https://www.unicode.org/Public/18.0.0/idna/` (see `refresh-ucd.sh` layout).
2. Matching UCD extracts (`DerivedBidiClass`, `Scripts`, `DerivedJoiningType`)
   available for the same version.
3. Node/ada oracle behavior on the new tables is checked against WPT
   (`cargo test --test wpt --test wpt_setters`) and differential fuzz smoke.

Until then, stay on the pin in `UNICODE_VERSION` (**17.0.0**). Draft UCD alone
is insufficient.
