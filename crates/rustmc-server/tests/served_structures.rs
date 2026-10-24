//! Do the chunks this server puts on the wire carry structures?
//!
//! # The claim this refutes
//!
//! `docs/SERVER.md` said "No structures. ... of the twenty structure sets only
//! mineshafts are ported and they are not run here", `crates/rustmc-server/src/world.rs`
//! carried the same line in its caveats, and `docs/FINDINGS.md` published
//! "the worldgen dump places 8 of 20 structure sets and the running server
//! places none of them: the parity figures and the world a player would join
//! are different configurations".
//!
//! All three are false, and the mechanism is one line:
//! [`World::generate_through`] calls `worldgen_assembly::generate::generate`,
//! which is the **same function** `tools/worldgen-dump` drives. The dump and
//! the server were already one configuration; nothing had measured it.
//!
//! # What this gate measures, and why it is not the trivial identity
//!
//! Asserting that the server's canvas equals the dump's canvas would be an
//! instrument measuring its own input — they are the same call. The question
//! that is *not* settled by that call is whether the **serving path** carries
//! the blocks: canvas -> [`World::chunk`] -> 24 sections -> paletted container
//! -> global network state id. A structure sitting in a canvas that the
//! encoder drops, or maps to the wrong global id, is a structure no player
//! sees.
//!
//! So every count below is taken by **decoding what would go on the wire**:
//! the `LevelChunkWithLight` the server hands the packet writer, its palette
//! entries resolved back through `block_props.json`'s global ids.
//!
//! # The control
//!
//! Each window is generated twice — through [`Through::Decoration`] and
//! through [`Through::Carvers`] — and every marker must be **absent** from
//! the carvers world. That is what makes a green here mean "the structure
//! stage put these blocks there" rather than "this seed happens to have
//! netherrack in it". The carvers world is also asserted non-trivial, because
//! a generator that returned an empty canvas would otherwise satisfy every
//! zero.
//!
//! Markers are blocks the overworld decoration stage cannot make. `rail` and
//! `cobweb` for a mineshaft and `trial_spawner`/`vault` for a trial chamber
//! are `docs/AGENT-BRIEF.md`'s own list; `netherrack` and `gold_block` in the
//! overworld come only from a ruined portal; `dirt_path`, `farmland` and
//! `wheat` only from a village; `sea_lantern` and `suspicious_sand` only from
//! an ocean ruin here, because the other three structures that carry them —
//! desert pyramid, desert well, trail ruins — are not ported; and
//! `dark_oak_trapdoor` only from a shipwreck, whose twenty templates name
//! four kinds of trapdoor between them.
//!
//! # Measured 2026-09-05 on `structures-live`, box threadripper
//!
//! ```text
//! set             seed  centre    r  marker                          decorated  carvers
//! mineshaft          0  (14,  5)  3  minecraft:rail                        341        0
//! mineshaft          0  (14,  5)  3  minecraft:cobweb                      199        0
//! trial_chambers     0  (14,  5)  3  minecraft:trial_spawner                15        0
//! trial_chambers     0  (14,  5)  3  minecraft:vault                        19        0
//! trial_chambers     0  (14,  5)  3  minecraft:chiseled_tuff_bricks       2620        0
//! ruined_portal      0  (20,  9)  1  minecraft:netherrack                  729        0
//! ruined_portal      0  (20,  9)  1  minecraft:gold_block                    1        0
//! village_savanna    0  (38, 45)  3  minecraft:dirt_path                   647        0
//! village_savanna    0  (38, 45)  3  minecraft:farmland                    353        0
//! village_savanna    0  (38, 45)  3  minecraft:wheat                       276        0
//! ocean_ruin_warm    0  (27, 20)  1  minecraft:cut_sandstone               242        0
//! ocean_ruin_warm    0  (27, 20)  1  minecraft:sea_lantern                   3        0
//! ocean_ruin_warm    0  (27, 20)  1  minecraft:suspicious_sand              20        0
//! shipwreck          0  (-21,-32) 1  minecraft:dark_oak_planks              88        0
//! shipwreck          0  (-21,-32) 1  minecraft:dark_oak_trapdoor             7        0
//! ```
//!
//! Those last two windows were found by scanning, not by a report function:
//! neither set has one. A 49x49 census at centre (30,20) and another at
//! (-25,-25), counting per chunk the block names that only these two sets'
//! templates carry, put the warm ruin in chunk (27,20) and the dark-oak
//! shipwreck across (-21,-33) and (-21,-32). The marker lists come from the
//! templates themselves — the 20 `shipwreck/*.nbt` palettes name four kinds
//! of trapdoor and nothing in overworld decoration places one.
//!
//! And the world a player actually joins — `status::WORLD_SEED` 0 at
//! `status::WORLD_RADIUS` 10, the square `main.rs` prepares before it listens
//! — carries **45 `minecraft:rail` and 14 `minecraft:cobweb`**: a mineshaft,
//! served, today. That window is [`the_default_world_a_player_joins`], and it
//! is the one the retired claim was about.
//!
//! Counts are asserted as presence rather than to the block, deliberately.
//! The exact figures are the table above and the commit that added it; the
//! assertion is the claim — *these blocks reach the wire* — which is what a
//! landing worldgen stack may legitimately move the size of and must not move
//! the existence of.
//!
//! Skipped without the vanilla extract, through `rustmc_data::fixture` rather
//! than a raw skip — see `ci/check-fixture-skips.py`.

