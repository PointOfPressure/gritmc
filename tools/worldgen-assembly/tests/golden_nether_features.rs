//! The nether's forest features and its fortress against every vanilla world
//! of a window.
//!
//! Seed 31415926, centre chunk (-200, 200): the one measured window that holds
//! `crimson_forest` and `warped_forest` side by side. Three vanilla harvests of
//! it exist (`oracle-work/van-nether-31415926-c-200_200-open-{a,b,c}`, all
//! `gen_order=rows`, so our decoration runs `zx`), and **they disagree with
//! each other** — a nether harvest is not reproducible and the
//! `-Dmax.bg.threads=1` clamp does not hold in this dimension. So nothing here
//! compares against one reference. Every count is taken against all three,
//! and the bar is what vanilla does to itself on the same positions.
//!
//! # What is counted
//!
//! Positions inside the square where two worlds disagree **and** at least one
//! of them holds a block of a forest family — stems, wart blocks, shroomlight,
//! the two vines, roots, fungi, sprouts. Charged to the family of the vanilla
//! side when it has one, else ours. Terrain the forests do not write is not
//! this test's business, and leaving it in would let a carver change move a
//! feature gate.
//!
//! # Why it can go red
//!
//! Before `nether_forest_vegetation`, `huge_fungus`, `weeping_vines` and
//! `twisting_vines` were ported this port wrote **none** of these blocks, so
//! every vanilla one was a mismatch: at the 2026-10-02 base this square
//! charges thousands of stem, wart and vine blocks against each reference
//! where the references charge each other hundreds. Delete any one of the
//! four features and its family's count leaves vanilla's band; the presence
//! assertion catches a feature that writes the right family in the wrong
//! amount, which the band alone would not (zero of a rare family is close to
//! the floor).

//! # The fossil case
//!
//! Seed 20260722 around chunk (64, 64): bone blocks and the dried ghast
//! `NetherFossilPiece.placeDriedGhast` drops beside half the fossils. Before
//! 2026-10-02 this port placed no dried ghast at all.
//!
//! # The portal case
//!
//! The `in_nether` ruined portal at seed 20260722 chunk (48, 64), placed for
//! the first time on 2026-10-02, blackstone replacement and all.
//!
//! # The fortress case
//!
//! The same instrument over the fortress started at chunk (12, 3), on the
//! square centred three chunks east of it where its castle half stands (two
//! stalk rooms, three corridor stairs, a throne), with the fortress
//! families: nether bricks, nether brick fence (every connection state, so a
//! fence the post-process pass did not reshape counts), nether brick stairs,
//! nether wart, spawner and chest. Before `structure/fortress.rs` wrote blocks
//! this port placed none of them; the walk itself is gated separately and
//! sharper, in `rustmc-worldgen`'s `golden_fortress_start`.

use std::collections::BTreeMap;
use std::path::Path;

use rustmc_worldgen::pipeline::{Area, Through};

fn plain(state: &str) -> &str {
    state.split_once('[').map_or(state, |(n, _)| n)
}

/// One measured square: where, which references, which block families.
struct Case {
    seed: i64,
    centre: (i32, i32),
    /// Chunks either side of the centre. The references were harvested at
    /// radius 7, so any radius up to that is inside vanilla's decorated area.
    radius: i32,
    refs: &'static str,
    families: &'static [&'static str],
    family: fn(&str) -> Option<&'static str>,
    /// Also compare the chests' loot tables and seeds and the spawners'
    /// mobs, block entity for block entity, against every reference.
    block_entities: bool,
}

fn forest_family(state: &str) -> Option<&'static str> {
    Some(match plain(state) {
        "minecraft:crimson_stem" | "minecraft:warped_stem" => "stem",
        "minecraft:nether_wart_block" | "minecraft:warped_wart_block" => "wart",
        "minecraft:shroomlight" => "shroomlight",
        "minecraft:weeping_vines" | "minecraft:weeping_vines_plant" => "weeping_vines",
        "minecraft:twisting_vines" | "minecraft:twisting_vines_plant" => "twisting_vines",
        "minecraft:crimson_roots" | "minecraft:warped_roots" => "roots",
        "minecraft:crimson_fungus" | "minecraft:warped_fungus" => "fungus",
        "minecraft:nether_sprouts" => "sprouts",
        _ => return None,
    })
}

