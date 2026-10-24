//! Every committed golden double that `serde_json` cannot read back exactly.
//!
//! # The defect
//!
//! `serde_json`'s default number parser is **not correctly rounded**. Reading
//! a 17-significant-digit decimal back can land one ULP away from the double
//! that wrote it, and no amount of extra digits fixes it — the feature that
//! does is `float_roundtrip`, which this workspace does not enable.
//!
//! Found 2026-09-05 while building `crates/rustmc-server/golden/world_border.json`:
//! the text `14.600000000000001` (bits `4624408687374709556`) came back from
//! `as_f64()` as `14.6` (bits `…555`), and a bit-exact comparison against
//! vanilla's own output failed on a value this crate had written correctly.
//!
//! It is a quiet defect of exactly the kind this project keeps finding. A
//! golden is the strongest evidence here, a comparison against one reads as
//! authoritative, and a reader that is one ULP out turns a *passing* check
//! into a check of the wrong number — or, worse, makes an author widen a
//! tolerance to accommodate a parser.
//!
//! # What this test does, and what it deliberately does not
//!
//! It walks every committed `golden/` fixture — **not** `data/vanilla`, which
//! is generated per tree and absent from a clean clone, so an allowance row
//! for it would be stale in exactly the trees that have not generated it.
//! (Measured anyway on 2026-09-05: `attributes.json` carries 7, read through
//! `defaultBits`.) It finds every decimal literal and parses each one **both
//! ways** — through `serde_json` and through Rust's own `str::parse::<f64>()`,
//! which *is* correctly rounded — and counts the disagreements.
//!
//! It **ratchets** rather than demanding zero. Making it zero would mean
//! rewriting other lanes' goldens, and the right fix for a golden is the one
//! `world_border.json` took: carry the raw bit pattern beside the decimal and
//! never compare the text. So this records the status quo, names the files, and
//! refuses growth. A new golden with an unreadable double reddens here on the
//! commit that adds it, which is the day it is cheap to fix.
//!
//! # This test can go red
//!
//! [`the_check_can_fail`] constructs the known bad literal and asserts the two
//! parsers disagree on it. If a future `serde_json` fixes its parser, that
//! assertion fails and this whole file becomes retirable — which is the
//! outcome to want, and it says so rather than quietly passing forever.

use std::path::{Path, PathBuf};