use std::collections::BTreeMap;
use std::sync::Arc;

use rustmc_server::chunk::ContainerKind;
use rustmc_server::configuration::CapturedRegistries;
use rustmc_server::terrain::StateIds;
use rustmc_server::world::{World, WorldTables};
use rustmc_worldgen::pipeline::{Area, Through};

fn repo(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// Is the extract this needs on disk?
fn have_extract() -> bool {
    rustmc_data::fixture::available(
        &repo("data/vanilla/block_props.json"),
        "block_props extract",
        "ci/block-props-golden.sh",
    ) && rustmc_data::fixture::available(
        &repo("data/vanilla/registries.bin"),
        "registry capture",
        "ci/dump-registries.sh",
    ) && worldgen_assembly::have_extract()
}

/// Global network state id to block name, over the whole state table.
///
/// The **id**, not the array position: this is the map the encoder's output
/// has to be read back through, so reading it any other way would decode the
/// packet with a different table from the one that wrote it.
fn names_by_id() -> BTreeMap<i32, String> {
    let text = std::fs::read_to_string(repo("data/vanilla/block_props.json")).expect("block table");
    let doc: serde_json::Value = serde_json::from_str(&text).expect("json");
    doc["table"]
        .as_array()
        .expect("table")
        .iter()
        .filter_map(|e| {
            let state = e["state"].as_str()?;
            let id = i32::try_from(e["id"].as_i64()?).ok()?;
            Some((id, state.split('[').next().unwrap_or(state).to_string()))
        })
        .collect()
}

/// Generate `area` for `seed` through `through`, then count block names in
/// the chunks the server would **send**.
fn served_census(seed: i64, area: Area, through: Through) -> BTreeMap<String, usize> {
    let registries =
        CapturedRegistries::load(&repo("data/vanilla/registries.bin")).expect("registry capture");
    let biomes = registries
        .entry_ids("minecraft:worldgen/biome")
        .expect("readable capture")
        .expect("the capture holds the biome registry");
    let ids = StateIds::load(&repo("data/vanilla/block_props.json")).expect("block table");
    let tables = Arc::new(WorldTables::load(&biomes, ids).expect("block tables"));
    let world = World::generate_through(seed, area, tables, through).expect("generated");
    let names = names_by_id();
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    for (chunk_x, chunk_z) in area.chunks() {
        let chunk = world.chunk(chunk_x, chunk_z).expect("encoded chunk");
        for section in &chunk.sections {
            for id in section.states.values(ContainerKind::BlockStates) {
                let name = names
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| format!("unmapped network id {id}"));
                *out.entry(name).or_default() += 1;
            }
        }
    }
    out
}

