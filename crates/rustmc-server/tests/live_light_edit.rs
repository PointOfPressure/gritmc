//! The light a player's edits reach — the live half of the engine.
//!
//! `light_golden.rs` grades the engine against vanilla and
//! `section_light_wired.rs` grades that it is connected to the packet. This
//! file grades the third thing, which was missing until 2026-09-05 and is the
//! reason `World::light` stopped being a `OnceLock`: that an edit made
//! **after** the light exists reaches the wire at all.
//!
//! The strongest available comparison is against
//! `light_incremental::light_region_with_edits`, because that function is the
//! form the golden graded — 2,340 of 2,340 post-edit sections agreeing with
//! vanilla's own `LevelLightEngine` over five edited worlds. So this file
//! does not re-derive what the light should be; it demands that the server's
//! live path produce, nibble for nibble, the answer the graded instrument
//! produces for the same region and the same edits. A disagreement is a
//! defect in the plumbing, which is exactly what this file is for.
//!
//! It is its own integration test binary because `RUSTMC_SECTION_LIGHT` is
//! process-global.

use rustmc_server::chunk::LightData;
use rustmc_server::configuration::CapturedRegistries;
use rustmc_server::edits::Blocks;
use rustmc_server::interact::BlockPos;
use rustmc_server::light::LightLayer;
use rustmc_server::terrain::StateIds;
use rustmc_server::world::{World, WorldTables};
use rustmc_worldgen::pipeline::Area;

const SEED: i64 = 20260722;

