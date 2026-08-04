//! Build script for `sorug`.
//!
//! 1. Ensures WPT fixtures exist (download if missing).
//! 2. Derives IDNA membership tables from **vendored** Unicode UCD files under
//!    `data/ucd/` (pinned in `UNICODE_VERSION`), then merges `data/idna_overlay.txt`
//!    for Node/ICU / WPT deltas.
//! 3. Emits `$OUT_DIR/idna_tables.rs` as sorted, merged `(u32, u32)` slices.
//!
//! No network access for UCD. Refresh with `./scripts/refresh-ucd.sh`.

use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const WPT_URL: &str =
    "https://raw.githubusercontent.com/web-platform-tests/wpt/master/url/resources/urltestdata.json";
const WPT_PATH: &str = "tests/urltestdata.json";
const WPT_SETTERS_URL: &str =
    "https://raw.githubusercontent.com/web-platform-tests/wpt/master/url/resources/setters_tests.json";
const WPT_SETTERS_PATH: &str = "tests/setters_tests.json";

const UCD_DIR: &str = "data/ucd";
const OVERLAY_PATH: &str = "data/idna_overlay.txt";

/// Known table sections → Rust static identifiers in the generated file.
const SECTIONS: &[(&str, &str)] = &[
    ("bidi_ignored", "BIDI_IGNORED"),
    ("joining_letters", "JOINING_LETTERS"),
    ("rtl_non_letters", "RTL_NON_LETTERS"),
    ("rtl_alphabetic", "RTL_ALPHABETIC"),
    ("bidi_numbers", "BIDI_NUMBERS"),
    ("legacy_arabic", "LEGACY_ARABIC"),
    ("disallowed", "DISALLOWED"),
    ("ltr_letters", "LTR_LETTERS"),
    ("arabic_ext_b_letters", "ARABIC_EXT_B_LETTERS"),
    ("arabic_ext_b_compatible", "ARABIC_EXT_B_COMPATIBLE"),
    ("arabic_ext_b_era_marks", "ARABIC_EXT_B_ERA_MARKS"),
    ("arabic_ext_b_era_ok", "ARABIC_EXT_B_ERA_OK"),
    ("nfkc_via_space", "NFKC_VIA_SPACE"),
    ("uts46_ignored", "UTS46_IGNORED"),
    ("uts46_needs_map", "UTS46_NEEDS_MAP"),
];

/// Scripts treated as strong RTL alphabetic blocks (gated with `is_alphabetic` at runtime).
const RTL_SCRIPTS: &[&str] = &[
    "Arabic",
    "Hebrew",
    "Syriac",
    "Thaana",
    "Nko",
    "Samaritan",
    "Mandaic",
    "Phoenician",
    "Lydian",
    "Carian",
    "Lycian",
    "Old_South_Arabian",
    "Old_North_Arabian",
    "Imperial_Aramaic",
    "Palmyrene",
    "Nabataean",
    "Hatran",
    "Manichaean",
    "Avestan",
    "Inscriptional_Parthian",
    "Inscriptional_Pahlavi",
    "Psalter_Pahlavi",
    "Old_Turkic",
    "Old_Hungarian",
    "Hanifi_Rohingya",
    "Yezidi",
    "Adlam",
    "Chorasmian",
    "Elymaic",
    "Sogdian",
    "Old_Sogdian",
    "Kharoshthi",
    "Meroitic_Cursive",
    "Meroitic_Hieroglyphs",
    "Cypriot",
    "Mende_Kikakui",
];

/// Scripts for LTR letter blocks (gated with `is_alphabetic` / ASCII).
const LTR_SCRIPTS: &[&str] = &["Latin", "Greek", "Cyrillic", "Coptic", "Glagolitic"];

fn main() {
    ensure_wpt_fixture(WPT_PATH, WPT_URL);
    ensure_wpt_fixture(WPT_SETTERS_PATH, WPT_SETTERS_URL);
    generate_idna_tables();
}

// ---------------------------------------------------------------------------
// WPT fixtures
// ---------------------------------------------------------------------------

