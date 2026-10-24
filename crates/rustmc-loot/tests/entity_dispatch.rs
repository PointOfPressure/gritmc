//! The `minecraft:entities/*` family of the loot corpus, rolled with the
//! entity parameter set this branch added.
//!
//! `block_dispatch.rs` proves the right table reaches the interpreter for a
//! block; `golden_loot.rs` proves the interpreter reproduces vanilla's vectors
//! for a table it is handed. Neither touches an entity table, because before
//! this branch every one of them refused on a missing `this_entity` — the
//! crate header says so in as many words.
//!
//! What this file grades is the third thing: **how far into the entity family
//! the new context reaches, and what the rest refuses on, by name**. The
//! refusal histogram is the point of it. A test that asserted only "the pig
//! drops porkchops" would go green with ninety-three tables silently
//! answering nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use rustmc_loot::{EntityCtx, Loot, LootDamageSource, LootEntity, LootItem, NoDrops, Stack};
use rustmc_worldgen::rng::legacy::LegacyRandomSource;
use rustmc_worldgen::rng::RandomSource;
use serde_json::Value;

/// The full literal path, for `block_dispatch.rs`'s reason: the generated-table
/// set is derived by grepping the tree for `"data/vanilla/<name>.json"`, so a
/// path assembled from parts puts this crate outside it.
fn load(rel: &str) -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    serde_json::from_str(
        &std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())),
    )
    .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn tables() -> Arc<BTreeMap<String, Value>> {
    Arc::new(serde_json::from_value(load("data/vanilla/loot_tables.json")).unwrap())
}

fn item_tags() -> Arc<BTreeMap<String, Vec<String>>> {
    Arc::new(serde_json::from_value(load("data/vanilla/item_tags.json")).unwrap())
}

fn dispatch() -> Loot {
    Loot::new(tables(), item_tags(), BTreeMap::new())
}

/// `data/minecraft/tags/enchantment/smelts_loot.json`, which is the ONE
/// enchantment tag the entity family names.
///
/// Synthetic rather than loaded, and that is the finding this helper records:
/// `#minecraft:smelts_loot` is an **enchantment** tag and `item_tags.json`
/// does not carry it — resolving it against the item tags finds nothing.
/// `ci/loot-golden.sh` dumps item tags only, so until an enchantment-tag
/// extract exists a caller supplies this map or the predicate refuses, and
/// the refusal is only reachable for a stack that HAS enchantments, which
/// `crate::item::ItemStack` cannot yet carry.
fn enchantment_tags() -> Arc<BTreeMap<String, Vec<String>>> {
    Arc::new(BTreeMap::from([(
        "minecraft:smelts_loot".to_string(),
        vec!["minecraft:fire_aspect".to_string()],
    )]))
}

/// A player killing a mob bare-handed: no equipment, so no Looting and no
/// `smelts_loot`, and the mob is not on fire.
fn melee_by_bare_player(kind: &str) -> EntityCtx {
    let player = LootEntity {
        kind: "minecraft:player".into(),
        player: true,
        ..Default::default()
    };
    EntityCtx {
        this_entity: Some(LootEntity {
            kind: kind.into(),
            ..Default::default()
        }),
        attacking_entity: Some(player.clone()),
        direct_attacking_entity: Some(player.clone()),
        last_damage_player: Some(player),
        damage_source: Some(LootDamageSource {
            kind: "minecraft:player_attack".into(),
            tags: BTreeSet::new(),
        }),
        origin: Some((0.0, 64.0, 0.0)),
    }
}

/// The jar's `RecipeType.SMELTING` reduced to the one pair this file needs.
///
/// Deliberately not the whole recipe extract: this crate has no recipe
/// fixture, and inventing one here would put a second copy of a table
/// `rustmc-server` already owns into the test tree. What it must NOT be is
/// `None`, because that makes `furnace_smelt` refuse, and a refusal would be
/// indistinguishable from the smelt not firing.
fn smelting() -> Arc<BTreeMap<String, String>> {
    Arc::new(BTreeMap::from([(
        "minecraft:porkchop".to_string(),
        "minecraft:cooked_porkchop".to_string(),
    )]))
}