/// Files allowed to contain a decimal `serde_json` cannot read back exactly,
/// with the count measured on 2026-09-05.
///
/// **A number here is a liability, not a budget.** Each one is a golden whose
/// decimal a reader may misread; the fix is to carry
/// `Double.doubleToRawLongBits` beside it, as `world_border.json` does, and to
/// compare the bits.
const ALLOWED: &[(&str, usize)] = &[
    // Every one of these six was checked on 2026-09-05, and the news is good:
    // **no reader in this workspace parses any of these decimals.** The
    // project had already converged on carrying the bits, independently and
    // in three different spellings, before this defect was named:
    //
    //   * `golden_rng.rs:135` — "Compare IEEE-754 bits, never the decimal
    //     repr" — reads `output_bits`;
    //   * `golden_noise.rs:5` — "Everything is compared as raw IEEE-754 bits"
    //     — reads `output_bits` as strings;
    //   * `crates/rustmc-server/src/attributes.rs:179` — a `bits()` helper
    //     over `defaultBits`/`minBits`/`maxBits`, with a doc comment at :44
    //     saying "the bits are what is read".
    //
    // So the decimals in these files are for a human, and this ratchet exists
    // to keep it that way rather than to record a debt. A NEW golden that
    // carries a misreadable decimal is not covered by any of that convention
    // and reddens here on the commit that adds it.
    ("crates/rustmc-server/golden/world_border.json", 2),
    ("crates/rustmc-worldgen/golden/legacy_lcg_derived.json", 12),
    ("crates/rustmc-worldgen/golden/noise_vectors.json", 5),
    (
        "crates/rustmc-worldgen/golden/xoroshiro128pp_derived.json",
        10,
    ),
    ("crates/rustmc-worldgen/golden/xwr_derived.json", 9),
    // Generated per tree by `ci/attributes-golden.sh`, so this row is about a
    // file a clean clone does not have; the test simply finds no offender
    // there and the stale-allowance rule below would then fire. That is why
    // it is not in the list — see the comment on `data/vanilla` in the test.
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every decimal literal in a JSON document, as text.
fn decimals(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut in_string = false;
    while i < b.len() {
        let c = b[i];
        if in_string {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_string = true;
            i += 1;
            continue;
        }
        if c == b'-' || c.is_ascii_digit() {
            let start = i;
            if c == b'-' {
                i += 1;
            }
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            let mut is_decimal = false;
            if i < b.len() && b[i] == b'.' {
                is_decimal = true;
                i += 1;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
                is_decimal = true;
                i += 1;
                if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
                    i += 1;
                }
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if is_decimal {
                out.push(text[start..i].to_string());
            }
            continue;
        }
        i += 1;
    }
    out
}

/// True when `serde_json` and Rust's own parser disagree about a literal.
fn misparsed(lit: &str) -> bool {
    let Ok(exact) = lit.parse::<f64>() else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(lit) else {
        return false;
    };
    let Some(got) = v.as_f64() else {
        return false;
    };
    got.to_bits() != exact.to_bits()
}

fn json_files() -> Vec<PathBuf> {
    let root = repo_root();
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root.join("crates")) {
        for e in entries.flatten() {
            let g = e.path().join("golden");
            if g.is_dir() {
                roots.push(g);
            }
        }
    }
    let mut out = Vec::new();
    while let Some(dir) = roots.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                roots.push(p);
            } else if p.extension().is_some_and(|x| x == "json") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn rel(p: &Path) -> String {
    let root = repo_root();
    p.strip_prefix(&root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn no_committed_golden_grows_an_unreadable_double() {
    let files = json_files();
    assert!(
        files.len() > 20,
        "only {} json files found — the walk is not reaching the goldens",
        files.len()
    );

    let mut offenders: Vec<(String, usize, String)> = Vec::new();
    let mut literals = 0usize;
    for p in &files {
        let Ok(text) = std::fs::read_to_string(p) else {
            continue;
        };
        let lits = decimals(&text);
        literals += lits.len();
        let mut bad = 0usize;
        let mut example = String::new();
        for l in lits {
            if misparsed(&l) {
                bad += 1;
                if example.is_empty() {
                    example = l;
                }
            }
        }
        if bad > 0 {
            offenders.push((rel(p), bad, example));
        }
    }

    eprintln!(
        "golden float precision: {} files, {literals} decimal literals, \
         {} files carrying a double serde_json cannot read back",
        files.len(),
        offenders.len()
    );
    for (f, n, e) in &offenders {
        eprintln!("  {n:>5} misread  e.g. {e:<24} {f}");
    }

    let allowed: std::collections::BTreeMap<&str, usize> = ALLOWED.iter().copied().collect();
    let mut failures = Vec::new();
    for (f, n, e) in &offenders {
        match allowed.get(f.as_str()) {
            None => failures.push(format!(
                "{f}: {n} double(s) serde_json cannot read back exactly (e.g. {e}). \
                 Carry the raw bit pattern beside the decimal and compare the bits — \
                 see crates/rustmc-server/golden/world_border.json — or add this file \
                 to ALLOWED with its count and a reason."
            )),
            Some(&cap) if *n > cap => {
                failures.push(format!("{f}: {n} misread doubles, allowance is {cap}"))
            }
            Some(_) => {}
        }
    }
    // A stale allowance is its own defect: a file that was fixed and left in
    // the list makes the next addition free. `data/vanilla` is exempt from
    // that rule and from the list — it is generated per tree by
    // `ci/gen-gameplay-tables.sh` and a clean clone has none of it, so a row
    // for `attributes.json` would be stale in exactly the trees that have not
    // generated it. It carries 7 misread doubles when present, and
    // `attributes.rs` reads them through `defaultBits`.
    for (f, cap) in &allowed {
        if !offenders.iter().any(|(o, _, _)| o == f) {
            failures.push(format!(
                "{f} is in ALLOWED at {cap} and carries no misread double — remove the row"
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The check can fail, demonstrated on the literal that found the defect.
#[test]
fn the_check_can_fail() {
    // `world_border.json`'s `shrink-t19` size, before that golden started
    // carrying bit patterns.
    let lit = "14.600000000000001";
    assert_eq!(
        lit.parse::<f64>().unwrap().to_bits(),
        4_624_408_687_374_709_556
    );
    assert!(
        misparsed(lit),
        "serde_json now reads this correctly — its parser has been fixed, \
         and this whole test can be retired"
    );
    // And a literal both parsers agree on, so `misparsed` is not simply true.
    assert!(!misparsed("14.6"));
    assert!(!misparsed("0.5"));
    assert!(!misparsed("1.0E10"));

    // The scanner finds decimals and not integers, and ignores anything inside
    // a string — a golden's hex payloads are strings full of digits.
    let doc = r#"{"a": 1, "b": 2.5, "c": "3.75", "d": -1.0e-3}"#;
    assert_eq!(decimals(doc), ["2.5", "-1.0e-3"]);
}
