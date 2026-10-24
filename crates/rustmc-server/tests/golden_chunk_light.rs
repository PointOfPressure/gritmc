//! Our light against **vanilla's own light bytes, in the packet that carries
//! the blocks it was computed from**.
//!
//! `light_harvest.rs` grades the engine against vanilla's saved region files.
//! This grades what a client actually receives, in the units the wire uses —
//! four bitsets and a list of 2048-byte `DataLayer`s — on a *generated* world.
//!
//! # Why this is not circular
//!
//! Both sides read their blocks out of the **same 117 packets**. A captured
//! `level_chunk_with_light` body holds the section palettes and the light side
//! by side, so this decodes vanilla's blocks, lights them with our engine, and
//! compares against the light vanilla shipped in the same bytes. No worldgen
//! residual can reach the number: our generator is not consulted.
//!
//! That is what the 2026-09-05 default flip needed and did not have.
//! `golden_chunk_packet.rs`'s 117-of-117 is a decode/encode round trip — it
//! constructs no `World`, reads no flag, and is green whether this server's
//! light is exact or absent.
//!
//! # WHAT IT FOUND: VANILLA'S OWN PACKET LIGHT LAGS ITS OWN BLOCKS
//!
//! Two captures were taken from **one** vanilla server at seed 31415926, the
//! second 120 s after the first, same chunks, same client:
//!
//! ```text
//!   capture       block nibbles   sky nibbles   window
//!   first          5,261            190         2,437,120
//!   +120 s            55            190         2,572,288
//! ```
//!
//! The block half falls by 99%. It is not our engine converging on anything —
//! the engine is deterministic and was run identically on both — it is
//! **vanilla's chunk packet carrying light that is one lava-flow behind its own
//! blocks**. The first sample attributed by `RUSTMC_LIGHT_DUMP` sits 14 blocks
//! from `minecraft:lava[level=2]` at (-54,-27,-53), whose *source* block
//! `lava[level=0]` is one block further out at (-54,-27,-52): fluid ticks
//! spread the lava after the chunk was lit, `LevelChunk.setBlockState` queued
//! the repair on the light thread, and the chunk was serialised with the new
//! block and the old light. Our answer — one brighter, exactly one lava block's
//! worth — is the converged one.
//!
//! **The correction vanilla sends for that is a `light_update`, and this
//! project has never recorded one**: `tools/oracle-runner/src/trace.rs`
//! classifies `cb::LIGHT_UPDATE` as housekeeping. So the residual here is not
//! a defect to chase, it is the measurement that says the packet matters.
//!
//! What is left after settling is attributed and small:
//!
//! * **190 sky nibbles, and they are vanilla disagreeing with its own rule.**
//!   `RUSTMC_LIGHT_DUMP=1` prints the column: at (-62, 67, 15) vanilla's saved
//!   sky is **15 at an `oak_leaves` block and 15 all the way down to y = 63**,
//!   where the water starts. `ChunkSkyLightSources.isEdgeOccluded` opens
//!   `if (below.getLightDampening() != 0) return true;` and
//!   `BlockState.getLightDampening()` for that leaf state is **1** — it is
//!   vanilla's own method that `ci/block-props-golden.sh` calls to build the
//!   column. So vanilla's source table for that column did not contain the
//!   leaves: it stopped at the water, four blocks lower, exactly as it would
//!   have before the tree was placed. Our transcription matches the bytecode
//!   line for line and our answer is the one the rule gives.
//!
//!   The shape corroborates it. **143 of the 190 sit on a chunk face**, where
//!   only 23.4% of a section's columns do — 3.2x — and a tree that straddles a
//!   border is written into this chunk by the *neighbour's* FEATURES stage.
//!   `ProtoChunk.setBlockState` runs its `ChunkSkyLightSources.update` only
//!   when the status is past `INITIALIZE_LIGHT` **and** its light engine is
//!   non-null, which during worldgen it is not — the same conditional that
//!   made vanilla's own golden record that an 8x8 stone lid darkens nothing
//!   (`docs/FINDINGS.md`, 2026-09-05).
//!
//!   It is stable across settling (190 on both captures from one server)
//!   because no block changes after load, so nothing ever asks vanilla to
//!   repair it, and it moves to **299** on a capture from a different run of
//!   the same seed, which is vanilla's non-deterministic worldgen reaching a
//!   light figure.
//! * **55 block nibbles**, the tail of the same lag.
//! * **27 sections vanilla sent an array for and we set no bit** — block
//!   sections 3 and 4. That is the registered `relit` divergence in its
//!   representation form: vanilla materialised a layer, then the light left
//!   it, and 2048 zero bytes ride in the data mask where a from-scratch
//!   answer carries nothing. `docs/cold-start/light-section-data.md`.
//!
//! # Running it
//!
//! The capture is Mojang-derived and therefore never committed. Take a
//! **settled** one — start the server, connect once, wait, connect again —
//! or the block figure measures the fluid ticks rather than the light:
//!
//! ```sh
//! ci/dump-chunks.sh <out> 25591 31415926
//! RUSTMC_CHUNK_CAPTURE=<out> cargo test --release -p rustmc-server \
//!     --test golden_chunk_light -- --nocapture
//! ```
//!
//! `RUSTMC_LIGHT_DUMP=1` adds the per-position attribution and the nearest
//! emitters to the first disagreement.

