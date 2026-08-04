# Vendored Unicode UCD (pinned)

Offline source for `build.rs` IDNA membership tables. **Do not fetch at compile time.**

| File | Upstream |
| --- | --- |
| `UNICODE_VERSION` | Pin (currently **16.0.0**) |
| `IdnaMappingTable.txt` | `https://www.unicode.org/Public/idna/<ver>/IdnaMappingTable.txt` |
| `DerivedBidiClass.txt` | `https://www.unicode.org/Public/<ver>/ucd/extracted/DerivedBidiClass.txt` |
| `Scripts.txt` | `https://www.unicode.org/Public/<ver>/ucd/Scripts.txt` |
| `DerivedJoiningType.txt` | `https://www.unicode.org/Public/<ver>/ucd/extracted/DerivedJoiningType.txt` |

Refresh:

```bash
./scripts/refresh-ucd.sh 16.0.0
cargo test
cargo test --test wpt --test wpt_setters
```

Node/WPT-only deltas live in `../idna_overlay.txt`, not here.