fn fortress_family(state: &str) -> Option<&'static str> {
    Some(match plain(state) {
        "minecraft:nether_bricks" => "bricks",
        "minecraft:nether_brick_fence" => "fence",
        "minecraft:nether_brick_stairs" => "stairs",
        "minecraft:nether_wart" => "nether_wart",
        "minecraft:spawner" => "spawner",
        "minecraft:chest" => "chest",
        _ => return None,
    })
}

const FOREST: Case = Case {
    seed: 31_415_926,
    centre: (-200, 200),
    radius: 3,
    refs: "van-nether-31415926-c-200_200-open-",
    families: &[
        "stem",
        "wart",
        "shroomlight",
        "weeping_vines",
        "twisting_vines",
        "roots",
        "fungus",
        "sprouts",
    ],
    family: forest_family,
    block_entities: false,
};

const FORTRESS: Case = Case {
    seed: 31_415_926,
    centre: (15, 3),
    radius: 3,
    refs: "van-nether-31415926-c12_3-open-",
    families: &[
        "bricks",
        "fence",
        "stairs",
        "nether_wart",
        "spawner",
        "chest",
    ],
    family: fortress_family,
    block_entities: true,
};

fn fossil_family(state: &str) -> Option<&'static str> {
    Some(match plain(state) {
        "minecraft:bone_block" => "bone_block",
        "minecraft:dried_ghast" => "dried_ghast",
        _ => return None,
    })
}

/// Seed 20260722's nether fossils around chunk (64, 64): five dried ghasts in
/// the square, one in each of the four rotations and a second facing west.
const FOSSIL: Case = Case {
    seed: 20_260_722,
    centre: (64, 64),
    radius: 4,
    refs: "van-nether-20260722-c6464-open-",
    families: &["bone_block", "dried_ghast"],
    family: fossil_family,
    block_entities: false,
};

fn portal_family(state: &str) -> Option<&'static str> {
    Some(match plain(state) {
        "minecraft:obsidian" | "minecraft:crying_obsidian" => "obsidian",
        "minecraft:polished_blackstone_bricks" => "bricks",
        "minecraft:cracked_polished_blackstone_bricks" => "cracked_bricks",
        "minecraft:polished_blackstone_brick_slab" => "slab",
        "minecraft:polished_blackstone_brick_stairs" => "stairs",
        _ => return None,
    })
}

/// The `in_nether` ruined portal at seed 20260722 chunk (48, 64), against
/// three of the r=16 references that saved it. Its setup has
/// `replace_with_blackstone: true`, so every stone brick in the template goes
/// through `BlackstoneReplaceProcessor` — and its six cracked bricks are the
/// entry this port's map was missing.
const PORTAL: Case = Case {
    seed: 20_260_722,
    centre: (48, 64),
    radius: 1,
    refs: "gen-wide-20260722-r16-nether-",
    families: &["obsidian", "bricks", "cracked_bricks", "slab", "stairs"],
    family: portal_family,
    block_entities: false,
};

/// A world's blocks over the square, as one flat vector.
struct Square {
    states: Vec<String>,
}

fn bounds(case: &Case) -> (i32, i32, i32, i32) {
    let (cx, cz) = case.centre;
    let r = case.radius;
    (
        (cx - r) * 16,
        (cz - r) * 16,
        (cx + r) * 16 + 15,
        (cz + r) * 16 + 15,
    )
}

fn walk(case: &Case, mut at: impl FnMut(i32, i32, i32) -> String) -> Square {
    let (x0, z0, x1, z1) = bounds(case);
    let mut states = Vec::new();
    for x in x0..=x1 {
        for z in z0..=z1 {
            for y in 0..128 {
                states.push(at(x, y, z));
            }
        }
    }
    Square { states }
}

fn read_vanilla(case: &Case, dir: &Path) -> Square {
    let mut columns = BTreeMap::new();
    for entry in std::fs::read_dir(dir).expect("reference directory") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("mca") {
            continue;
        }
        for column in rustmc_world::anvil::read_region(&path).expect("region parses") {
            columns.insert((column.x, column.z), column);
        }
    }
    walk(case, |x, y, z| {
        let column = columns
            .get(&(x.div_euclid(16), z.div_euclid(16)))
            .unwrap_or_else(|| panic!("{} has no chunk at block ({x}, {z})", dir.display()));
        column
            .block_at(x, y, z)
            .unwrap_or("minecraft:air")
            .to_string()
    })
}

