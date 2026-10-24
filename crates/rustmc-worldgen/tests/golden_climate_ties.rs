//! Are the biome-search ties real, and how many are there?
//!
//! `Climate.RTree.search` compares with a strict `>`, so a box that is
//! *exactly* as close as the running candidate never displaces it. The
//! candidate is seeded from the previous query's result (vanilla's
//! `ThreadLocal lastResult`), which makes the answer at a tied point a
//! function of query order rather than of position.
//!
//! All of that is faithful to vanilla and already covered by
//! `golden_climate.rs`. What is *not* obvious, and what this file measures,
//! is how often it bites. The r7 whole-world diff disagreed with vanilla
//! about exactly four biome columns out of 2,704, and before writing any
//! code to chase them it is worth knowing whether they are ties — because a
//! tie is not a defect we can fix. Vanilla generates chunks on several
//! worker threads and each carries its own `lastResult`, so at a tied point
//! *vanilla does not agree with itself between runs*.
//!
//! Reference (26.2 decompile, behaviour ported — never transcribed,
//! CLAUDE.md rule 4): `Climate.RTree.SubTree#search`, `Climate.RTree.Leaf`.

use rustmc_worldgen::biome::climate::{Parameter, ParameterList, ParameterPoint, TargetPoint};
use rustmc_worldgen::biome::multi_noise::ClimateSampler;
use rustmc_worldgen::density::router::{OverworldNoises, OverworldRouter};
use serde_json::Value;

const CLIMATE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/climate_vectors.json");
const ROUTER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/router_vectors.json");

fn read(path: &str) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("golden vector {path} unreadable: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path} is not JSON: {e}"))
}

fn long(v: &Value) -> i64 {
    v.as_str()
        .unwrap_or_else(|| panic!("expected a long-as-string, got {v}"))
        .parse()
        .unwrap_or_else(|e| panic!("bad i64 {v}: {e}"))
}

fn table() -> Vec<(ParameterPoint, String)> {
    let doc = read(CLIMATE);
    let case = doc["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|c| c["case"] == "overworld_climate")
        .expect("no overworld_climate case")
        .clone();
    case["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|entry| {
            let s = entry["space"].as_array().expect("space");
            let axis = |i: usize| Parameter::new(long(&s[i * 2]), long(&s[i * 2 + 1]));
            (
                ParameterPoint {
                    temperature: axis(0),
                    humidity: axis(1),
                    continentalness: axis(2),
                    erosion: axis(3),
                    depth: axis(4),
                    weirdness: axis(5),
                    offset: long(&s[12]),
                },
                entry["biome"].as_str().expect("biome").to_string(),
            )
        })
        .collect()
}

fn noises() -> OverworldNoises {
    let doc = read(ROUTER);
    let mut set = OverworldNoises::new();
    for entry in doc["parameters"].as_array().expect("parameters") {
        let amplitudes: Vec<f64> = entry["amplitudes"]
            .as_array()
            .expect("amplitudes")
            .iter()
            .map(|v| f64::from_bits(long(v) as u64))
            .collect();
        set = set.with(
            entry["name"].as_str().expect("name"),
            rustmc_worldgen::noise::NoiseParameters::new(
                entry["firstOctave"].as_i64().expect("firstOctave") as i32,
                amplitudes,
            ),
        );
    }
    set
}

/// Every box whose distance equals the minimum, by brute force. The RTree is
/// an index over exactly this set, so one winner means the search has no
/// freedom and two means the answer depends on which was seen first.
fn tied_at(list: &ParameterList<String>, target: TargetPoint) -> Vec<String> {
    let array = target.to_parameter_array();
    let mut best = i64::MAX;
    let mut winners: Vec<String> = Vec::new();
    for (point, name) in list.values() {
        let space = point.parameter_space();
        let mut total: i64 = 0;
        for (i, axis) in space.iter().enumerate() {
            let d = axis.distance(array[i]);
            total += d * d;
        }
        if total < best {
            best = total;
            winners.clear();
            winners.push(name.clone());
        } else if total == best {
            winners.push(name.clone());
        }
    }
    winners
}