fn ensure_wpt_fixture(path: &str, url: &str) {
    println!("cargo:rerun-if-changed={path}");

    let path_buf = Path::new(path);
    if path_buf.is_file() {
        return;
    }

    if let Some(parent) = path_buf.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|e| {
            panic!("failed to create {}: {e}", parent.display());
        });
    }

    eprintln!("cargo:warning=downloading WPT fixture → {path}");

    let status = Command::new("curl")
        .args(["-fsSL", "-o", path, url])
        .status()
        .unwrap_or_else(|e| {
            panic!(
                "failed to run curl ({e}). Install curl or place {path} manually from:\n  {url}"
            );
        });

    assert!(
        status.success(),
        "curl failed downloading {path} (status {status}). Source:\n  {url}"
    );
    assert!(
        path_buf.is_file(),
        "download reported success but {path} is missing"
    );
}

// ---------------------------------------------------------------------------
// IDNA range tables (UCD + overlay)
// ---------------------------------------------------------------------------

fn generate_idna_tables() {
    let ucd = Path::new(UCD_DIR);
    println!("cargo:rerun-if-changed={UCD_DIR}/UNICODE_VERSION");
    println!("cargo:rerun-if-changed={UCD_DIR}/IdnaMappingTable.txt");
    println!("cargo:rerun-if-changed={UCD_DIR}/DerivedBidiClass.txt");
    println!("cargo:rerun-if-changed={UCD_DIR}/Scripts.txt");
    println!("cargo:rerun-if-changed={UCD_DIR}/DerivedJoiningType.txt");
    println!("cargo:rerun-if-changed={OVERLAY_PATH}");
    println!("cargo:rerun-if-changed=build.rs");

    let version = fs::read_to_string(ucd.join("UNICODE_VERSION"))
        .unwrap_or_else(|e| panic!("missing {UCD_DIR}/UNICODE_VERSION: {e}"))
        .trim()
        .to_string();

    let mut tables = derive_from_ucd(ucd);
    merge_overlay(&mut tables, Path::new(OVERLAY_PATH));
    validate_required_sections(&tables);

    let mut rust = String::with_capacity(64 * 1024);
    let _ = writeln!(
        rust,
        "// @generated by build.rs from vendored UCD {version} + {OVERLAY_PATH} — do not edit."
    );
    rust.push_str("// Inclusive Unicode scalar ranges as (lo, hi) pairs, sorted & merged.\n");
    rust.push_str("// Lookups use `unicode_ranges::in_range_table` (binary search, no alloc).\n\n");

    for &(section, rust_name) in SECTIONS {
        let ranges = tables.get(section).map_or(&[][..], Vec::as_slice);
        write_static_table(&mut rust, rust_name, ranges);
    }

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let dest = out_dir.join("idna_tables.rs");
    fs::write(&dest, rust).unwrap_or_else(|e| {
        panic!("failed to write {}: {e}", dest.display());
    });
}

fn derive_from_ucd(ucd: &Path) -> BTreeMap<String, Vec<(u32, u32)>> {
    let mut tables: BTreeMap<String, Vec<(u32, u32)>> = BTreeMap::new();

    let idna = parse_idna_mapping_table(&read_ucd(ucd, "IdnaMappingTable.txt"));
    tables.insert("disallowed".into(), merge_ranges(idna.disallowed));
    tables.insert("uts46_ignored".into(), merge_ranges(idna.ignored));
    tables.insert("uts46_needs_map".into(), merge_ranges(idna.needs_map));
    tables.insert("nfkc_via_space".into(), merge_ranges(idna.nfkc_via_space));

    let bidi = parse_property_file(&read_ucd(ucd, "DerivedBidiClass.txt"));
    // Seed empty: Node-aligned CheckBidi mark set lives in idna_overlay.txt
    // (full UCD NSM is broader and flips several Node corpus cases).
    tables.insert("bidi_ignored".into(), Vec::new());
    tables.insert(
        "bidi_numbers".into(),
        merge_ranges(collect_prop(&bidi, &["EN", "AN"])),
    );

    let scripts = parse_property_file(&read_ucd(ucd, "Scripts.txt"));
    tables.insert(
        "rtl_alphabetic".into(),
        merge_ranges(collect_prop(&scripts, RTL_SCRIPTS)),
    );
    tables.insert(
        "ltr_letters".into(),
        merge_ranges(collect_prop(&scripts, LTR_SCRIPTS)),
    );

    let joining = parse_property_file(&read_ucd(ucd, "DerivedJoiningType.txt"));
    // Dual / Right / Left / Join_Causing — runtime still gates with is_alphabetic().
    tables.insert(
        "joining_letters".into(),
        merge_ranges(collect_prop(&joining, &["D", "R", "L", "C"])),
    );

    // Overlay-only sections start empty (filled by merge_overlay).
    for key in [
        "rtl_non_letters",
        "legacy_arabic",
        "arabic_ext_b_letters",
        "arabic_ext_b_compatible",
        "arabic_ext_b_era_marks",
        "arabic_ext_b_era_ok",
    ] {
        tables.entry(key.into()).or_default();
    }

    tables
}

