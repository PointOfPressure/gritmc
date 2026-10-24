//! The section light comparison: vanilla first, ours second.
//!
//! # What this is, and what it replaces
//!
//! Nothing compared the light section of `ClientboundLevelChunkWithLightPacket`
//! before this file. Every chunk we send carries `LightData::default()` — four
//! empty bitsets and two empty array lists — which classifies **0 of 26**
//! sections, and no gate anywhere would have gone red for it.
//!
//! # Why it is not a corpus check
//!
//! `trace::observe` drops a chunk packet whose block-entity list is empty:
//! 132 chunk packets inside the measured window, 1 recorded. And
//! `ci/t2-gameplay.sh` runs on superflat, whose sky light this project has now
//! measured at exactly **two distinct nibble values, {0, 15}** — a comparison
//! there is the `710` defect, a check that runs green while the code path it
//! names does not.
//!
//! So the reference is `crates/rustmc-server/golden/light_vectors.json`, which
//! `ci/light-golden.sh` dumps out of vanilla's own `LevelLightEngine` through
//! `ClientboundLightUpdatePacketData` — the wire artefact itself. The golden
//! carries the **world** as well as the light, so this test lights vanilla's
//! own input rather than a second copy of the world builders that could drift
//! away from it (ORACLE.md 5b).
//!
//! # `slab_roof` is not decoration
//!
//! A bottom slab has `lightDampening` **0** and an occlusion shape that closes
//! its DOWN face, so an engine that reads only the dampening lights a whole
//! room under a slab roof that vanilla leaves dark. That case is in the suite
//! because **6,762 of 32,366 block states (20.9%)** are in that position and
//! counting them establishes only how many a dampening-only engine COULD be
//! wrong about, never that it is wrong about one. It passes now; it did not
//! before `shape_occludes` existed.
//!
//! # Vanilla first
//!
//! Every assertion here reads `(vanilla, ours)` in that order and every
//! message prints vanilla's value first. Backwards inverts the sign of every
//! difference, and this project has paid for that once already.

use std::collections::BTreeMap;
use std::path::PathBuf;

use rustmc_server::light::{classify, LightGeometry, LightLayer};
use rustmc_server::light_engine::{light, LightProps, LitChunk, Region};

/// Load `data/vanilla/block_props.json` into the four columns the engine
/// needs, plus the state-string index the golden's palettes are written in.
/// Cargo runs an integration test with the *package* as its working
/// directory, not the workspace root, so every repo-relative path in a test
/// goes through here. Copied from `tests/entity_move.rs` rather than invented.
fn repo(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn load_props() -> (LightProps, BTreeMap<String, u16>) {
    let path = repo("data/vanilla/block_props.json");
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e} — run ci/gen-gameplay-tables.sh in this tree",
            path.display()
        )
    });
    let v: serde_json::Value = serde_json::from_str(&raw).expect("block_props.json is not json");
    let table = v["table"]
        .as_array()
        .expect("block_props.json has no table");

    let n = table.len();
    let mut props = LightProps {
        emission: vec![0; n],
        dampening: vec![0; n],
        shape_occluding: vec![false; n],
        occlusion_faces: vec![0; n],
    };
    let mut ids = BTreeMap::new();
    for row in table {
        let id = usize::try_from(row["id"].as_u64().expect("row has no id")).expect("id fits");
        // A table without the light columns is a table generated before
        // 2026-08-18. Say so, rather than lighting a world in which every
        // block is transparent and emits nothing — which would look like a
        // port defect and is a stale table.
        let emission = row["lightEmission"].as_u64().unwrap_or_else(|| {
            panic!(
                "block_props.json has no lightEmission column — it predates the \
                 light port. Re-run ci/block-props-golden.sh."
            )
        });
        props.emission[id] = u8::try_from(emission).expect("emission 0..15");
        props.dampening[id] =
            u8::try_from(row["lightDampening"].as_u64().expect("lightDampening")).expect("0..15");
        props.shape_occluding[id] = row["shapeLightOcclusion"].as_bool().expect("flag");
        let faces = row["lightOcclusionFaces"]
            .as_str()
            .expect("lightOcclusionFaces");
        let mut bits = 0u8;
        for (d, c) in faces.chars().take(6).enumerate() {
            if c == '1' {
                bits |= 1 << d;
            }
        }
        props.occlusion_faces[id] = bits;
        ids.insert(
            row["state"].as_str().expect("state").to_owned(),
            u16::try_from(id).expect("state id fits u16"),
        );
    }
    (props, ids)
}