/// The four columns the r7 whole-world diff disagreed about, as block
/// coordinates with a y inside the disputed range. Recorded from the diff so
/// that a later change which fixes or breaks them says so out loud.
const DISPUTED: [(i32, i32, i32); 4] = [
    (1012, 40, 1096),
    (1056, 40, 1108),
    (1100, 40, 972),
    (1104, 40, 972),
];

#[test]
fn the_disputed_columns_are_exact_ties() {
    let list = ParameterList::new(table());
    let router = OverworldRouter::new(20_260_722, &noises(), false);
    let sampler = ClimateSampler::from_router(&router);
    let mut untied = Vec::new();
    for (x, y, z) in DISPUTED {
        let target = sampler.sample(x >> 2, y >> 2, z >> 2);
        let winners = tied_at(&list, target);
        println!(
            "({x},{y},{z}) -> {} candidate(s): {winners:?}",
            winners.len()
        );
        if winners.len() < 2 {
            untied.push((x, y, z, winners));
        }
    }
    assert!(
        untied.is_empty(),
        "these r7 disagreements have a single nearest box, so they are NOT \
         order-dependent ties and need a different explanation: {untied:?}"
    );
}

/// How common are ties across a real slab of world? This decides whether the
/// order-dependence is a corner case or a wall.
#[test]
fn ties_are_rare_but_not_absent() {
    let list = ParameterList::new(table());
    let router = OverworldRouter::new(20_260_722, &noises(), false);
    let sampler = ClimateSampler::from_router(&router);
    let mut total = 0usize;
    let mut tied = 0usize;
    for qx in 253..285 {
        for qz in 235..267 {
            for qy in (-16..80).step_by(8) {
                total += 1;
                if tied_at(&list, sampler.sample(qx, qy, qz)).len() > 1 {
                    tied += 1;
                }
            }
        }
    }
    let rate = tied as f64 / total as f64;
    println!("climate ties: {tied}/{total} = {:.6}%", rate * 100.0);
    assert!(
        tied > 0,
        "no ties in {total} samples — then the r7 biome disagreements have \
         another cause entirely and the tie explanation is wrong"
    );
    assert!(
        rate < 0.02,
        "ties in {:.4}% of samples — too common to write off as a corner \
         case; the biome fill would have to reproduce vanilla's query order",
        rate * 100.0
    );
}

/// Do the **carver** biome queries land on ties?
///
/// `applyCarvers` resolves a carver list per source chunk from the biome at
/// `QuartPos.fromBlock(sourceMinBlockX)`, quart y 0 — one climate query per
/// source chunk, and a tie there swaps the whole list rather than one block.
/// That makes it a candidate for the r7 carver rows, which are 1,589 blocks
/// in 717 mostly-tiny groups while the carver gate itself is exact over
/// 2.5M blocks.
///
/// This is the measurement that decides whether the candidate is real. The
/// carved area is r7 around chunk (64, 64) and carvers sweep 8 chunks, so the
/// source chunks run from 49 to 79 in both axes.
#[test]
fn how_many_carver_source_chunks_land_on_a_tie() {
    // Both seeds. The first measurement here only covered 20260722 and
    // reported zero, which was read as "the carver lists are safe" — but a
    // tie is a property of the *climate at a position*, and a different seed
    // samples different positions entirely.
    for seed in [20_260_722_i64, 31_415_926_i64] {
        how_many_ties_at(seed);
    }
}

fn how_many_ties_at(seed: i64) {
    let list = ParameterList::new(table());
    let router = OverworldRouter::new(seed, &noises(), false);
    let sampler = ClimateSampler::from_router(&router);

    let mut total = 0usize;
    let mut tied = Vec::new();
    for source_x in 49..=79 {
        for source_z in 49..=79 {
            total += 1;
            let target = sampler.sample(source_x * 4, 0, source_z * 4);
            let winners = tied_at(&list, target);
            if winners.len() > 1 {
                tied.push((source_x, source_z, winners));
            }
        }
    }
    println!(
        "seed {seed}: carver source chunks on a climate tie: {}/{total}",
        tied.len()
    );
    for (x, z, winners) in tied.iter().take(6) {
        println!("  chunk ({x}, {z}): {winners:?}");
    }
    // No assertion on the count: this exists to answer the question, and
    // either answer is informative. Zero would kill the hypothesis outright.
    assert!(total > 0);
}
