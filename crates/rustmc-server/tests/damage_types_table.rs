//! `crate::damage_types::DamageTypeRegistry` against the real
//! `data/vanilla/damage_types.json`, and the finding it exists to keep true.
//!
//! # The finding
//!
//! `DamageTypeTags` declares 35 `TagKey`s; the jar ships 34 tag files. The one
//! with no file is `minecraft:bypasses_cooldown`, and a `TagKey` with no data
//! file is an **empty tag** — so `DamageSource.is(BYPASSES_COOLDOWN)` is false
//! for all 51 damage types and the `&& !source.is(BYPASSES_COOLDOWN)` conjunct
//! at `LivingEntity.hurtServer` offset 196 is dead in 26.2.
//!
//! `crate::hurt` transcribes the conjunct anyway — fidelity is not deferred,
//! and a datapack can populate the tag. What this file does is stop the
//! *finding* going stale silently: if a later version ships the file, the
//! assertion below reds and somebody re-reads the branch.
//!
//! Skipped without the table on disk, like every other golden-backed test in
//! this crate.

use std::collections::BTreeSet;
use std::path::PathBuf;

use rustmc_server::damage_types::{DamageTypeRegistry, TABLE_PATH};

fn repo(relative: &str) -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn table() -> Option<DamageTypeRegistry> {
    let path = repo(TABLE_PATH);
    if !rustmc_data::fixture::available(&path, "damage type table", "ci/damage-types-golden.sh") {
        return None;
    }
    Some(DamageTypeRegistry::load(&path).expect("the damage type table loads"))
}

#[test]
fn the_registry_is_fifty_one_types_and_thirty_five_declared_tags() {
    let Some(r) = table() else {
        return;
    };
    assert_eq!(r.len(), 51, "the damage type registry is not 51 rows");
    assert_eq!(
        r.tags_declared_in_code(),
        35,
        "DamageTypeTags no longer declares 35 TagKeys"
    );
}

/// The finding, pinned. See the module docs.
#[test]
fn bypasses_cooldown_is_the_one_declared_tag_with_no_data_file() {
    let Some(r) = table() else {
        return;
    };
    let expected: BTreeSet<String> = ["minecraft:bypasses_cooldown".to_string()]
        .into_iter()
        .collect();
    assert_eq!(
        r.tags_with_no_file(),
        &expected,
        "the set of declared-but-file-less damage type tags has moved. If \
         minecraft:bypasses_cooldown now HAS a file, the conjunct at \
         LivingEntity.hurtServer offset 196 is no longer dead and \
         crate::hurt's cooldown arm needs a scenario that exercises it."
    );

    // ...and the consequence, stated as a property of the data rather than of
    // the tag list: no damage type bypasses the cooldown.
    let bypassers: Vec<&str> = r
        .ids()
        .filter(|id| {
            r.tags_for(id)
                .expect("a listed id resolves")
                .bypasses_cooldown
        })
        .collect();
    assert!(
        bypassers.is_empty(),
        "these types bypass the hurt cooldown: {bypassers:?}"
    );
}

/// The tag reader must discriminate, or `DamageTags` would be the same struct
/// for every type and no test could tell a real table from a constant.
#[test]
fn the_ten_read_tags_produce_many_distinct_structs() {
    let Some(r) = table() else {
        return;
    };
    let mut signatures = BTreeSet::new();
    let mut per_field = [0_usize; 10];
    for id in r.ids() {
        let t = r.tags_for(id).expect("a listed id resolves");
        let bits = [
            t.bypasses_armor,
            t.bypasses_effects,
            t.bypasses_resistance,
            t.bypasses_enchantments,
            t.bypasses_cooldown,
            t.is_fire,
            t.is_freezing,
            t.damages_helmet,
            t.no_impact,
            t.no_knockback,
        ];
        for (i, b) in bits.iter().enumerate() {
            if *b {
                per_field[i] += 1;
            }
        }
        signatures.insert(bits);
    }
    eprintln!(
        "damage tags: {} distinct DamageTags over {} types; per-field counts {per_field:?}",
        signatures.len(),
        r.len()
    );
    assert!(
        signatures.len() >= 5,
        "only {} distinct DamageTags across {} types — the reader is close to a constant",
        signatures.len(),
        r.len()
    );
    // Nine of the ten must be set by at least one type; the tenth is
    // bypasses_cooldown, which cannot be (see above).
    let unset: Vec<usize> = per_field
        .iter()
        .enumerate()
        .filter(|(_, n)| **n == 0)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        unset,
        vec![4],
        "the only field no damage type sets should be bypasses_cooldown (index 4)"
    );
}

/// Three rows whose behaviour is fixed by code paths this branch transcribes,
/// so a table of plausible-looking wrong values cannot pass.
#[test]
fn the_three_types_the_ported_paths_deal_are_tagged_as_those_paths_expect() {
    let Some(r) = table() else {
        return;
    };
    // FoodData.tick's starvation hit — DamageSources.starve().
    let starve = r.tags_for("minecraft:starve").expect("minecraft:starve");
    assert!(starve.bypasses_armor, "armour would soak starvation");

    // PoisonMobEffect's hit — DamageSources.magic().
    let magic = r.tags_for("minecraft:magic").expect("minecraft:magic");
    assert!(magic.bypasses_armor, "armour would soak poison");

    // The one hurtServer refuses outright under Fire Resistance.
    let in_fire = r.tags_for("minecraft:in_fire").expect("minecraft:in_fire");
    assert!(
        in_fire.is_fire,
        "Fire Resistance would not stop standing in fire"
    );

    // And a control: a plain melee hit must NOT bypass armour, or the whole
    // armour stage would be unreachable in practice.
    let attack = r
        .tags_for("minecraft:player_attack")
        .expect("minecraft:player_attack");
    assert!(!attack.bypasses_armor);
}
