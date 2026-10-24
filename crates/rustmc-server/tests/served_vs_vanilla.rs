//! The chunks this server would send, against a vanilla world's own bytes,
//! inside the boxes vanilla says its structures occupy.
//!
//! # What no other gate in this repo does
//!
//! `crates/rustmc-worldgen/tests/golden_structure_starts.rs` reads vanilla's
//! saved `structures.starts` and checks our **placement** against them. The
//! region differ compares our **dump** against a harvest over a whole window.
//! Neither one asks the question a player's client asks: are the *blocks in
//! the packet* the blocks vanilla put there?
//!
//! That path is canvas -> [`World::chunk`] -> 24 sections -> paletted
//! container -> global network state id, and it is the half of the pipeline
//! the worldgen gates cannot see. This test walks it: it decodes the
//! `LevelChunkWithLight` the packet writer is handed, resolves every palette
//! entry through `block_props.json`'s ids, and compares the resulting state
//! **string** — properties included — against `read_region`'s.
//!
//! # The window is the project's own
//!
//! Seed 20260722, centre chunk (64,64), r=16: the square every published
//! overworld parity figure is taken on. One generation, and every structure
//! box inside it scored, rather than one generation per structure.
//!
//! The boxes are **read from the reference at run time**, out of the
//! `structures.starts` NBT vanilla wrote into each chunk. Nothing here is a
//! hardcoded bounding box, so a reference regenerated with different
//! structures re-derives its own boxes instead of scoring stale ones.
//!
//! # The floor, and why every row carries one
//!
//! `gen-wide-20260722-r16-ow-b` is the twin of `-a`. Two unmodified vanilla
//! runs of one seed disagree, so each row records what the two references
//! charge each other over the same positions. Rows whose residual is under
//! their floor are corroboration and nothing more — `ruined_portal (48,64)`
//! is 7 against a floor of 7 and `trial_chambers (53,74)` is 3,215 against
//! 3,433, and neither is evidence of a defect.
//!
//! # THE MEASUREMENT THIS TEST NEARLY GOT WRONG
//!
//! `read_region` returns every column in the file, and a harvest's outer ring
//! is written at a **proto status** with no blocks in it. Scored without a
//! status filter, those columns answer `None` for every position, which
//! `block_at` reports as air, and our stone is then charged against it. The
//! first run of this comparison read **560,883 of 566,720 wrong for one
//! mineshaft** — 99% — and the whole of it was the reference having no blocks
//! rather than different ones. Filtering to `status == "minecraft:full"` took
//! that row to 397 against a floor of 519.
//!
//! So a row's `positions` count is asserted **exactly**, not as a lower
//! bound. It is the one number that says the comparison happened: a coverage
//! that silently shrinks to zero would otherwise turn every row into a green.
//! Rule 7 — when a measurement clears (or convicts) everything, suspect the
//! measurement.
//!
//! # Ratchet, not a pin, and a signature beside it
//!
//! `positions` is asserted `==` and `mismatches` is asserted **equal too**,
//! by the two-sided rule this repo already uses for `t1-known-red.toml` and
//! `ci/check-fixture-skips.py`: above the declaration is a regression, below
//! it is a stale declaration. A ratchet nobody lowers stops being evidence —
//! it will accept the residual creeping back up to it and the improvement
//! that should have been recorded never is. So a landing stack that improves
//! one of these rows is red until it writes the new number down, and the
//! failure message carries the row to paste.
//!
//! A count alone is a weak thing to ratchet, and this repo already knows it:
//! `ci/checks/t1-known-red.toml` adjudicates its two standing reds by
//! **signature** — the count, the y bands and the confusion table — precisely
//! so that a known red which changes its nature reads as red. Each row here
//! therefore also carries its three largest `vanilla -> ours` block-name
//! classes, asserted exactly. A residual that stays at 770 while its content
//! changes is a different residual, and without this it would pass.

use std::collections::BTreeMap;
use std::sync::Arc;

use rustmc_server::chunk::ContainerKind;
use rustmc_server::configuration::CapturedRegistries;
use rustmc_server::terrain::StateIds;
use rustmc_server::world::{World, WorldTables};
use rustmc_world::anvil::{read_region, ChunkColumn};
use rustmc_worldgen::pipeline::{Area, Through};

