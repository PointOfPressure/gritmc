//! The decrease queue, graded against vanilla. Vanilla first.
//!
//! # Why this file exists, stated as the measurement that forced it
//!
//! `tests/light_incremental_golden.rs` reports **3744/3744 sections agree
//! with vanilla** and that green is real, but it is green for a narrower
//! reason than it looks. Three injections into the incremental engine were
//! applied to the tree and **none of them turned it red**:
//!
//! * swapping `propagateDecreases()` and `propagateIncreases()` in
//!   `runLightUpdates` — pass;
//! * replacing `repeatFirstLayer` with a dark layer — pass;
//! * replacing every `increaseSkipOneDirection` with
//!   `increaseLightFromEmission`, i.e. deleting the skip bit and adding the
//!   emission flag — pass.
//!
//! The reason is structural. Every static case reaches the engine only
//! through `propagateLightSources`, which enqueues nothing but increases, so
//! `propagateDecreases` drains an empty queue and the whole decrease half is
//! dead. **A port with no decrease queue at all passes every static case.**
//! Counting sections there says how many a decrease-less engine could be
//! wrong about, never that it is wrong about one — the same shape as the
//! 6,762 shape-occluding states that `slab_roof` had to turn into a reading.
//!
//! These five cases are that reading. Each builds a world, lights it, then
//! edits it the way `ServerLevel.setBlock` does — set the state, call
//! `LevelLightEngine.checkBlock`, drain again — and the golden records the
//! edit with both its `from` and its `to` state so this side can reproduce
//! it rather than guess it.
//!
//! | case | what it is the only case to reach |
//! |---|---|
//! | `relit` | `checkNode`'s decrease arm: emission fell below the stored level |
//! | `lit_dark_room` | `checkNode`'s `PULL_LIGHT_IN_ENTRY` arm with a new emission |
//! | `roof_opened` | `SkyLightEngine.updateSourcesInColumn` / `addSourcesAbove` |
//! | `sky_sealed` | `removeSourcesBelow` — a column of 15s that must be **removed** |
//! | `source_moved` | `propagateDecrease`'s `else` arm, decrease and increase in flight together |
//!
//! # The instrument cannot be vacuous
//!
//! `assert_mutations_are_recorded` refuses a case whose `mutations` list is
//! empty, and `the_edit_actually_changes_the_light` asserts that vanilla's
//! post-edit light **differs** from vanilla's pre-edit light for every case
//! — computed on vanilla's own vectors, not on ours. A future golden in
//! which an edit stopped mattering would be refused rather than passed.

mod lightfix;

use std::collections::BTreeMap;

use rustmc_server::light::{classify, LightGeometry, LightLayer};
use rustmc_server::light_engine::Region;
use rustmc_server::light_incremental::{as_long, light_region, light_region_with_edits, Pos};

use lightfix::{arrays, chunk_from, load_props, mask, repo};

fn bit(words: &[i64], i: usize) -> bool {
    words
        .get(i / 64)
        .is_some_and(|w| (*w as u64) >> (i % 64) & 1 == 1)
}

fn golden() -> serde_json::Value {
    let path = repo("crates/rustmc-server/golden/light_vectors.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e} — run ci/light-golden.sh", path.display()));
    serde_json::from_str(&raw).expect("light_vectors.json is not json")
}

fn geometry_of(g: &serde_json::Value) -> LightGeometry {
    LightGeometry {
        min_section_y: i32::try_from(g["min_section_y"].as_i64().expect("min_section_y"))
            .expect("i32"),
        sections_count: i32::try_from(g["sections_count"].as_i64().expect("sections_count"))
            .expect("i32"),
    }
}

/// One recorded edit.
struct Edit {
    pos: Pos,
    from: u16,
    to: u16,
}

fn edits_of(case: &serde_json::Value, ids: &BTreeMap<String, u16>) -> Vec<Edit> {
    case["mutations"]
        .as_array()
        .expect("every case carries a mutations array, even if empty")
        .iter()
        .map(|m| {
            let x = i32::try_from(m["x"].as_i64().expect("x")).expect("i32");
            let y = i32::try_from(m["y"].as_i64().expect("y")).expect("i32");
            let z = i32::try_from(m["z"].as_i64().expect("z")).expect("i32");
            let name = |k: &str| {
                let s = m[k].as_str().expect("state string");
                *ids.get(s)
                    .unwrap_or_else(|| panic!("block_props.json has no state {s}"))
            };
            Edit {
                pos: as_long(x, y, z),
                from: name("from"),
                to: name("to"),
            }
        })
        .collect()
}

