//! `Beardifier`, against vanilla's own, component by component.
//!
//! Vectors: `crates/rustmc-worldgen/golden/beard_vectors.json`, from
//! `ci/beard-golden.sh`, which runs vanilla's `Beardifier` class directly.
//!
//! # Why this file is four gates and not one
//!
//! Every change in the sequence that led here had a byte-identical gate — a
//! control built from the parent commit, `cmp` on region files, proved
//! neutral. The beard cannot have one once it is fed pieces: it is *meant* to
//! change terrain. What is left is a residual moving in a predicted
//! direction, and a residual cannot say **which** of five pieces is wrong.
//!
//! A kernel wrong in the last bit, a `fastInvSqrt` replaced by `1/sqrt`, a
//! switch arm wired to the wrong `TerrainAdjustment`, an index nest
//! transposed — each of those produces a number that is plausible, different,
//! and impossible to attribute from the composed diff alone. So each is
//! gated against vanilla separately, and the composed world diff is checked
//! afterwards rather than instead.
//!
//! # The blind spot this project just paid for, and how it is avoided here
//!
//! The beardifier was invisible for the whole project because three
//! independent instruments agreed while all three were blind: a datapack grep
//! tested a marker that is never in the datapack, and two fill probes both
//! passed `BeardifierMarker.INSTANCE` on purpose, so vanilla's own `doFill`
//! obligingly ran on a beardless density and agreed line for line.
//!
//! The lesson is that "do the instruments agree?" is a different question
//! from "what would make them all agree while all wrong?", and the answer to
//! the second is: **a chain of gates that terminates in our own code.** So
//! every assertion below terminates in a value produced by vanilla's
//! `Beardifier`, reached by reflection, with no marker anywhere in the path.
//!
//! # What is NOT gated here, stated so nothing quietly stands in for it
//!
//! `Beardifier.forStructuresInChunk` — which pieces a chunk collects, the
//! rigid/non-rigid split, the `groundLevelDelta` each contributes, and the
//! junction window. It needs a `StructureManager`, so it has no oracle in
//! this file. Everything below answers only for the pieces it is *given*.

use rustmc_worldgen::density::beard::{
    beard_contribution, bury_contribution, kernel, Beardifier, Junction, Rigid,
};
use rustmc_worldgen::structure::pieces::BoundingBox;
use rustmc_worldgen::structure::template_pool::TerrainAdaptation;
use std::path::Path;

const VECTORS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/beard_vectors.json");

fn load() -> serde_json::Value {
    let path = Path::new(VECTORS);
    assert!(
        path.exists(),
        "no beard vectors at {VECTORS} — run ci/beard-golden.sh. This gate is \
         not optional: it is the only thing standing between a subtly wrong \
         kernel and a plausible number nobody can attribute."
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("readable")).expect("json")
}

fn i64_of(value: &serde_json::Value) -> i64 {
    value.as_i64().expect("an integer")
}

/// All 13,824 entries, as raw float bits.
///
/// The array is compared in its own flat storage order rather than
/// re-derived index by index, so a port whose values are right and whose
/// index nest is transposed still fails. That is not hypothetical: the loop
/// that builds it feeds its **outer** variable into the *z* term of the value
/// and the *first* factor of the index, which is the kind of thing that
/// survives being read and dies on being compared.
#[test]
fn the_kernel_is_bit_identical_to_vanillas() {
    let doc = load();
    assert_eq!(
        i64_of(&doc["kernel_radius"]),
        12,
        "BEARD_KERNEL_RADIUS moved"
    );
    let want = doc["kernel"].as_array().expect("kernel");
    let ours = kernel();
    assert_eq!(want.len(), ours.len(), "kernel length");

    let mut differ = 0usize;
    let mut first: Option<(usize, i32, i32)> = None;
    for (index, entry) in want.iter().enumerate() {
        #[allow(clippy::cast_possible_truncation)]
        let expected = i64_of(entry) as i32;
        #[allow(clippy::cast_possible_wrap)]
        let got = ours[index].to_bits() as i32;
        if expected != got {
            differ += 1;
            if first.is_none() {
                first = Some((index, expected, got));
            }
        }
    }
    assert_eq!(
        differ,
        0,
        "{differ} of {} kernel entries differ from vanilla's; first at {first:?}. \
         The kernel is `Math.pow(Math.E, -lengthSquared(x, y + 0.5, z) / 16.0)` \
         narrowed to f32 — if only the last bit differs, the suspect is \
         `powf` against the JVM's `Math.pow`, not the formula.",
        want.len()
    );
}

/// `getBeardContribution`, including every out-of-kernel-range case.
///
/// This is the one that gates `fast_inv_sqrt`. Substituting `1.0 / x.sqrt()`
/// leaves every value wrong by roughly a part in a thousand — invisible to a
/// decimal comparison, fatal to a bit one.
#[test]
fn the_beard_contribution_is_bit_identical_to_vanillas() {
    let doc = load();
    let rows = doc["beard"].as_array().expect("beard");
    assert!(rows.len() > 1000, "the sweep should be broad");

    let mut nonzero = 0usize;
    for row in rows {
        let row = row.as_array().expect("a row");
        #[allow(clippy::cast_possible_truncation)]
        let (dx, dy, dz, dg) = (
            i64_of(&row[0]) as i32,
            i64_of(&row[1]) as i32,
            i64_of(&row[2]) as i32,
            i64_of(&row[3]) as i32,
        );
        #[allow(clippy::cast_sign_loss)]
        let want = f64::from_bits(i64_of(&row[4]) as u64);
        let got = beard_contribution(dx, dy, dz, dg);
        assert_eq!(
            got.to_bits(),
            want.to_bits(),
            "getBeardContribution({dx}, {dy}, {dz}, {dg}) = {got} but vanilla says {want}"
        );
        if want != 0.0 {
            nonzero += 1;
        }
    }
    assert!(
        nonzero > 200,
        "only {nonzero} of {} beard vectors are non-zero — a sweep that is \
         almost all zeros gates the guard and nothing else",
        rows.len()
    );
}

