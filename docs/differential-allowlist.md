# Differential allowlist (sorug vs rust-url)

Living inventory of intentional divergences between **sorug** (Node/ada/WPT-aligned)
and **servo/`url` 2.5** (ICU-backed). The fuzz harness
[`fuzz/fuzz_targets/url_fuzz.rs`](../fuzz/fuzz_targets/url_fuzz.rs) encodes these as
`is_known_rust_url_*` predicates. Oracle for regressions:
[`tests/fuzz_regressions.rs`](../tests/fuzz_regressions.rs) (ada/Node).

**Policy:** never “fix” sorug toward rust-url when WPT/Node disagree. Prefer
algorithm fixes when sorug is wrong; allowlist only when rust-url is the outlier.

## Path / file

| Allowlist helper | What rust-url does | Why sorug differs |
| --- | --- | --- |
| `is_known_rust_url_file_slash_deviation` | Collapses empty `file:` path segments | WHATWG / WPT / ada preserve them (`file:////foo`) |
| `is_known_rust_url_file_host_drop` | Drops some non-`localhost` file hosts | WPT keeps host (`file://ofe/x`) |
| `is_known_rust_url_opaque_trailing_space` | Leaves trailing U+0020 before `?`/`#` in opaque paths | WHATWG percent-encodes / normalizes |
| `encode_path_gap_chars` | Leaves `^` `` ` `` `{` `}` literal in paths | WPT / sorug percent-encode |
| `is_known_rust_url_sticky_drive_dotdot` / `relative_drive_collapse` | Sticky Windows drive / `|` through `..` | ada / sorug drop remnants |
| `is_known_rust_url_file_parse_leniency` | Accepts some `file:` forms sorug rejects | Align with ada/Chrome |

## Authority

| Allowlist helper | What rust-url does | Why sorug differs |
| --- | --- | --- |
| `is_known_rust_url_empty_at_authority` | Accepts `scheme://@` → empty host | ada / Chrome / sorug reject |

## IDNA / UTS #46

| Allowlist helper | What rust-url does | Why sorug differs |
| --- | --- | --- |
| `is_known_rust_url_idna_table_delta` | ICU table / ACE acceptance gaps | UCD 17 + `data/idna_overlay.txt` (Node) |
| `is_known_rust_url_arabic_punct_idna` | Encodes U+061D/U+061E hosts | Node/ada reject (`domainToASCII` empty) |
| `is_known_rust_url_disallowed_idna` | ACE-encodes some UCD `disallowed` cps | Node/ada / sorug reject |
| `is_known_rust_url_bidi_idna` | Looser CheckBidi / Ext-B mixes | Node CheckBidi |
| `is_known_rust_url_zwnj_idna` | Accepts some ZWNJ contexts | CheckJoiners / Node |

Opposite direction (sorug ok / servo err) is also allowlisted when rust-url ICU
rejects ACE that WHATWG `beStrict=false` keeps — see harness `is_idna_error` +
`host_has_ace_label`.

## Not allowlisted (must fix)

| Class | Action |
| --- | --- |
| sorug fails, Node/ada succeed | Algorithm / Punycode / overlay bug → fix + `fuzz_regressions` |
| Long ACE (>128 octets) | Fixed 2026-08-08: growable ACE buffer (`beStrict=false`) |
| Crash / panic / OOB | Always a bug |

## Ops checklist

1. Daily [`fuzz-smoke.yml`](../.github/workflows/fuzz-smoke.yml) green.
2. Weekly [`fuzz-long.yml`](../.github/workflows/fuzz-long.yml) green; triage new artifacts.
3. Weekly [`wpt-freshness.yml`](../.github/workflows/wpt-freshness.yml); open `wpt-freshness` issues stay empty.
4. New stable diffs → minimize → regression test; update this table when adding harness allowlists.