/// The post-edit world, straight out of the golden's `world` dump.
fn after_region(
    case: &serde_json::Value,
    ids: &BTreeMap<String, u16>,
    geometry: LightGeometry,
    air: u16,
) -> Region {
    let mut chunks = BTreeMap::new();
    for ch in case["chunks"].as_array().expect("chunks") {
        let cx = i32::try_from(ch["x"].as_i64().expect("x")).expect("i32");
        let cz = i32::try_from(ch["z"].as_i64().expect("z")).expect("i32");
        chunks.insert((cx, cz), chunk_from(&ch["world"], ids, air));
    }
    Region {
        geometry,
        chunks,
        air,
    }
}

/// The pre-edit world: the post-edit one with every recorded edit inverted.
///
/// Inverting is what makes this gradeable at all. The golden dumps the world
/// after the mutation, so a consumer that lit *that* world from scratch would
/// be reproducing the control, not the edit.
fn before_region(after: &Region, edits: &[Edit]) -> Region {
    let mut r = after.clone();
    for e in edits.iter().rev() {
        let (cx, sy, cz) = rustmc_server::light_incremental::block_to_section(e.pos);
        let idx = rustmc_server::light_incremental::nibble_index(e.pos);
        let c = r
            .chunks
            .get_mut(&(cx, cz))
            .expect("the edit is inside the dumped grid");
        let s = c
            .sections
            .get_mut(&sy)
            .expect("the edited section is non-air after the edit");
        assert_eq!(
            s[idx], e.to,
            "the golden's world does not carry the edit's `to` state at its own position"
        );
        s[idx] = e.from;
    }
    r
}

fn vanilla_light(ch: &serde_json::Value) -> rustmc_server::chunk::LightData {
    rustmc_server::chunk::LightData {
        sky_mask: mask(&ch["sky_mask"]),
        block_mask: mask(&ch["block_mask"]),
        empty_sky_mask: mask(&ch["empty_sky_mask"]),
        empty_block_mask: mask(&ch["empty_block_mask"]),
        sky: arrays(&ch["sky"]),
        block: arrays(&ch["block"]),
    }
}

/// The headline: our incremental repair against vanilla's, section by
/// section, on every case that carries an edit.
#[test]
fn the_decrease_queue_reproduces_vanillas_repair() {
    let (props, ids) = load_props();
    let g = golden();
    let geometry = geometry_of(&g);
    let air = *ids.get("minecraft:air").expect("air");

    let mut total = 0usize;
    let mut matched = 0usize;
    let mut cases = 0usize;
    let mut per_case: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();

    for case in g["cases"].as_array().expect("cases") {
        let name = case["name"].as_str().expect("name");
        let edits = edits_of(case, &ids);
        if edits.is_empty() {
            continue;
        }
        cases += 1;
        let after = after_region(case, &ids, geometry, air);
        let before = before_region(&after, &edits);
        let positions: Vec<Pos> = edits.iter().map(|e| e.pos).collect();
        let lit = light_region_with_edits(&before, &after, &props, &positions);

        let mut case_total = 0usize;
        let mut case_matched = 0usize;
        for ch in case["chunks"].as_array().expect("chunks") {
            let cx = i32::try_from(ch["x"].as_i64().expect("x")).expect("i32");
            let cz = i32::try_from(ch["z"].as_i64().expect("z")).expect("i32");
            let ours = classify(geometry, &lit.chunk(cx, cz));
            let v = vanilla_light(ch);
            for (layer, vmask, vempty, varrays, omask, oempty, oarrays) in [
                (
                    LightLayer::Sky,
                    &v.sky_mask,
                    &v.empty_sky_mask,
                    &v.sky,
                    &ours.sky_mask,
                    &ours.empty_sky_mask,
                    &ours.sky,
                ),
                (
                    LightLayer::Block,
                    &v.block_mask,
                    &v.empty_block_mask,
                    &v.block,
                    &ours.block_mask,
                    &ours.empty_block_mask,
                    &ours.block,
                ),
            ] {
                let n = usize::try_from(geometry.light_section_count()).expect("count");
                let (mut vseen, mut oseen) = (0usize, 0usize);
                for i in 0..n {
                    case_total += 1;
                    let (vd, ve) = (bit(vmask, i), bit(vempty, i));
                    let (od, oe) = (bit(omask, i), bit(oempty, i));
                    let mut ok = vd == od && ve == oe;
                    if !ok && failures.len() < 24 {
                        failures.push(format!(
                            "{name} ({cx},{cz}) {layer:?} bit {i}: vanilla data={vd} empty={ve}, \
                             ours data={od} empty={oe}"
                        ));
                    }
                    if ok && vd {
                        let (a, b) = (varrays.get(vseen), oarrays.get(oseen));
                        ok = a == b;
                        if !ok && failures.len() < 24 {
                            let diff = match (a, b) {
                                (Some(a), Some(b)) => {
                                    a.iter().zip(b).filter(|(x, y)| x != y).count()
                                }
                                _ => 0,
                            };
                            failures.push(format!(
                                "{name} ({cx},{cz}) {layer:?} section {}: {diff} of 2048 bytes \
                                 differ",
                                geometry.section_y_of_bit(i32::try_from(i).expect("i32"))
                            ));
                        }
                    }
                    if vd {
                        vseen += 1;
                    }
                    if od {
                        oseen += 1;
                    }
                    if ok {
                        case_matched += 1;
                    }
                }
            }
        }
        per_case.push(format!(
            "{name} {case_matched}/{case_total} ({} edits)",
            edits.len()
        ));
        total += case_total;
        matched += case_matched;
    }

    println!("post-edit section light, vanilla first:");
    for line in &per_case {
        println!("  {line}");
    }
    println!("  TOTAL {matched}/{total} sections agree with vanilla, over {cases} edited worlds");
    assert!(
        cases >= 5,
        "expected at least five edited cases in the golden, found {cases} — \
         a suite that lost its mutation cases must refuse, not pass"
    );
    assert_eq!(
        matched,
        total,
        "{} of {total} sections disagree after the edit:\n{}",
        total - matched,
        failures.join("\n")
    );
}