use rustmc_server::chunk::{ContainerKind, LevelChunkWithLight, OVERWORLD_SECTIONS};
use rustmc_server::light::{LightGeometry, LightLayer};
use rustmc_server::light_engine::{LitChunk, Region};
use rustmc_server::terrain::StateIds;

/// `RMCCHK01`, count, then per entry: i32 x, i32 z, u32 len, body.
fn capture() -> Option<Vec<(i32, i32, Vec<u8>)>> {
    let path = std::env::var("RUSTMC_CHUNK_CAPTURE").ok()?;
    let data = std::fs::read(&path).unwrap_or_else(|e| panic!("RUSTMC_CHUNK_CAPTURE={path}: {e}"));
    assert_eq!(&data[..8], b"RMCCHK01", "not a chunk capture: {path}");
    let n = u32::from_be_bytes(data[8..12].try_into().unwrap()) as usize;
    let mut out = Vec::with_capacity(n);
    let mut i = 12;
    for _ in 0..n {
        let x = i32::from_be_bytes(data[i..i + 4].try_into().unwrap());
        let z = i32::from_be_bytes(data[i + 4..i + 8].try_into().unwrap());
        let len = u32::from_be_bytes(data[i + 8..i + 12].try_into().unwrap()) as usize;
        i += 12;
        out.push((x, z, data[i..i + len].to_vec()));
        i += len;
    }
    Some(out)
}

