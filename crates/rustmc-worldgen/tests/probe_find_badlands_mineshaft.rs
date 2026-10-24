//! Find a chunk where the mesa mineshaft would actually be chosen.
//!
//! The defect is bounded: `has_structure/mineshaft` and
//! `has_structure/mineshaft_mesa` are disjoint (51 biomes against 3), so this
//! port's unconditional `mineshaft` is correct everywhere except the badlands,
//! where vanilla builds `mineshaft_mesa` instead.
//!
//! Fixing it is easy. *Gating* the fix is the problem: no saved vanilla world
//! here contains badlands, so a mesa mineshaft would be implemented against
//! nothing — which is precisely the trade refused for `settle_height`.
//!
//! So this looks for the target first. What is needed is a chunk that is both
//! a **mineshaft placement chunk** and inside `#minecraft:is_badlands`, close
//! enough to the origin that a vanilla server can be asked to generate it
//! without generating half a world.
//!
//! The output is coordinates for `ci/gen-vanilla-region.sh`, not an assertion.

use rustmc_worldgen::structure::placement::StructurePlacement;

const BADLANDS: [&str; 3] = [
    "minecraft:badlands",
    "minecraft:eroded_badlands",
    "minecraft:wooded_badlands",
];

#[test]
fn where_a_mesa_mineshaft_would_be_chosen() {
    let path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .join("oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/structure_set/mineshafts.json");
    if !rustmc_data::fixture::available(&path, "datagen extract", "ci/vanilla-server.sh data") {
        return;
    }
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("json");
    let placement = StructurePlacement::parse(&doc["placement"]).expect("mineshafts placement");

    // Placement first: it is pure arithmetic and rejects most chunks, so the
    // expensive biome sample only runs on the survivors.
    let seeds: [i64; 2] = [31_415_926, 20_260_722];
    for seed in seeds {
        let mut source = biome_source(seed);
        let mut placements = 0;
        let mut found: Vec<(i32, i32, String)> = Vec::new();
        for cz in -96..=96 {
            for cx in -96..=96 {
                if !placement
                    .is_structure_chunk_ignoring_exclusion(seed, cx, cz)
                    .unwrap_or(false)
                {
                    continue;
                }
                placements += 1;
                // `isValidBiome` samples at the stub position in quart
                // coordinates. A mineshaft's stub is built from
                // `BlockPos(chunkPos.getMiddleBlockX(), 50, chunkPos.getMinBlockZ())`.
                let (bx, by, bz) = (cx * 16 + 8, 50, cz * 16);
                let biome = source.get_noise_biome(bx >> 2, by >> 2, bz >> 2).clone();
                if BADLANDS.contains(&biome.as_str()) {
                    found.push((cx, cz, biome));
                }
            }
        }
        println!(
            "seed {seed}: {placements} mineshaft placement chunks in +-96, \
             {} of them badlands",
            found.len()
        );
        for (cx, cz, biome) in found.iter().take(8) {
            let d = cx.abs().max(cz.abs());
            println!("  chunk ({cx},{cz}) {biome}  chebyshev {d} from origin");
        }
    }
}

/// The biome table and the noise parameters, both from the golden vectors the
/// climate gate already uses — so this samples through exactly the wiring that
/// is gated bit-exact against vanilla, not a second implementation of it.
fn biome_source(seed: i64) -> rustmc_worldgen::biome::multi_noise::MultiNoiseBiomeSource<String> {
    use rustmc_worldgen::biome::multi_noise::{ClimateSampler, MultiNoiseBiomeSource};
    use rustmc_worldgen::density::router::{OverworldNoises, OverworldRouter};
    use rustmc_worldgen::noise::NoiseParameters;

    let doc: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/golden/router_vectors.json"
        ))
        .expect("router vectors"),
    )
    .expect("json");

    let mut noises = OverworldNoises::new();
    for entry in doc["parameters"].as_array().expect("parameters") {
        let amplitudes: Vec<f64> = entry["amplitudes"]
            .as_array()
            .expect("amplitudes")
            .iter()
            .map(|v| f64::from_bits(v.as_str().expect("bits").parse::<i64>().expect("i64") as u64))
            .collect();
        noises = noises.with(
            entry["name"].as_str().expect("name"),
            NoiseParameters::new(
                entry["firstOctave"].as_i64().expect("firstOctave") as i32,
                amplitudes,
            ),
        );
    }

    let router = OverworldRouter::new(seed, &noises, false);
    MultiNoiseBiomeSource::new(ClimateSampler::from_router(&router), biome_table())
}

/// The overworld parameter table, from the climate gate's own golden vectors.
fn biome_table() -> rustmc_worldgen::biome::climate::ParameterList<String> {
    use rustmc_worldgen::biome::climate::{Parameter, ParameterList, ParameterPoint};

    let doc: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/golden/climate_vectors.json"
        ))
        .expect("climate vectors"),
    )
    .expect("json");

    let long = |v: &serde_json::Value| -> i64 {
        v.as_str()
            .unwrap_or_else(|| panic!("expected a long-as-string, got {v}"))
            .parse()
            .expect("i64")
    };

    let case = doc["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|c| c["case"] == "overworld_climate")
        .expect("no overworld_climate case");

    let entries = case["entries"]
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
        .collect();
    ParameterList::new(entries)
}

/// Ocean-ruin placement chunks near the origin, for the `settle_height` hunt.
///
/// `getHeight` only lowers a piece when the ground under most of its footprint
/// is more than two blocks below where the heightmap put it. At the one ruin
/// gated against a server it is a no-op, so the column walk is still untested
/// against terrain. Finding a ruin on uneven ground is the same shape of
/// problem as finding a badlands mineshaft, and the same method applies:
/// enumerate the cheap arithmetic first, and only then pay for terrain.
///
/// This half is the enumeration. The output is a candidate list.
#[test]
fn ocean_ruin_placement_chunks_near_the_origin() {
    let path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .join("oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/structure_set/ocean_ruins.json");
    if !rustmc_data::fixture::available(&path, "datagen extract", "ci/vanilla-server.sh data") {
        return;
    }
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("json");
    let placement = StructurePlacement::parse(&doc["placement"]).expect("ocean_ruins placement");

    const SEED: i64 = 31_415_926;
    let mut source = biome_source(SEED);
    let warm = [
        "minecraft:lukewarm_ocean",
        "minecraft:warm_ocean",
        "minecraft:deep_lukewarm_ocean",
    ];

    let mut hits = Vec::new();
    for cz in -160..=160 {
        for cx in -160..=160 {
            if !placement.is_placement_chunk(SEED, cx, cz).unwrap_or(false) {
                continue;
            }
            // The ruin's own origin column, in quart coordinates. y 48 is
            // where the gated ruin sits; the biome is near enough constant in
            // y through the water column that this only needs to be close.
            let biome = source
                .get_noise_biome((cx * 16) >> 2, 48 >> 2, (cz * 16) >> 2)
                .clone();
            let d = cx.abs().max(cz.abs());
            hits.push((d, cx, cz, biome));
        }
    }
    hits.sort();
    let total = hits.len();
    hits.retain(|(_, _, _, b)| warm.contains(&b.as_str()));
    println!(
        "{total} ocean-ruin placement chunks within +-160 at seed {SEED}; \
         {} of them warm",
        hits.len()
    );
    for (d, cx, cz, biome) in hits.iter().take(10) {
        println!("  WARM ({cx},{cz}) chebyshev {d}  {biome}");
    }
}