fn read_ucd(ucd: &Path, name: &str) -> String {
    let path = ucd.join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "failed to read {}: {e}\nRun ./scripts/refresh-ucd.sh to vendor UCD files.",
            path.display()
        );
    })
}

struct IdnaDerived {
    disallowed: Vec<(u32, u32)>,
    ignored: Vec<(u32, u32)>,
    needs_map: Vec<(u32, u32)>,
    nfkc_via_space: Vec<(u32, u32)>,
}

/// WHATWG URL: UseSTD3ASCIIRules=false, Transitional_Processing=false.
///
/// - `disallowed` → reject
/// - `ignored` → delete in map
/// - `mapped` / `disallowed_STD3_mapped` → needs map pass
/// - `disallowed_STD3_valid` → treat as valid (not disallowed)
/// - `deviation` → non-transitional keep (ß, final sigma, ZWJ/ZWNJ) — not needs_map
fn parse_idna_mapping_table(text: &str) -> IdnaDerived {
    let mut out = IdnaDerived {
        disallowed: Vec::new(),
        ignored: Vec::new(),
        needs_map: Vec::new(),
        nfkc_via_space: Vec::new(),
    };

    for (lineno, raw) in text.lines().enumerate() {
        let line = strip_ucd_comment(raw).trim();
        if line.is_empty() || line.starts_with('@') {
            continue;
        }
        let fields: Vec<&str> = line.split(';').map(str::trim).collect();
        if fields.len() < 2 {
            panic!("IdnaMappingTable.txt:{}: expected status field: {raw}", lineno + 1);
        }
        let (lo, hi) = parse_ucd_codepoints(fields[0]).unwrap_or_else(|e| {
            panic!("IdnaMappingTable.txt:{}: {e}: {}", lineno + 1, fields[0]);
        });
        let status = fields[1];
        let mapping = fields.get(2).copied().unwrap_or("");

        match status {
            "disallowed" => out.disallowed.push((lo, hi)),
            "ignored" => {
                out.ignored.push((lo, hi));
                // Map pass must run so `map_char` can delete ignored CPs (WHATWG ToASCII).
                out.needs_map.push((lo, hi));
            }
            "mapped" | "disallowed_STD3_mapped" => {
                out.needs_map.push((lo, hi));
                if mapping_starts_with_space(mapping) {
                    out.nfkc_via_space.push((lo, hi));
                }
            }
            "disallowed_STD3_valid" | "valid" | "deviation" => {}
            other => panic!(
                "IdnaMappingTable.txt:{}: unknown status {other}",
                lineno + 1
            ),
        }
    }
    out
}

fn mapping_starts_with_space(mapping: &str) -> bool {
    let mapping = mapping.trim();
    if mapping.is_empty() {
        return false;
    }
    mapping
        .split_whitespace()
        .next()
        .is_some_and(|tok| matches!(tok.to_ascii_uppercase().as_str(), "0020" | "20"))
}