/// `RandomSequence.createSequence` for a named sequence at world seed 0 is a
/// `XoroshiroRandomSource`; a legacy source is what a **non-zero**
/// `lootTableSeed` installs. This helper is the legacy one, used where the
/// test only needs a reproducible stream and not the sequence's own numbers —
/// the sequence itself is exercised in `rustmc-server`'s `mob_combat` test,
/// which is where the world seed lives.
fn legacy(seed: i64) -> LegacyRandomSource {
    LegacyRandomSource::new(seed)
}

/// The pig, the whole way through, against a hand-stepped generator.
///
/// One draw and one only: `set_count`'s `uniform(1, 3)` is
/// `Mth.nextInt(random, 1, 3)` = `nextInt(3) + 1`. `furnace_smelt`'s two
/// `entity_properties` terms draw nothing (an entity predicate never touches
/// the generator) and `enchanted_count_increase` returns at
/// `EnchantedCountIncreaseFunction.run` offset 40 without calling
/// `count.getFloat` when the level is zero.
///
/// The positive control is the second half: the same roll on a generator
/// stepped one extra time gives a different count for at least one seed, so a
/// port that consumed no draw at all could not pass both halves.
#[test]
fn a_bare_handed_pig_kill_drops_one_to_three_raw_porkchops_and_takes_one_draw() {
    let loot = dispatch();
    let mut seen: BTreeSet<i32> = BTreeSet::new();
    for seed in 0..64 {
        let mut rng = legacy(seed);
        let expected = rng.next_i32_bound(3) + 1;

        let mut rng = legacy(seed);
        let out = loot
            .entity_drops(
                "entities/pig",
                melee_by_bare_player("minecraft:pig"),
                Some(smelting()),
                enchantment_tags(),
                0.0,
                &mut rng,
            )
            .expect("the pig table must not refuse with a full entity context");
        assert_eq!(
            out,
            vec![Stack::new("minecraft:porkchop", expected)],
            "seed {seed}"
        );
        // Exactly one draw: whatever the generator produces next must be what
        // a control that took one draw produces next.
        let mut control = legacy(seed);
        let _ = control.next_i32_bound(3);
        assert_eq!(
            rng.next_i32(),
            control.next_i32(),
            "draw count at seed {seed}"
        );
        seen.insert(expected);
    }
    assert_eq!(
        seen,
        BTreeSet::from([1, 2, 3]),
        "64 seeds must reach all three counts, or the table is not being rolled"
    );
}

/// The bat has a table with **no pools**, which is not the same as having no
/// table. `EntityTypes.BAT` does not call `noLootTable()`, so
/// `getDefaultLootTable` is `Some(entities/bat)` and the roll produces an
/// empty list without refusing.
///
/// This is the case a dispatch gets wrong by collapsing "no table" and "empty
/// table" — `NoDrops::NoTableByDesign` versus `Ok(vec![])` — and it is the
/// only mob on this server other than the pig.
#[test]
fn the_bats_table_exists_and_is_empty_which_is_not_a_refusal() {
    let out = dispatch()
        .entity_drops(
            "entities/bat",
            melee_by_bare_player("minecraft:bat"),
            Some(smelting()),
            enchantment_tags(),
            0.0,
            &mut legacy(1),
        )
        .expect("entities/bat is a real table");
    assert!(out.is_empty());
}

