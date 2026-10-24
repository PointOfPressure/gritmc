//! The block entities an end city and a woodland mansion leave behind, against
//! vanilla's own saved compounds, and the loot seeds a player's first open
//! rolls from.
//!
//! # Why this is not a block diff
//!
//! `served_vs_vanilla.rs` and the `region-differ diff` tables compare block
//! states. A chest is one block state whether or not it carries a loot table,
//! and a banner is one block state whatever its patterns are, so every block
//! figure for these two structures was 0 while **0 of their 63 block entities
//! existed** (2026-10-02, `region-differ bediff`, vanilla first): the end
//! city's 16 banners, 3 chests and ender chest, and a mansion's 7 and 36.
//!
//! # What was missing, and why each half can go red
//!
//! * The emitter had no `banner` or `ender_chest` class, refused a
//!   metadata-only payload, and withheld every chest's template `Items`.
//! * Neither placer threaded the postProcess stream into `place_piece`, so
//!   `placeInWorld`'s one `nextLong` per template container was never spent,
//!   and neither recorded the table its `Chest` marker sets — the mansion
//!   drew the seed and dropped it, the end city never handled the marker.
//!
//! The seeds are the sharp half. A `LootTableSeed` is the stream's value at
//! one position, so it is right only if every draw before it on that chunk's
//! stream is — the template chest's draw, then the marker's, in template
//! order. Leave out either and every seed after it moves. That is also what a
//! player gets: `lootroll::adopt_generated` hands the record to the server's
//! store, and the first open rolls `LootTable.fill` on exactly this seed.
//!
//! # References
//!
//! * End: `oracle-work/van-end-20260722-c8362` (seed 20260722, centre
//!   (83,62), r=7), whose twin `-b` agrees on all 20 block entities in the
//!   city's boxes. Read at run time; the compounds are compared whole.
//! * Mansion: seed 30, start (37,-47). Its clamped twin harvests
//!   (`sl2-wm2-{a,b}`, r=4 at (37,-46), `-Dmax.bg.threads=1`, rows) live in
//!   a lane's dev directory rather than `oracle-work`, so the storeroom and
//!   its marker chest are pinned here from `-a`, where `-b` agrees on every
//!   chest in the nine chunks generated.
//!
//! # Falsified
//!
//! With `place_piece`'s stream put back to `None` in both placers, all three
//! tests go red: the end city's three seeds move, the mansion's marker seed
//! reads -237254229101828014 against vanilla's 8157007271250325635, and the
//! storeroom's 26 empty chests vanish.

use std::collections::BTreeMap;

use rustmc_world::anvil::{read_region, ChunkColumn};
use rustmc_world::nbt::Nbt;
use rustmc_worldgen::pipeline::{Area, Through};