/// Per-family disagreements between `vanilla` and `other`, charged to the
/// vanilla side's family when it has one.
fn charge(case: &Case, vanilla: &Square, other: &Square) -> BTreeMap<&'static str, usize> {
    let mut out: BTreeMap<&'static str, usize> = case.families.iter().map(|f| (*f, 0)).collect();
    for (v, o) in vanilla.states.iter().zip(&other.states) {
        if v == o {
            continue;
        }
        if let Some(f) = (case.family)(v).or_else(|| (case.family)(o)) {
            *out.get_mut(f).expect("family listed") += 1;
        }
    }
    out
}

fn census(case: &Case, square: &Square) -> BTreeMap<&'static str, usize> {
    let mut out: BTreeMap<&'static str, usize> = case.families.iter().map(|f| (*f, 0)).collect();
    for s in &square.states {
        if let Some(f) = (case.family)(s) {
            *out.get_mut(f).expect("family listed") += 1;
        }
    }
    out
}

/// Read off the references, never asserted: every one stamps `zx`. Set once,
/// because the two tests of this binary run on two threads.
fn decorate_in_zx() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        std::env::set_var("RUSTMC_DECORATE_ORDER", "zx");
        // The block-entity side channel. It changes no block state — that is
        // `ci/be-channel-identical.sh`'s gate — so the forest case is the
        // same measurement with it on.
        std::env::set_var("RUSTMC_RECORD_BLOCK_ENTITIES", "1");
    });
}

/// One chest or spawner as a comparable line: position, then the loot table
/// and seed, or the mob.
type Be = ((i32, i32, i32), String);

fn be_line(nbt: &std::collections::BTreeMap<String, rustmc_world::nbt::Nbt>) -> Option<String> {
    use rustmc_world::nbt::Nbt;
    if let Some(Nbt::String(table)) = nbt.get("LootTable") {
        let seed = match nbt.get("LootTableSeed") {
            Some(Nbt::Long(v)) => *v,
            _ => 0,
        };
        return Some(format!("chest {table} seed {seed}"));
    }
    if let Some(Nbt::Compound(data)) = nbt.get("SpawnData") {
        if let Some(Nbt::Compound(entity)) = data.get("entity") {
            if let Some(Nbt::String(id)) = entity.get("id") {
                return Some(format!("spawner {id}"));
            }
        }
    }
    None
}

fn inside(case: &Case, x: i32, z: i32) -> bool {
    let (x0, z0, x1, z1) = bounds(case);
    (x0..=x1).contains(&x) && (z0..=z1).contains(&z)
}

fn vanilla_bes(case: &Case, dir: &Path) -> Vec<Be> {
    use rustmc_world::nbt::Nbt;
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).expect("reference directory") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("mca") {
            continue;
        }
        for column in rustmc_world::anvil::read_region(&path).expect("region parses") {
            for be in &column.block_entities {
                let Nbt::Compound(map) = be else { continue };
                let coord = |k: &str| match map.get(k) {
                    Some(Nbt::Int(v)) => *v,
                    _ => i32::MIN,
                };
                let (x, y, z) = (coord("x"), coord("y"), coord("z"));
                if !inside(case, x, z) {
                    continue;
                }
                if let Some(line) = be_line(map) {
                    out.push(((x, y, z), line));
                }
            }
        }
    }
    out.sort();
    out
}