fn repo(relative: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// The overworld's geometry, which is what the capture was taken in.
const GEOMETRY: LightGeometry = LightGeometry::OVERWORLD;

/// Build the lit region out of the captured packets themselves.
///
/// A packet's palette entries are **global block-state ids** — the same
/// numbering `block_props.json` is indexed by, because both come out of the
/// same jar — so they go straight into a `Region` with no name lookup and no
/// re-resolution. That is the property that makes this comparison read
/// vanilla's blocks rather than a reconstruction of them.
fn region(packets: &[(i32, i32, LevelChunkWithLight)], air: u16) -> Region {
    let mut chunks = std::collections::BTreeMap::new();
    for (cx, cz, p) in packets {
        let mut chunk = LitChunk::default();
        for (i, s) in p.sections.iter().enumerate() {
            let sy = GEOMETRY.min_section_y + i32::try_from(i).expect("24 sections");
            let values = s.states.values(ContainerKind::BlockStates);
            assert_eq!(values.len(), 4096, "a section is 4096 cells");
            // `LevelChunkSection.hasOnlyAir()` — an all-air section is absent
            // from the map, which is what the stored-section rule reads. The
            // packet's own `nonEmptyBlockCount` says so directly, and using
            // it rather than re-deriving it means a disagreement between the
            // count and the cells would surface here rather than silently.
            if s.non_empty_blocks == 0 {
                continue;
            }
            chunk.sections.insert(
                sy,
                values
                    .iter()
                    .map(|&v| u16::try_from(v).unwrap_or(0))
                    .collect(),
            );
        }
        chunks.insert((*cx, *cz), chunk);
    }
    Region {
        geometry: GEOMETRY,
        chunks,
        air,
    }
}

struct Loaded {
    packets: Vec<(i32, i32, LevelChunkWithLight)>,
    air: u16,
    props: rustmc_server::light_engine::LightProps,
}

fn load() -> Option<Loaded> {
    let raw = capture()?;
    let ids = StateIds::load(&repo("data/vanilla/block_props.json")).expect("block table");
    let props =
        rustmc_server::light_engine::LightProps::load(&repo("data/vanilla/block_props.json"))
            .expect("block_props.json carries the light columns");
    let air = u16::try_from(ids.get("minecraft:air").expect("air is a block")).expect("fits");
    let packets = raw
        .into_iter()
        .map(|(x, z, body)| {
            let p = LevelChunkWithLight::decode(&body, OVERWORLD_SECTIONS)
                .unwrap_or_else(|e| panic!("chunk ({x}, {z}): decode: {e}"));
            (x, z, p)
        })
        .collect();
    Some(Loaded {
        packets,
        air,
        props,
    })
}

/// Every nibble vanilla put in the packet, against every nibble we would put
/// there for the same blocks.
///
/// Only chunks with all eight neighbours in the capture are scored, for the
/// reason `light_harvest` gives: the stored-section rule is a 3x3x3 cube in
/// section space, and an edge chunk would be lit against a wall vanilla did
/// not have.
#[test]
fn our_light_matches_vanillas_in_the_captured_chunk_packets() {
    let Some(l) = load() else {
        eprintln!("SKIP golden_chunk_light: set RUSTMC_CHUNK_CAPTURE (see the module docs)");
        return;
    };
    let region = region(&l.packets, l.air);
    let lit = rustmc_server::light_incremental::light_region(&region, &l.props);

    let present: std::collections::BTreeSet<(i32, i32)> = region.chunks.keys().copied().collect();
    let scored: Vec<(i32, i32)> = present
        .iter()
        .copied()
        .filter(|&(x, z)| (-1..=1).all(|dx| (-1..=1).all(|dz| present.contains(&(x + dx, z + dz)))))
        .collect();
    assert!(
        scored.len() >= 20,
        "only {} chunks have all eight neighbours; the capture is too small \
         or too ragged to grade",
        scored.len()
    );

    let mut compared = 0u64;
    let mut differ = 0u64;
    let mut absent_here = 0u64;
    let mut absent_there = 0u64;
    let mut census: std::collections::BTreeMap<(u8, i32, u8, u8), u64> =
        std::collections::BTreeMap::new();
    let mut per_chunk_diff: std::collections::BTreeMap<(i32, i32), u64> =
        std::collections::BTreeMap::new();
    let (mut sky_diff, mut block_diff) = (0u64, 0u64);
    let mut sky_on_face = 0u64;
    let mut samples: Vec<(LightLayer, i32, i32, i32, u8, u8)> = Vec::new();
    let mut sky_samples: Vec<(i32, i32, i32, u8, u8)> = Vec::new();
    let mut absent_census: std::collections::BTreeMap<(u8, i32), u64> =
        std::collections::BTreeMap::new();
    for &(cx, cz) in &scored {
        let (_, _, p) = l
            .packets
            .iter()
            .find(|(x, z, _)| (*x, *z) == (cx, cz))
            .expect("scored chunks come from the capture");
        let ours = rustmc_server::light::classify(GEOMETRY, &lit.chunk(cx, cz));
        for layer in [LightLayer::Sky, LightLayer::Block] {
            let (mask, arrays, our_mask, our_arrays) = match layer {
                LightLayer::Sky => (&p.light.sky_mask, &p.light.sky, &ours.sky_mask, &ours.sky),
                LightLayer::Block => (
                    &p.light.block_mask,
                    &p.light.block,
                    &ours.block_mask,
                    &ours.block,
                ),
            };
            let n = usize::try_from(GEOMETRY.light_section_count()).expect("26");
            let bit = |m: &[i64], i: usize| -> bool {
                m.get(i / 64).is_some_and(|w| (w >> (i % 64)) & 1 == 1)
            };
            // Walk both masks together, so a section vanilla sent an array
            // for and we did not is charged rather than skipped.
            let (mut vi, mut oi) = (0usize, 0usize);
            for i in 0..n {
                let theirs = bit(mask, i);
                let mine = bit(our_mask, i);
                let sy = GEOMETRY.section_y_of_bit(i32::try_from(i).expect("bit"));
                match (theirs, mine) {
                    (true, true) => {
                        let a = &arrays[vi];
                        let b = &our_arrays[oi];
                        vi += 1;
                        oi += 1;
                        for k in 0..4096 {
                            let v = (a[k >> 1] >> (4 * (k & 1))) & 15;
                            let o = (b[k >> 1] >> (4 * (k & 1))) & 15;
                            compared += 1;
                            if v != o {
                                differ += 1;
                                if layer == LightLayer::Sky && sky_samples.len() < 12 {
                                    let (lx, ly, lz) = (k & 15, (k >> 8) & 15, (k >> 4) & 15);
                                    sky_samples.push((
                                        cx * 16 + i32::try_from(lx).expect("x"),
                                        sy * 16 + i32::try_from(ly).expect("y"),
                                        cz * 16 + i32::try_from(lz).expect("z"),
                                        v,
                                        o,
                                    ));
                                }
                                if std::env::var_os("RUSTMC_LIGHT_DUMP").is_some()
                                    && samples.len() < 24
                                {
                                    let (lx, ly, lz) = (k & 15, (k >> 8) & 15, (k >> 4) & 15);
                                    samples.push((
                                        layer,
                                        cx * 16 + i32::try_from(lx).expect("x"),
                                        sy * 16 + i32::try_from(ly).expect("y"),
                                        cz * 16 + i32::try_from(lz).expect("z"),
                                        v,
                                        o,
                                    ));
                                }
                                *per_chunk_diff.entry((cx, cz)).or_default() += 1;
                                match layer {
                                    LightLayer::Sky => {
                                        sky_diff += 1;
                                        // Is this position on a chunk face?
                                        // A tree that straddles a chunk
                                        // border is written by the NEIGHBOUR's
                                        // FEATURES stage, and `ProtoChunk.setBlockState`
                                        // runs its `ChunkSkyLightSources.update`
                                        // only past INITIALIZE_LIGHT with a
                                        // non-null engine — so a late write
                                        // leaves vanilla's own source table
                                        // stale for that column.
                                        let (lx, lz) = (k & 15, (k >> 4) & 15);
                                        if lx == 0 || lx == 15 || lz == 0 || lz == 15 {
                                            sky_on_face += 1;
                                        }
                                    }
                                    LightLayer::Block => block_diff += 1,
                                }
                                *census.entry((layer as u8, sy, v, o)).or_default() += 1;
                            }
                        }
                    }
                    (true, false) => {
                        vi += 1;
                        absent_here += 1;
                        *absent_census.entry((layer as u8, sy)).or_default() += 1;
                    }
                    (false, true) => {
                        oi += 1;
                        absent_there += 1;
                    }
                    (false, false) => {}
                }
            }
        }
    }

    let mut per_chunk: Vec<((i32, i32), u64, i32)> = per_chunk_diff
        .into_iter()
        .map(|((cx, cz), n)| {
            // How many rings of loaded chunks sit outside this one. A chunk
            // whose residual disappears at ring 2 is being scored against a
            // neighbourhood the capture does not contain, which is a fact
            // about the window and not about the engine.
            let mut ring = 0;
            while (-(ring + 1)..=(ring + 1)).all(|dx| {
                (-(ring + 1)..=(ring + 1)).all(|dz| present.contains(&(cx + dx, cz + dz)))
            }) {
                ring += 1;
            }
            ((cx, cz), n, ring)
        })
        .collect();
    per_chunk.sort_by_key(|(_, n, _)| std::cmp::Reverse(*n));
    let mut by_ring: std::collections::BTreeMap<i32, (u64, u64)> =
        std::collections::BTreeMap::new();
    for &(_, n, ring) in &per_chunk {
        let e = by_ring.entry(ring).or_default();
        e.0 += n;
        e.1 += 1;
    }
    for (ring, (n, chunks)) in &by_ring {
        eprintln!("  margin ring {ring}: {n} differing nibbles over {chunks} chunks");
    }
    eprintln!(
        "  by layer: sky {sky_diff} (of which {sky_on_face} on a chunk face), block {block_diff}"
    );

    if std::env::var_os("RUSTMC_LIGHT_DUMP").is_some() {
        let text = std::fs::read_to_string(repo("data/vanilla/block_props.json")).expect("read");
        let doc: serde_json::Value = serde_json::from_str(&text).expect("json");
        let mut names: std::collections::BTreeMap<i64, String> = std::collections::BTreeMap::new();
        for e in doc["table"].as_array().expect("table") {
            if let (Some(n), Some(i)) = (e["state"].as_str(), e["id"].as_i64()) {
                names.insert(i, n.to_string());
            }
        }
        let at = |x: i32, y: i32, z: i32| -> String {
            region
                .chunks
                .get(&(x >> 4, z >> 4))
                .and_then(|c| c.sections.get(&y.div_euclid(16)))
                .map_or_else(
                    || "<unloaded>".to_string(),
                    |sec| {
                        let id = sec[rustmc_server::light::DataLayer::index(
                            u8::try_from(x.rem_euclid(16)).expect("x"),
                            u8::try_from(y.rem_euclid(16)).expect("y"),
                            u8::try_from(z.rem_euclid(16)).expect("z"),
                        )];
                        format!(
                            "{} e{} d{}",
                            names.get(&i64::from(id)).map_or("?", String::as_str),
                            l.props.emission[id as usize],
                            l.props.dampening[id as usize]
                        )
                    },
                )
        };
        // The nearest emitters to the first sample, so the residual is
        // attributed to a source rather than guessed at.
        if let Some((_, sx, sy0, sz, _, _)) = samples.first() {
            let mut found: Vec<(i32, i32, i32, i32, u8, String)> = Vec::new();
            for dy in -24..=24i32 {
                for dz in -24..=24i32 {
                    for dx in -24..=24i32 {
                        let (x, y, z) = (sx + dx, sy0 + dy, sz + dz);
                        let Some(c) = region.chunks.get(&(x >> 4, z >> 4)) else {
                            continue;
                        };
                        let Some(sec) = c.sections.get(&y.div_euclid(16)) else {
                            continue;
                        };
                        let id = sec[rustmc_server::light::DataLayer::index(
                            u8::try_from(x.rem_euclid(16)).expect("x"),
                            u8::try_from(y.rem_euclid(16)).expect("y"),
                            u8::try_from(z.rem_euclid(16)).expect("z"),
                        )];
                        let e = l.props.emission[id as usize];
                        if e > 0 {
                            found.push((
                                dx.abs() + dy.abs() + dz.abs(),
                                x,
                                y,
                                z,
                                e,
                                names.get(&i64::from(id)).cloned().unwrap_or_default(),
                            ));
                        }
                    }
                }
            }
            found.sort_unstable();
            for (d, x, y, z, e, n) in found.into_iter().take(6) {
                eprintln!("  EMITTER manhattan {d} at ({x},{y},{z}) emission {e}: {n}");
            }
        }
        if let Some((x, _, z, _, _)) = sky_samples.first().copied() {
            let p = l
                .packets
                .iter()
                .find(|(cx, cz, _)| (*cx, *cz) == (x >> 4, z >> 4))
                .map(|(_, _, p)| p);
            let n = usize::try_from(GEOMETRY.light_section_count()).expect("26");
            let van = |y: i32| -> String {
                let Some(p) = p else { return "?".into() };
                let sy = y.div_euclid(16);
                let bit = usize::try_from(sy - GEOMETRY.min_light_section()).expect("bit");
                let mut k = 0usize;
                for i in 0..n {
                    let set = p
                        .light
                        .sky_mask
                        .get(i / 64)
                        .is_some_and(|w| (w >> (i % 64)) & 1 == 1);
                    if i == bit {
                        return if set {
                            let a = &p.light.sky[k];
                            let idx = rustmc_server::light::DataLayer::index(
                                u8::try_from(x.rem_euclid(16)).expect("x"),
                                u8::try_from(y.rem_euclid(16)).expect("y"),
                                u8::try_from(z.rem_euclid(16)).expect("z"),
                            );
                            format!("{}", (a[idx >> 1] >> (4 * (idx & 1))) & 15)
                        } else {
                            "-".into()
                        };
                    }
                    if set {
                        k += 1;
                    }
                }
                "?".into()
            };
            for y in (58..=80).rev() {
                eprintln!(
                    "  COLUMN ({x},{y},{z}) vanilla sky {} ours {} [{}]",
                    van(y),
                    lit.level_at(
                        LightLayer::Sky,
                        rustmc_server::light_incremental::as_long(x, y, z)
                    ),
                    at(x, y, z)
                );
            }
        }
        for (x, y, z, v, o) in &sky_samples {
            eprintln!(
                "  SKYDUMP ({x},{y},{z}) vanilla {v} ours {o} lowest_source_y={} top={:?}: \
                 self [{}] up [{}] up2 [{}] -x [{}] +x [{}] -z [{}] +z [{}]",
                rustmc_server::light_engine::lowest_source_y(
                    &region,
                    &l.props,
                    x >> 4,
                    z >> 4,
                    u8::try_from(x.rem_euclid(16)).expect("x"),
                    u8::try_from(z.rem_euclid(16)).expect("z"),
                ),
                region
                    .chunks
                    .get(&(x >> 4, z >> 4))
                    .and_then(|c| c.sections.keys().max().copied()),
                at(*x, *y, *z),
                at(*x, *y + 1, *z),
                at(*x, *y + 2, *z),
                at(*x - 1, *y, *z),
                at(*x + 1, *y, *z),
                at(*x, *y, *z - 1),
                at(*x, *y, *z + 1),
            );
        }
        for (layer, x, y, z, v, o) in &samples {
            eprintln!(
                "  DUMP {layer:?} ({x},{y},{z}) vanilla {v} ours {o}: self [{}] up [{}] down [{}] \
                 -x [{}] +x [{}] -z [{}] +z [{}]",
                at(*x, *y, *z),
                at(*x, *y + 1, *z),
                at(*x, *y - 1, *z),
                at(*x - 1, *y, *z),
                at(*x + 1, *y, *z),
                at(*x, *y, *z - 1),
                at(*x, *y, *z + 1),
            );
        }
    }

    for ((layer, sy), n) in &absent_census {
        let name = if *layer == LightLayer::Sky as u8 {
            "sky"
        } else {
            "block"
        };
        eprintln!("  ABSENT {name} section {sy}: vanilla sent an array, we set no bit — {n} times");
    }
    for (x, y, z, v, o) in &sky_samples {
        eprintln!("  SKY ({x},{y},{z}) vanilla {v} ours {o}");
    }

    let mut worst: Vec<_> = census.into_iter().collect();
    worst.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for ((layer, sy, v, o), n) in worst.into_iter().take(12) {
        let name = if layer == LightLayer::Sky as u8 {
            "sky"
        } else {
            "block"
        };
        eprintln!("  {name} section {sy}: vanilla {v} ours {o} — {n} nibbles");
    }
    eprintln!(
        "chunk-packet light, vanilla first: {differ} of {compared} nibbles differ over \
         {} chunks of {} captured, seed 31415926; {absent_here} sections vanilla sent \
         and we did not, {absent_there} we sent and vanilla did not",
        scored.len(),
        l.packets.len()
    );
    assert!(
        compared > 1_000_000,
        "a window of {compared} nibbles is too small to mean anything"
    );
    // What is asserted is what is stable. The block half depends on how
    // settled the captured server was — 5,261 on a fresh capture, 55 on one
    // taken 120 s later from the same server — so a fixed ceiling on it would
    // red on an honest fresh capture and say nothing. See the module docs.
    assert_eq!(
        absent_there, 0,
        "we put an array in a mask bit vanilla left unset — we are claiming \
         light for a section vanilla says it has none for, which no lag can \
         explain"
    );
    // A gross ceiling, and deliberately not a tight one. Three captures at
    // this seed carry 190, 190 and **299** sky nibbles and 26, 26 and 27
    // absent sections: the two figures move between *server runs* as well as
    // with settledness, which is what vanilla's own non-deterministic
    // worldgen predicts, so a tight number here would be a number about one
    // capture. What this catches is an engine that has stopped working:
    // 0.1% of the window is ~2,500 nibbles against a measured 245.
    let ceiling = compared / 1000;
    assert!(
        differ <= ceiling,
        "{differ} of {compared} nibbles differ, over the 0.1% gross ceiling of \
         {ceiling}. Settled captures at this seed measure 245. Attribute it \
         with RUSTMC_LIGHT_DUMP=1 before widening anything."
    );
}

/// The positive control, and it is the one `light_harvest` had to learn:
/// a control naming a constant has to be checked against the population it
/// runs over.
///
/// Making the capture's commonest non-air state emit light must move the
/// figure, and the denominator must not move with it — otherwise the two
/// numbers are not comparable.
#[test]
fn making_the_commonest_state_emit_light_moves_the_figure() {
    let Some(l) = load() else {
        eprintln!("SKIP golden_chunk_light: set RUSTMC_CHUNK_CAPTURE");
        return;
    };
    let region = region(&l.packets, l.air);
    let mut counts: std::collections::BTreeMap<u16, u64> = std::collections::BTreeMap::new();
    for c in region.chunks.values() {
        for s in c.sections.values() {
            for &v in s {
                if v != l.air {
                    *counts.entry(v).or_default() += 1;
                }
            }
        }
    }
    let (&state, &n) = counts
        .iter()
        .max_by_key(|(_, n)| **n)
        .expect("the capture holds non-air blocks");
    let mut props = l.props.clone();
    props.emission[state as usize] = 15;

    let before = rustmc_server::light_incremental::light_region(&region, &l.props);
    let after = rustmc_server::light_incremental::light_region(&region, &props);
    let mut moved = 0u64;
    for &(cx, cz) in region.chunks.keys() {
        for sy in GEOMETRY.min_light_section()..GEOMETRY.max_light_section() {
            for k in 0..4096u32 {
                let p = rustmc_server::light_incremental::as_long(
                    cx * 16 + i32::try_from(k & 15).expect("x"),
                    sy * 16 + i32::try_from((k >> 8) & 15).expect("y"),
                    cz * 16 + i32::try_from((k >> 4) & 15).expect("z"),
                );
                if before.level_at(LightLayer::Block, p) != after.level_at(LightLayer::Block, p) {
                    moved += 1;
                }
            }
        }
    }
    eprintln!(
        "control: state id {state} ({n} cells, the capture's commonest non-air) \
         emitting 15 moves {moved} block-light nibbles"
    );
    assert!(
        moved > 0,
        "the control moved nothing, so the comparison above is not reading \
         the light it says it is"
    );
}
