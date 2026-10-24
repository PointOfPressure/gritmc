//! The **incremental** engine against vanilla's own light, vanilla first.
//!
//! # What this grades that `light_golden.rs` does not
//!
//! `light_golden.rs` grades `light_engine::light`, a max-plus fixpoint. This
//! grades `light_incremental::light_region`, which is vanilla's actual
//! algorithm: `updateSectionStatus` per section, `setLightEnabled` per chunk,
//! `propagateLightSources` per chunk, then `runLightUpdates` until
//! `hasLightWork()` is false — two FIFO queues, `QueueEntry` packing, the
//! decrease pass before the increase pass, the sky column shortcut, the
//! empty-section walk.
//!
//! The two answers **should** agree on a from-scratch world and the reason is
//! worth stating: converged light is order-independent, which
//! `ci/light-golden.sh` measures by running vanilla's engine twice and
//! `cmp`ing. So this is not a weaker check than `light_golden.rs` — it is the
//! same vectors read by a completely different algorithm, and a disagreement
//! between the two is a real finding about one of them.
//!
//! It is also the only thing in the tree that can be wrong about
//! `QueueEntry`, about the skip-one-direction bits, about
//! `repeatFirstLayer`, or about `propagateLightSources` writing 15 straight
//! into the layer. None of those exist on the fixpoint's side.
//!
//! # The `relit` case is graded against its control, not against vanilla
//!
//! `relit` reaches the same final world by removing a glowstone rather than
//! by never placing one, and vanilla's answer differs: it carries an array of
//! 2048 zero bytes where `sealed_dark` carries none. That is history, not a
//! port defect (`docs/cold-start/light-section-data.md`), and this file
//! reproduces the from-scratch control for it exactly as `light_golden.rs`
//! does — the *mutation* half is what
//! `tests/light_incremental_mutation.rs` grades.

mod lightfix;

use std::collections::BTreeMap;

use rustmc_server::light::{classify, LightGeometry, LightLayer};
use rustmc_server::light_engine::Region;
use rustmc_server::light_incremental::light_region;

use lightfix::{arrays, chunk_from, load_props, mask, repo};

fn bit(words: &[i64], i: usize) -> bool {
    words
        .get(i / 64)
        .is_some_and(|w| (*w as u64) >> (i % 64) & 1 == 1)
}

/// Sections agreed, sections compared, and the failures — the same units
/// `light_golden.rs` reports, so the two numbers are comparable.
struct Verdict {
    total: usize,
    matched: usize,
    failures: Vec<String>,
}