/// A burning pig takes the `furnace_smelt` branch, and the branch does two
/// things: it changes the item AND resets the count to 1.
///
/// `SmeltItemFunction.run` offsets 74..109 compute
/// `(useInputCount ? input.count : 1) * result.getCount()`, and the pig table
/// sets no `use_input_count`. So three raw porkchops become **one** cooked
/// one — the count is not carried over, which is the reading a port that only
/// swapped the item id would get wrong.
///
/// The draw is unchanged: the smelt happens after `set_count`, so the count it
/// discards was still drawn.
#[test]
fn a_burning_pig_smelts_to_exactly_one_cooked_porkchop() {
    let loot = dispatch();
    // A seed whose first nextInt(3) is not 0, so the raw count would have been
    // more than one and the reset to 1 is visible rather than coincidental.
    let seed = (0i64..256)
        .find(|s| legacy(*s).next_i32_bound(3) != 0)
        .expect("some seed draws a non-zero count");
    let raw = legacy(seed).next_i32_bound(3) + 1;
    assert!(raw > 1, "positive control: the raw count must exceed 1");

    let mut ctx = melee_by_bare_player("minecraft:pig");
    ctx.this_entity.as_mut().expect("this entity").on_fire = true;
    let out = loot
        .entity_drops(
            "entities/pig",
            ctx,
            Some(smelting()),
            enchantment_tags(),
            0.0,
            &mut legacy(seed),
        )
        .expect("no refusal");
    assert_eq!(out, vec![Stack::new("minecraft:cooked_porkchop", 1)]);
}

/// The other arm of the same `any_of`: the killer's main hand carries an
/// enchantment in `#minecraft:smelts_loot`. The pig itself is not on fire.
///
/// This exercises `entity_properties` on `direct_attacker` all the way down
/// through `EntityEquipmentPredicate` -> `ItemPredicate` -> the
/// `minecraft:enchantments` component predicate and its `#tag` reference, and
/// it is the only path in this file that resolves a tag at all.
///
/// The tag comes from [`enchantment_tags`] and **not** from
/// `item_tags.json`. This test asserted the latter first and the gate caught
/// it: `#minecraft:smelts_loot` is in `data/minecraft/tags/enchantment/`, and
/// `ci/loot-golden.sh` dumps `tags/item/` only, so the item map does not carry
/// it and never did.
#[test]
fn a_killer_holding_a_smelts_loot_enchantment_cooks_the_drop() {
    let tags = enchantment_tags();
    let smelts = tags
        .get("minecraft:smelts_loot")
        .expect("the fixture in this file names it");
    let ench = smelts
        .first()
        .expect("the smelts_loot tag must not be empty")
        .clone();
    let bare = ench.rsplit(':').next().unwrap_or(&ench).to_string();

    let weapon = LootItem {
        item: "minecraft:iron_sword".into(),
        enchants: BTreeMap::from([(bare, 1)]),
    };
    let killer = LootEntity {
        kind: "minecraft:player".into(),
        player: true,
        equipment: BTreeMap::from([("mainhand".to_string(), weapon)]),
        ..Default::default()
    };
    let mut ctx = melee_by_bare_player("minecraft:pig");
    ctx.attacking_entity = Some(killer.clone());
    ctx.direct_attacking_entity = Some(killer);

    let out = dispatch()
        .entity_drops(
            "entities/pig",
            ctx,
            Some(smelting()),
            enchantment_tags(),
            0.0,
            &mut legacy(3),
        )
        .expect("no refusal");
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].item, "minecraft:cooked_porkchop");
}

/// The negative half of the test above: the same weapon with an enchantment
/// that is NOT in the tag leaves the drop raw.
///
/// Without this, `a_killer_holding_a_smelts_loot_enchantment_cooks_the_drop`
/// would pass for an implementation that answered true for any non-empty
/// enchantment map.
#[test]
fn an_unrelated_enchantment_does_not_cook_the_drop() {
    let weapon = LootItem {
        item: "minecraft:iron_sword".into(),
        enchants: BTreeMap::from([("sharpness".to_string(), 5)]),
    };
    let killer = LootEntity {
        kind: "minecraft:player".into(),
        player: true,
        equipment: BTreeMap::from([("mainhand".to_string(), weapon)]),
        ..Default::default()
    };
    let mut ctx = melee_by_bare_player("minecraft:pig");
    ctx.attacking_entity = Some(killer.clone());
    ctx.direct_attacking_entity = Some(killer);

    let out = dispatch()
        .entity_drops(
            "entities/pig",
            ctx,
            Some(smelting()),
            enchantment_tags(),
            0.0,
            &mut legacy(3),
        )
        .expect("no refusal");
    assert_eq!(out[0].item, "minecraft:porkchop");
}

