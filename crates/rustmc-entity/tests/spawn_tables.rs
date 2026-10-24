//! The three spawn tables, read as this crate reads them, and swept.
//!
//! # What each assertion can go red on
//!
//! Every number below is either a second reading of the jar (the
//! generators' own cross-checks are the first) or a property of the
//! transcription that a wrong edit breaks. None of them is a count this
//! test computed and then compared with itself — the failure mode
//! `docs/AGENT-BRIEF.md` opens with.

use std::path::{Path, PathBuf};

use rustmc_entity::spawn::rules::{PENDING, TRANSCRIBED};
use rustmc_entity::spawn::tables::{HeightmapType, SpawnPlacementType, SpawnTables};
use rustmc_entity::spawn::MobCategory;

fn repo(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn tables() -> Option<SpawnTables> {
    let biomes = repo("data/vanilla/biome_spawners.json");
    let placements = repo("data/vanilla/spawn_placements.json");
    let facts = repo("data/vanilla/entity_spawn_facts.json");
    if !rustmc_data::fixture::available(&biomes, "biome spawner table", "ci/dump-biome-spawners.sh")
    {
        return None;
    }
    if !rustmc_data::fixture::available(
        &placements,
        "spawn placement table",
        "ci/dump-spawn-placements.sh",
    ) {
        return None;
    }
    if !rustmc_data::fixture::available(
        &facts,
        "entity spawn facts table",
        "ci/dump-entity-spawn-facts.sh",
    ) {
        return None;
    }
    Some(SpawnTables::load(&biomes, &placements, &facts).expect("the three spawn tables load"))
}

#[test]
fn the_tables_hold_what_the_generators_said_they_wrote() {
    let Some(t) = tables() else { return };
    // Each of these is the generator's printed count, and each of those
    // was itself checked against a second source before it was written:
    // 66 biomes against en_us.json's biome.minecraft.* keys, 83
    // registrations against the register() site count, 158 types against
    // minecraft:entity_type.
    assert_eq!(t.biome_count(), 66, "biomes in biome_spawners.json");
    assert_eq!(t.placement_count(), 83, "SpawnPlacements registrations");
    assert_eq!(t.type_count(), 158, "entity types");
}

#[test]
fn every_registered_predicate_is_transcribed_or_pending() {
    let Some(t) = tables() else { return };
    let mut unknown = Vec::new();
    for (kind, row) in t.placements_iter() {
        let name = row.predicate.as_str();
        if !TRANSCRIBED.contains(&name) && !PENDING.contains(&name) {
            unknown.push(format!("{kind} -> {name}"));
        }
    }
    assert!(
        unknown.is_empty(),
        "SpawnPlacements registers {} predicate(s) that are in neither \
         spawn::rules::TRANSCRIBED nor PENDING: {unknown:?}. \
         spawn::rules::evaluate panics on these rather than defaulting.",
        unknown.len()
    );
}

#[test]
fn the_transcribed_predicates_carry_the_share_the_module_claims() {
    let Some(t) = tables() else { return };
    let covered = t
        .placements_iter()
        .filter(|(_, r)| TRANSCRIBED.contains(&r.predicate.as_str()))
        .count();
    // 48 of 83. The module docs say so; this is the assertion that keeps
    // them true rather than merely written.
    assert_eq!(covered, 48, "registrations a transcribed predicate covers");
    assert_eq!(TRANSCRIBED.len(), 6);
    assert_eq!(PENDING.len(), 33);
    let distinct: std::collections::BTreeSet<&str> = t
        .placements_iter()
        .map(|(_, r)| r.predicate.as_str())
        .collect();
    assert_eq!(distinct.len(), 39, "distinct spawn predicates");
}

#[test]
fn the_placement_defaults_come_from_the_table_and_not_from_a_literal() {
    let Some(t) = tables() else { return };
    // `getPlacementType`/`getHeightmapType` on a type with no row.
    // **Six types in a spawning category have none** — allay, bee,
    // piglin_brute, sniffer, tadpole, zombie_nautilus — so the default is
    // not a theoretical branch, and NO_RESTRICTIONS means they get no
    // world-border test and no ON_GROUND column check either.
    for kind in [
        "minecraft:allay",
        "minecraft:bee",
        "minecraft:piglin_brute",
        "minecraft:sniffer",
        "minecraft:tadpole",
        "minecraft:zombie_nautilus",
    ] {
        assert!(t.placement(kind).is_none(), "{kind} has a placement row");
        assert_eq!(
            t.placement_type(kind),
            SpawnPlacementType::NoRestrictions,
            "{kind}"
        );
        assert_eq!(
            t.heightmap_type(kind),
            HeightmapType::MotionBlockingNoLeaves,
            "{kind}"
        );
    }
    // The control: a type that IS registered must not answer the default.
    assert_eq!(
        t.placement_type("minecraft:cod"),
        SpawnPlacementType::InWater
    );
}

#[test]
fn can_spawn_far_from_player_is_defaulted_by_category_not_by_the_builder() {
    let Some(t) = tables() else { return };
    let mut far = 0;
    let mut far_by_default = 0;
    let mut explicit = Vec::new();
    for kind in [
        "minecraft:pig",
        "minecraft:cow",
        "minecraft:zombie",
        "minecraft:pillager",
        "minecraft:shulker",
        "minecraft:bat",
    ] {
        let f = t.facts(kind).expect("a known type");
        if f.can_spawn_far_from_player {
            far += 1;
            if matches!(f.category, MobCategory::Creature | MobCategory::Misc) {
                far_by_default += 1;
            } else {
                explicit.push(kind);
            }
        }
    }
    // pig and cow are CREATURE and get it from EntityType$Builder.<init>;
    // pillager and shulker are the only two types in the whole registry
    // that get it from an explicit .canSpawnFarFromPlayer() call.
    assert_eq!(far, 4, "of the six sampled");
    assert_eq!(far_by_default, 2);
    assert_eq!(explicit, ["minecraft:pillager", "minecraft:shulker"]);
    assert!(
        !t.facts("minecraft:zombie")
            .unwrap()
            .can_spawn_far_from_player
    );
    assert!(!t.facts("minecraft:bat").unwrap().can_spawn_far_from_player);
}

#[test]
fn a_biome_lists_a_type_under_a_category_that_is_not_its_own() {
    let Some(t) = tables() else { return };
    // FOUND IN THE DATA, not assumed: `minecraft:ocelot` is CREATURE and
    // appears in the MONSTER spawner list of `jungle` and
    // `bamboo_jungle`. Every gate downstream of the draw then uses the
    // TYPE's category — the despawn distance in
    // isValidSpawnPostitionForType, the cap in SpawnState.afterSpawn — so
    // an ocelot spawned through the monster pass is charged to the
    // creature cap. A port that used the pass's category would be wrong
    // here and nowhere else.
    let jungle = t.biome("jungle").expect("jungle");
    let monster = jungle.mobs(MobCategory::Monster);
    let ocelot = monster
        .entries()
        .iter()
        .find(|e| e.data.kind == "minecraft:ocelot")
        .expect("ocelot is in jungle's MONSTER list");
    assert_eq!(ocelot.weight, 2);
    assert_eq!(
        t.facts("minecraft:ocelot").unwrap().category,
        MobCategory::Creature
    );
}

#[test]
fn a_weighted_list_can_hold_the_same_spawner_data_twice() {
    let Some(t) = tables() else { return };
    // Three jungle biomes list `chicken 4..4` twice at weight 10 each.
    // `WeightedList` keeps both entries, so the chicken's real share is
    // 20/total; de-duplicating the list on (type, min, max) — which is
    // exactly `SpawnerData`'s record equality — would halve it.
    for name in ["jungle", "bamboo_jungle", "sparse_jungle"] {
        let list = t.biome(name).expect(name).mobs(MobCategory::Creature);
        let chickens: Vec<_> = list
            .entries()
            .iter()
            .filter(|e| e.data.kind == "minecraft:chicken")
            .collect();
        assert_eq!(chickens.len(), 2, "{name}: chicken entries");
        assert!(chickens.iter().all(|e| e.weight == 10));
        assert_eq!(chickens[0].data, chickens[1].data);
    }
}

#[test]
fn only_two_biomes_carry_a_spawn_cost() {
    let Some(t) = tables() else { return };
    // The `spawnPotentials` density check is data, and there is almost
    // none of it: 5 entries in 2 biomes of 66. A port with no
    // PotentialCalculator at all is wrong in soul_sand_valley and
    // warped_forest and correct in the other 64.
    let ssv = t.biome("soul_sand_valley").expect("soul_sand_valley");
    let cost = ssv.spawn_cost("minecraft:ghast").expect("ghast cost");
    assert!((cost.charge - 0.7).abs() < 1e-12);
    assert!((cost.energy_budget - 0.15).abs() < 1e-12);
    let wf = t.biome("warped_forest").expect("warped_forest");
    let e = wf.spawn_cost("minecraft:enderman").expect("enderman cost");
    assert!((e.charge - 1.0).abs() < 1e-12);
    assert!((e.energy_budget - 0.12).abs() < 1e-12);
    assert!(t
        .biome("plains")
        .expect("plains")
        .spawn_cost("minecraft:pig")
        .is_none());
}

#[test]
fn every_mob_category_constant_matches_the_enum_bytecode() {
    let Some(t) = tables() else { return };
    // The second reading. `spawn::MobCategory`'s five accessors are
    // hand-written const fns; `ci/dump-entity-spawn-facts.sh` reads the
    // same values out of `MobCategory.<clinit>`'s eight constructor calls.
    // A wrong constant in either place is a red here — including MISC's
    // max of **-1**, which is easy to write as 0 because nothing reads it.
    let cats = t.categories();
    assert_eq!(cats.len(), 8, "MobCategory constructions in the jar");
    assert_eq!(
        cats.len(),
        rustmc_entity::spawn::ALL.len(),
        "the enum and ALL disagree about how many categories there are"
    );
    for (i, c) in cats.iter().enumerate() {
        // Declaration order is load-bearing: SPAWNING_CATEGORIES is
        // values() filtered on != MISC, and spawnForChunk walks it.
        let ours = rustmc_entity::spawn::ALL[i];
        assert_eq!(c.ordinal, i32::try_from(i).unwrap(), "{}", c.name);
        assert_eq!(
            ours.serialized_name(),
            c.serialized,
            "{} order/name",
            c.name
        );
        assert_eq!(
            ours.max_instances_per_chunk(),
            c.max_instances_per_chunk,
            "{} max",
            c.name
        );
        assert_eq!(ours.is_friendly(), c.is_friendly, "{} isFriendly", c.name);
        assert_eq!(
            ours.is_persistent(),
            c.is_persistent,
            "{} isPersistent",
            c.name
        );
        assert_eq!(
            ours.despawn_distance(),
            c.despawn_distance,
            "{} despawnDistance",
            c.name
        );
        assert_eq!(
            ours.no_despawn_distance(),
            t.no_despawn_distance(),
            "{}",
            c.name
        );
        assert_eq!(
            MobCategory::from_serialized_name(&c.serialized),
            Some(ours),
            "{} round trip",
            c.name
        );
    }
    // SPAWNING_CATEGORIES is exactly ALL minus MISC, in order.
    let expected: Vec<_> = cats
        .iter()
        .filter(|c| c.serialized != "misc")
        .map(|c| c.serialized.as_str())
        .collect();
    let ours: Vec<_> = rustmc_entity::spawn::SPAWNING_CATEGORIES
        .iter()
        .map(|c| c.serialized_name())
        .collect();
    assert_eq!(ours, expected, "SPAWNING_CATEGORIES");
    // The two gate flags, stated as counts so a flipped bit is visible:
    // exactly one category is unfriendly and exactly two are persistent.
    assert_eq!(cats.iter().filter(|c| !c.is_friendly).count(), 1);
    assert_eq!(cats.iter().filter(|c| c.is_persistent).count(), 2);
}

#[test]
fn the_module_category_constants_agree_with_the_tick_module() {
    // The crate holds three MobCategory enums and this is the assertion
    // that keeps the duplication honest. `tick::MobCategory` is the one
    // `Mob.checkDespawn` reads; if the two ever disagree about a distance
    // one of them has been edited alone.
    use rustmc_entity::tick::MobCategory as Tick;
    let pairs = [
        (MobCategory::Monster, Tick::Monster),
        (MobCategory::Creature, Tick::Creature),
        (MobCategory::Ambient, Tick::Ambient),
        (MobCategory::Axolotls, Tick::Axolotls),
        (
            MobCategory::UndergroundWaterCreature,
            Tick::UndergroundWaterCreature,
        ),
        (MobCategory::WaterCreature, Tick::WaterCreature),
        (MobCategory::WaterAmbient, Tick::WaterAmbient),
        (MobCategory::Misc, Tick::Misc),
    ];
    for (a, b) in pairs {
        assert_eq!(
            a.despawn_distance(),
            b.despawn_distance(),
            "{a:?} despawn distance"
        );
        assert_eq!(a.no_despawn_distance(), b.no_despawn_distance());
    }
}
