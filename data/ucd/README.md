# Vendored Unicode UCD (pinned)

Offline source for `build.rs` IDNA membership tables. **Do not fetch at compile time.**

| File | Upstream |
| --- | --- |
| `UNICODE_VERSION` | Pin (currently **16.0.0**; bump via `refresh-ucd.sh`) |
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
