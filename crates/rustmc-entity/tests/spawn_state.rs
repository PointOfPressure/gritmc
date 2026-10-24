//! `SpawnState`, `PotentialCalculator` and `LocalMobCapCalculator`.
//!
//! These are the three pieces of the spawner that decide **how many** mobs
//! a world holds, and none of them is exercised by
//! `tests/spawn_decisions.rs`, which drives the position walk with
//! `NoFilter`. Every constant asserted below is either read out of the
//! bytecode (289, the seven categories, the two gate flags) or out of
//! `data/vanilla/entity_spawn_facts.json`, which reads them for us.

use std::path::{Path, PathBuf};

use rustmc_data::{BlockPos, ChunkPos};
use rustmc_entity::spawn::state::{
    CountedEntity, LocalMobCapCalculator, PotentialCalculator, SpawnState,
};
use rustmc_entity::spawn::tables::SpawnTables;
use rustmc_entity::spawn::{MobCategory, MAGIC_NUMBER, SPAWNING_CATEGORIES};

fn repo(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn tables() -> Option<SpawnTables> {
    let biomes = repo("data/vanilla/biome_spawners.json");
    if !rustmc_data::fixture::available(&biomes, "biome spawner table", "ci/dump-biome-spawners.sh")
    {
        return None;
    }
    Some(
        SpawnTables::load(
            &biomes,
            &repo("data/vanilla/spawn_placements.json"),
            &repo("data/vanilla/entity_spawn_facts.json"),
        )
        .expect("the three spawn tables load"),
    )
}

fn mob(kind: &str, x: i32, y: i32, z: i32) -> CountedEntity {
    CountedEntity {
        kind: kind.to_owned(),
        pos: BlockPos { x, y, z },
        persistence_required: false,
        requires_custom_persistence: false,
        is_mob: true,
    }
}

fn state(entities: &[CountedEntity], chunks: i32, t: &SpawnTables) -> SpawnState {
    SpawnState::create(
        chunks,
        entities,
        &|_| true,
        LocalMobCapCalculator::default(),
        t,
        &|_| "plains".to_owned(),
    )
}

#[test]
fn the_global_cap_is_max_times_chunks_over_289_with_integer_division() {
    let Some(t) = tables() else { return };
    // `canSpawnForCategoryGlobal`: `count < max * spawnableChunkCount / 289`.
    // MONSTER's max is 70, so at 289 loaded chunks the cap is exactly 70,
    // and at 288 it is `70 * 288 / 289 == 69` — the floor, not a rounding.
    assert_eq!(MAGIC_NUMBER, 289);
    let s = state(&[], 289, &t);
    assert!(s.can_spawn_for_category_global(MobCategory::Monster));

    let sixty_nine: Vec<_> = (0..69).map(|i| mob("minecraft:zombie", i, 64, 0)).collect();
    let s = state(&sixty_nine, 288, &t);
    assert_eq!(s.category_count(MobCategory::Monster), 69);
    // 70 * 288 / 289 = 69, and the test is `count < cap`, so 69 is full.
    assert!(!s.can_spawn_for_category_global(MobCategory::Monster));
    // One more chunk and the cap is 70, which 69 is under.
    let s = state(&sixty_nine, 289, &t);
    assert!(s.can_spawn_for_category_global(MobCategory::Monster));
}

#[test]
fn a_small_loaded_area_floors_every_cap_to_zero() {
    let Some(t) = tables() else { return };
    // AXOLOTLS/UNDERGROUND_WATER_CREATURE/WATER_CREATURE have max 5, so
    // `5 * chunks / 289` is 0 for any chunk count under 58. This is the
    // integer division being behaviour and not an implementation detail:
    // a world with a handful of loaded chunks spawns no axolotls at all,
    // whatever the biome says.
    let s = state(&[], 57, &t);
    for c in SPAWNING_CATEGORIES {
        let cap = c.max_instances_per_chunk() * 57 / MAGIC_NUMBER;
        assert_eq!(
            s.can_spawn_for_category_global(c),
            cap > 0,
            "{c:?} at 57 chunks, cap {cap}"
        );
    }
    assert!(!s.can_spawn_for_category_global(MobCategory::Axolotls));
    assert!(s.can_spawn_for_category_global(MobCategory::Monster));
}

#[test]
fn create_state_skips_persistent_mobs_misc_types_and_unloaded_chunks() {
    let Some(t) = tables() else { return };
    let mut persistent = mob("minecraft:zombie", 0, 64, 0);
    persistent.persistence_required = true;
    let mut custom = mob("minecraft:zombie", 1, 64, 0);
    custom.requires_custom_persistence = true;
    let misc = mob("minecraft:item", 2, 64, 0);
    let counted = mob("minecraft:zombie", 3, 64, 0);
    let far = mob("minecraft:zombie", 10_000, 64, 0);

    let all = [persistent, custom, misc, counted, far.clone()];
    // Everything loaded: the three skips leave two.
    let s = state(&all, 1_000_000, &t);
    assert_eq!(s.category_count(MobCategory::Monster), 2);
    assert_eq!(s.category_count(MobCategory::Misc), 0);

    // `ChunkGetter.query` drops an entity whose chunk is not loaded, so
    // it counts against neither cap. Refuse the far one's chunk only.
    let far_chunk = ChunkPos::containing(far.pos);
    let s = SpawnState::create(
        1_000_000,
        &all,
        &|c| c != far_chunk,
        LocalMobCapCalculator::default(),
        &t,
        &|_| "plains".to_owned(),
    );
    assert_eq!(s.category_count(MobCategory::Monster), 1);
}

#[test]
fn a_non_mob_entity_is_still_counted_because_the_misc_test_is_outside_the_instanceof() {
    let Some(t) = tables() else { return };
    // The bytecode's skip chain tests `instanceof Mob` for the two
    // persistence flags and then falls through to the category test for
    // EVERY entity. A non-Mob whose category is not MISC would be counted
    // — no vanilla type is in that position, but a port that nested the
    // category test inside the instanceof would answer differently, and
    // this is the assertion that would notice.
    let mut not_a_mob = mob("minecraft:zombie", 0, 64, 0);
    not_a_mob.is_mob = false;
    not_a_mob.persistence_required = true; // ignored: not a Mob
    let s = state(&[not_a_mob], 1_000_000, &t);
    assert_eq!(s.category_count(MobCategory::Monster), 1);
}

#[test]
fn the_two_gate_flags_select_exactly_one_category_each() {
    let Some(t) = tables() else { return };
    let s = state(&[], 1_000_000, &t);

    let all = s.filtered_spawning_categories(true, true);
    assert_eq!(all.len(), 7, "every spawning category, caps clear");
    assert_eq!(all, SPAWNING_CATEGORIES.to_vec(), "and in that order");

    // spawn-monsters=false drops MONSTER and nothing else.
    let no_enemies = s.filtered_spawning_categories(false, true);
    assert_eq!(no_enemies.len(), 6);
    assert!(!no_enemies.contains(&MobCategory::Monster));

    // The 400-tick cadence drops CREATURE and nothing else — this is the
    // one most likely to be ported as "friendlies every 400 ticks", which
    // would drop five more.
    let off_cadence = s.filtered_spawning_categories(true, false);
    assert_eq!(off_cadence.len(), 6);
    assert!(!off_cadence.contains(&MobCategory::Creature));
    assert!(off_cadence.contains(&MobCategory::Ambient));
    assert!(off_cadence.contains(&MobCategory::Axolotls));
    assert!(off_cadence.contains(&MobCategory::WaterAmbient));

    // Both off: five categories, which is the corpus substrate's shape.
    assert_eq!(s.filtered_spawning_categories(false, false).len(), 5);
}

#[test]
fn the_global_cap_also_filters_the_category_list() {
    let Some(t) = tables() else { return };
    // A full MONSTER cap removes MONSTER from the list before any chunk
    // is looked at, which is what makes a saturated world cheap.
    let full: Vec<_> = (0..70).map(|i| mob("minecraft:zombie", i, 64, 0)).collect();
    let s = state(&full, 289, &t);
    let cats = s.filtered_spawning_categories(true, true);
    assert!(!cats.contains(&MobCategory::Monster));
    assert_eq!(cats.len(), 6);
}

#[test]
fn a_zero_charge_is_not_stored_and_a_same_block_charge_is_infinite() {
    let mut p = PotentialCalculator::default();
    let at = |x, y, z| BlockPos { x, y, z };
    p.add_charge(at(0, 0, 0), 0.0);
    assert!(
        p.is_empty(),
        "addCharge's first four instructions refuse 0.0"
    );

    p.add_charge(at(0, 0, 0), 1.0);
    assert_eq!(p.len(), 1);
    // `PointCharge.getPotentialChange` returns Infinity at distSqr == 0.
    assert!(p.potential_energy_change(at(0, 0, 0), 1.0).is_infinite());
    // ... and a zero probe charge short-circuits to 0 before the walk, so
    // it does not become Infinity * 0 = NaN.
    assert_eq!(p.potential_energy_change(at(0, 0, 0), 0.0), 0.0);

    // distToLowCornerSqr: no +0.5 anywhere, unlike the spawn-point test.
    // 3-4-5 in the x/z plane, so sqrt(distSqr) is exactly 5.
    let mut q = PotentialCalculator::default();
    q.add_charge(at(3, 0, 4), 2.0);
    let got = q.potential_energy_change(at(0, 0, 0), 0.5);
    assert!((got - (2.0 / 5.0) * 0.5).abs() < 1e-12, "{got}");
}

#[test]
fn the_real_soul_sand_valley_numbers_admit_one_ghast_and_then_refuse_the_next() {
    let Some(t) = tables() else { return };
    // The only place in the game where the density check does anything.
    // charge 0.7, energyBudget 0.15, so a second ghast is admitted only
    // while `0.7 * (0.7 / d) <= 0.15`, i.e. d >= 3.2666..., i.e. more than
    // 3 blocks away.
    let mut s = SpawnState::create(
        1_000_000,
        &[],
        &|_| true,
        LocalMobCapCalculator::default(),
        &t,
        &|_| "soul_sand_valley".to_owned(),
    );
    let at = |x| BlockPos { x, y: 40, z: 0 };
    // Nothing charged yet: the potential change is 0 <= 0.15.
    assert!(s.can_spawn("minecraft:ghast", at(0), "soul_sand_valley", &t));
    s.after_spawn("minecraft:ghast", at(0), "soul_sand_valley", &t);
    assert_eq!(s.potential().len(), 1);
    // 3 blocks away: 0.7 * 0.7 / 3 = 0.163... > 0.15 -> refused.
    assert!(!s.can_spawn("minecraft:ghast", at(3), "soul_sand_valley", &t));
    // 4 blocks away: 0.7 * 0.7 / 4 = 0.1225 <= 0.15 -> allowed.
    assert!(s.can_spawn("minecraft:ghast", at(4), "soul_sand_valley", &t));
    // A type with no cost entry is unconditionally allowed and charges 0,
    // which addCharge then declines to store.
    assert!(s.can_spawn("minecraft:zombie", at(0), "soul_sand_valley", &t));
    s.after_spawn("minecraft:zombie", at(0), "soul_sand_valley", &t);
    assert_eq!(
        s.potential().len(),
        1,
        "the zombie's zero charge is not stored"
    );
    // And in a biome with no spawn_costs at all, nothing is ever refused.
    assert!(s.can_spawn("minecraft:ghast", at(0), "plains", &t));
}

#[test]
fn a_chunk_with_no_near_player_cannot_spawn_and_that_is_the_local_cap() {
    let mut l = LocalMobCapCalculator::default();
    let c = ChunkPos { x: 0, z: 0 };
    // `canSpawn` falls out of the player loop to `iconst_0`. A chunk
    // nobody is near refuses every category — this is the local cap and
    // NOT the 24-block distance rule, which is a different gate.
    assert!(!l.can_spawn(MobCategory::Monster, c));

    l.set_players_near(c, vec![1]);
    // A player with no MobCounts entry yet is `null`, which takes the
    // true arm without consulting a count.
    assert!(l.can_spawn(MobCategory::Monster, c));

    // MONSTER's per-player cap is its max, 70.
    for _ in 0..70 {
        l.add_mob(c, MobCategory::Monster);
    }
    assert!(!l.can_spawn(MobCategory::Monster, c));
    // ... and only that category is full.
    assert!(l.can_spawn(MobCategory::Creature, c));

    // A second player who is under the cap re-opens the chunk: the loop
    // returns true on the FIRST player that is under, not on all of them.
    l.set_players_near(c, vec![1, 2]);
    assert!(l.can_spawn(MobCategory::Monster, c));
}