/// A block position.
type Pos = (i32, i32, i32);
/// What a ship's block entity saves that the test reads: id, table, seed and
/// how many items.
type Saved = (String, Option<String>, Option<i64>, usize);
/// One `coded_structure_loot_is_vanillas` row: name, seed, window, and the
/// `(id, LootTable, LootTableSeed)` vanilla saved at each position.
type Case = (&'static str, i64, Area, Vec<(Pos, (String, String, i64))>);

fn repo(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// The block-entity side channel is opt-in; the server switches it on in
/// `main`, and so must anything measuring what the server serves.
fn record() {
    std::env::set_var("RUSTMC_RECORD_BLOCK_ENTITIES", "1");
}

fn full_chunks(dir: &std::path::Path) -> BTreeMap<(i32, i32), ChunkColumn> {
    let mut out = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("mca") {
            continue;
        }
        for c in read_region(&p).unwrap_or_default() {
            // A proto column has its starts and none of its block entities.
            if c.status == "minecraft:full" {
                out.insert((c.x, c.z), c);
            }
        }
    }
    out
}

fn position(be: &Nbt) -> Option<(i32, i32, i32)> {
    let Nbt::Compound(m) = be else { return None };
    let int = |k: &str| match m.get(k) {
        Some(Nbt::Int(v)) => Some(*v),
        _ => None,
    };
    Some((int("x")?, int("y")?, int("z")?))
}

fn id(be: &Nbt) -> String {
    match be {
        Nbt::Compound(m) => match m.get("id") {
            Some(Nbt::String(s)) => s.clone(),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

/// Every block entity of the reference's `end_city` starts, compound for
/// compound, against the ones the generated End's columns carry.
#[test]
fn an_end_citys_block_entities_are_vanillas() {
    let reference_dir = repo("oracle-work/van-end-20260722-c8362");
    if !rustmc_data::fixture::available(
        &reference_dir,
        "the End reference holding a city",
        "ci/gen-vanilla-region.sh",
    ) || !worldgen_assembly::have_extract()
    {
        return;
    }
    let reference = full_chunks(&reference_dir);
    let mut boxes: Vec<[i32; 6]> = Vec::new();
    for column in reference.values() {
        for start in &column.structure_starts {
            if start.id == "minecraft:end_city" {
                boxes.extend(start.pieces.iter().map(|p| p.bounding_box));
            }
        }
    }
    assert_eq!(boxes.len(), 70, "the reference's one city is 70 pieces");
    let inside = |(x, y, z): (i32, i32, i32)| {
        boxes
            .iter()
            .any(|b| b[0] <= x && x <= b[3] && b[1] <= y && y <= b[4] && b[2] <= z && z <= b[5])
    };
    let theirs: BTreeMap<(i32, i32, i32), Nbt> = reference
        .values()
        .flat_map(|c| c.block_entities.iter())
        .filter_map(|be| Some((position(be)?, be.clone())))
        .filter(|(p, _)| inside(*p))
        .collect();

    record();
    let area = Area::square(83, 62, 3);
    let generated =
        worldgen_assembly::generate::generate_end(20_260_722, area, Through::Decoration)
            .expect("the End generates");
    let mut ours: BTreeMap<(i32, i32, i32), Nbt> = BTreeMap::new();
    for (cx, cz) in area.chunks() {
        let column = rustmc_worldgen::anvil_bridge::column_from_canvas(
            &generated.canvas,
            cx,
            cz,
            "minecraft:full",
            rustmc_worldgen::anvil_bridge::DATA_VERSION_26_2,
        );
        for be in column.block_entities {
            if let Some(p) = position(&be).filter(|p| inside(*p)) {
                ours.insert(p, be);
            }
        }
    }

    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for be in theirs.values() {
        *kinds.entry(id(be)).or_default() += 1;
    }
    assert_eq!(
        kinds,
        BTreeMap::from([
            ("minecraft:banner".to_string(), 16),
            ("minecraft:chest".to_string(), 3),
            ("minecraft:ender_chest".to_string(), 1),
        ]),
        "what vanilla saved inside the city's boxes"
    );
    let wrong: Vec<_> = theirs
        .iter()
        .filter(|(p, be)| ours.get(p) != Some(be))
        .map(|(p, be)| (p, id(be), ours.get(p).map(id)))
        .collect();
    let spurious: Vec<_> = ours.keys().filter(|p| !theirs.contains_key(p)).collect();
    assert!(
        wrong.is_empty() && spurious.is_empty(),
        "{} of {} block entities differ from vanilla's compound (vanilla id, ours id): {wrong:?}; \
         {} spurious: {spurious:?}",
        wrong.len(),
        theirs.len(),
        spurious.len()
    );
}

/// What the server's store holds after generation, for a chest a player has
/// not opened: the table, and the seed the first open rolls from.
fn loot_in(
    canvas: &rustmc_worldgen::canvas::Canvas,
    area: Area,
) -> BTreeMap<(i32, i32, i32), (String, i64)> {
    let store = rustmc_server::blockentity::BlockEntities::new();
    rustmc_server::lootroll::adopt_generated(canvas, area.chunks(), &store);
    let mut out = BTreeMap::new();
    for (cx, cz) in area.chunks() {
        for (pos, be) in store.in_chunk(cx, cz) {
            if let Some(l) = be.loot() {
                out.insert(pos, (l.table.clone(), l.seed));
            }
        }
    }
    out
}

/// The three end city chests a player can open in the reference window, and
/// the seeds vanilla's own save gives them.
#[test]
fn an_end_citys_chests_roll_from_vanillas_seeds() {
    if !worldgen_assembly::have_extract() {
        return;
    }
    record();
    let area = Area::square(83, 62, 3);
    let generated =
        worldgen_assembly::generate::generate_end(20_260_722, area, Through::Decoration)
            .expect("the End generates");
    let t = "minecraft:chests/end_city_treasure".to_string();
    assert_eq!(
        loot_in(&generated.canvas, area),
        BTreeMap::from([
            ((1334, 113, 1006), (t.clone(), -6_784_931_705_338_781_005)),
            ((1336, 113, 1008), (t.clone(), 6_230_349_790_300_740_308)),
            ((1339, 137, 997), (t, -7_193_103_465_026_875_993)),
        ])
    );
}

/// A woodland mansion's storeroom and the marker chest behind it, seed 30,
/// start (37,-47), chunks 35..37 x -48..-46.
///
/// The 26 storeroom chests are template chests whose tag is `{Items:[]}`:
/// vanilla saves them as `Items:[]` and our emitter may only say so when the
/// placer recorded `placeInWorld`'s draw, which it does only with the stream
/// threaded. The marker chest at (590,88,-747) shares a chunk with some of
/// them, so its seed is the stream's value AFTER their draws. Measured with
/// the threading removed (2026-10-02, `region-differ bediff` in-box): the 26
/// read missing and this seed read a payload mismatch.
///
/// Pinned from `sl2-wm2-a` (clamped, rows, r=4 at (37,-46)); `-b` agrees on
/// every chest in these nine chunks.
#[test]
fn a_mansions_chests_are_vanillas() {
    if !worldgen_assembly::have_extract() {
        return;
    }
    record();
    let area = Area::square(36, -47, 1);
    let generated =
        worldgen_assembly::generate::generate(30, area, Through::Decoration).expect("generates");
    let t = "minecraft:chests/woodland_mansion".to_string();
    let got: BTreeMap<_, _> = loot_in(&generated.canvas, area)
        .into_iter()
        .filter(|(_, (table, _))| *table == t)
        .collect();
    assert_eq!(
        got,
        BTreeMap::from([((590, 88, -747), (t, 8_157_007_271_250_325_635))])
    );

    let mut empty = 0;
    let mut named = Vec::new();
    for (cx, cz) in area.chunks() {
        let column = rustmc_worldgen::anvil_bridge::column_from_canvas(
            &generated.canvas,
            cx,
            cz,
            "minecraft:full",
            rustmc_worldgen::anvil_bridge::DATA_VERSION_26_2,
        );
        for be in &column.block_entities {
            let Nbt::Compound(m) = be else { continue };
            if id(be) != "minecraft:chest" || m.contains_key("LootTable") {
                continue;
            }
            match m.get("Items") {
                Some(Nbt::List(items)) if items.is_empty() => empty += 1,
                Some(Nbt::List(items)) => named.push((position(be), items.len())),
                _ => {}
            }
        }
    }
    assert_eq!(
        empty, 26,
        "the storeroom's empty chests, as vanilla saves them"
    );
    assert_eq!(
        named,
        vec![(Some((599, 74, -733)), 1)],
        "the one template chest with an item"
    );
}

/// A city whose tower bridge grew a ship: seed 20260722, start (81,3), 20
/// pieces. Pinned from clamped twin End harvests `sl2-ec1-{a,b}` (r=4 at
/// (82,0), rows), which agree on all six block entities: four
/// `end_city_treasure` chests (two in the ship's hold), the ship's brewing
/// stand with its two potions of strong healing, and the dragon head on its
/// bow. Until these harvests existed no reference this project held had a
/// ship in it, and the ship branch was a caveat.
#[test]
fn an_end_city_ships_block_entities_are_vanillas() {
    if !worldgen_assembly::have_extract() {
        return;
    }
    record();
    let area = Area::square(82, 0, 3);
    let generated =
        worldgen_assembly::generate::generate_end(20_260_722, area, Through::Decoration)
            .expect("the End generates");
    let mut got: BTreeMap<Pos, Saved> = BTreeMap::new();
    for (cx, cz) in area.chunks() {
        let column = rustmc_worldgen::anvil_bridge::column_from_canvas(
            &generated.canvas,
            cx,
            cz,
            "minecraft:full",
            rustmc_worldgen::anvil_bridge::DATA_VERSION_26_2,
        );
        for be in &column.block_entities {
            let Nbt::Compound(m) = be else { continue };
            let table = match m.get("LootTable") {
                Some(Nbt::String(t)) => Some(t.clone()),
                _ => None,
            };
            let seed = match m.get("LootTableSeed") {
                Some(Nbt::Long(s)) => Some(*s),
                _ => None,
            };
            let items = match m.get("Items") {
                Some(Nbt::List(l)) => l.len(),
                _ => 0,
            };
            if let Some(p) = position(be) {
                got.insert(p, (id(be), table, seed, items));
            }
        }
    }
    let t = || Some("minecraft:chests/end_city_treasure".to_string());
    let chest = "minecraft:chest".to_string();
    assert_eq!(
        got,
        BTreeMap::from([
            (
                (1305, 118, -28),
                (chest.clone(), t(), Some(-8_142_429_475_052_324_032), 0)
            ),
            (
                (1306, 121, -10),
                ("minecraft:brewing_stand".to_string(), None, None, 2)
            ),
            (
                (1306, 122, -35),
                ("minecraft:skull".to_string(), None, None, 0)
            ),
            (
                (1307, 118, -28),
                (chest.clone(), t(), Some(-8_201_755_498_446_625_539), 0)
            ),
            (
                (1310, 116, 56),
                (chest.clone(), t(), Some(-1_999_820_346_268_238_787), 0)
            ),
            (
                (1312, 116, 54),
                (chest, t(), Some(5_495_759_529_435_871_858), 0)
            ),
        ])
    );
}

/// What the generated columns of `area` save, by position: the type id, the
/// `LootTable` and the `LootTableSeed`.
fn loot_saved(seed: i64, area: Area) -> BTreeMap<(i32, i32, i32), (String, String, i64)> {
    record();
    let generated =
        worldgen_assembly::generate::generate(seed, area, Through::Decoration).expect("generates");
    let mut out = BTreeMap::new();
    for (cx, cz) in area.chunks() {
        let column = rustmc_worldgen::anvil_bridge::column_from_canvas(
            &generated.canvas,
            cx,
            cz,
            "minecraft:full",
            rustmc_worldgen::anvil_bridge::DATA_VERSION_26_2,
        );
        for be in &column.block_entities {
            let Nbt::Compound(m) = be else { continue };
            let (Some(Nbt::String(table)), Some(Nbt::Long(s)), Some(p)) =
                (m.get("LootTable"), m.get("LootTableSeed"), position(be))
            else {
                continue;
            };
            out.insert(p, (id(be), table.clone(), *s));
        }
    }
    out
}

/// One chest or suspicious block per structure whose placer writes a table
/// by code — `createChest`, `createDispenser`, a data marker, a rule's
/// `append_loot`, `placeSuspiciousSand` — at the seed vanilla saved. Every row
/// is pinned from a clamped twin harvest of this lane (`sl2-<name>-a`, rows,
/// whose `-b` agrees), and every one of them was absent before 2026-10-02:
/// the placers drew the seed or skipped the draw and kept nothing.
#[test]
fn coded_structure_loot_is_vanillas() {
    if !worldgen_assembly::have_extract() {
        return;
    }
    let s = |x: &str| x.to_string();
    let cases: [Case; 8] = [
        (
            "stronghold (sl2-sh1)",
            12_345,
            Area::square(111, 23, 1),
            vec![
                (
                    (1774, -18, 368),
                    (
                        s("minecraft:chest"),
                        s("minecraft:chests/stronghold_corridor"),
                        -8_081_048_912_504_694_134,
                    ),
                ),
                (
                    (1793, -6, 379),
                    (
                        s("minecraft:chest"),
                        s("minecraft:chests/stronghold_library"),
                        7_176_684_546_426_977_686,
                    ),
                ),
            ],
        ),
        (
            "jungle temple (sl2-jungle1)",
            31_415_926,
            Area::square(44, 38, 1),
            vec![
                (
                    (705, 67, 611),
                    (
                        s("minecraft:dispenser"),
                        s("minecraft:chests/jungle_temple_dispenser"),
                        4_029_026_636_676_205_432,
                    ),
                ),
                (
                    (707, 66, 616),
                    (
                        s("minecraft:chest"),
                        s("minecraft:chests/jungle_temple"),
                        -9_090_932_767_545_854_567,
                    ),
                ),
            ],
        ),
        (
            "trail ruins (sl2-trail2)",
            42,
            Area::square(1, -35, 1),
            vec![
                (
                    (22, 88, -545),
                    (
                        s("minecraft:brushable_block"),
                        s("minecraft:archaeology/trail_ruins_common"),
                        -1_573_631_738_053_661_360,
                    ),
                ),
                (
                    (22, 90, -546),
                    (
                        s("minecraft:brushable_block"),
                        s("minecraft:archaeology/trail_ruins_rare"),
                        9_008_223_155_355_885_994,
                    ),
                ),
            ],
        ),
        (
            "desert pyramid (sl2-pyr2)",
            2024,
            Area::square(4, -19, 1),
            vec![(
                (72, 53, -294),
                (
                    s("minecraft:chest"),
                    s("minecraft:chests/desert_pyramid"),
                    -1_084_719_576_820_247_650,
                ),
            )],
        ),
        (
            "cold ocean ruin (sl2-cold1)",
            12_345,
            Area::square(64, 66, 1),
            vec![(
                (1038, 51, 1070),
                (
                    s("minecraft:chest"),
                    s("minecraft:chests/underwater_ruin_small"),
                    -4_755_029_527_994_522_164,
                ),
            )],
        ),
        (
            "beached shipwreck (sl2-wreck1)",
            31_415_926,
            Area::square(84, 58, 1),
            vec![
                (
                    (1346, 61, 930),
                    (
                        s("minecraft:chest"),
                        s("minecraft:chests/shipwreck_treasure"),
                        1_975_088_659_968_662_293,
                    ),
                ),
                (
                    (1347, 59, 936),
                    (
                        s("minecraft:chest"),
                        s("minecraft:chests/shipwreck_map"),
                        -4_407_334_701_122_478_909,
                    ),
                ),
            ],
        ),
        (
            "buried treasure (sl2-wreck1)",
            31_415_926,
            Area::square(87, 55, 1),
            vec![(
                (1401, 64, 889),
                (
                    s("minecraft:chest"),
                    s("minecraft:chests/buried_treasure"),
                    7_553_302_194_029_431_463,
                ),
            )],
        ),
        (
            "igloo basement (sl2-igloo2)",
            12_345,
            Area::square(20, 72, 1),
            vec![(
                (321, 59, 1156),
                (
                    s("minecraft:chest"),
                    s("minecraft:chests/igloo_chest"),
                    -17_627_126_690_112_351,
                ),
            )],
        ),
    ];
    let mut wrong = Vec::new();
    for (name, seed, area, want) in cases {
        let got = loot_saved(seed, area);
        for (pos, expected) in want {
            if got.get(&pos) != Some(&expected) {
                wrong.push(format!(
                    "{name} {pos:?}: vanilla {expected:?}, ours {:?}",
                    got.get(&pos)
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Every evoker, vindicator, allay and elder guardian a mansion or monument
/// spawns, against vanilla's saved compounds, down to the two values
/// `finalizeSpawn` draws from the chunk's level random: `follow_range`'s
/// `random_spawn_bonus` amount (as its f64 bits) and `LeftHanded`.
///
/// Pinned from the clamped twin harvests' own `entities/` files (the world
/// directories behind `sl2-wm1-{a,b}` and `sl2-om1-a`), FNV-1a 64 over the
/// sorted `id,x,y,z,amount-bits,left-handed` lines; the two seed 8 runs
/// agree on all 32 (one of them left-handed). The full compounds, typed,
/// match key for key apart from `UUID` (scratch `entcmp2.py`; 96 of 96 over
/// both mansions and all four monuments).
#[test]
fn structure_mobs_draw_vanillas_spawn_bonus_and_handedness() {
    if !worldgen_assembly::have_extract() {
        return;
    }
    record();
    let fingerprint = |seed: i64, area: Area| -> (usize, String) {
        let generated = worldgen_assembly::generate::generate(seed, area, Through::Decoration)
            .expect("generates");
        let mut rows = Vec::new();
        for (cx, cz) in area.chunks() {
            for e in generated.canvas.entity_records(cx, cz) {
                let Nbt::Compound(m) = e else { continue };
                let name = id(e);
                if !matches!(
                    name.as_str(),
                    "minecraft:evoker"
                        | "minecraft:vindicator"
                        | "minecraft:allay"
                        | "minecraft:elder_guardian"
                ) {
                    continue;
                }
                let Some(Nbt::List(pos)) = m.get("Pos") else {
                    continue;
                };
                let coord = |i: usize| match pos.get(i) {
                    #[allow(clippy::cast_possible_truncation)]
                    Some(Nbt::Double(v)) => v.floor() as i64,
                    _ => i64::MIN,
                };
                let amount = e
                    .get("attributes")
                    .and_then(|a| match a {
                        Nbt::List(l) => l.first(),
                        _ => None,
                    })
                    .and_then(|a| a.get("modifiers"))
                    .and_then(|l| match l {
                        Nbt::List(l) => l.first(),
                        _ => None,
                    })
                    .and_then(|m| m.get("amount"));
                let Some(Nbt::Double(amount)) = amount else {
                    continue;
                };
                let left = matches!(m.get("LeftHanded"), Some(Nbt::Byte(1)));
                rows.push(format!(
                    "{name},{},{},{},{:016x},{}",
                    coord(0),
                    coord(1),
                    coord(2),
                    amount.to_bits(),
                    u8::from(left)
                ));
            }
        }
        rows.sort();
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for row in &rows {
            for byte in row.bytes().chain(std::iter::once(b'\n')) {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        (rows.len(), format!("{hash:#x}"))
    };
    assert_eq!(
        fingerprint(8, Area::square(34, 15, 4)),
        (32, "0x2e3e2d9ddabf9230".to_string()),
        "the seed 8 mansion's 32 mobs"
    );
    assert_eq!(
        fingerprint(42, Area::square(49, -11, 4)),
        (3, "0x65bdb753c0543f59".to_string()),
        "the seed 42 monument's three elder guardians"
    );
}

/// The live server's level after it generates a tile: the structure
/// entities `World::generate_in` hands to `Entities::shared()`, by kind and
/// exact position, against vanilla's saved `entities/` files — the seed 8
/// mansion window (32: 22 vindicators, 2 evokers, 8 allays in 4 groups) and
/// the seed 20260722 ship city (13 shulkers and the elytra frame). FNV-1a 64
/// over the sorted `id,x,y,z` lines (positions as f64 debug strings), pinned
/// from `sl2-wm1-a` and `sl2-ec1-a`'s world directories.
///
/// Each one must also resolve a network type id (an `add_entity` a client
/// can read), the mobs must carry their maximum health in their synched
/// data, and the frame its elytra.
#[test]
fn the_server_level_holds_the_structure_entities_vanilla_saved() {
    use rustmc_server::entities::Entities;
    use rustmc_server::world::{World, WorldTables};
    if !worldgen_assembly::have_extract() || !repo("data/vanilla/block_props.json").exists() {
        return;
    }
    record();
    let registries = rustmc_server::configuration::CapturedRegistries::load(&repo(
        "data/vanilla/registries.bin",
    ))
    .expect("registry capture");
    let biomes = registries
        .entry_ids("minecraft:worldgen/biome")
        .expect("readable capture")
        .expect("the capture holds the biome registry");
    let ids = rustmc_server::terrain::StateIds::load(&repo("data/vanilla/block_props.json"))
        .expect("block table");
    let tables = std::sync::Arc::new(WorldTables::load(&biomes, ids).expect("block tables"));
    let kinds = rustmc_server::entities::shared_kind_ids().expect("entity type ids");
    let check = |dimension, seed: i64, area: Area, want: (usize, &str)| -> World {
        let world = World::generate_in(
            dimension,
            seed,
            area,
            std::sync::Arc::clone(&tables),
            Through::Decoration,
        )
        .expect("generated");
        let mut rows = Vec::new();
        for e in Entities::shared().all() {
            #[allow(clippy::cast_possible_truncation)]
            let (cx, cz) = ((e.x.floor() as i32) >> 4, (e.z.floor() as i32) >> 4);
            if !area.chunks().any(|c| c == (cx, cz)) {
                continue;
            }
            assert!(
                kinds.get(e.kind).is_some(),
                "{} has no network type id",
                e.kind.name
            );
            let values = e.synched.non_default_values().unwrap_or_default();
            if e.kind.name == "minecraft:item_frame" {
                assert!(
                    values.iter().any(|v| v.id == 9),
                    "the frame holds its elytra"
                );
            } else if e.kind.name != "minecraft:chest_minecart" {
                assert!(
                    values.iter().any(|v| v.id == 9),
                    "{} synchs its health",
                    e.kind.name
                );
            }
            rows.push(format!("{},{:?},{:?},{:?}", e.kind.name, e.x, e.y, e.z));
        }
        rows.sort();
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for row in &rows {
            for byte in row.bytes().chain(std::iter::once(b'\n')) {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        assert_eq!(
            (rows.len(), format!("{hash:#x}")),
            (want.0, want.1.to_string())
        );
        world
    };
    let mansion = check(
        rustmc_server::dimension::Dimension::Overworld,
        8,
        Area::square(34, 15, 4),
        (32, "0x1b7cbe8db580764c"),
    );
    // And the save: a chunk the server writes carries its generated
    // entities into the sibling `entities/` store. The mansion's chunks are
    // saved because their chests are in the block-entity store, and every
    // mob standing in one of them must come back out of `entities/`.
    let dir = std::env::temp_dir().join(format!("sbe-save-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let saved = mansion.save_to(&dir.join("region")).expect("saved");
    assert!(saved > 0, "the mansion's chest chunks are saved");
    let mut back = 0;
    for entry in std::fs::read_dir(dir.join("entities")).expect("an entities/ directory") {
        for chunk in
            rustmc_world::entitystore::read_region(&entry.expect("entry").path()).expect("readable")
        {
            assert!(!chunk.entities.is_empty());
            back += chunk.entities.len();
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    // Vanilla's own count for the same chunks: the four that hold a loot
    // container in `sl2-wm1-a` — (32,16), (33,16), (34,14), (34,15) — carry
    // eight of the mansion's mobs in its `entities/` files.
    assert_eq!(back, 8, "mobs standing in the mansion's saved chunks");
    let _ = check(
        rustmc_server::dimension::Dimension::TheEnd,
        20_260_722,
        Area::square(82, 0, 4),
        (14, "0xf9d1c9711c68a270"),
    );
}

/// A mineshaft corridor's chest minecarts: `setInitialPos(x + 0.5, y + 0.5,
/// z + 0.5)` and `setLootTable(ABANDONED_MINESHAFT, random.nextLong())`,
/// the seed from the corridor's own postProcess stream. Pinned from both
/// clamped runs of `sl2-igloo1` (seed 12345, centre (36,41) r=3), which agree
/// on all four; before 2026-10-02 the draw was spent and no minecart existed.
#[test]
fn mineshaft_chest_minecarts_are_vanillas() {
    if !worldgen_assembly::have_extract() {
        return;
    }
    record();
    let area = Area::square(36, 41, 3);
    let generated = worldgen_assembly::generate::generate(12_345, area, Through::Decoration)
        .expect("generates");
    let mut got = Vec::new();
    for (cx, cz) in area.chunks() {
        for e in generated.canvas.entity_records(cx, cz) {
            if id(e) != "minecraft:chest_minecart" {
                continue;
            }
            let Nbt::Compound(m) = e else { continue };
            let (Some(Nbt::List(pos)), Some(Nbt::Long(seed)), Some(Nbt::String(table))) =
                (m.get("Pos"), m.get("LootTableSeed"), m.get("LootTable"))
            else {
                continue;
            };
            assert_eq!(table, "minecraft:chests/abandoned_mineshaft");
            let p: Vec<String> = pos.iter().map(|v| format!("{v:?}")).collect();
            got.push((p.join(","), *seed));
        }
    }
    got.sort();
    let d = |x: f64, y: f64, z: f64| {
        [x, y, z]
            .iter()
            .map(|v| format!("{:?}", Nbt::Double(*v)))
            .collect::<Vec<_>>()
            .join(",")
    };
    let mut want = vec![
        (d(571.5, 32.5, 673.5), 7_612_154_386_917_417_481),
        (d(587.5, 36.5, 669.5), -3_946_600_008_378_780_144),
        (d(631.5, 34.5, 629.5), -7_422_342_581_776_022_213),
        (d(637.5, 38.5, 662.5), -4_571_899_099_640_746_981),
    ];
    want.sort();
    assert_eq!(got, want);
}

/// A swamp hut's witch and its black cat, spawned once each at local
/// (2,2,5) and finalized from the level random — the cat's `variant` from
/// `#cats_spawn_as_black` (`nextInt(1)`) and its `sound_variant` from
/// `nextInt(2)` over `classic`, `royal`. Pinned from both clamped runs of
/// `sl2-hut1` (seed 31415926, centre (0,-21) r=3): the witch's
/// `random_spawn_bonus` bits, the cat's, handedness, and the cat's `royal`.
#[test]
fn a_swamp_huts_witch_and_cat_are_vanillas() {
    if !worldgen_assembly::have_extract() {
        return;
    }
    record();
    let area = Area::square(0, -21, 3);
    let generated = worldgen_assembly::generate::generate(31_415_926, area, Through::Decoration)
        .expect("generates");
    let mut rows = Vec::new();
    for (cx, cz) in area.chunks() {
        for e in generated.canvas.entity_records(cx, cz) {
            let name = id(e);
            if name != "minecraft:witch" && name != "minecraft:cat" {
                continue;
            }
            let Nbt::Compound(m) = e else { continue };
            let Some(Nbt::List(pos)) = m.get("Pos") else {
                continue;
            };
            let coord = |i: usize| match pos.get(i) {
                #[allow(clippy::cast_possible_truncation)]
                Some(Nbt::Double(v)) => v.floor() as i64,
                _ => i64::MIN,
            };
            let amount = e
                .get("attributes")
                .and_then(|a| match a {
                    Nbt::List(l) => l.first(),
                    _ => None,
                })
                .and_then(|a| a.get("modifiers"))
                .and_then(|l| match l {
                    Nbt::List(l) => l.first(),
                    _ => None,
                })
                .and_then(|m| m.get("amount"));
            let Some(Nbt::Double(amount)) = amount else {
                continue;
            };
            let sound = match m.get("sound_variant") {
                Some(Nbt::String(s)) => s.clone(),
                _ => "-".to_string(),
            };
            let left = u8::from(matches!(m.get("LeftHanded"), Some(Nbt::Byte(1))));
            rows.push(format!(
                "{name},{},{},{},{:016x},{left},{sound}",
                coord(0),
                coord(1),
                coord(2),
                amount.to_bits()
            ));
        }
    }
    rows.sort();
    assert_eq!(
        rows,
        vec![
            "minecraft:cat,3,65,-334,3fafbccf5e4ceef7,0,minecraft:royal".to_string(),
            "minecraft:witch,3,65,-334,bfa2d2da4e2e3af7,0,-".to_string(),
        ]
    );
}

/// An ocean ruin's drowned, from its `drowned` markers: everything
/// `finalizeSpawn` draws from the level random — `Mob`'s follow-range bonus
/// and handedness, then `Zombie`'s loot/baby/door rolls, `Drowned`'s
/// equipment roll (here a trident) and nautilus shell (here one, left-handed)
/// — pinned from both clamped runs of `sl2-cold1` (seed 12345, centre
/// (65,67) r=3), which agree on all five.
#[test]
fn an_ocean_ruins_drowned_are_vanillas() {
    if !worldgen_assembly::have_extract() {
        return;
    }
    record();
    let area = Area::square(65, 67, 3);
    let generated = worldgen_assembly::generate::generate(12_345, area, Through::Decoration)
        .expect("generates");
    let mut rows = Vec::new();
    for (cx, cz) in area.chunks() {
        for e in generated.canvas.entity_records(cx, cz) {
            if id(e) != "minecraft:drowned" {
                continue;
            }
            let Nbt::Compound(m) = e else { continue };
            let Some(Nbt::List(pos)) = m.get("Pos") else {
                continue;
            };
            let coord = |i: usize| match pos.get(i) {
                #[allow(clippy::cast_possible_truncation)]
                Some(Nbt::Double(v)) => v.floor() as i64,
                _ => i64::MIN,
            };
            let Some(Nbt::List(attributes)) = m.get("attributes") else {
                continue;
            };
            let amount = attributes
                .iter()
                .find(|a| {
                    matches!(a.get("id"), Some(Nbt::String(s)) if s == "minecraft:follow_range")
                })
                .and_then(|a| a.get("modifiers"))
                .and_then(|l| match l {
                    Nbt::List(l) => l.first(),
                    _ => None,
                })
                .and_then(|m| m.get("amount"));
            let Some(Nbt::Double(amount)) = amount else {
                continue;
            };
            let mut equipment: Vec<String> = match m.get("equipment") {
                Some(Nbt::Compound(eq)) => eq
                    .iter()
                    .map(|(slot, stack)| match stack.get("id") {
                        Some(Nbt::String(i)) => format!("{slot}:{i}"),
                        _ => slot.clone(),
                    })
                    .collect(),
                _ => Vec::new(),
            };
            equipment.sort();
            let left = u8::from(matches!(m.get("LeftHanded"), Some(Nbt::Byte(1))));
            rows.push(format!(
                "{},{},{},{:016x},{left},{}",
                coord(0),
                coord(1),
                coord(2),
                amount.to_bits(),
                if equipment.is_empty() {
                    "-".to_string()
                } else {
                    equipment.join("+")
                }
            ));
        }
    }
    rows.sort();
    assert_eq!(
        rows,
        vec![
            "1037,57,1068,bf713ae5b876beee,0,-",
            "1037,57,1071,bfb732c9159da0fb,0,-",
            "1038,52,1069,3f8f7db4e795c4cf,0,mainhand:minecraft:trident",
            "1038,57,1070,bee2e8421c20a7b5,1,offhand:minecraft:nautilus_shell",
            "1039,57,1068,bfa1ea575ddbaa1c,0,-",
        ]
    );
}
