//! **Two independent transcriptions of `LivingEntity.calculateFallDamage`,
//! compared against each other over a grid.**
//!
//! # Why this exists rather than a merge
//!
//! `origin/player-systems` transcribed `calculateFallPower`,
//! `calculateFallDamage` and `causeFallDamage` into
//! `crates/rustmc-server/src/falldamage.rs`, taking the two attribute values
//! as an explicit [`crate::falldamage::FallContext`] because it had no
//! attribute table wired. `origin/player-movement`, working from the same
//! bytecode a day later and without reading that file, transcribed the same
//! two methods into `crates/rustmc-server/src/fall.rs` reading
//! `safe_fall_distance` and `fall_damage_multiplier` off
//! `data/vanilla/attributes.json`.
//!
//! The obvious tidy-up is to delete one. That would be a mistake, and it is
//! the mistake this project's central defect is made of: a single
//! implementation checked against a second reading of the same `javap` dump
//! **by the same pair of eyes** is a check that cannot fail. Two readings
//! taken independently, compared over a grid, can — and this file is the only
//! place in this crate where a transcription is graded by anything other than
//! its own author's arithmetic.
//!
//! So the duplication is deliberate and this file is its justification. The
//! server uses `fall.rs`'s (see `crate::world_send::apply_landing`); this
//! grades it against `falldamage.rs`'s.
//!
//! # What would make it fail
//!
//! Any disagreement in the epsilon (`1.0E-6`, added before the safe distance
//! is subtracted), in the widening of the `float` multiplier (`f2d` before
//! the multiply, so the whole product is `double`), or in `Mth.floor`'s
//! saturating `d2i`. The negative-distance rows exercise the last of those:
//! `Mth.floor` of a negative power is a negative int, and both must clamp it
//! the same way through the `damage > 0` gate their callers apply.

use rustmc_server::attributes::Attributes;
use rustmc_server::fall::FallDamage;
use rustmc_server::falldamage::{calculate_fall_damage, calculate_fall_power, FallContext};

fn repo(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// The player's two fall attributes, read from the table both ports name.
fn player_attributes() -> (Attributes, f64, f64) {
    let attrs = Attributes::load(&repo("data/vanilla/attributes.json"))
        .expect("data/vanilla/attributes.json — run ci/gen-gameplay-tables.sh");
    let safe = attrs
        .base("minecraft:player", "minecraft:safe_fall_distance")
        .expect("the player has a safe_fall_distance");
    let mult = attrs
        .base("minecraft:player", "minecraft:fall_damage_multiplier")
        .expect("the player has a fall_damage_multiplier");
    (attrs, safe, mult)
}

#[test]
fn the_two_transcriptions_agree_on_every_distance_and_multiplier() {
    let (attrs, safe, mult) = player_attributes();
    let a = FallDamage::for_type(&attrs, "minecraft:player").expect("player has both attributes");
    let ctx = FallContext {
        safe_fall_distance: safe,
        fall_damage_multiplier: mult,
        ..FallContext::default()
    };

    let mut compared = 0_u32;
    let mut nonzero = 0_u32;
    // Every hundredth of a block from -1 to 40, and the five `fallOn`
    // multipliers the jar's overrides actually use.
    let mut d = -100_i32;
    while d <= 4000 {
        let distance = f64::from(d) / 100.0;
        for multiplier in [1.0_f32, 0.2, 0.0, 0.5, 2.0] {
            let from_movement = a.damage(distance, multiplier);
            let from_systems = calculate_fall_damage(distance, multiplier, ctx);
            assert_eq!(
                from_movement, from_systems,
                "fall.rs and falldamage.rs disagree at distance={distance} multiplier={multiplier}"
            );
            compared += 1;
            if from_movement > 0 {
                nonzero += 1;
            }
        }
        d += 1;
    }

    // **The vacuity guard.** Without it this test passes if both functions
    // return zero everywhere — which is exactly what they do with the
    // attribute table absent and `FallContext::default()`'s honest zeroes.
    assert_eq!(compared, 20_505, "the grid changed size");
    assert!(
        nonzero > 5_000,
        "only {nonzero} of {compared} cells produced damage; the grid is not exercising the mechanism"
    );

    // And the power, which is the half the damage floor could be hiding.
    let mut d = 0_i32;
    while d <= 4000 {
        let distance = f64::from(d) / 100.0;
        assert!(
            (a.fall_power(distance) - calculate_fall_power(distance, safe)).abs() < f64::EPSILON,
            "calculateFallPower disagrees at {distance}"
        );
        d += 1;
    }
}

#[test]
fn the_published_table_holds_in_both_and_the_boundary_is_the_epsilon() {
    let (attrs, safe, mult) = player_attributes();
    let a = FallDamage::for_type(&attrs, "minecraft:player").expect("player has both attributes");
    let ctx = FallContext {
        safe_fall_distance: safe,
        fall_damage_multiplier: mult,
        ..FallContext::default()
    };

    // `docs/cold-start/player-movement.md`'s table: 3/4/10/23 -> 0/1/7/20.
    for (distance, want) in [(3.0_f64, 0), (4.0, 1), (5.0, 2), (10.0, 7), (23.0, 20)] {
        assert_eq!(a.damage(distance, 1.0), want, "fall.rs at {distance}");
        assert_eq!(
            calculate_fall_damage(distance, 1.0, ctx),
            want,
            "falldamage.rs at {distance}"
        );
    }

    // The epsilon's own claim, in both: a fall of exactly the safe distance
    // yields `+1e-6`, which floors to 0, and one ulp longer still floors to
    // 0. Removing the epsilon changes neither, which is why it needs saying
    // rather than testing for a damage difference.
    assert!((calculate_fall_power(safe, safe) - 1.0e-6).abs() < 1.0e-12);
    assert_eq!(a.damage(safe, 1.0), 0);
    assert_eq!(calculate_fall_damage(safe, 1.0, ctx), 0);

    // The positive control on the boundary: one whole block past the safe
    // distance is one damage in both.
    assert_eq!(a.damage(safe + 1.0, 1.0), 1);
    assert_eq!(calculate_fall_damage(safe + 1.0, 1.0, ctx), 1);
}