fn check(case: &Case, name: &str) {
    let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../oracle-work"));
    let refs: Vec<_> = ["a", "b", "c"]
        .iter()
        .map(|t| root.join(format!("{}{t}", case.refs)))
        .collect();
    for r in &refs {
        assert!(
            rustmc_data::fixture::available(r, "nether reference", "ci/gen-vanilla-region.sh"),
            "missing {}",
            r.display()
        );
        let stamp = std::fs::read_to_string(r.join("HARVEST-ORDER")).expect("HARVEST-ORDER");
        assert!(
            stamp.lines().any(|l| l == "dump_order=zx"),
            "{} is not a rows harvest; this test decorates in zx",
            r.display()
        );
    }
    decorate_in_zx();

    let ours = worldgen_assembly::generate::generate_nether(
        case.seed,
        Area::square(case.centre.0, case.centre.1, case.radius),
        Through::Decoration,
    )
    .expect("generates");
    let mut our_bes: Vec<Be> = Vec::new();
    if case.block_entities {
        let (cx, cz) = case.centre;
        for chunk_x in cx - case.radius..=cx + case.radius {
            for chunk_z in cz - case.radius..=cz + case.radius {
                for (pos, record) in ours.canvas.block_entity_records(chunk_x, chunk_z) {
                    let mut nbt = record.nbt.clone();
                    if let Some(seed) = record.loot_seed {
                        nbt.insert(
                            "LootTableSeed".to_string(),
                            rustmc_world::nbt::Nbt::Long(seed),
                        );
                    }
                    if let Some(line) = be_line(&nbt) {
                        our_bes.push((*pos, line));
                    }
                }
            }
        }
        our_bes.sort();
    }
    let ours = walk(case, |x, y, z| ours.canvas.block_at(x, y, z).to_string());
    let vanilla: Vec<Square> = refs.iter().map(|r| read_vanilla(case, r)).collect();

    let mut floor: BTreeMap<&str, usize> = BTreeMap::new();
    for i in 0..vanilla.len() {
        for j in i + 1..vanilla.len() {
            for (f, n) in charge(case, &vanilla[i], &vanilla[j]) {
                let e = floor.entry(f).or_default();
                *e = (*e).max(n);
            }
        }
    }
    let mut worst: BTreeMap<&str, usize> = BTreeMap::new();
    for v in &vanilla {
        for (f, n) in charge(case, v, &ours) {
            let e = worst.entry(f).or_default();
            *e = (*e).max(n);
        }
    }
    let mine = census(case, &ours);
    let least_vanilla: BTreeMap<&str, usize> = case
        .families
        .iter()
        .map(|f| {
            (
                *f,
                vanilla
                    .iter()
                    .map(|v| census(case, v)[f])
                    .min()
                    .unwrap_or(0),
            )
        })
        .collect();

    let mut report = String::new();
    let mut failures = Vec::new();
    for &f in case.families {
        let (w, fl, have, need) = (worst[f], floor[f], mine[f], least_vanilla[f]);
        report.push_str(&format!(
            "  {f:15} ours-vs-worst-ref {w:6}  vanilla-pair-max {fl:6}  ours {have:6}  vanilla-min {need:6}\n"
        ));
        // Vanilla's own worst pair, plus a quarter, plus a small absolute
        // allowance for the rare families whose floor is a handful.
        if w > fl + fl / 4 + 16 {
            failures.push(format!("{f}: {w} against a vanilla worst pair of {fl}"));
        }
        // Within a third of the sparsest vanilla world.
        if have * 3 < need * 2 {
            failures.push(format!(
                "{f}: ours has {have}, the sparsest vanilla world {need}"
            ));
        }
    }
    if case.block_entities {
        for r in &refs {
            let theirs = vanilla_bes(case, r);
            if theirs != our_bes {
                failures.push(format!(
                    "block entities against {}: vanilla {theirs:?}, ours {our_bes:?}",
                    r.display()
                ));
            }
        }
        report.push_str(&format!(
            "  block entities  {} chests and spawners, each against all 3 references\n",
            our_bes.len()
        ));
    }
    println!(
        "NETHER {name}, seed {} centre {:?} r={}, 3 references:\n{report}",
        case.seed, case.centre, case.radius
    );
    assert!(failures.is_empty(), "{}\n{report}", failures.join("\n"));
}

#[test]
fn the_forest_features_sit_inside_vanillas_own_spread() {
    check(&FOREST, "FOREST");
}

#[test]
fn the_fortress_sits_inside_vanillas_own_spread() {
    check(&FORTRESS, "FORTRESS");
}

#[test]
fn the_fossils_and_their_dried_ghasts_sit_inside_vanillas_own_spread() {
    check(&FOSSIL, "FOSSIL");
}

#[test]
fn the_nether_ruined_portal_sits_inside_vanillas_own_spread() {
    check(&PORTAL, "PORTAL");
}