fn compare_case(
    case: &serde_json::Value,
    props: &rustmc_server::light_engine::LightProps,
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
    let lit = light_region(&region, props);

    let mut v = Verdict {
        total: 0,
        matched: 0,
        failures: Vec::new(),
    };
    for ch in case["chunks"].as_array().expect("chunks") {
        let cx = i32::try_from(ch["x"].as_i64().expect("x")).expect("i32");
        let cz = i32::try_from(ch["z"].as_i64().expect("z")).expect("i32");
        let ours = classify(geometry, &lit.chunk(cx, cz));

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
                if !ok && v.failures.len() < 24 {
                    v.failures.push(format!(
                        "{name} ({cx},{cz}) {layer:?} bit {i}: vanilla data={vd} empty={ve}, \
                         ours data={od} empty={oe}"
                    ));
                }
                if ok && vd {
                    let (a, b) = (varrays.get(vseen), oarrays.get(oseen));
                    ok = a == b;
                    if !ok && v.failures.len() < 24 {
                        let diff = match (a, b) {
                            (Some(a), Some(b)) => a.iter().zip(b).filter(|(x, y)| x != y).count(),
                            _ => 0,
                        };
                        v.failures.push(format!(
                            "{name} ({cx},{cz}) {layer:?} section {}: {diff} of 2048 bytes differ",
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

fn golden() -> serde_json::Value {
    let path = repo("crates/rustmc-server/golden/light_vectors.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e} — run ci/light-golden.sh", path.display()));
    serde_json::from_str(&raw).expect("light_vectors.json is not json")
}

/// The headline. Every case except `relit`, which is graded against its
/// from-scratch control in the test below rather than against vanilla's
/// post-mutation answer.
#[test]
fn the_incremental_engine_reproduces_vanillas_section_light() {
    let (props, ids) = load_props();
    let g = golden();
    let geometry = LightGeometry {
        min_section_y: i32::try_from(g["min_section_y"].as_i64().expect("min_section_y"))
            .expect("i32"),
        sections_count: i32::try_from(g["sections_count"].as_i64().expect("sections_count"))
            .expect("i32"),
    };
    let air = *ids
        .get("minecraft:air")
        .expect("air is in block_props.json");

    let mut total = 0usize;
    let mut matched = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut per_case: Vec<String> = Vec::new();
    for case in g["cases"].as_array().expect("cases") {
        let name = case["name"].as_str().expect("name");
        if name == "relit" {
            continue;
        }
        let v = compare_case(case, &props, &ids, geometry, air);
        per_case.push(format!("{name} {}/{}", v.matched, v.total));
        total += v.total;
        matched += v.matched;
        failures.extend(v.failures);
    }
    // The number, with its denominator, printed whether it passes or fails —
    // a figure nobody can read off a green run is not a figure.
    println!(
        "incremental section light, vanilla first:\n  {}",
        per_case.join("  ")
    );
    println!("  TOTAL {matched}/{total} sections agree with vanilla");
    assert!(
        total > 0,
        "the golden carried no cases — that is a vacuous green, not a pass"
    );
    assert_eq!(
        matched,
        total,
        "{} of {total} sections disagree:\n{}",
        total - matched,
        failures.join("\n")
    );
}

/// The two engines are different algorithms over the same input. On a
/// from-scratch world they must agree, and if they ever do not, exactly one
/// of them is wrong about vanilla — which is a finding, not a flake.
#[test]
fn the_fixpoint_and_the_queues_agree_on_every_golden_world() {
    let (props, ids) = load_props();
    let g = golden();
    let geometry = LightGeometry {
        min_section_y: i32::try_from(g["min_section_y"].as_i64().expect("min_section_y"))
            .expect("i32"),
        sections_count: i32::try_from(g["sections_count"].as_i64().expect("sections_count"))
            .expect("i32"),
    };
    let air = *ids.get("minecraft:air").expect("air");
    let mut compared = 0usize;
    for case in g["cases"].as_array().expect("cases") {
        let name = case["name"].as_str().expect("name");
        let mut chunks = BTreeMap::new();
        for ch in case["chunks"].as_array().expect("chunks") {
            let cx = i32::try_from(ch["x"].as_i64().expect("x")).expect("i32");
            let cz = i32::try_from(ch["z"].as_i64().expect("z")).expect("i32");
            chunks.insert((cx, cz), chunk_from(&ch["world"], ids_ref(&ids), air));
        }
        let region = Region {
            geometry,
            chunks,
            air,
        };
        let fixpoint = rustmc_server::light_engine::light(&region, &props);
        let queues = light_region(&region, &props);
        for &(cx, cz) in region.chunks.keys() {
            let a = classify(geometry, &fixpoint.chunk(cx, cz));
            let b = classify(geometry, &queues.chunk(cx, cz));
            assert_eq!(
                a.sky_mask, b.sky_mask,
                "{name} ({cx},{cz}) sky_mask: fixpoint vs queues"
            );
            assert_eq!(
                a.block_mask, b.block_mask,
                "{name} ({cx},{cz}) block_mask: fixpoint vs queues"
            );
            assert_eq!(
                a.empty_sky_mask, b.empty_sky_mask,
                "{name} ({cx},{cz}) empty_sky_mask: fixpoint vs queues"
            );
            assert_eq!(
                a.empty_block_mask, b.empty_block_mask,
                "{name} ({cx},{cz}) empty_block_mask: fixpoint vs queues"
            );
            assert_eq!(a.sky, b.sky, "{name} ({cx},{cz}) sky arrays");
            assert_eq!(a.block, b.block, "{name} ({cx},{cz}) block arrays");
            compared += 1;
        }
    }
    assert!(compared > 0, "compared nothing");
    println!("fixpoint vs queues: {compared} chunks identical");
}

const fn ids_ref(m: &BTreeMap<String, u16>) -> &BTreeMap<String, u16> {
    m
}