/// The control that stops the above being an assertion about nothing: for
/// every edited case, vanilla's own post-edit light must **differ** from the
/// light of the pre-edit world. Computed on vanilla's side of the comparison
/// alone, so it cannot be satisfied by anything we do.
///
/// If a future golden's edit stopped changing the light, this says so; the
/// headline above would still be green, and would be measuring nothing.
#[test]
fn the_edit_actually_changes_the_light() {
    let (props, ids) = load_props();
    let g = golden();
    let geometry = geometry_of(&g);
    let air = *ids.get("minecraft:air").expect("air");
    let mut checked = 0usize;
    for case in g["cases"].as_array().expect("cases") {
        let name = case["name"].as_str().expect("name");
        let edits = edits_of(case, &ids);
        if edits.is_empty() {
            continue;
        }
        let after = after_region(case, &ids, geometry, air);
        let before = before_region(&after, &edits);
        // The pre-edit world's light, from scratch — the state the engine was
        // in when the edit arrived.
        let pre = light_region(&before, &props);
        let mut moved = 0usize;
        for ch in case["chunks"].as_array().expect("chunks") {
            let cx = i32::try_from(ch["x"].as_i64().expect("x")).expect("i32");
            let cz = i32::try_from(ch["z"].as_i64().expect("z")).expect("i32");
            let v = vanilla_light(ch);
            let p = classify(geometry, &pre.chunk(cx, cz));
            if p.sky != v.sky
                || p.block != v.block
                || p.sky_mask != v.sky_mask
                || p.block_mask != v.block_mask
            {
                moved += 1;
            }
        }
        assert!(
            moved > 0,
            "{name}: vanilla's post-edit light is identical to the pre-edit world's, so \
             this case grades nothing"
        );
        println!("{name}: the edit moves the light in {moved} of 9 chunks");
        checked += 1;
    }
    assert!(checked >= 5, "only {checked} edited cases");
}

/// A case that claims a mutator must record what it did. A golden written by
/// a mutator that forgot to call `edit` would otherwise silently become a
/// from-scratch case.
#[test]
fn every_mutated_case_records_its_edits() {
    let g = golden();
    for case in g["cases"].as_array().expect("cases") {
        let name = case["name"].as_str().expect("name");
        let drained = case["mutation_drain_iterations"]
            .as_u64()
            .expect("mutation_drain_iterations");
        let n = case["mutations"].as_array().expect("mutations").len();
        assert_eq!(
            drained > 0,
            n > 0,
            "{name}: mutation_drain_iterations={drained} but {n} edits recorded — a mutator \
             that edits without recording makes its case ungradeable"
        );
    }
}