const SEED: i64 = 20_260_722;
const CENTRE: (i32, i32) = (64, 64);
const RADIUS: i32 = 16;

fn repo(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// One scored structure box.
struct Row {
    id: &'static str,
    chunk: (i32, i32),
    /// Positions compared — asserted exactly. See the module header.
    positions: usize,
    /// Served state != reference state. A ratchet: `<=`.
    mismatches: usize,
    /// What the two references charge each other over the same positions.
    floor: usize,
    /// The three largest `vanilla -> ours` block-name classes, largest first,
    /// ties broken by the pair's own text so the order is total.
    ///
    /// Names only, properties stripped: a `facing` that moved is a different
    /// finding from a block that is not there, and lumping them would make
    /// this signature move for reasons it is not about.
    top: &'static [(&'static str, &'static str, usize)],
}

/// Measured 2026-09-05, box threadripper, release, branch `structures-live`.
///
/// Reference `gen-wide-20260722-r16-ow-a`, twin `-b`, our world generated in
/// one pass at seed 20260722, centre (64,64), r=16, `Through::Decoration`,
/// and read back through the chunk encoder.
const EXPECTED: &[Row] = &[
    Row {
        id: "minecraft:mineshaft",
        chunk: (67, 61),
        positions: 484484,
        mismatches: 331,
        floor: 114,
        top: &[
            ("minecraft:air", "minecraft:cave_air", 63),
            ("minecraft:cave_air", "minecraft:sulfur", 59),
            ("minecraft:sulfur", "minecraft:water", 52),
        ],
    },
    Row {
        id: "minecraft:mineshaft",
        chunk: (70, 62),
        positions: 533358,
        mismatches: 770,
        floor: 131,
        top: &[
            ("minecraft:sulfur", "minecraft:water", 133),
            ("minecraft:sulfur", "minecraft:cave_air", 126),
            ("minecraft:cave_air", "minecraft:sulfur", 92),
        ],
    },
    Row {
        id: "minecraft:mineshaft",
        chunk: (71, 55),
        positions: 317790,
        mismatches: 123,
        floor: 126,
        top: &[
            ("minecraft:cave_air", "minecraft:deepslate", 28),
            ("minecraft:deepslate", "minecraft:cave_air", 21),
            ("minecraft:deepslate_gold_ore", "minecraft:deepslate", 11),
        ],
    },
    Row {
        id: "minecraft:mineshaft",
        chunk: (82, 74),
        positions: 130134,
        mismatches: 52,
        floor: 52,
        top: &[
            ("minecraft:granite", "minecraft:diorite", 33),
            ("minecraft:granite", "minecraft:andesite", 18),
            ("minecraft:iron_ore", "minecraft:deepslate_iron_ore", 1),
        ],
    },
    Row {
        id: "minecraft:ruined_portal",
        chunk: (48, 64),
        positions: 648,
        mismatches: 7,
        floor: 7,
        top: &[("minecraft:granite", "minecraft:diorite", 7)],
    },
    Row {
        id: "minecraft:shipwreck",
        chunk: (57, 65),
        positions: 1944,
        mismatches: 20,
        floor: 20,
        top: &[
            ("minecraft:water", "minecraft:tall_seagrass", 6),
            ("minecraft:seagrass", "minecraft:water", 4),
            ("minecraft:water", "minecraft:seagrass", 4),
        ],
    },
    Row {
        id: "minecraft:shipwreck",
        chunk: (81, 57),
        positions: 1512,
        mismatches: 0,
        floor: 0,
        top: &[],
    },
    Row {
        id: "minecraft:trial_chambers",
        chunk: (53, 74),
        positions: 625600,
        mismatches: 3215,
        floor: 3433,
        top: &[
            ("minecraft:cave_air", "minecraft:sulfur", 399),
            ("minecraft:sulfur", "minecraft:water", 347),
            ("minecraft:sulfur", "minecraft:cave_air", 311),
        ],
    },
    Row {
        id: "minecraft:trial_chambers",
        chunk: (74, 78),
        positions: 598000,
        mismatches: 794,
        floor: 372,
        top: &[
            ("minecraft:granite", "minecraft:andesite", 249),
            ("minecraft:andesite", "minecraft:granite", 209),
            ("minecraft:diorite", "minecraft:tuff", 139),
        ],
    },
    Row {
        id: "minecraft:trial_chambers",
        chunk: (75, 55),
        positions: 409344,
        mismatches: 28,
        floor: 26,
        top: &[
            ("minecraft:deepslate", "minecraft:deepslate_gold_ore", 11),
            ("minecraft:deepslate_gold_ore", "minecraft:tuff", 7),
            ("minecraft:diorite", "minecraft:granite", 7),
        ],
    },
    Row {
        id: "minecraft:village_savanna",
        chunk: (46, 77),
        positions: 102810,
        mismatches: 4,
        floor: 2,
        top: &[
            ("minecraft:air", "minecraft:short_grass", 2),
            ("minecraft:air", "minecraft:tall_grass", 1),
            ("minecraft:short_grass", "minecraft:tall_grass", 1),
        ],
    },
    Row {
        id: "minecraft:village_savanna",
        chunk: (81, 77),
        positions: 255420,
        mismatches: 212,
        floor: 16,
        top: &[
            ("minecraft:stone", "minecraft:coal_ore", 77),
            ("minecraft:acacia_leaves", "minecraft:air", 57),
            ("minecraft:hay_block", "minecraft:air", 30),
        ],
    },
];

/// Global network state id to full block state string.
fn states_by_id() -> BTreeMap<i32, String> {
    let text = std::fs::read_to_string(repo("data/vanilla/block_props.json")).expect("block table");
    let doc: serde_json::Value = serde_json::from_str(&text).expect("json");
    doc["table"]
        .as_array()
        .expect("table")
        .iter()
        .filter_map(|e| {
            Some((
                i32::try_from(e["id"].as_i64()?).ok()?,
                e["state"].as_str()?.to_string(),
            ))
        })
        .collect()
}

/// Every **full-status** column of a harvest, by chunk.
///
/// The status filter is the whole of the module header's rule-7 paragraph.
fn full_chunks(dir: &std::path::Path) -> BTreeMap<(i32, i32), ChunkColumn> {
    chunks_of(dir, true)
}

/// Every column, status included, for reading `structures.starts` off.
///
/// A start is recorded at `STRUCTURE_STARTS`, which is the first status a
/// chunk reaches, so a proto column carries its starts and none of its
/// blocks. Reading the two from the same filtered map conflates "vanilla does
/// not place this here" with "vanilla placed it and this harvest stopped
/// before writing the blocks" — and those have opposite meanings.
fn all_chunks(dir: &std::path::Path) -> BTreeMap<(i32, i32), ChunkColumn> {
    chunks_of(dir, false)
}

fn chunks_of(dir: &std::path::Path, only_full: bool) -> BTreeMap<(i32, i32), ChunkColumn> {
    let mut out = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("mca") {
            continue;
        }
        let Ok(columns) = read_region(&p) else {
            continue;
        };
        for c in columns {
            if !only_full || c.status == "minecraft:full" {
                out.insert((c.x, c.z), c);
            }
        }
    }
    out
}