/// `BitSet.toLongArray()` as the golden writes it — decimal strings, because
/// a JSON number cannot hold the range.
fn mask(v: &serde_json::Value) -> Vec<i64> {
    v.as_array()
        .expect("mask is an array")
        .iter()
        .map(|w| {
            w.as_str()
                .expect("mask word is a string")
                .parse()
                .expect("i64")
        })
        .collect()
}

/// The golden writes each `DataLayer` as 4096 lowercase hex characters.
fn arrays(v: &serde_json::Value) -> Vec<Vec<u8>> {
    v.as_array()
        .expect("arrays")
        .iter()
        .map(|a| {
            let hex = a.as_str().expect("hex string");
            assert_eq!(
                hex.len(),
                4096,
                "a DataLayer is 2048 bytes, got {}",
                hex.len() / 2
            );
            hex.as_bytes()
                .chunks(2)
                .map(|p| {
                    u8::from_str_radix(std::str::from_utf8(p).expect("ascii"), 16)
                        .expect("hex byte")
                })
                .collect()
        })
        .collect()
}

/// Rebuild one chunk from the golden's palette + run-length world dump.
fn chunk_from(world: &serde_json::Value, ids: &BTreeMap<String, u16>, air: u16) -> LitChunk {
    let mut out = LitChunk::default();
    for sec in world.as_array().expect("world is an array") {
        let section_y = i32::try_from(sec["section_y"].as_i64().expect("section_y")).expect("i32");
        let palette: Vec<u16> = sec["palette"]
            .as_array()
            .expect("palette")
            .iter()
            .map(|s| {
                let name = s.as_str().expect("palette entry");
                *ids.get(name).unwrap_or_else(|| {
                    panic!("block_props.json has no state {name}, which the golden's world uses")
                })
            })
            .collect();
        let mut blocks = Vec::with_capacity(4096);
        for run in sec["runs"].as_array().expect("runs") {
            let r = run.as_array().expect("run pair");
            let idx = usize::try_from(r[0].as_u64().expect("run index")).expect("usize");
            let len = usize::try_from(r[1].as_u64().expect("run length")).expect("usize");
            for _ in 0..len {
                blocks.push(palette[idx]);
            }
        }
        assert_eq!(
            blocks.len(),
            4096,
            "section {section_y} run-length decodes to {} blocks, not 4096",
            blocks.len()
        );
        let _ = air;
        out.sections.insert(section_y, blocks);
    }
    out
}

struct Verdict {
    total: usize,
    matched: usize,
    failures: Vec<String>,
    /// Our classification per chunk, kept so a history case can be compared
    /// against its from-scratch control instead of against vanilla's
    /// post-mutation answer.
    ours: BTreeMap<(i32, i32), rustmc_server::chunk::LightData>,
    /// Vanilla's, for the same reason.
    vanilla: BTreeMap<(i32, i32), rustmc_server::chunk::LightData>,
}

