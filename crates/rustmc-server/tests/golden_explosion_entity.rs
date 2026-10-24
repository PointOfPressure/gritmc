//! Vanilla's own `getSeenPercent` grid and `getEntityDamageAmount`, against ours.
//!
//! `data/vanilla/explosion_entity.json` is recorded by
//! `ci/explosion-entity-golden.sh`, which allocates a `ServerLevel` subclass
//! and an `Entity` subclass without their constructors and overrides
//! `Level.clip` to **record** the origin it was asked about. So the sample
//! grid in that file is the one vanilla built, in the order it built it.
//!
//! # The split this file exists to hold
//!
//! The entity half of an explosion is arithmetic plus one raycast. The
//! arithmetic — the grid, the fraction, the damage formula, the knockback
//! scale — is ported and is compared here against vanilla's own output. The
//! raycast is `Level.clip`, which this workspace has no counterpart for at
//! all, so the predicate is injected and the golden's three player cases
//! (all-MISS, every-third-HIT, all-HIT) drive it from the recorded
//! `hitEvery`. That is the whole of the refusal: not "we approximate the
//! raycast" but "we reproduce everything around it and the raycast is
//! somebody else's item".
//!
//! Every coordinate is compared as raw bits. A decimal rendering round-trips
//! only through a correctly rounded parser and a reader cannot see a half-ulp.

use rustmc_server::explosion::{entity_damage_amount, seen_percent, seen_percent_samples};
use std::path::{Path, PathBuf};

/// The whole path in one literal, so `ci/gen-gameplay-tables.sh`'s grep sees
/// it and `ci/check-generator-arms.sh` finds the arm reached.
const ENTITY_TABLE: &str = "data/vanilla/explosion_entity.json";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn doc() -> Option<serde_json::Value> {
    let p = repo_root().join(ENTITY_TABLE);
    if !rustmc_data::fixture::available(
        Path::new(&p),
        ENTITY_TABLE,
        "ci/explosion-entity-golden.sh",
    ) {
        return None;
    }
    Some(serde_json::from_str(&std::fs::read_to_string(&p).expect("read")).expect("json"))
}

#[allow(clippy::cast_sign_loss)]
fn f64s(v: &serde_json::Value) -> Vec<f64> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|x| f64::from_bits(x.as_i64().expect("bits") as u64))
        .collect()
}

#[test]
fn our_seen_percent_grid_is_vanillas_sample_for_sample() {
    let Some(doc) = doc() else { return };
    let cases = doc["seenCases"].as_array().expect("seenCases");
    assert!(!cases.is_empty(), "no cases — the golden is empty");
    let mut compared = 0usize;
    let mut samples_seen = 0usize;

    for c in cases {
        let name = c["name"].as_str().expect("name");
        let mn = f64s(&c["minBits"]);
        let mx = f64s(&c["maxBits"]);
        let want = f64s(&c["origins"]);

        let got = seen_percent_samples((mn[0], mn[1], mn[2]), (mx[0], mx[1], mx[2]));
        assert_eq!(
            got.len() * 3,
            want.len(),
            "{name}: {} samples against vanilla's {}",
            got.len(),
            want.len() / 3
        );
        for (i, s) in got.iter().enumerate() {
            assert_eq!(
                (s.0.to_bits(), s.1.to_bits(), s.2.to_bits()),
                (
                    want[i * 3].to_bits(),
                    want[i * 3 + 1].to_bits(),
                    want[i * 3 + 2].to_bits()
                ),
                "{name}: sample {i} diverged"
            );
        }

        // The fraction, driven by the same clip pattern the dumper's stub used:
        // HIT on every `hitEvery`-th call, in call order, and never when it is 0.
        let hit_every = c["hitEvery"].as_i64().expect("hitEvery");
        let mut calls = 0i64;
        let pct = seen_percent(&got, |_| {
            calls += 1;
            !(hit_every > 0 && calls % hit_every == 0)
        });
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let want_pct =
            f32::from_bits(c["seenPercentBits"].as_i64().expect("seenPercentBits") as u32);
        assert_eq!(
            pct.to_bits(),
            want_pct.to_bits(),
            "{name}: seen percent {pct} against vanilla's {want_pct}"
        );

        compared += 1;
        samples_seen += got.len();
    }

    // The instrument's own floor: the assertions above are vacuous over an
    // empty case list or an empty grid, so the totals are asserted against
    // what `ci/explosion-entity-golden.sh` printed when the table was recorded.
    assert_eq!(compared, 7, "the golden lost a case");
    assert_eq!(samples_seen, 364, "the golden's total sample count moved");
}

#[test]
fn our_damage_formula_is_vanillas_bit_for_bit() {
    let Some(doc) = doc() else { return };
    let cases = doc["damageCases"].as_array().expect("damageCases");
    assert!(!cases.is_empty(), "no cases — the golden is empty");
    let mut compared = 0usize;
    let mut inside = 0usize;
    let mut outside_negative = 0usize;

    for c in cases {
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let radius = f32::from_bits(c["radiusBits"].as_i64().expect("radiusBits") as u32);
        #[allow(clippy::cast_sign_loss)]
        let distance = f64::from_bits(c["distanceBits"].as_i64().expect("distanceBits") as u64);
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let seen = f32::from_bits(c["seenBits"].as_i64().expect("seenBits") as u32);
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let want = f32::from_bits(c["damageBits"].as_i64().expect("damageBits") as u32);

        let got = entity_damage_amount(radius, distance, seen);
        assert_eq!(
            got.to_bits(),
            want.to_bits(),
            "r={radius} d={distance} seen={seen}: {got} against vanilla's {want}"
        );

        // `getKnockbackMultiplier` on the base calculator is `fconst_1`.
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let kb = f32::from_bits(
            c["knockbackMultiplierBits"]
                .as_i64()
                .expect("knockbackMultiplierBits") as u32,
        );
        assert!(
            (kb - 1.0).abs() < f32::EPSILON,
            "the base calculator's knockback multiplier is {kb}"
        );

        if distance <= 2.0 * f64::from(radius) {
            inside += 1;
        } else if got < 0.0 {
            outside_negative += 1;
        }
        compared += 1;
    }

    assert_eq!(compared, 140, "the golden lost a damage case");
    assert_eq!(inside, 120, "the inside-the-guard population moved");
    // Vanilla's formula goes NEGATIVE past `d0 > 1.0`, and `hurtEntities`
    // never asks because it `continue`s at offsets 214-221. Asserted rather
    // than clamped: a clamp here would disagree with vanilla for any caller
    // that ever did ask.
    // 12 of the 20, not all of them: `seen == 0.0` collapses `d1` to zero and
    // the `+ 1.0` survives whatever the distance, and three more cases just
    // past the boundary are still held up by the quadratic term. Measured,
    // 2026-09-06.
    assert_eq!(
        outside_negative, 12,
        "the formula stopped going negative outside the d0 <= 1 guard"
    );
}