#[test]
fn the_bury_contribution_is_bit_identical_to_vanillas() {
    let doc = load();
    let rows = doc["bury"].as_array().expect("bury");
    let mut nonzero = 0usize;
    for row in rows {
        let row = row.as_array().expect("a row");
        #[allow(clippy::cast_sign_loss)]
        let (x, y, z, want) = (
            f64::from_bits(i64_of(&row[0]) as u64),
            f64::from_bits(i64_of(&row[1]) as u64),
            f64::from_bits(i64_of(&row[2]) as u64),
            f64::from_bits(i64_of(&row[3]) as u64),
        );
        let got = bury_contribution(x, y, z);
        assert_eq!(
            got.to_bits(),
            want.to_bits(),
            "getBuryContribution({x}, {y}, {z}) = {got} but vanilla says {want}"
        );
        if want != 0.0 {
            nonzero += 1;
        }
    }
    assert!(nonzero > 50, "only {nonzero} non-zero bury vectors");
}

/// The composed `Beardifier.compute`, for a piece of every
/// `TerrainAdjustment` and for jigsaw junctions.
///
/// This is what catches a switch arm on the wrong enum value. The two
/// switches in `compute` do **not** partition `TerrainAdjustment` the same
/// way — `BURY` shares the first with `BEARD_THIN` and the second with
/// nothing — so a port that reads the method as one switch passes
/// `BEARD_THIN` and fails `BURY`.
#[test]
fn compute_is_bit_identical_to_vanillas_for_every_adjustment() {
    let doc = load();
    let cases = doc["compute"].as_array().expect("compute");
    assert_eq!(cases.len(), 18, "the golden should carry every arm");

    let mut seen: Vec<String> = Vec::new();
    for case in cases {
        let name = case["name"].as_str().expect("a name").to_owned();

        let pieces: Vec<Rigid> = case["pieces"]
            .as_array()
            .expect("pieces")
            .iter()
            .map(|piece| {
                let piece = piece.as_array().expect("a piece");
                #[allow(clippy::cast_possible_truncation)]
                let bounds = BoundingBox::new(
                    i64_of(&piece[0]) as i32,
                    i64_of(&piece[1]) as i32,
                    i64_of(&piece[2]) as i32,
                    i64_of(&piece[3]) as i32,
                    i64_of(&piece[4]) as i32,
                    i64_of(&piece[5]) as i32,
                );
                let adjustment =
                    TerrainAdaptation::parse(piece[6].as_str().expect("an adjustment"))
                        .expect("a known adjustment");
                #[allow(clippy::cast_possible_truncation)]
                Rigid {
                    bounds,
                    adjustment,
                    ground_level_delta: i64_of(&piece[7]) as i32,
                }
            })
            .collect();

        let junctions: Vec<Junction> = case["junctions"]
            .as_array()
            .expect("junctions")
            .iter()
            .map(|junction| {
                let junction = junction.as_array().expect("a junction");
                #[allow(clippy::cast_possible_truncation)]
                Junction {
                    source_x: i64_of(&junction[0]) as i32,
                    source_ground_y: i64_of(&junction[1]) as i32,
                    source_z: i64_of(&junction[2]) as i32,
                }
            })
            .collect();

        let affected = case["affected"].as_array().map(|b| {
            #[allow(clippy::cast_possible_truncation)]
            BoundingBox::new(
                i64_of(&b[0]) as i32,
                i64_of(&b[1]) as i32,
                i64_of(&b[2]) as i32,
                i64_of(&b[3]) as i32,
                i64_of(&b[4]) as i32,
                i64_of(&b[5]) as i32,
            )
        });

        let beardifier = Beardifier::new(pieces, junctions, affected);

        let mut nonzero = 0usize;
        for sample in case["samples"].as_array().expect("samples") {
            let sample = sample.as_array().expect("a sample");
            #[allow(clippy::cast_possible_truncation)]
            let (x, y, z) = (
                i64_of(&sample[0]) as i32,
                i64_of(&sample[1]) as i32,
                i64_of(&sample[2]) as i32,
            );
            #[allow(clippy::cast_sign_loss)]
            let want = f64::from_bits(i64_of(&sample[3]) as u64);
            let got = beardifier.compute(x, y, z);
            assert_eq!(
                got.to_bits(),
                want.to_bits(),
                "case {name}: compute({x}, {y}, {z}) = {got} but vanilla says {want}"
            );
            if want != 0.0 {
                nonzero += 1;
            }
        }

        // A case whose every sample is zero has gated nothing, and `none` is
        // the only adjustment for which that is the right answer. Asserting
        // it here rather than trusting the sweep is the difference between a
        // gate and a formality — the whole reason this term went unnoticed
        // was three instruments agreeing on zero.
        if name.starts_with("none/") {
            assert_eq!(nonzero, 0, "case {name} should be zero everywhere");
        } else {
            assert!(
                nonzero > 20,
                "case {name} produced only {nonzero} non-zero samples — it is \
                 not exercising the arm it exists for"
            );
        }
        seen.push(name);
    }

    for wanted in [
        "none/gld0",
        "bury/gld0",
        "beard_thin/gld0",
        "beard_box/gld0",
        "encapsulate/gld0",
        "two_pieces",
        "junctions_only",
        "mixed",
    ] {
        assert!(
            seen.iter().any(|name| name == wanted),
            "the golden is missing the {wanted} case"
        );
    }
}