/// Looting adds `round(level * uniform(0, 1))` and takes a draw **only** when
/// the level is non-zero — `EnchantedCountIncreaseFunction.run`'s `ifne 42` at
/// offset 37 returns before `count.getFloat`.
///
/// Both halves are asserted against the same seed, so the assertion is about
/// the draw and not about the count.
#[test]
fn looting_costs_a_second_draw_and_no_looting_costs_none() {
    let loot = dispatch();

    let mut bare_rng = legacy(11);
    let _ = loot
        .entity_drops(
            "entities/pig",
            melee_by_bare_player("minecraft:pig"),
            Some(smelting()),
            enchantment_tags(),
            0.0,
            &mut bare_rng,
        )
        .expect("no refusal");

    let weapon = LootItem {
        item: "minecraft:iron_sword".into(),
        enchants: BTreeMap::from([("looting".to_string(), 3)]),
    };
    let killer = LootEntity {
        kind: "minecraft:player".into(),
        player: true,
        equipment: BTreeMap::from([("mainhand".to_string(), weapon)]),
        ..Default::default()
    };
    let mut ctx = melee_by_bare_player("minecraft:pig");
    ctx.attacking_entity = Some(killer.clone());
    ctx.direct_attacking_entity = Some(killer);

    let mut looting_rng = legacy(11);
    let looted = loot
        .entity_drops(
            "entities/pig",
            ctx,
            Some(smelting()),
            enchantment_tags(),
            0.0,
            &mut looting_rng,
        )
        .expect("no refusal");

    let mut one = legacy(11);
    let _ = one.next_i32_bound(3);
    let mut two = legacy(11);
    let _ = two.next_i32_bound(3);
    let _ = two.next_f32();

    assert_eq!(
        bare_rng.next_i32(),
        one.next_i32(),
        "bare hand takes one draw"
    );
    assert_eq!(
        looting_rng.next_i32(),
        two.next_i32(),
        "looting takes a second"
    );
    assert!(looted[0].count >= 1);
}

/// Every `minecraft:entities/*` table in the corpus, rolled with a full
/// context, classified.
///
/// The ratchet is on the **answered** count and on the refusal causes. A
/// refusal is a legitimate outcome for a table needing a subsystem this server
/// does not have, and naming the causes is what stops the number drifting
/// upward for the wrong reason: if the count rises because a condition started
/// answering `false` instead of refusing, the cause histogram loses an entry
/// and this test says so.
#[test]
fn every_entity_table_is_answered_or_refused_with_a_named_cause() {
    let tables = tables();
    let loot = Loot::new(Arc::clone(&tables), item_tags(), BTreeMap::new());

    let entity_tables: Vec<&String> = tables
        .keys()
        .filter(|k| k.starts_with("entities/"))
        .collect();
    // MEASURED on the Threadripper, 2026-09-05: **109** tables, not the 94 a
    // listing of the jar's flat `loot_table/entities/` directory gives — the
    // extra fifteen are nested subtables (`entities/sheep/<colour>` and the
    // like), which `ci/loot-golden.sh`'s recursive walk picks up and a
    // non-recursive one would silently miss. A floor rather than an equality,
    // because this branch does not own the extractor.
    assert!(
        entity_tables.len() >= 109,
        "109 entity loot tables measured 2026-09-05; found {}",
        entity_tables.len()
    );

    let mut answered = 0usize;
    let mut causes: BTreeMap<String, usize> = BTreeMap::new();
    for id in &entity_tables {
        let kind = format!("minecraft:{}", id.trim_start_matches("entities/"));
        let out = loot.entity_drops(
            id,
            melee_by_bare_player(&kind),
            Some(smelting()),
            enchantment_tags(),
            0.0,
            &mut legacy(7),
        );
        match out {
            Ok(_) => answered += 1,
            Err(NoDrops::Refused(why)) => *causes.entry(why).or_default() += 1,
            Err(other) => panic!("{id}: {other:?}"),
        }
    }

    // Printed rather than only asserted, so a change in the mix is visible in
    // the log of a passing run and not only in a failing one.
    println!(
        "entity tables: {} total, {answered} answered",
        entity_tables.len()
    );
    for (why, n) in &causes {
        println!("  refused x{n}: {why}");
    }

    // MEASURED: **89 of 109** answer with a full entity context, against
    // **0 of 109** before this branch — every one of them refused on a missing
    // `this_entity`. A floor, ratcheted: the twenty that still refuse do so
    // for reasons the histogram above names, and a change that drops one of
    // the 89 without adding a named refusal is exactly the silent drop-deletion
    // this test exists for.
    assert!(
        answered >= 89,
        "89 of 109 answered when this was measured (2026-09-05); got {answered}"
    );
    assert_eq!(
        answered + causes.values().sum::<usize>(),
        entity_tables.len(),
        "every table is either answered or refused"
    );
    assert!(
        !causes.is_empty(),
        "positive control: some entity tables MUST still refuse. If none does, \
         a condition has started answering false instead of refusing, which \
         silently deletes drops"
    );
}

