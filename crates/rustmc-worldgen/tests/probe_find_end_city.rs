//! Where the nearest `end_city` is, and why the r=16 End reference holds none.
//!
//! `oracle-work/van-end-20260722-r16` is 1,089 chunks of real vanilla End —
//! chunks 48..80 in both axes, seed 20260722 — and a `region-differ census`
//! over it finds **no purpur of any kind**, while finding 12,513
//! `chorus_plant`. That was written down as "should not be possible if end
//! cities were generating", on the arithmetic that spacing 20 / separation 11
//! puts 2-3 cities in a 33x33-chunk square.
//!
//! That arithmetic is for a **linear** spread. `end_cities.json` is
//! `spread_type: triangular`, which is two `nextInt(9)` averaged, so the
//! candidate chunk sits near the *middle* of its 20-chunk region and the
//! window's partial regions almost never contribute one. This probe measures
//! the count instead of estimating it.
//!
//! # What this claims and does not claim
//!
//! It claims each reported chunk is a **structure chunk** for
//! `minecraft:end_cities` — `random_spread` plus the frequency coin, pure
//! arithmetic, already gated against the JVM by `golden_structure_placement`.
//!
//! It reports the biome of the chunk (the End's classifier is y-independent
//! and constant across a chunk, see `biome::end`) and whether that biome is in
//! `#minecraft:has_structure/end_city`. That is two of vanilla's three gates.
//! The third is `EndCityStructure.findGenerationPoint`, which needs terrain:
//! it takes the minimum `WORLD_SURFACE_WG` height over the start piece's
//! footprint and rejects below y=60. A chunk this probe likes can still be
//! rejected there, so the output is a **shortlist and a count**, and a vanilla
//! run is the arbiter — same contract as `probe_find_village`.
//!
//! # The arbiter has now run, and the shortlist's first entry was a city
//!
//! 2026-07-30: `oracle-work/van-end-20260722-c8362` is a real vanilla End
//! harvest, seed 20260722, r=7 around this probe's **nearest** candidate,
//! chunk (83,62). Its census over chunks (76,55)..(90,69) returns 3,183
//! `purpur_block`, 1,226 `purpur_stairs`, 1,164 `purpur_pillar`, 918
//! `end_stone_bricks`, 202 `magenta_stained_glass`, 198 `purpur_slab`, 114
//! `end_rod`, 16 `magenta_wall_banner`, 9 `ladder`, 3 `chest` and 1
//! `ender_chest` — **7,034 end city blocks**. So the first shortlist entry
//! cleared `findGenerationPoint` too, and this probe is a working targeting
//! instrument rather than an untested one. Nothing is yet known about whether
//! the other three entries clear the height gate; each would cost one more
//! vanilla run.
//!
//! ```text
//!   cargo test --release -p rustmc-worldgen --test probe_find_end_city -- --nocapture
//! ```

mod common;

use common::facts::resolve_biome_tag;
use common::{data_path, have_extract, read};
use rustmc_worldgen::biome::end::biome_at_quart;
use rustmc_worldgen::density::router::OverworldRouter;
use rustmc_worldgen::structure::placement::StructurePlacement;

const SEED: i64 = 20_260_722;
/// The r=16 reference's extent, inclusive, in chunks.
const WINDOW: (i32, i32) = (48, 80);