/// property_name → list of inclusive ranges.
fn parse_property_file(text: &str) -> BTreeMap<String, Vec<(u32, u32)>> {
    let mut map: BTreeMap<String, Vec<(u32, u32)>> = BTreeMap::new();
    for (lineno, raw) in text.lines().enumerate() {
        let line = strip_ucd_comment(raw).trim();
        if line.is_empty() || line.starts_with('@') {
            continue;
        }
        let fields: Vec<&str> = line.split(';').map(str::trim).collect();
        if fields.len() < 2 {
            panic!("UCD property file:{}: bad line: {raw}", lineno + 1);
        }
        let (lo, hi) = parse_ucd_codepoints(fields[0]).unwrap_or_else(|e| {
            panic!("UCD property file:{}: {e}: {}", lineno + 1, fields[0]);
        });
        // Second field may be "Arab # Lo" style — take first token.
        let prop = fields[1]
            .split_whitespace()
            .next()
            .unwrap_or(fields[1])
            .to_string();
        map.entry(prop).or_default().push((lo, hi));
    }
    map
}

fn collect_prop(map: &BTreeMap<String, Vec<(u32, u32)>>, names: &[&str]) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for name in names {
        if let Some(ranges) = map.get(*name) {
            out.extend(ranges.iter().copied());
        }
    }
    out
}

fn parse_ucd_codepoints(token: &str) -> Result<(u32, u32), &'static str> {
    let token = token.trim();
    if let Some((a, b)) = token.split_once("..") {
        Ok((parse_hex(a.trim())?, parse_hex(b.trim())?))
    } else {
        let cp = parse_hex(token)?;
        Ok((cp, cp))
    }
}

fn strip_ucd_comment(line: &str) -> &str {
    match line.find('#') {
        Some(i) => &line[..i],
        None => line,
    }
}

fn merge_overlay(tables: &mut BTreeMap<String, Vec<(u32, u32)>>, path: &Path) {
    let text = fs::read_to_string(path).unwrap_or_else(|e| {
        panic!("failed to read {}: {e}", path.display());
    });
    let overlay = parse_overlay_file(&text);
    for (section, ops) in overlay {
        let entry = tables.entry(section.clone()).or_default();
        let mut adds = Vec::new();
        let mut removes = Vec::new();
        for op in ops {
            match op {
                OverlayOp::Add(lo, hi) => adds.push((lo, hi)),
                OverlayOp::Remove(lo, hi) => removes.push((lo, hi)),
            }
        }
        if !adds.is_empty() {
            entry.extend(adds);
            *entry = merge_ranges(std::mem::take(entry));
        }
        if !removes.is_empty() {
            *entry = subtract_ranges(std::mem::take(entry), merge_ranges(removes));
        }
    }
}

#[derive(Clone, Debug)]
enum OverlayOp {
    Add(u32, u32),
    Remove(u32, u32),
}

fn parse_overlay_file(text: &str) -> BTreeMap<String, Vec<OverlayOp>> {
    let mut tables: BTreeMap<String, Vec<OverlayOp>> = BTreeMap::new();
    let mut current: Option<String> = None;

    for (lineno, raw) in text.lines().enumerate() {
        let line_no = lineno + 1;
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }

        if let Some(name) = parse_section_header(line) {
            assert!(
                is_ident(&name),
                "{OVERLAY_PATH}:{line_no}: invalid section name [{name}]"
            );
            tables.entry(name.clone()).or_default();
            current = Some(name);
            continue;
        }

        let Some(section) = current.as_ref() else {
            panic!("{OVERLAY_PATH}:{line_no}: range outside any [section]: {line}");
        };

        let (remove, token) = if let Some(rest) = line.strip_prefix('-') {
            (true, rest.trim())
        } else {
            (false, line)
        };
        let (lo, hi) = parse_range_token(token).unwrap_or_else(|e| {
            panic!("{OVERLAY_PATH}:{line_no}: {e}: {line}");
        });
        assert!(lo <= hi, "{OVERLAY_PATH}:{line_no}: inverted range");
        assert!(hi <= 0x10_FFFF, "{OVERLAY_PATH}:{line_no}: past U+10FFFF");
        let op = if remove {
            OverlayOp::Remove(lo, hi)
        } else {
            OverlayOp::Add(lo, hi)
        };
        tables.get_mut(section).unwrap().push(op);
    }
    tables
}