/// One window: the markers must be served, and must not be there without the
/// structure stage.
fn window_serves(seed: i64, centre: (i32, i32), radius: i32, markers: &[&str]) {
    if !have_extract() {
        return;
    }
    let area = Area::square(centre.0, centre.1, radius);
    let decorated = served_census(seed, area, Through::Decoration);
    let carved = served_census(seed, area, Through::Carvers);

    // The control has to be a world, not an empty canvas, or every zero
    // below would be satisfied by a generator that produced nothing.
    let stone = carved.get("minecraft:stone").copied().unwrap_or(0);
    assert!(
        stone > 10_000,
        "the carvers control at seed {seed} centre {centre:?} r={radius} holds \
         {stone} stone: that is not a generated world, so its zeroes prove nothing"
    );

    for marker in markers {
        let served = decorated.get(*marker).copied().unwrap_or(0);
        let control = carved.get(*marker).copied().unwrap_or(0);
        assert_eq!(
            control, 0,
            "{marker} is in the carvers world at seed {seed} centre {centre:?} \
             r={radius}, so it is not a structure marker and cannot witness one"
        );
        assert!(
            served > 0,
            "no {marker} reaches the wire at seed {seed} centre {centre:?} \
             r={radius}: either the structure stage stopped running or the \
             chunk encoder is dropping what it wrote"
        );
    }
}

/// A mineshaft and a trial chamber, in one window.
///
/// Seed 0 chunk (14,5) is a `trial_chambers` start — `worldgen_assembly`'s own
/// `trial_chamber_report` names it, union box x 196..287, y −52..8, z 37..146
/// — and the mineshaft that shares the window is unrelated to it.
#[test]
fn a_mineshaft_and_a_trial_chamber_reach_the_wire() {
    window_serves(
        0,
        (14, 5),
        3,
        &[
            "minecraft:rail",
            "minecraft:cobweb",
            "minecraft:trial_spawner",
            "minecraft:vault",
            "minecraft:chiseled_tuff_bricks",
        ],
    );
}

/// A ruined portal. `netherrack` in the overworld comes from nothing else.
#[test]
fn a_ruined_portal_reaches_the_wire() {
    window_serves(
        0,
        (20, 9),
        1,
        &["minecraft:netherrack", "minecraft:gold_block"],
    );
}

/// A savanna village — the structure the retired claim named first.
#[test]
fn a_village_reaches_the_wire() {
    window_serves(
        0,
        (38, 45),
        3,
        &[
            "minecraft:dirt_path",
            "minecraft:farmland",
            "minecraft:wheat",
        ],
    );
}

/// A warm ocean ruin. Sandstone, sea lanterns and the archaeology sand.
#[test]
fn an_ocean_ruin_reaches_the_wire() {
    window_serves(
        0,
        (27, 20),
        1,
        &[
            "minecraft:cut_sandstone",
            "minecraft:sea_lantern",
            "minecraft:suspicious_sand",
        ],
    );
}

/// A dark-oak shipwreck, straddling chunks (-21,-33) and (-21,-32).
///
/// This window holds a submerged one, so a green here says nothing about the
/// beached branch — that is [`a_beached_shipwreck_reaches_the_wire`]'s.
#[test]
fn a_shipwreck_reaches_the_wire() {
    window_serves(
        0,
        (-21, -32),
        1,
        &["minecraft:dark_oak_planks", "minecraft:dark_oak_trapdoor"],
    );
}

/// A beached shipwreck: seed 31415926, start chunk (84,57), the one vanilla
/// recorded in `vanilla-s2-r7`'s structure-start ring and never generated
/// there. Clamped twin harvests of r=3 around it read 0 of 1,296 positions
/// wrong inside its box against 383 before the beached port.
#[test]
fn a_beached_shipwreck_reaches_the_wire() {
    window_serves(
        31_415_926,
        (84, 57),
        1,
        &["minecraft:dark_oak_stairs", "minecraft:jungle_fence"],
    );
}

/// A swamp hut: seed 42, start chunk (-17,22), facing east. A cauldron and a
/// potted red mushroom are placed by the hut and by nothing a carvers-only
/// world holds.
#[test]
fn a_swamp_hut_reaches_the_wire() {
    window_serves(
        42,
        (-17, 22),
        1,
        &["minecraft:cauldron", "minecraft:potted_red_mushroom"],
    );
}

/// A jungle temple: seed 31415926, start chunk (44,38), facing east. Its
/// trap is the only source of sticky pistons and tripwire hooks in a world.
#[test]
fn a_jungle_temple_reaches_the_wire() {
    window_serves(
        31_415_926,
        (44, 38),
        1,
        &[
            "minecraft:sticky_piston",
            "minecraft:tripwire_hook",
            "minecraft:chiseled_stone_bricks",
        ],
    );
}