/// Rolling an entity table with **no** entity context answers, as vanilla
/// does: every optional read is `false` for an absent parameter and none of
/// them draws (`golden_loot`'s `entity_tables_with_no_entity_reproduce_vanillas_vectors`
/// grades that against the jar's own vectors). So the empty context and an
/// unburning adult pig with nobody else in the context roll the same stacks
/// from the same seed — the `/kill` case.
///
/// This replaced entity-ports' "an empty context must refuse" when `train-g`
/// put that branch's entity context on the loot lane's interpreter: the
/// refusal was a guard against an unwritten context, and the loot lane's
/// golden vectors showed vanilla's answer for it.
#[test]
fn an_entity_table_without_a_context_answers_as_an_unattended_death() {
    let roll = |ctx: EntityCtx, seed: i64| {
        dispatch().entity_drops(
            "entities/pig",
            ctx,
            Some(smelting()),
            enchantment_tags(),
            0.0,
            &mut legacy(seed),
        )
    };
    for seed in 0..32 {
        let empty = roll(EntityCtx::default(), seed).expect("no context is an answer");
        let alone = roll(
            EntityCtx {
                this_entity: Some(LootEntity {
                    kind: "minecraft:pig".into(),
                    ..Default::default()
                }),
                ..Default::default()
            },
            seed,
        )
        .expect("a pig alone is an answer");
        assert_eq!(empty, alone, "seed {seed}");
        assert!(
            empty.iter().all(|s| s.item == "minecraft:porkchop"),
            "seed {seed}: {empty:?}"
        );
    }
    // Positive control: a burning pig is not the empty context.
    let burning = roll(
        EntityCtx {
            this_entity: Some(LootEntity {
                kind: "minecraft:pig".into(),
                on_fire: true,
                ..Default::default()
            }),
            ..Default::default()
        },
        1,
    )
    .expect("a burning pig is an answer");
    assert!(burning
        .iter()
        .any(|s| s.item == "minecraft:cooked_porkchop"));
}

/// A smelting map that is absent is not the same as a recipe that is missing.
///
/// With `smelting: None` the burning pig's `furnace_smelt` refuses; with an
/// empty map it is vanilla's "no smelting recipe" warning and the stack passes
/// through raw. Both halves asserted, because collapsing them is exactly the
/// defect [`Ctx::smelting`]'s doc names.
#[test]
fn an_absent_recipe_map_refuses_and_an_empty_one_does_not() {
    let loot = dispatch();
    let mut burning = melee_by_bare_player("minecraft:pig");
    burning.this_entity.as_mut().expect("this").on_fire = true;

    let err = loot
        .entity_drops(
            "entities/pig",
            burning.clone(),
            None,
            enchantment_tags(),
            0.0,
            &mut legacy(2),
        )
        .expect_err("no recipe map must refuse");
    assert!(matches!(&err, NoDrops::Refused(why) if why.contains("furnace_smelt")));

    let empty = Arc::new(BTreeMap::new());
    let out = loot
        .entity_drops(
            "entities/pig",
            burning,
            Some(empty),
            enchantment_tags(),
            0.0,
            &mut legacy(2),
        )
        .expect("an empty map is a missing recipe, not a refusal");
    assert_eq!(out[0].item, "minecraft:porkchop");
}