/// Subtract merged `remove` ranges from merged `base` (both inclusive, sorted).
fn subtract_ranges(base: Vec<(u32, u32)>, remove: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    if base.is_empty() || remove.is_empty() {
        return base;
    }
    let mut out = Vec::with_capacity(base.len());
    let mut ri = 0usize;
    for (lo, hi) in base {
        while ri < remove.len() && remove[ri].1 < lo {
            ri += 1;
        }
        let mut i = ri;
        let mut cursor = lo;
        while i < remove.len() && remove[i].0 <= hi {
            let (rlo, rhi) = remove[i];
            if rlo > cursor {
                out.push((cursor, (rlo - 1).min(hi)));
            }
            if rhi >= hi {
                cursor = hi.saturating_add(1);
                break;
            }
            cursor = rhi.saturating_add(1);
            i += 1;
        }
        if cursor <= hi {
            out.push((cursor, hi));
        }
    }
    merge_ranges(out)
}

fn validate_required_sections(tables: &BTreeMap<String, Vec<(u32, u32)>>) {
    for &(section, _) in SECTIONS {
        let Some(ranges) = tables.get(section) else {
            panic!("IDNA tables: missing required section [{section}]");
        };
        assert!(
            !ranges.is_empty(),
            "IDNA tables: section [{section}] is empty after UCD+overlay"
        );
    }
    for name in tables.keys() {
        assert!(
            SECTIONS.iter().any(|(s, _)| s == name),
            "IDNA tables: unknown section [{name}]; known: {}",
            SECTIONS
                .iter()
                .map(|(s, _)| *s)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}

fn write_static_table(out: &mut String, name: &str, ranges: &[(u32, u32)]) {
    let cps: u64 = ranges
        .iter()
        .map(|&(lo, hi)| u64::from(hi.saturating_sub(lo).saturating_add(1)))
        .sum();
    let _ = writeln!(
        out,
        "/// Inclusive `(lo, hi)` ranges — {} entries, {cps} code points.",
        ranges.len(),
    );
    let _ = writeln!(out, "pub static {name}: &[(u32, u32)] = &[");
    for &(lo, hi) in ranges {
        let _ = writeln!(out, "    (0x{lo:04X}, 0x{hi:04X}),");
    }
    let _ = writeln!(out, "];\n");
}

fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(i) => &line[..i],
        None => line,
    }
}

fn parse_section_header(line: &str) -> Option<String> {
    let line = line.trim();
    if line.starts_with('[') && line.ends_with(']') && line.len() >= 3 {
        Some(line[1..line.len() - 1].trim().to_string())
    } else {
        None
    }
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn parse_range_token(token: &str) -> Result<(u32, u32), &'static str> {
    if let Some((a, b)) = token.split_once("..=") {
        let lo = parse_hex(a.trim())?;
        let hi = parse_hex(b.trim())?;
        Ok((lo, hi))
    } else if token.contains("..") {
        Err("use inclusive `HEX..=HEX` (not exclusive `..`)")
    } else {
        let cp = parse_hex(token)?;
        Ok((cp, cp))
    }
}

fn parse_hex(s: &str) -> Result<u32, &'static str> {
    if s.is_empty() || s.len() > 6 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("expected 1–6 hex digits");
    }
    u32::from_str_radix(s, 16).map_err(|_| "invalid hex")
}

/// Sort by `lo`, then coalesce overlapping / adjacent inclusive ranges.
fn merge_ranges(mut ranges: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    if ranges.is_empty() {
        return ranges;
    }
    ranges.sort_unstable_by_key(|&(lo, _)| lo);
    let mut out = Vec::with_capacity(ranges.len());
    let mut cur = ranges[0];
    for &(lo, hi) in &ranges[1..] {
        if lo <= cur.1.saturating_add(1) {
            cur.1 = cur.1.max(hi);
        } else {
            out.push(cur);
            cur = (lo, hi);
        }
    }
    out.push(cur);
    debug_assert!(is_strictly_sorted_disjoint(&out));
    out
}

fn is_strictly_sorted_disjoint(ranges: &[(u32, u32)]) -> bool {
    ranges
        .windows(2)
        .all(|w| w[0].0 <= w[0].1 && w[0].1.saturating_add(1) < w[1].0)
}