/// Compare one case, counting **sections** rather than cases: the headline
/// figure this whole item is named for is "how many of 26 do we classify",
/// so the check reports it in those units.
fn compare_case(
    case: &serde_json::Value,
    props: &LightProps,
    ids: &BTreeMap<String, u16>,
    geometry: LightGeometry,
    air: u16,
) -> Verdict {
    let name = case["name"].as_str().expect("case name");
    let mut chunks = BTreeMap::new();
    for ch in case["chunks"].as_array().expect("chunks") {
        let cx = i32::try_from(ch["x"].as_i64().expect("x")).expect("i32");
        let cz = i32::try_from(ch["z"].as_i64().expect("z")).expect("i32");
        chunks.insert((cx, cz), chunk_from(&ch["world"], ids, air));
    }
    let region = Region {
        geometry,
        chunks,
        air,
    };
    let lit = light(&region, props);

    let mut v = Verdict {
        total: 0,
        matched: 0,
        failures: Vec::new(),
        ours: BTreeMap::new(),
        vanilla: BTreeMap::new(),
    };
    for ch in case["chunks"].as_array().expect("chunks") {
        let cx = i32::try_from(ch["x"].as_i64().expect("x")).expect("i32");
        let cz = i32::try_from(ch["z"].as_i64().expect("z")).expect("i32");
        let ours = classify(geometry, &lit.chunk(cx, cz));
        v.ours.insert((cx, cz), ours.clone());
        v.vanilla.insert(
            (cx, cz),
            rustmc_server::chunk::LightData {
                sky_mask: mask(&ch["sky_mask"]),
                block_mask: mask(&ch["block_mask"]),
                empty_sky_mask: mask(&ch["empty_sky_mask"]),
                empty_block_mask: mask(&ch["empty_block_mask"]),
                sky: arrays(&ch["sky"]),
                block: arrays(&ch["block"]),
            },
        );

        // Field by field, vanilla first.
        for (field, vanilla, mine) in [
            ("sky_mask", mask(&ch["sky_mask"]), ours.sky_mask.clone()),
            (
                "block_mask",
                mask(&ch["block_mask"]),
                ours.block_mask.clone(),
            ),
            (
                "empty_sky_mask",
                mask(&ch["empty_sky_mask"]),
                ours.empty_sky_mask.clone(),
            ),
            (
                "empty_block_mask",
                mask(&ch["empty_block_mask"]),
                ours.empty_block_mask.clone(),
            ),
        ] {
            if vanilla != mine {
                v.failures.push(format!(
                    "{name} ({cx},{cz}) {field}: vanilla {vanilla:?}, ours {mine:?}"
                ));
            }
        }

        // Per-section accounting: a section counts as classified only when
        // vanilla and we agree about which of the three states it is in AND,
        // when it carries data, about all 2048 bytes.
        for (layer, vmask, vempty, varrays, omask, oempty, oarrays) in [
            (
                LightLayer::Sky,
                mask(&ch["sky_mask"]),
                mask(&ch["empty_sky_mask"]),
                arrays(&ch["sky"]),
                ours.sky_mask.clone(),
                ours.empty_sky_mask.clone(),
                ours.sky.clone(),
            ),
            (
                LightLayer::Block,
                mask(&ch["block_mask"]),
                mask(&ch["empty_block_mask"]),
                arrays(&ch["block"]),
                ours.block_mask.clone(),
                ours.empty_block_mask.clone(),
                ours.block.clone(),
            ),
        ] {
            let n = usize::try_from(geometry.light_section_count()).expect("count");
            let mut vseen = 0usize;
            let mut oseen = 0usize;
            for i in 0..n {
                v.total += 1;
                let vd = bit(&vmask, i);
                let ve = bit(&vempty, i);
                let od = bit(&omask, i);
                let oe = bit(&oempty, i);
                let mut ok = vd == od && ve == oe;
                if ok && vd {
                    ok = varrays.get(vseen) == oarrays.get(oseen);
                    if !ok {
                        v.failures.push(format!(
                            "{name} ({cx},{cz}) {layer:?} section {} data differs",
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
                    v.matched += 1;
                }
            }
        }
    }
    v
}

fn bit(words: &[i64], i: usize) -> bool {
    words
        .get(i / 64)
        .is_some_and(|w| (*w as u64 >> (i % 64)) & 1 == 1)
}

#[test]
fn section_light_matches_vanilla() {
    let raw = std::fs::read_to_string(repo("crates/rustmc-server/golden/light_vectors.json"))
        .expect("crates/rustmc-server/golden/light_vectors.json — run ci/light-golden.sh");
    let g: serde_json::Value = serde_json::from_str(&raw).expect("golden is json");
    let geometry = LightGeometry {
        min_section_y: i32::try_from(g["min_section_y"].as_i64().expect("min_section_y"))
            .expect("i32"),
        sections_count: i32::try_from(g["sections_count"].as_i64().expect("sections_count"))
            .expect("i32"),
    };
    assert_eq!(
        i64::from(geometry.light_section_count()),
        g["light_section_count"].as_i64().expect("count"),
        "vanilla's own light_section_count disagrees with LightGeometry"
    );

    let (props, ids) = load_props();
    let air = *ids.get("minecraft:air").expect("air is in block_props");

    let mut total = 0usize;
    let mut matched = 0usize;
    let mut failures = Vec::new();
    let mut per_case = Vec::new();
    let mut verdicts: BTreeMap<String, Verdict> = BTreeMap::new();
    let mut history_cases: Vec<(String, String)> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    for case in g["cases"].as_array().expect("cases") {
        let name = case["name"].as_str().expect("name").to_owned();
        let v = compare_case(case, &props, &ids, geometry, air);
        // A case reached by mutating an already-lit world carries the name of
        // a control case that builds the SAME final world from scratch. Our
        // engine lights from scratch and is handed only the final world, so
        // holding it to the post-mutation answer would be demanding an answer
        // its input does not contain. It is held to the control instead, and
        // separately required to DIFFER from vanilla's own post-mutation
        // answer — see the registered divergence below.
        // A mutated case with no control is not gradeable here AT ALL. This
        // test lights the dumped world from scratch and the dumped world is
        // the world after the edit; for `roof_opened` and `sky_sealed`
        // vanilla's incremental repair and a from-scratch relight are
        // genuinely different answers and neither is defective.
        // `tests/light_incremental_mutation.rs` grades those, by reproducing
        // the edit.
        let mutated = !case["mutations"]
            .as_array()
            .expect("every case carries a mutations array")
            .is_empty();
        if let Some(control) = case["from_scratch_control"].as_str() {
            history_cases.push((name.clone(), control.to_owned()));
        } else if mutated {
            skipped.push(name.clone());
        } else {
            per_case.push(format!("{name:12} {:5}/{:<5}", v.matched, v.total));
            total += v.total;
            matched += v.matched;
            failures.extend(v.failures.clone());
        }
        verdicts.insert(name, v);
    }

    // The headline the item is named for, printed whether or not it passes,
    // because "0 of 26" was the finding and the number moving is the result.
    eprintln!("section-light classification, vanilla first:");
    for line in &per_case {
        eprintln!("  {line}");
    }
    eprintln!("  TOTAL {matched}/{total} sections agree with vanilla");
    if !skipped.is_empty() {
        eprintln!(
            "  not gradeable from scratch, see tests/light_incremental_mutation.rs: {}",
            skipped.join(" ")
        );
    }

    assert!(
        failures.is_empty(),
        "{} of {total} sections disagree with vanilla; first 20:\n{}",
        total - matched,
        failures
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );

    // ---- the registered divergence -------------------------------------
    //
    // Measured 2026-08-18 and it is not a port defect: vanilla's own section
    // light for ONE AND THE SAME final world differs according to how the
    // world was reached. `relit` removes a glowstone from a lit sealed room;
    // `sealed_dark` builds that identical room, glowstone already absent, and
    // lights it once. Vanilla gives:
    //
    //   relit        block_mask .....1....  1 array, 2048 ZERO bytes
    //   sealed_dark  block_mask ..........  0 arrays, section 0 in empty mask
    //
    // because `DataLayer.isEmpty()` is `data == null && defaultValue == 0`
    // and the decrease that darkened the room left the array allocated.
    //
    // Two things are asserted here, and the second is the one that keeps this
    // honest. First, that we reproduce the from-scratch control exactly —
    // that is the answer our input can support. Second, that vanilla's two
    // answers really do differ, so this block cannot quietly become vacuous
    // if a future jar stops distinguishing them: if it does, this test says
    // so instead of passing.
    assert!(
        !history_cases.is_empty(),
        "no case carries a from_scratch_control — the history divergence is \
         no longer covered by the golden, and this block is now vacuous"
    );
    for (name, control) in &history_cases {
        let mutated = &verdicts[name];
        let scratch = &verdicts[control];
        let mut differed = 0usize;
        for (pos, ours) in &mutated.ours {
            assert_eq!(
                scratch.vanilla.get(pos),
                Some(ours),
                "{name}: our from-scratch light of chunk {pos:?} does not match \
                 vanilla's own from-scratch light of the same world ({control})"
            );
            if mutated.vanilla.get(pos) != Some(ours) {
                differed += 1;
            }
        }
        assert_eq!(
            differed,
            mutated.ours.len(),
            "{name}: vanilla's post-mutation answer agrees with its own \
             from-scratch answer on {} of {} chunks. The materialise-then-darken \
             divergence this case exists to pin has stopped happening, so the \
             assertion above is measuring nothing.",
            mutated.ours.len() - differed,
            mutated.ours.len()
        );
        eprintln!(
            "  registered divergence: {name} differs from vanilla on {differed}/{} chunks \
             (edit history, not block states) and matches {control} exactly",
            mutated.ours.len()
        );
    }
}