fn repo(relative: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn world() -> Option<World> {
    if !worldgen_assembly::have_extract() {
        eprintln!("skipped: no vanilla extract");
        return None;
    }
    let registries = CapturedRegistries::load(&repo("data/vanilla/registries.bin")).ok()?;
    let biomes = registries
        .entry_ids("minecraft:worldgen/biome")
        .expect("readable capture")
        .expect("the capture holds the biome registry");
    let ids = StateIds::load(&repo("data/vanilla/block_props.json")).expect("block table");
    let tables = std::sync::Arc::new(WorldTables::load(&biomes, ids).expect("block tables"));
    Some(World::generate(SEED, Area::square(0, 0, 1), tables).expect("generated"))
}

/// A spot in chunk (0,0) with air above it and something solid below, so a
/// torch placed there is a real light source in a real room rather than a
/// block hanging in the sky where nothing can get darker.
fn ground(w: &World) -> BlockPos {
    let (x, z) = (4, 4);
    #[allow(clippy::cast_possible_truncation)]
    let y = w.surface_at(x, z) as i32;
    (x, y, z)
}

/// The defect this file was written for: the light was computed once, so a
/// torch placed after the first chunk send changed nothing on the wire.
///
/// The first `chunk` call is what builds the light — without it the edit
/// would be folded into a from-scratch build and the test would pass on the
/// broken code.
#[test]
fn a_torch_placed_after_the_first_chunk_send_reaches_the_wire() {
    let Some(w) = world() else { return };
    let before = w.chunk(0, 0).expect("chunk encodes");
    assert_ne!(
        before.light,
        LightData::default(),
        "the light must be on by default or this file grades nothing"
    );

    let p = ground(&w);
    let torch = (p.0, p.1 + 1, p.2);
    assert!(
        w.set_block(torch, "minecraft:torch").is_some(),
        "the torch must land inside the inner area"
    );

    let after = w.chunk(0, 0).expect("chunk encodes");
    assert_ne!(
        after.light, before.light,
        "a torch placed after the light was built did not change the chunk \
         packet — this is the `OnceLock` defect and it is back"
    );

    // And the nibble at the torch is vanilla's emission for it, not merely
    // "different". `Blocks.getLightEmission` for a lit torch is 14.
    assert_eq!(
        w.light_level(LightLayer::Block, torch),
        14,
        "a torch's own block light is its emission"
    );
}

/// The live path must agree with the graded instrument, nibble for nibble,
/// over every section of every chunk in the region.
///
/// `light_region_with_edits` is what was measured against vanilla's own
/// engine at 2,340 of 2,340 post-edit sections. If the server's incremental
/// plumbing disagrees with it the plumbing is wrong, because the plumbing is
/// the only thing that differs.
#[test]
fn the_live_path_reproduces_the_graded_post_edit_instrument() {
    use rustmc_server::light_incremental::{as_long, light_region_with_edits};

    let Some(w) = world() else { return };
    // Build the light BEFORE the edit, which is what makes this the
    // incremental path rather than a rebuild.
    let region_before = w.build_light_region();
    let _ = w.chunk(0, 0).expect("chunk encodes");

    let p = ground(&w);
    let edits = [
        (p.0, p.1 + 1, p.2),
        (p.0 + 2, p.1 + 1, p.2 + 2),
        // A solid block put back where air was: the decrease queue's case.
        (p.0, p.1 + 2, p.2),
    ];
    for (i, e) in edits.iter().enumerate() {
        let state = if i == 2 {
            "minecraft:stone"
        } else {
            "minecraft:torch"
        };
        assert!(w.set_block(*e, state).is_some(), "edit {i} must land");
    }
    let region_after = w.build_light_region();
    assert_ne!(
        format!("{region_before:?}").len(),
        0,
        "regions are comparable"
    );

    let props = w.light_props();
    let positions: Vec<i64> = edits.iter().map(|e| as_long(e.0, e.1, e.2)).collect();
    let expected = light_region_with_edits(&region_before, &region_after, props, &positions);

    // Every nibble of every stored section, both layers, over the whole
    // region — not a section count and not a mask.
    let g = w.light_geometry();
    let mut compared = 0u64;
    let mut differ = 0u64;
    let mut per_pair: std::collections::BTreeMap<(i32, i32, u8, i32), u64> =
        std::collections::BTreeMap::new();
    for cz in -1..=1 {
        for cx in -1..=1 {
            for sy in g.min_light_section()..g.min_light_section() + g.light_section_count() {
                for layer in [LightLayer::Sky, LightLayer::Block] {
                    for y in 0..16 {
                        for z in 0..16 {
                            for x in 0..16 {
                                let pos = (cx * 16 + x, sy * 16 + y, cz * 16 + z);
                                let want = expected.level_at(layer, as_long(pos.0, pos.1, pos.2));
                                let got = w.light_level(layer, pos);
                                compared += 1;
                                if want != got {
                                    differ += 1;
                                    *per_pair.entry((cx, cz, layer as u8, sy)).or_default() += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let mut rows: Vec<_> = per_pair.into_iter().filter(|(_, n)| *n > 0).collect();
    rows.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for ((cx, cz, layer, sy), n) in rows.iter().take(24) {
        eprintln!("  chunk ({cx},{cz}) layer {layer} section {sy}: {n} nibbles");
    }
    eprintln!(
        "post-edit live path: {differ} of {compared} nibbles differ from \
         light_region_with_edits over 9 chunks, 3 edits, seed {SEED}"
    );
    assert!(
        compared > 1_000_000,
        "a window of {compared} nibbles is too small to mean anything"
    );
    assert_eq!(
        differ, 0,
        "the live path disagrees with the graded instrument"
    );
}

/// The positive control: the comparison above can go red.
///
/// It runs the same sweep with one edit **withheld** from the instrument, so
/// the two sides are genuinely lighting different worlds. A green here would
/// mean the sweep is not reading what it claims to.
#[test]
fn withholding_an_edit_from_the_instrument_moves_the_figure() {
    use rustmc_server::light_incremental::{as_long, light_region_with_edits};

    let Some(w) = world() else { return };
    let region_before = w.build_light_region();
    let _ = w.chunk(0, 0).expect("chunk encodes");

    let p = ground(&w);
    let torch = (p.0, p.1 + 1, p.2);
    assert!(w.set_block(torch, "minecraft:torch").is_some());
    let region_after = w.build_light_region();

    // The instrument is told about NO edits, so it repairs nothing and keeps
    // the pre-edit light.
    let expected = light_region_with_edits(&region_before, &region_after, w.light_props(), &[]);
    let mut differ = 0u64;
    let g = w.light_geometry();
    for sy in g.min_light_section()..g.min_light_section() + g.light_section_count() {
        for y in 0..16 {
            for z in 0..16 {
                for x in 0..16 {
                    let pos = (x, sy * 16 + y, z);
                    if expected.level_at(LightLayer::Block, as_long(pos.0, pos.1, pos.2))
                        != w.light_level(LightLayer::Block, pos)
                    {
                        differ += 1;
                    }
                }
            }
        }
    }
    eprintln!("control: {differ} block-light nibbles differ when the instrument is told nothing");
    assert!(
        differ > 0,
        "withholding the torch from the instrument changed nothing, so the \
         sweep in the test above is not comparing what it says it is"
    );
}

/// `ChunkHolder.broadcastChanges`'s light half, on the wire.
///
/// Three properties, and each one is a thing a plausible port gets wrong:
///
/// 1. **A section whose light array actually changed must be named by a
///    mask.** The mask comes from `sectionsAffectedByLightUpdates`, not from
///    a diff, so nothing in the engine checks this.
///
///    The comparison is between sections **present on both sides**, and that
///    restriction is vanilla's rather than a convenience. A section that
///    merely starts being stored — which happens across a whole 3x3 of chunks
///    the moment a block lands in a previously all-air section, because
///    `updateSectionStatus` walks a 3x3x3 cube — is added to
///    `changedSections` by `LayerLightSectionStorage.markNewInconsistencies`
///    and **not** to `sectionsAffectedByLightUpdates`. `changedSections`
///    drives the visible-data copy; only the affected set reaches the packet.
///    So vanilla does not name those either, and a check that demanded it
///    would be demanding a defect. Measured on the way to this: 9 sky
///    section-5 pairs, one per chunk, from a single torch.
/// 2. **The packet must be well formed** — a mask bit per array, 2048 bytes
///    per array, and a body that decodes with nothing left over.
/// 3. **The filters must clear.** `broadcastChanges` clears both bitsets
///    after sending, so a second call with no further edit owes nothing. A
///    port that forgot would resend every section for the rest of the
///    session.
#[test]
fn a_block_edit_owes_a_partial_light_update_and_only_once() {
    let Some(w) = world() else { return };
    let _ = w.chunk(0, 0).expect("chunk encodes");
    assert!(
        w.light_updates().is_empty(),
        "a world nobody has edited owes no light_update — `hasChangesToBroadcast`"
    );

    let g = w.light_geometry();
    let n = usize::try_from(g.light_section_count()).expect("26");
    let bit = |m: &[i64], i: usize| -> bool {
        m.get(i / 64)
            .is_some_and(|word| (word >> (i % 64)) & 1 == 1)
    };
    // The wire form, which is the only form that can say "absent" as opposed
    // to "all zero": a section named by neither the data mask nor the empty
    // mask is one the client keeps its own copy of.
    let sample = |w: &World| -> std::collections::BTreeMap<(i32, i32, u8, usize), Option<Vec<u8>>> {
        let mut out = std::collections::BTreeMap::new();
        for cz in -1..=1 {
            for cx in -1..=1 {
                let p = w.chunk(cx, cz).expect("chunk encodes");
                for (layer, mask, empty, arrays) in [
                    (
                        LightLayer::Sky,
                        &p.light.sky_mask,
                        &p.light.empty_sky_mask,
                        &p.light.sky,
                    ),
                    (
                        LightLayer::Block,
                        &p.light.block_mask,
                        &p.light.empty_block_mask,
                        &p.light.block,
                    ),
                ] {
                    let mut k = 0usize;
                    for i in 0..n {
                        let v = if bit(mask, i) {
                            let a = arrays[k].clone();
                            k += 1;
                            Some(a)
                        } else if bit(empty, i) {
                            Some(Vec::new())
                        } else {
                            None
                        };
                        out.insert((cx, cz, layer as u8, i), v);
                    }
                }
            }
        }
        out
    };
    let before = sample(&w);

    let p = ground(&w);
    let torch = (p.0, p.1 + 1, p.2);
    assert!(w.set_block(torch, "minecraft:torch").is_some());

    let updates = w.light_updates();
    assert!(
        !updates.is_empty(),
        "a torch placed in a built world owes at least one light_update"
    );

    // (2) Well formed, and the set of section/layer pairs the packets name.
    let mut named: std::collections::BTreeSet<(i32, i32, u8, usize)> =
        std::collections::BTreeSet::new();
    for u in &updates {
        for (layer, mask, arrays) in [
            (LightLayer::Sky, &u.light.sky_mask, Some(&u.light.sky)),
            (LightLayer::Block, &u.light.block_mask, Some(&u.light.block)),
            (LightLayer::Sky, &u.light.empty_sky_mask, None),
            (LightLayer::Block, &u.light.empty_block_mask, None),
        ] {
            let mut bits = 0usize;
            for i in 0..n {
                if bit(mask, i) {
                    bits += 1;
                    named.insert((u.chunk_x, u.chunk_z, layer as u8, i));
                }
            }
            if let Some(a) = arrays {
                assert_eq!(bits, a.len(), "a mask bit claims an array");
                for d in a {
                    assert_eq!(d.len(), 2048, "a DataLayer on the wire is 2048 bytes");
                }
            }
        }
        let body = u.encode().expect("encodes");
        assert_eq!(
            rustmc_server::chunk::LightUpdate::decode(&body).expect("decodes"),
            *u
        );
    }

    let after = sample(&w);
    let (mut moved, mut newly_stored) = (0usize, 0usize);
    let mut unnamed = Vec::new();
    for (k, a) in &after {
        let b = before.get(k).expect("the same key set both times");
        if b == a {
            continue;
        }
        if b.is_none() || a.is_none() {
            newly_stored += 1;
            continue;
        }
        moved += 1;
        if !named.contains(k) {
            unnamed.push(*k);
        }
    }
    eprintln!(
        "light_update after one torch: {} chunks, {} section/layer pairs named, \
         {moved} pairs changed while present on both sides, {newly_stored} that \
         started or stopped being stored (markNewInconsistencies, not the \
         affected set)",
        updates.len(),
        named.len()
    );
    assert!(
        moved > 0,
        "the torch moved no light, so this grades nothing"
    );
    assert!(
        unnamed.is_empty(),
        "these section/layer pairs changed and no mask named them: {unnamed:?}"
    );

    // (3) The filters cleared.
    assert!(
        w.light_updates().is_empty(),
        "broadcastChanges clears both filters; a second call owed something"
    );
}