/// The union bounding box of a start's pieces.
fn union_box(start: &rustmc_world::anvil::StructureStart) -> [i32; 6] {
    let mut bb = [i32::MAX, i32::MAX, i32::MAX, i32::MIN, i32::MIN, i32::MIN];
    for piece in &start.pieces {
        for (slot, value) in bb.iter_mut().zip(piece.bounding_box).take(3) {
            *slot = (*slot).min(value);
        }
        for (slot, value) in bb.iter_mut().zip(piece.bounding_box).skip(3) {
            *slot = (*slot).max(value);
        }
    }
    bb
}

#[test]
fn the_served_chunks_match_vanilla_inside_every_structure_box() {
    let world_dir = repo("oracle-work/gen-wide-20260722-r16-ow-a");
    let twin_dir = repo("oracle-work/gen-wide-20260722-r16-ow-b");
    if !rustmc_data::fixture::available(
        &world_dir,
        "the r16 overworld reference",
        "ci/gen-vanilla-region.sh",
    ) || !rustmc_data::fixture::available(&twin_dir, "its twin", "ci/gen-vanilla-region.sh")
        || !rustmc_data::fixture::available(
            &repo("data/vanilla/block_props.json"),
            "block_props extract",
            "ci/block-props-golden.sh",
        )
        || !worldgen_assembly::have_extract()
    {
        return;
    }

    let reference = full_chunks(&world_dir);
    let twin = full_chunks(&twin_dir);
    assert!(
        !reference.is_empty() && !twin.is_empty(),
        "both references parsed to zero full chunks — nothing below is a comparison"
    );

    let area = Area::square(CENTRE.0, CENTRE.1, RADIUS);
    let served: std::collections::BTreeSet<(i32, i32)> = area.chunks().collect();

    // The boxes, from the reference's own `structures.starts`.
    let mut boxes: Vec<(String, (i32, i32), [i32; 6])> = Vec::new();
    for column in reference.values() {
        for start in &column.structure_starts {
            if start.pieces.is_empty() {
                continue;
            }
            boxes.push((
                start.id.clone(),
                (start.chunk_x, start.chunk_z),
                union_box(start),
            ));
        }
    }
    boxes.sort();
    boxes.dedup();

    // One generation for the whole window.
    let registries =
        CapturedRegistries::load(&repo("data/vanilla/registries.bin")).expect("registry capture");
    let biomes = registries
        .entry_ids("minecraft:worldgen/biome")
        .expect("readable capture")
        .expect("the capture holds the biome registry");
    let ids = StateIds::load(&repo("data/vanilla/block_props.json")).expect("block table");
    let tables = Arc::new(WorldTables::load(&biomes, ids).expect("block tables"));
    let world =
        World::generate_through(SEED, area, tables, Through::Decoration).expect("generated");
    let states = states_by_id();

    // Our served blocks, decoded once per chunk.
    let mut ours: BTreeMap<(i32, i32, i32), String> = BTreeMap::new();
    for (chunk_x, chunk_z) in area.chunks() {
        if !reference.contains_key(&(chunk_x, chunk_z)) || !twin.contains_key(&(chunk_x, chunk_z)) {
            continue;
        }
        let chunk = world.chunk(chunk_x, chunk_z).expect("encoded chunk");
        for (si, section) in chunk.sections.iter().enumerate() {
            let base = -64 + i32::try_from(si).expect("section index") * 16;
            for (i, id) in section
                .states
                .values(ContainerKind::BlockStates)
                .into_iter()
                .enumerate()
            {
                if id == 0 {
                    continue; // air is the default on the read side too
                }
                let y = base + i32::try_from(i / 256).expect("y");
                let z = chunk_z * 16 + i32::try_from((i % 256) / 16).expect("z");
                let x = chunk_x * 16 + i32::try_from(i % 16).expect("x");
                ours.insert(
                    (x, y, z),
                    states
                        .get(&id)
                        .cloned()
                        .unwrap_or_else(|| format!("unmapped network id {id}")),
                );
            }
        }
    }

    // A structure this port places where vanilla places NONE is invisible to
    // every row above: the rows only look inside boxes vanilla itself
    // recorded. So the whole window is swept for the one block that would
    // betray a spurious buried treasure — a chest of ours at a position
    // vanilla has something else at. It is cheap because it reads `ours`,
    // which is already built, and looks the reference up only on a hit.
    //
    // Written when `buried_treasure` was ported, and it caught that port's
    // own defect on its first run — 68 chests where vanilla has none, from a
    // placement built on `isPlacementChunk` alone with the frequency coin
    // missing. Every box-scoped row above stayed green through all 68.
    //
    // WHAT IT DOES NOT COVER, said out loud rather than left to be assumed:
    // it sweeps for ONE block name. A spurious structure that places no chest
    // is invisible to it, and so is a spurious block of any other kind. The
    // general form of this check is a whole-window diff, which is the region
    // differ's job and not this test's; what is here is the cheapest thing
    // that would have caught the defect it was written for.
    let mut spurious: Vec<(i32, i32, i32, String)> = Vec::new();
    for ((x, y, z), state) in &ours {
        if state.split('[').next() != Some("minecraft:chest") {
            continue;
        }
        let key = (x >> 4, z >> 4);
        let Some(a) = reference.get(&key) else {
            continue;
        };
        let there = a.block_at(*x, *y, *z).unwrap_or("minecraft:air");
        if there.split('[').next() != Some("minecraft:chest") {
            spurious.push((*x, *y, *z, there.to_string()));
        }
    }
    assert!(
        spurious.is_empty(),
        "{} chest(s) served where vanilla has none — a structure placed where \
         vanilla places nothing is invisible to every box-scoped row: {:?}",
        spurious.len(),
        &spurious[..spurious.len().min(5)]
    );

    let mut rows: Vec<Row> = Vec::new();
    let mut report = String::new();
    for (id, chunk, bb) in &boxes {
        let (mut positions, mut mismatches, mut floor) = (0usize, 0usize, 0usize);
        let mut classes: BTreeMap<(String, String), usize> = BTreeMap::new();
        for x in bb[0]..=bb[3] {
            for z in bb[2]..=bb[5] {
                let key = (x >> 4, z >> 4);
                if !served.contains(&key) {
                    continue;
                }
                let (Some(a), Some(b)) = (reference.get(&key), twin.get(&key)) else {
                    continue;
                };
                for y in bb[1]..=bb[4] {
                    let va = a.block_at(x, y, z).unwrap_or("minecraft:air");
                    let vb = b.block_at(x, y, z).unwrap_or("minecraft:air");
                    let o = ours.get(&(x, y, z)).map_or("minecraft:air", String::as_str);
                    positions += 1;
                    if va != vb {
                        floor += 1;
                    }
                    if va != o {
                        mismatches += 1;
                        let vn = va.split('[').next().unwrap_or(va);
                        let on = o.split('[').next().unwrap_or(o);
                        *classes.entry((vn.to_string(), on.to_string())).or_default() += 1;
                    }
                }
            }
        }
        if positions == 0 {
            continue;
        }
        let mut ranked: Vec<((String, String), usize)> = classes.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        ranked.truncate(3);
        let top: &'static [(&'static str, &'static str, usize)] = Box::leak(
            ranked
                .iter()
                .map(|((v, o), n)| {
                    (
                        &*Box::leak(v.clone().into_boxed_str()),
                        &*Box::leak(o.clone().into_boxed_str()),
                        *n,
                    )
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        let printed: Vec<String> = top
            .iter()
            .map(|(v, o, n)| format!("(\"{v}\", \"{o}\", {n})"))
            .collect();
        report.push_str(&format!(
            "    Row {{ id: \"{id}\", chunk: ({}, {}), positions: {positions}, \
             mismatches: {mismatches}, floor: {floor}, top: &[{}] }},\n",
            chunk.0,
            chunk.1,
            printed.join(", ")
        ));
        rows.push(Row {
            id: Box::leak(id.clone().into_boxed_str()),
            chunk: *chunk,
            positions,
            mismatches,
            floor,
            top,
        });
    }

    let total: usize = rows.iter().map(|r| r.positions).sum();
    let bad: usize = rows.iter().map(|r| r.mismatches).sum();
    let floor: usize = rows.iter().map(|r| r.floor).sum();
    eprintln!(
        "served-vs-vanilla: {} boxes, {bad} of {total} positions differ, floor {floor}",
        rows.len()
    );
    eprint!("{report}");

    // Adjudicate against the declaration.
    let declared: BTreeMap<(&str, (i32, i32)), &Row> =
        EXPECTED.iter().map(|r| ((r.id, r.chunk), r)).collect();
    let mut faults: Vec<String> = Vec::new();
    for r in &rows {
        let Some(d) = declared.get(&(r.id, r.chunk)) else {
            faults.push(format!(
                "{} {:?} is scored and undeclared — add it to EXPECTED with the \
                 figures printed above",
                r.id, r.chunk
            ));
            continue;
        };
        if r.positions != d.positions {
            faults.push(format!(
                "{} {:?} compared {} positions, declared {} — the comparison's \
                 coverage moved, so its residual is not the declared one's successor",
                r.id, r.chunk, r.positions, d.positions
            ));
        }
        if r.top != d.top && r.mismatches >= d.mismatches {
            faults.push(format!(
                "{} {:?} CHANGED ITS NATURE: top classes are {:?}, declared {:?}. \
                 A residual whose content moved is a different residual even when \
                 its count did not.",
                r.id, r.chunk, r.top, d.top
            ));
        }
        if r.mismatches > d.mismatches {
            faults.push(format!(
                "{} {:?} REGRESSED: {} mismatches against a declared {} (floor {})",
                r.id, r.chunk, r.mismatches, d.mismatches, r.floor
            ));
        }
        // A RATCHET NOBODY LOWERS STOPS BEING EVIDENCE. This is
        // `ci/checks/t1-known-red.toml`'s rule and `ci/check-fixture-skips.py`'s
        // rule, applied here for the same reason: a declaration that stays
        // above a residual it no longer describes will accept the residual
        // creeping back to it, silently, and the improvement that should have
        // been recorded never is. So an improvement is red until it is
        // written down, and the message carries the row to paste.
        if r.mismatches < d.mismatches {
            faults.push(format!(
                "{} {:?} IMPROVED to {} from a declared {} — lower the \
                 declaration in the commit that did it, or the ratchet stops \
                 being evidence. The row is printed above.",
                r.id, r.chunk, r.mismatches, d.mismatches
            ));
        }
    }
    for d in EXPECTED {
        if !rows.iter().any(|r| r.id == d.id && r.chunk == d.chunk) {
            faults.push(format!(
                "{} {:?} is declared and was never scored — a row that stopped \
                 being measured is not a row that passed",
                d.id, d.chunk
            ));
        }
    }
    assert!(
        faults.is_empty(),
        "served-vs-vanilla:\n  {}",
        faults.join("\n  ")
    );
}

/// Boxes vanilla places just OUTSIDE the r=16 parity window.
///
/// The window is the one every published overworld figure is taken on and it
/// is not moving to suit this test, so the sets whose only start in this
/// harvest lies outside it get their own small windows here. Without this,
/// `buried_treasures` — the one set vanilla places in the seed-20260722
/// harvest that this server did not — would have no row anywhere, and a
/// census with no row is not a census that passed.
const OUTSIDE: &[Row] = &[Row {
    id: "minecraft:buried_treasure",
    chunk: (73, 46),
    positions: 1,
    mismatches: 0,
    floor: 0,
    top: &[],
}];

/// What `score_one` measured: positions compared, mismatches, the twin floor,
/// and the largest `vanilla -> ours` name classes.
type Scored = (usize, usize, usize, Vec<((String, String), usize)>);

/// Score one box in its own window.
fn score_one(
    seed: i64,
    reference: &BTreeMap<(i32, i32), ChunkColumn>,
    twin: &BTreeMap<(i32, i32), ChunkColumn>,
    states: &BTreeMap<i32, String>,
    id: &str,
    chunk: (i32, i32),
    bb: [i32; 6],
) -> Option<Scored> {
    // A window wide enough for the box plus a ring, generated on its own.
    let span = ((bb[3] >> 4) - (bb[0] >> 4)).max((bb[5] >> 4) - (bb[2] >> 4));
    let radius = span / 2 + 3;
    let centre = (
        ((bb[0] >> 4) + (bb[3] >> 4)) / 2,
        ((bb[2] >> 4) + (bb[5] >> 4)) / 2,
    );
    let area = Area::square(centre.0, centre.1, radius);
    let served: std::collections::BTreeSet<(i32, i32)> = area.chunks().collect();

    let registries =
        CapturedRegistries::load(&repo("data/vanilla/registries.bin")).expect("registry capture");
    let biomes = registries
        .entry_ids("minecraft:worldgen/biome")
        .expect("readable capture")
        .expect("the capture holds the biome registry");
    let ids = StateIds::load(&repo("data/vanilla/block_props.json")).expect("block table");
    let tables = Arc::new(WorldTables::load(&biomes, ids).expect("block tables"));
    let world =
        World::generate_through(seed, area, tables, Through::Decoration).expect("generated");

    let (mut positions, mut mismatches, mut floor) = (0usize, 0usize, 0usize);
    let mut classes: BTreeMap<(String, String), usize> = BTreeMap::new();
    for x in bb[0]..=bb[3] {
        for z in bb[2]..=bb[5] {
            let key = (x >> 4, z >> 4);
            if !served.contains(&key) {
                continue;
            }
            let (Some(a), Some(b)) = (reference.get(&key), twin.get(&key)) else {
                continue;
            };
            let chunk_data = world.chunk(key.0, key.1).expect("encoded chunk");
            for y in bb[1]..=bb[4] {
                let va = a.block_at(x, y, z).unwrap_or("minecraft:air");
                let vb = b.block_at(x, y, z).unwrap_or("minecraft:air");
                let o = served_state(&chunk_data, states, x, y, z);
                positions += 1;
                if va != vb {
                    floor += 1;
                }
                if va != o {
                    mismatches += 1;
                    let vn = va.split('[').next().unwrap_or(va).to_string();
                    let on = o.split('[').next().unwrap_or(&o).to_string();
                    *classes.entry((vn, on)).or_default() += 1;
                }
            }
        }
    }
    if positions == 0 {
        eprintln!("{id} {chunk:?}: no coverage");
        return None;
    }
    let mut ranked: Vec<((String, String), usize)> = classes.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.truncate(3);
    Some((positions, mismatches, floor, ranked))
}

/// One position out of an encoded chunk, as a full state string.
fn served_state(
    chunk: &rustmc_server::chunk::LevelChunkWithLight,
    states: &BTreeMap<i32, String>,
    x: i32,
    y: i32,
    z: i32,
) -> String {
    let si = usize::try_from((y + 64) / 16).unwrap_or(0);
    let Some(section) = chunk.sections.get(si) else {
        return "minecraft:air".to_string();
    };
    let ly = (y + 64).rem_euclid(16);
    let idx = usize::try_from(ly * 256 + z.rem_euclid(16) * 16 + x.rem_euclid(16)).unwrap_or(0);
    section
        .states
        .values(ContainerKind::BlockStates)
        .get(idx)
        .and_then(|id| states.get(id).cloned())
        .unwrap_or_else(|| "minecraft:air".to_string())
}

#[test]
fn the_boxes_outside_the_parity_window() {
    let world_dir = repo("oracle-work/gen-wide-20260722-r16-ow-a");
    let twin_dir = repo("oracle-work/gen-wide-20260722-r16-ow-b");
    if !rustmc_data::fixture::available(
        &world_dir,
        "the r16 overworld reference",
        "ci/gen-vanilla-region.sh",
    ) || !rustmc_data::fixture::available(&twin_dir, "its twin", "ci/gen-vanilla-region.sh")
        || !rustmc_data::fixture::available(
            &repo("data/vanilla/block_props.json"),
            "block_props extract",
            "ci/block-props-golden.sh",
        )
        || !worldgen_assembly::have_extract()
    {
        return;
    }
    let reference = full_chunks(&world_dir);
    let twin = full_chunks(&twin_dir);
    let states = states_by_id();

    let mut faults: Vec<String> = Vec::new();
    for d in OUTSIDE {
        // The box, from the reference's own NBT rather than a literal.
        let Some(bb) = reference.get(&d.chunk).and_then(|c| {
            c.structure_starts
                .iter()
                .find(|s| s.id == d.id && !s.pieces.is_empty())
                .map(union_box)
        }) else {
            faults.push(format!(
                "{} {:?} has no start in the reference — the declaration names \
                 a structure this harvest does not place",
                d.id, d.chunk
            ));
            continue;
        };
        let Some((positions, mismatches, floor, ranked)) =
            score_one(SEED, &reference, &twin, &states, d.id, d.chunk, bb)
        else {
            faults.push(format!(
                "{} {:?} scored zero positions — a box with no coverage is not a box that passed",
                d.id, d.chunk
            ));
            continue;
        };
        eprintln!(
            "outside: {} {:?} box {bb:?} positions={positions} mismatches={mismatches} \
             floor={floor} top={ranked:?}",
            d.id, d.chunk
        );
        if positions != d.positions {
            faults.push(format!(
                "{} {:?} compared {positions} positions, declared {}",
                d.id, d.chunk, d.positions
            ));
        }
        if mismatches > d.mismatches {
            faults.push(format!(
                "{} {:?} REGRESSED: {mismatches} mismatches against a declared {} (floor {floor})",
                d.id, d.chunk, d.mismatches
            ));
        }
    }
    assert!(
        faults.is_empty(),
        "outside the window:\n  {}",
        faults.join("\n  ")
    );
}

/// A structure this port DECLARES it does not place, and what refusing costs.
///
/// `worldgen-assembly`'s `Generated::caveats` names two partial sets — the
/// `ocean_ruins` set is placed for its WARM variant only, and
/// `shipwreck_starts` refuses the BEACHED half of `shipwrecks` and returns a
/// count of what it skipped. A caveat with no number beside it is a claim
/// nobody has priced, and this is where the price goes.
struct Refusal {
    id: &'static str,
    chunk: (i32, i32),
    seed: i64,
    /// The harvest that holds this start, and a second run of the same seed
    /// to charge it against.
    world: &'static str,
    twin: &'static str,
    /// `Some(n)` — this many positions were compared. `None` — no reference
    /// this project holds covers the box at full status, and that is asserted
    /// rather than left as a silent zero.
    positions: Option<usize>,
    /// A CEILING, not a residual, and the difference matters. Nothing is
    /// placed here at all, so every position in the box may differ for
    /// reasons that have nothing to do with the refusal; what this number
    /// says is that the refusal cannot cost more than it.
    ceiling: usize,
}

/// Measured 2026-09-05, box threadripper, branch `structures-live`.
const REFUSALS: &[Refusal] = &[
    Refusal {
        id: "minecraft:shipwreck_beached",
        chunk: (84, 57),
        seed: 31_415_926,
        world: "oracle-work/vanilla-s2-r7",
        twin: "oracle-work/van-s2-c62",
        positions: None,
        ceiling: 0,
    },
    Refusal {
        id: "minecraft:ocean_ruin_cold",
        chunk: (2, 2),
        seed: 20_260_722,
        world: "oracle-work/vanilla-r7",
        twin: "oracle-work/vanilla-r7-b",
        positions: None,
        ceiling: 0,
    },
];

#[test]
fn the_declared_refusals_carry_a_price_or_say_they_cannot() {
    if !rustmc_data::fixture::available(
        &repo("data/vanilla/block_props.json"),
        "block_props extract",
        "ci/block-props-golden.sh",
    ) || !worldgen_assembly::have_extract()
    {
        return;
    }
    let states = states_by_id();
    let mut faults: Vec<String> = Vec::new();
    for d in REFUSALS {
        let world_dir = repo(d.world);
        let twin_dir = repo(d.twin);
        if !world_dir.is_dir() || !twin_dir.is_dir() {
            eprintln!("refusal {} {:?}: no reference on disk", d.id, d.chunk);
            continue;
        }
        let reference = full_chunks(&world_dir);
        let twin = full_chunks(&twin_dir);
        let recorded = all_chunks(&world_dir);
        let Some(bb) = recorded.get(&d.chunk).and_then(|c| {
            c.structure_starts
                .iter()
                .find(|s| s.id == d.id && !s.pieces.is_empty())
                .map(union_box)
        }) else {
            faults.push(format!(
                "{} {:?} has no start in {} at ANY status — the refusal names \
                 a structure this harvest does not place",
                d.id, d.chunk, d.world
            ));
            continue;
        };
        let scored = score_one(d.seed, &reference, &twin, &states, d.id, d.chunk, bb);
        match (scored, d.positions) {
            (None, None) => {
                let status = recorded
                    .get(&d.chunk)
                    .map_or("absent", |c| c.status.as_str());
                eprintln!(
                    "refusal {} {:?}: NO COVERAGE — the start is recorded (chunk \
                     status {status}) and no reference this project holds carries \
                     its BLOCKS at full status, so the refusal has no price",
                    d.id, d.chunk
                );
            }
            (None, Some(want)) => faults.push(format!(
                "{} {:?} scored zero positions, declared {want}",
                d.id, d.chunk
            )),
            (Some((positions, mismatches, floor, ranked)), want) => {
                eprintln!(
                    "refusal {} {:?} box {bb:?} positions={positions} \
                     mismatches={mismatches} floor={floor} top={ranked:?}",
                    d.id, d.chunk
                );
                match want {
                    None => faults.push(format!(
                        "{} {:?} IS coverable — {positions} positions, {mismatches} \
                         mismatches. It is declared uncoverable; give it a ceiling",
                        d.id, d.chunk
                    )),
                    Some(want) if positions != want => faults.push(format!(
                        "{} {:?} compared {positions} positions, declared {want}",
                        d.id, d.chunk
                    )),
                    Some(_) if mismatches > d.ceiling => faults.push(format!(
                        "{} {:?} costs {mismatches}, declared ceiling {}",
                        d.id, d.chunk, d.ceiling
                    )),
                    Some(_) => {}
                }
            }
        }
    }
    assert!(faults.is_empty(), "refusals:\n  {}", faults.join("\n  "));
}