#[test]
fn where_an_end_city_is_and_why_the_r16_window_has_none() {
    if !have_extract() {
        return;
    }
    let set = read(
        data_path("worldgen/structure_set/end_cities.json")
            .to_str()
            .expect("utf-8 path"),
    );
    let placement = StructurePlacement::parse(&set["placement"]).expect("end_cities placement");
    let structure = read(
        data_path("worldgen/structure/end_city.json")
            .to_str()
            .expect("utf-8 path"),
    );
    let tag = structure["biomes"]
        .as_str()
        .expect("biome tag")
        .trim_start_matches('#')
        .trim_start_matches("minecraft:");
    let allowed = resolve_biome_tag(tag).expect("resolve has_structure/end_city");
    println!("end_city biomes: {allowed:?}");
    println!("placement: {:?}", placement.spread);

    let router = OverworldRouter::end(SEED);

    // Every structure chunk out to chunk radius 96 — 193x193 = 37,249 chunks,
    // 88 regions of the 20-chunk grid.
    let mut candidates: Vec<(i32, i32, &'static str, bool)> = Vec::new();
    for cx in -96..=96 {
        for cz in -96..=96 {
            if !placement
                .is_structure_chunk_ignoring_exclusion(SEED, cx, cz)
                .expect("a random spread")
            {
                continue;
            }
            let biome = biome_at_quart(&router.erosion, cx * 4 + 2, 16, cz * 4 + 2);
            let ok = allowed.contains(biome);
            candidates.push((cx, cz, biome, ok));
        }
    }

    let in_window: Vec<_> = candidates
        .iter()
        .filter(|(cx, cz, _, _)| {
            (WINDOW.0..=WINDOW.1).contains(cx) && (WINDOW.0..=WINDOW.1).contains(cz)
        })
        .collect();
    println!(
        "\nstructure chunks inside the r=16 window ({}..{})^2: {}",
        WINDOW.0,
        WINDOW.1,
        in_window.len()
    );
    for (cx, cz, biome, ok) in &in_window {
        println!("  ({cx},{cz}) {biome} allowed={ok}");
        // `EndCityStructure` offsets its start piece by up to 7 blocks before
        // taking a height, so the position whose biome vanilla tests can sit
        // in a neighbouring chunk. Print the 3x3 neighbourhood: if none of the
        // nine is an allowed biome, the gate rejects whichever cell vanilla
        // samples, and the exact offset stops mattering.
        for dz in -1..=1 {
            let row: Vec<String> = (-1..=1)
                .map(|dx| {
                    let b =
                        biome_at_quart(&router.erosion, (cx + dx) * 4 + 2, 16, (cz + dz) * 4 + 2);
                    format!(
                        "{}{}",
                        b.trim_start_matches("minecraft:"),
                        if allowed.contains(b) { "*ALLOWED*" } else { "" }
                    )
                })
                .collect();
            println!("      z{:+}: {}", dz, row.join("  "));
        }
    }

    // A window's biome histogram. This prices the *other* feature that
    // appears in no reference window: `end_gateway_return` is
    // `rarity_filter { chance: 700 }` and lives in `end_highlands` only, so
    // the number of gateways a harvest could have contained is a function of
    // how many of its chunks are highlands. Run over both windows that have
    // actually been harvested, because a pricing quoted for the wrong extent
    // is how the "2-3 cities are owed" estimate went wrong in the first place.
    let price = |label: &str, x0: i32, z0: i32, x1: i32, z1: i32| {
        let mut hist: std::collections::BTreeMap<&str, u32> = std::collections::BTreeMap::new();
        for cx in x0..=x1 {
            for cz in z0..=z1 {
                *hist
                    .entry(biome_at_quart(&router.erosion, cx * 4 + 2, 16, cz * 4 + 2))
                    .or_default() += 1;
            }
        }
        let total: u32 = hist.values().sum();
        println!("\n{label} biome histogram ({total} chunks):");
        for (biome, n) in &hist {
            println!("  {n:5}  {biome}");
        }
        let highlands = hist
            .get("minecraft:end_highlands")
            .copied()
            .unwrap_or_default();
        let p_none = (699.0_f64 / 700.0).powi(i32::try_from(highlands).expect("i32"));
        println!(
            "end_gateway_return: {highlands} end_highlands chunks x 1/700 = {:.3} expected, P(zero) = {:.1}%",
            f64::from(highlands) / 700.0,
            p_none * 100.0
        );
    };
    price("r=16 window", WINDOW.0, WINDOW.0, WINDOW.1, WINDOW.1);
    // The harvested end city window: r=7 around chunk (83,62).
    price("city window (83,62) r=7", 76, 55, 90, 69);

    // Nearest biome-passing candidate to the window's centre, so a vanilla
    // harvest can be aimed at one.
    let mut passing: Vec<_> = candidates.iter().filter(|c| c.3).collect();
    passing.sort_by_key(|(cx, cz, _, _)| {
        i64::from(cx - 64) * i64::from(cx - 64) + i64::from(cz - 64) * i64::from(cz - 64)
    });
    println!(
        "\nbiome-passing structure chunks: {} of {} candidates in radius 96",
        passing.len(),
        candidates.len()
    );
    println!("nearest 12 to chunk (64,64), the r=16 centre:");
    for (cx, cz, biome, _) in passing.iter().take(12) {
        println!(
            "  chunk ({cx},{cz}) block ({},{}) {biome}",
            cx * 16,
            cz * 16
        );
    }
    let mut by_origin = passing.clone();
    by_origin.sort_by_key(|(cx, cz, _, _)| {
        i64::from(*cx) * i64::from(*cx) + i64::from(*cz) * i64::from(*cz)
    });
    println!("nearest 6 to the origin:");
    for (cx, cz, biome, _) in by_origin.iter().take(6) {
        println!(
            "  chunk ({cx},{cz}) block ({},{}) {biome}",
            cx * 16,
            cz * 16
        );
    }
}