/// A desert pyramid: seed 2024, start chunk (4,-19), facing south. The
/// pressure plate and the TNT are its trap; blue terracotta its floor star.
#[test]
fn a_desert_pyramid_reaches_the_wire() {
    window_serves(
        2024,
        (4, -19),
        1,
        &[
            "minecraft:stone_pressure_plate",
            "minecraft:tnt",
            "minecraft:blue_terracotta",
        ],
    );
}

/// An igloo: seed 12345, start chunk (36,41) — the one
/// `gen-wide-12345-r16-ow-a` records in its start ring. The carpet and the
/// bed are the dome's furniture.
#[test]
fn an_igloo_reaches_the_wire() {
    window_serves(
        12_345,
        (36, 41),
        1,
        &["minecraft:white_carpet", "minecraft:red_bed"],
    );
}

/// A pillager outpost: seed 9, start chunk (10,-19), 17 pieces as vanilla
/// saves them. The banners and the dark-oak watchtower; a carvers world grows
/// no trees.
#[test]
fn a_pillager_outpost_reaches_the_wire() {
    window_serves(
        9,
        (10, -19),
        2,
        &["minecraft:white_wall_banner", "minecraft:dark_oak_log"],
    );
}

/// Trail ruins: seed 42, start chunk (2,-34), buried under the surface. Mud
/// bricks, glazed terracotta, and the capped archaeology processor's
/// suspicious gravel.
#[test]
fn trail_ruins_reach_the_wire() {
    window_serves(
        42,
        (2, -35),
        2,
        &[
            "minecraft:mud_bricks",
            "minecraft:light_gray_glazed_terracotta",
            "minecraft:suspicious_gravel",
        ],
    );
}

/// A stronghold's portal room: seed 12345, the ring-1 stronghold whose start
/// is chunk (110,23); its portal room sits in chunk (114,23..24). The frames
/// and the silverfish spawner are the room's own.
#[test]
fn a_stronghold_reaches_the_wire() {
    window_serves(
        12_345,
        (114, 24),
        1,
        &["minecraft:end_portal_frame", "minecraft:spawner"],
    );
}

/// A woodland mansion: seed 8, the start at chunk (32,16), whose 615 pieces
/// match vanilla's saved `Children` in order. Dark oak planks and cobblestone
/// walls are the mansion's own; neither grows in a dark forest.
#[test]
fn a_woodland_mansion_reaches_the_wire() {
    window_serves(
        8,
        (33, 16),
        1,
        &["minecraft:dark_oak_planks", "minecraft:cobblestone_wall"],
    );
}

/// An ancient city: seed 12345, the start at chunk (32,37), whose 89 pieces
/// match vanilla's saved `Children` in order. Reinforced deepslate is the
/// city centre's portal frame and gray wool its carpet; neither is a natural
/// block of the deep dark.
#[test]
fn an_ancient_city_reaches_the_wire() {
    window_serves(
        12345,
        (32, 37),
        1,
        &["minecraft:reinforced_deepslate", "minecraft:gray_wool"],
    );
}

/// An ocean monument: seed 42, the start at chunk (49,-11), facing east.
/// Prismarine bricks and dark prismarine are the monument's own; no feature
/// places either.
#[test]
fn an_ocean_monument_reaches_the_wire() {
    window_serves(
        42,
        (49, -11),
        1,
        &["minecraft:prismarine_bricks", "minecraft:dark_prismarine"],
    );
}

/// The square `crates/rustmc-server/src/main.rs` generates before it listens.
///
/// Not a window chosen to hold a structure: this is `status::WORLD_SEED` and
/// `status::WORLD_RADIUS` exactly as shipped, so what it finds is what a
/// player joining this server on any given day walks into. It finds a
/// mineshaft.
#[test]
fn the_default_world_a_player_joins() {
    window_serves(
        rustmc_server::status::WORLD_SEED,
        (0, 0),
        rustmc_server::status::WORLD_RADIUS,
        &["minecraft:rail", "minecraft:cobweb"],
    );
}
