//! worldgen-dump — generate noise-stage chunks, to disk or to a stopwatch.
//!
//! Two jobs, and the second is the one that matters longer term:
//!
//! - `region` writes real Anvil region files, so a generated world can be
//!   opened and looked at rather than only asserted about.
//! - `bench` times chunk generation. Chunk generation is the project's
//!   declared P1 target (PRODUCT.md) — Minecom measured a 7.5-8.5x deficit
//!   against vanilla here, and a number that is never taken cannot be
//!   improved.
//!
//! # What is being timed, precisely
//!
//! The **noise stage only**: density, aquifer, ore veins, block fill. No
//! surfaces, carvers, features or structures, and no disk IO in `bench`.
//! Comparing this against vanilla's full chunk generation would be
//! meaningless in our favour, so don't. It is a baseline to move, and the
//! honest comparison arrives when the stages do.

use rustmc_worldgen::anvil_bridge::to_chunk_column;
use rustmc_worldgen::chunk_gen::{FillInputs, FillSettings, NoiseFill};
use rustmc_worldgen::density::aquifer::AquiferNoises;
use rustmc_worldgen::density::ore_vein::OreVeinifier;
use rustmc_worldgen::density::router::{OverworldNoises, OverworldRouter};
use rustmc_worldgen::noise::{NoiseParameters, NoiseSeeding};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

const USAGE: &str = "\
worldgen-dump — noise-stage chunk generation

  worldgen-dump bench <seed> <chunks>
      Generate <chunks> chunks and report the timing. No disk IO.

  worldgen-dump scale <seed> <chunks> [max-threads]
      Generate <chunks> chunks at 1, 2, 4, ... threads and report how
      throughput scales.

  worldgen-dump region <seed> <radius> <out-dir>
      Generate the (2r+1)^2 chunks around the origin and write region files.
      NOISE STAGE ONLY — no surfaces, carvers or features.

  worldgen-dump column <seed> <x> <z> <y-lo> [y-hi] [center-x] [center-z]
      Print one block column stage by stage, with the noise-stage
      heightmap fossils. Answers which stage made a block what it is —
      IN THE WORLD IT GENERATES, which is a radius-2 square centred on
      the column's own chunk unless you say otherwise. Decoration writes
      one chunk out, so a different centre is a different world; to
      explain a block in a `full` dump, pass that dump's centre.

  worldgen-dump full <seed> <radius> <out-dir> [center-x] [center-z]
      Generate the same square through EVERY stage and write region files.
      This is the one to compare against vanilla; `region` is not, and
      comparing it would flatter this generator enormously.

  worldgen-dump full-nether <seed> <radius> <out-dir> [center-x] [center-z]
      The same, in the NETHER. A separate subcommand rather than a flag on
      `full` because the two share no settings: different noise router,
      different seeding algorithm, different height, different default
      block and fluid. Every stage except structures; the printed caveats
      say what that leaves out. `RUSTMC_THROUGH` stops earlier.

  worldgen-dump full-end <seed> <radius> <out-dir> [center-x] [center-z]
      The same, in the END. Noise, biomes, the surface rule and decoration —
      there are no carvers in the End at all, and the End's whole feature
      list is five placed features. The one structure set — end cities —
      IS built as of the end_city walk, and measures 100.000000% over the
      one harvested window containing one. What is NOT run is
      `end_gateway_return`; the printed caveats say so and are the
      authority. `RUSTMC_THROUGH=noise|biomes|surface` stops
      earlier; `carvers` lands on the decoration stage because there is no
      carver stage to stop at.

  worldgen-dump mineshaft-ceiling <seed> <vanilla-region-dir> [blocks-dir]
      Recover, from vanilla's own blocks, the stream offset each corridor
      entered its ceiling pass at. The draw counter this project lacked.

  worldgen-dump nether-fossil-starts <seed> <radius> [center-x] [center-z]
      Print OUR nether_fossil starts in exactly the format
      `jigsaw-starts <ref> nether_fossil` prints VANILLA's, so the two can
      be diffed line for line. Chunk, template, rotation and template
      position all come out of vanilla's own saved `Children[]` on that
      side, which makes this structure's placement one of the few with a
      complete oracle.

  worldgen-dump fortress-starts <seed> <radius> [center-x] [center-z]
      The placement chunks where the nether_complexes set places a fortress
      rather than a bastion, inside the square. Layout only.
  worldgen-dump bastion-starts <seed> <radius> [center-x] [center-z]
      OUR bastion_remnant starts in exactly the format
      `jigsaw-starts <ref> bastion_remnant` prints VANILLA's, so the two
      diff line for line. Element, rotation, template position, ground
      level delta, projection, bounding box and junction count all come
      from vanilla's own saved `Children[]` on that side.
      Layout only: it runs no terrain, so it separates a wrong
      assembly from a right assembly with wrong blocks in it.

  worldgen-dump ruined-portal-starts <seed> <radius> [center-x] [center-z]
      The same, for ruined portals: OUR starts in the format
      `jigsaw-starts <ref> ruined_portal` prints VANILLA's saved ones.
      The field it exists for is `TP` — vanilla saves the CHUNK ORIGIN
      with `findSuitableY`'s y, not the box centre, and a port that
      takes the centre puts every block of the portal 4 east and 12
      south while its template, rotation, mirror and placement all
      still gate green.

  worldgen-dump ruined-portal-apron <seed> <vanilla-region-dir> [blocks-dir]
      Recover, from vanilla's own netherrack, the draw index it entered
      `spreadNetherrack` at. The apron's radius comes from one
      `nextInt(max(1, 8 - avgSpan/2))`, so a stream one draw out of step
      spreads netherrack where vanilla has none and omits it where vanilla
      has some — which is what a wrong RULE reads like too. This is the
      only instrument that separates them. Point `blocks-dir` at our own
      region to validate it against a stream we already know.

  worldgen-dump jigsaw-starts <vanilla-region-dir> [id-substring]
      Print the piece list vanilla saved into its own chunks, in the
      order the generator produced them. For a jigsaw structure that is
      element, rotation, position, ground level delta and bounding box;
      for a plain `TemplateStructurePiece` walk — the End city — it is
      template, `Rot`, `TPX/TPY/TPZ`, `GD` and the box. This is the ONLY
      oracle either family has — `DecorateGolden` runs features, not
      structures — and it pins the element draw, the rotation draw and
      the position separately, which a block diff cannot.

  worldgen-dump village-oracle <seed> <vanilla-region-dir> [radius] [cx] [cz]
      The same harness as `trial-chamber-oracle`, for all five village
      variants. Note the height source: this runs `village_report`, which
      asks `getFirstFreeHeight` — a pure beardless noise column, which is
      what vanilla asks at STRUCTURE_STARTS — whereas `village-pieces`
      below reads the CANVAS heightmap of an already-generated square.
      Those two disagree, and only the first one is vanilla.

  worldgen-dump trial-chamber-oracle <seed> <vanilla-region-dir> [radius] [cx] [cz]
      Gate OUR trial chamber walk against vanilla's own saved `Children[]`,
      piece by piece: element, rotation, position, ground level delta and
      bounding box. No JVM and no generated world — vanilla's answer is
      already in the region files. The denominator is VANILLA's start count,
      so a port that generates nothing scores 0 of 3 rather than 0 of 0, and
      a reference holding no chamber exits non-zero as VACUOUS rather than
      reading as a pass.

  worldgen-dump mineshaft-refs <seed> <vanilla-region-dir>
      Compare our mineshaft reference set — membership AND iteration
      order — against the `structures.References` vanilla saved into
      every chunk. No JVM, no generation: it reads the region files.
      Also gates each room's `Entrances` boxes, which the room hollows
      out without drawing, so no draw-count check can see them.

  worldgen-dump mineshaft-mesa <seed> <vanilla-region-dir>
      Gate the mesa mineshaft against the starts vanilla saved, and
      recover the `getBaseHeight` each one was placed against from its
      own piece boxes. No JVM, no generation.

Noise parameters are read from the committed golden vectors, because
rustmc-data cannot load the extracted registry yet. Set RUSTMC_NOISE_JSON
to point elsewhere.
";

/// `mineshaft-mesa <seed> <vanilla-region-dir>` — gate the mesa mineshaft
/// against the starts vanilla saved, without a JVM and without a canvas.
///
/// A mesa mine differs from a normal one in exactly two ways, both read from
/// the bytecode: three block states, and the vertical rule in
/// `MineshaftStructure#generatePiecesAndAdjust`, which is
///
/// ```java
///   int surface = chunkGenerator.getBaseHeight(c.getX(), c.getZ(), WORLD_SURFACE_WG, ...);
///   int target  = surface <= seaLevel ? seaLevel
///                                     : Mth.randomBetweenInclusive(random, seaLevel, surface);
///   builder.offsetPiecesVertically(target - c.getY());
/// ```
///
/// where `c` is the union box's `getCenter()`. The layout above it is
/// **identical** to a normal mine's — same seed, same draws, same order — so
/// the only unknown is `surface`, and a saved start pins it: vanilla's room
/// box gives `dy = room.minY - 50`, and only some surface values produce that
/// `dy` through the draw above. Sweeping the candidates and keeping the ones
/// that reproduce the whole piece list recovers `getBaseHeight` from
/// vanilla's own output — the same trick `mineshaft-ceiling` plays on the
/// ceiling row.
///
/// That matters because our side intends to answer `getBaseHeight` from the
/// canvas's `WORLD_SURFACE_WG`, and those are only equal if vanilla's
/// standalone noise column agrees with our generated chunk. This measures it
/// instead of assuming it.
fn mineshaft_mesa(seed: &str, vanilla_dir: &Path) -> Result<(), String> {
    use rustmc_worldgen::rng::legacy::LegacyRandomSource;
    use rustmc_worldgen::rng::worldgen_random::WorldgenRandom;
    use rustmc_worldgen::structure::mineshaft::{generate_with_kinds_using, MineshaftType};

    let seed: i64 = seed.parse().map_err(|e| format!("bad seed: {e}"))?;
    let chunks = load_full(vanilla_dir)?;

    // One entry per start, keyed by the chunk it started in. A start is
    // serialised into every chunk it touches, so the same mine arrives many
    // times and the copies must agree.
    let mut saved: BTreeMap<(i32, i32), Vec<rustmc_world::anvil::StructurePiece>> = BTreeMap::new();
    for chunk in chunks.values() {
        for start in &chunk.structure_starts {
            if start.id != "minecraft:mineshaft_mesa" {
                continue;
            }
            saved
                .entry((start.chunk_x, start.chunk_z))
                .or_insert_with(|| start.pieces.clone());
        }
    }
    println!(
        "vanilla mesa mineshafts in {}: {}",
        vanilla_dir.display(),
        saved.len()
    );
    if saved.is_empty() {
        // Say what *is* there. "No mesa mineshafts" and "this tool is looking
        // under the wrong key" produce the same empty result, and only one of
        // them is a finding.
        let mut ids: BTreeMap<&str, u32> = BTreeMap::new();
        for chunk in chunks.values() {
            for start in &chunk.structure_starts {
                *ids.entry(start.id.as_str()).or_insert(0) += 1;
            }
        }
        println!("  structure starts saved in this world, by id:");
        for (id, count) in &ids {
            println!("    {count:>5}x {id}");
        }
        // And where the non-mineshaft ones are, because a cluster of blocks
        // this project does not generate has to be attributed to something
        // before it can be called a mineshaft fault.
        for chunk in chunks.values() {
            for start in &chunk.structure_starts {
                if start.id.starts_with("minecraft:mineshaft") || start.pieces.is_empty() {
                    continue;
                }
                let mut b = start.pieces[0].bounding_box;
                for p in &start.pieces {
                    b = [
                        b[0].min(p.bounding_box[0]),
                        b[1].min(p.bounding_box[1]),
                        b[2].min(p.bounding_box[2]),
                        b[3].max(p.bounding_box[3]),
                        b[4].max(p.bounding_box[4]),
                        b[5].max(p.bounding_box[5]),
                    ];
                }
                println!(
                    "    {} start ({}, {}) {} pieces, union {},{},{}..{},{},{}",
                    start.id,
                    start.chunk_x,
                    start.chunk_z,
                    start.pieces.len(),
                    b[0],
                    b[1],
                    b[2],
                    b[3],
                    b[4],
                    b[5]
                );
            }
        }
        println!("MESA nothing to compare");
        return Ok(());
    }

    // `getBaseHeight` cannot exceed the world height, and cannot be below the
    // sea level and still take a draw. Sweeping to 400 is far past any
    // overworld surface and costs nothing.
    const SEA_LEVEL: i32 = 63;
    let (mut exact, mut total) = (0u32, 0u32);
    for ((cx, cz), want) in &saved {
        total += 1;
        // `MST` is vanilla's own record of the type, and it should say mesa
        // on every piece of a mesa start.
        let wrong_type = want.iter().filter(|p| p.mineshaft_type != Some(1)).count();
        let want_dy = want
            .iter()
            .find(|p| p.id == "minecraft:msroom")
            .map(|p| p.bounding_box[1] - 50);

        let mut fits: Vec<i32> = Vec::new();
        for surface in SEA_LEVEL..=400 {
            let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
            random.set_large_feature_seed(seed, *cx, *cz);
            let Ok(layout) = generate_with_kinds_using(
                &mut random,
                *cx,
                *cz,
                MineshaftType::Mesa,
                SEA_LEVEL,
                -64,
                |_, _| Some(surface),
            ) else {
                continue;
            };
            if layout.pieces.len() != want.len() {
                continue;
            }
            let same = layout.pieces.iter().zip(want).all(|(ours, theirs)| {
                let b = ours.bounding_box;
                ours.id == theirs.id
                    && [b.min_x, b.min_y, b.min_z, b.max_x, b.max_y, b.max_z] == theirs.bounding_box
            });
            if same {
                fits.push(surface);
            }
        }
        let verdict = match fits.len() {
            0 => "NO SURFACE FITS".to_string(),
            1 => format!("surface {}", fits[0]),
            n => format!("{n} surfaces fit, {}..{}", fits[0], fits[n - 1]),
        };
        if fits.is_empty() {
            println!(
                "  start ({cx}, {cz}) {} pieces, vanilla dy {want_dy:?} -> {verdict}",
                want.len()
            );
        } else {
            exact += 1;
            println!(
                "  start ({cx}, {cz}) {} pieces, vanilla dy {want_dy:?} -> {verdict}  \
                 (every piece box exact)",
                want.len()
            );
        }
        if wrong_type > 0 {
            println!("    {wrong_type} pieces do not carry MST=1");
        }
    }
    println!("MESA LAYOUT {exact} of {total} starts reproduced exactly");
    if exact == total {
        println!("MESA 100.000000%");
    }
    Ok(())
}

/// `mineshaft-refs <seed> <vanilla-region-dir>` — gate the reference set.
///
/// `StructureManager#startsForStructure` walks a chunk's reference set and
/// hands each start to `placeInChunk` in **iteration order**, sharing one
/// `RandomSource`. So two things decide every block a mineshaft writes into
/// a chunk, and nothing was checking either:
///
/// - which starts are in the set (`createReferences` scans ±8 chunks and
///   keeps those whose *union* box meets this chunk in x/z);
/// - what order the set iterates in (`LongOpenHashSet`, reproduced in
///   [`rustmc_worldgen::fastutil_long_set`]).
///
/// Vanilla saves the set into the chunk NBT as a `LongArray`, and
/// `LongOpenHashSet#toLongArray` fills that array **through the iterator** —
/// so the file records the order, not just the membership. That makes a
/// generated region a complete oracle for both questions at once.
/// Print every jigsaw start a vanilla region directory carries.
///
/// Reads region files only — no generation, no JVM. The output is the
/// assembler's answer sheet: `JigsawPlacement.addPieces` produces exactly this
/// list, in this order, and each line names one element draw, one rotation
/// draw and the position they resolved to.
/// Assemble one jigsaw structure and, given a golden vector, diff it.
///
/// The comparison is by **index** (standing rule 10): two piece lists that
/// disagree are reported at the first index where they differ, with both
/// sides printed, because a list that is one element short everywhere after
/// piece 4 is one wrong decision at piece 4 and not 170 wrong decisions.
///
/// Exits non-zero on a divergence, so this is usable as a gate.
fn village_pieces(
    seed: &str,
    cx: &str,
    cz: &str,
    structure: Option<&str>,
    golden: Option<&str>,
) -> Result<(), String> {
    use rustmc_worldgen::structure::jigsaw;
    use rustmc_worldgen::structure::template_pool::JigsawStructure;
    use worldgen_assembly::jigsaw_library::FileLibrary;

    let seed: i64 = seed.parse().map_err(|e| format!("bad seed: {e}"))?;
    let cx: i32 = cx.parse().map_err(|e| format!("bad chunk x: {e}"))?;
    let cz: i32 = cz.parse().map_err(|e| format!("bad chunk z: {e}"))?;
    let structure = structure.unwrap_or("village_savanna");

    // **No world is generated, and that is a correction rather than a
    // speed-up.** This used to run the pipeline through the carvers and read
    // the canvas's `WORLD_SURFACE_WG`. That is not the height vanilla asks
    // for: `JigsawPlacement` asks `ChunkGenerator.getFirstFreeHeight`, which
    // is `getBaseHeight` and nothing else — a pure beardless noise column
    // that needs no chunk. The two disagree, and at seed 20260722 chunk
    // (46,77) the canvas source builds a **103**-piece village where vanilla
    // builds 98.
    //
    // Reading a generated heightmap also cannot be right in principle: vanilla
    // lays structures out at STRUCTURE_STARTS, before any neighbour chunk
    // exists, so there is no heightmap there to read.
    //
    // It costs nothing to fix — the radius-6 noise pass was almost the whole
    // 40 s runtime — but it is worth being clear that the point is the answer,
    // not the clock.

    let path = format!(
        "{}/worldgen/structure/{structure}.json",
        worldgen_assembly::DATA
    );
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?)
            .map_err(|e| format!("{path}: {e}"))?;
    let jigsaw_structure = JigsawStructure::parse(&json).map_err(|e| format!("{path}: {}", e.0))?;

    let params = worldgen_assembly::noise_parameters();
    let noises = worldgen_assembly::overworld_noises(&params);
    let router = rustmc_worldgen::density::router::OverworldRouter::new(seed, &noises, false);
    let seeding = rustmc_worldgen::noise::registry::NoiseSeeding::new(seed);
    let fill = rustmc_worldgen::chunk_gen::FillSettings::overworld();
    let mut library = FileLibrary::new(|x: i32, z: i32| {
        worldgen_assembly::generate::overworld_first_free_height(&router, &seeding, fill, x, z)
    });

    let mut random = rustmc_worldgen::rng::worldgen_random::WorldgenRandom::new(
        rustmc_worldgen::rng::legacy::LegacyRandomSource::new(0),
    );
    random.set_large_feature_seed(seed, cx, cz);
    // `ConstantHeight.sample` spends no draws, which is why the start height
    // can be resolved before the random is used at all.
    let start_y = match &jigsaw_structure.start_height {
        rustmc_worldgen::structure::template_pool::StartHeight::Constant(anchor) => {
            anchor.resolve(-64, 384)
        }
        rustmc_worldgen::structure::template_pool::StartHeight::Uniform { .. } => {
            return Err("a uniform start height draws, and that draw is not ported".to_string())
        }
    };
    let settings = jigsaw::Settings {
        start_pool: jigsaw_structure.start_pool.clone(),
        start_jigsaw_name: jigsaw_structure.start_jigsaw_name.clone(),
        max_depth: jigsaw_structure.size,
        start_pos: (cx * 16, start_y, cz * 16),
        use_expansion_hack: jigsaw_structure.use_expansion_hack,
        project_start_to_heightmap: jigsaw_structure.project_start_to_heightmap.is_some(),
        max_distance_horizontal: jigsaw_structure.max_distance_from_center,
        max_distance_vertical: jigsaw_structure.max_distance_from_center,
        level_min_y: -64,
        level_max_y: 319,
        padding_bottom: jigsaw_structure.dimension_padding,
        padding_top: jigsaw_structure.dimension_padding,
    };

    let Some(pieces) = jigsaw::add_pieces(&mut random, &mut library, &settings) else {
        return Err(format!("{structure} produced no start at ({cx}, {cz})"));
    };
    if !library.missing.is_empty() {
        println!("MISSING {} entries: {:?}", library.missing.len(), {
            let mut m = library.missing.clone();
            m.sort();
            m.dedup();
            m
        });
    }
    println!("PIECES {}", pieces.len());
    for (i, piece) in pieces.iter().enumerate() {
        println!(
            "  {i:3}  {:<58} {:<20} pos={},{},{} gld={} BB=[{} {} {} .. {} {} {}] junctions={}",
            piece.element.describe(),
            rotation_name(piece.rotation),
            piece.pos.0,
            piece.pos.1,
            piece.pos.2,
            piece.ground_level_delta,
            piece.bbox.min_x,
            piece.bbox.min_y,
            piece.bbox.min_z,
            piece.bbox.max_x,
            piece.bbox.max_y,
            piece.bbox.max_z,
            piece.junctions.len(),
        );
    }

    let Some(golden) = golden else {
        return Ok(());
    };
    let doc: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(golden).map_err(|e| format!("{golden}: {e}"))?,
    )
    .map_err(|e| format!("{golden}: {e}"))?;
    let want = doc["pieces"].as_array().ok_or("the vector has no pieces")?;
    let mut first_divergence = None;
    for i in 0..want.len().max(pieces.len()) {
        // Everything the file records about a piece, not just its name: the
        // bounding box is where the expansion hack shows up, the ground level
        // delta is the three-way height resolution, and the junction count is
        // the shape of the walk. A comparison on the name alone would pass on
        // a village built by a different set of decisions.
        let ours = pieces.get(i).map(|p| {
            (
                p.element.location().unwrap_or("-").to_string(),
                rotation_name(p.rotation).to_string(),
                [p.pos.0, p.pos.1, p.pos.2],
                [
                    p.bbox.min_x,
                    p.bbox.min_y,
                    p.bbox.min_z,
                    p.bbox.max_x,
                    p.bbox.max_y,
                    p.bbox.max_z,
                ],
                p.ground_level_delta,
                p.junctions.len(),
            )
        });
        let theirs = want.get(i).map(|p| {
            let int = |v: &serde_json::Value| v.as_i64().unwrap_or(0) as i32;
            (
                p["element"].as_str().unwrap_or("-").to_string(),
                p["rotation"].as_str().unwrap_or("-").to_string(),
                [int(&p["pos"][0]), int(&p["pos"][1]), int(&p["pos"][2])],
                [
                    int(&p["bb"][0]),
                    int(&p["bb"][1]),
                    int(&p["bb"][2]),
                    int(&p["bb"][3]),
                    int(&p["bb"][4]),
                    int(&p["bb"][5]),
                ],
                int(&p["ground_level_delta"]),
                p["junctions"].as_u64().unwrap_or(0) as usize,
            )
        });
        if ours != theirs {
            first_divergence = Some((i, ours, theirs));
            break;
        }
    }
    match first_divergence {
        None => {
            println!("MATCH {} pieces, index for index", pieces.len());
            Ok(())
        }
        Some((i, ours, theirs)) => {
            println!(
                "DIVERGE at index {i} of {} vanilla / {} ours",
                want.len(),
                pieces.len()
            );
            println!("  vanilla {theirs:?}");
            println!("  ours    {ours:?}");
            Err(format!("piece lists diverge at index {i}"))
        }
    }
}

fn rotation_name(rotation: rustmc_worldgen::structure::template::Rotation) -> &'static str {
    use rustmc_worldgen::structure::template::Rotation;
    match rotation {
        Rotation::None => "NONE",
        Rotation::Clockwise90 => "CLOCKWISE_90",
        Rotation::Clockwise180 => "CLOCKWISE_180",
        Rotation::Counterclockwise90 => "COUNTERCLOCKWISE_90",
    }
}

/// A JSON string, or `null` for a piece that carries no such field.
fn quoted(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_string(), |v| format!("\"{v}\""))
}

/// `scattered-starts <seed> <cx> <cz> <r>` — the pyramids, jungle temples,
/// swamp huts and igloos our placement finds in the square, one line each.
fn scattered_starts_cmd(args: &[String]) -> Result<(), String> {
    let seed: i64 = args[0].parse().map_err(|e| format!("seed: {e}"))?;
    let cx: i32 = args[1].parse().map_err(|e| format!("cx: {e}"))?;
    let cz: i32 = args[2].parse().map_err(|e| format!("cz: {e}"))?;
    let r: i32 = args[3].parse().map_err(|e| format!("r: {e}"))?;
    let area = rustmc_worldgen::pipeline::Area::square(cx, cz, r);
    let (scattered, igloos) = worldgen_assembly::generate::scattered_report(seed, area)?;
    for start in &scattered {
        let state = start.state.lock().expect("lock");
        println!(
            "START {:?} chunk ({}, {}) facing {:?} BB={:?}",
            start.kind, start.start_chunk.0, start.start_chunk.1, state.piece.facing, start.initial
        );
    }
    for outpost in worldgen_assembly::generate::outpost_report(seed, area)? {
        println!(
            "START Outpost chunk ({}, {}) pieces {} union={:?}",
            outpost.start_chunk.0,
            outpost.start_chunk.1,
            outpost.pieces.len(),
            outpost.union_box
        );
    }
    for city in worldgen_assembly::generate::ancient_city_report(seed, area)? {
        println!(
            "START AncientCity chunk ({}, {}) pieces {} union={:?}",
            city.start_chunk.0,
            city.start_chunk.1,
            city.pieces.len(),
            city.union_box
        );
    }
    for ruin in worldgen_assembly::generate::trail_ruin_report(seed, area)? {
        println!(
            "START TrailRuins chunk ({}, {}) pieces {} union={:?}",
            ruin.start_chunk.0,
            ruin.start_chunk.1,
            ruin.pieces.len(),
            ruin.union_box
        );
    }
    for igloo in &igloos {
        println!(
            "START Igloo chunk ({}, {}) rotation {:?} pieces {}",
            igloo.start_chunk.0,
            igloo.start_chunk.1,
            igloo.rotation,
            igloo.pieces.len()
        );
    }
    Ok(())
}

fn jigsaw_starts(vanilla_dir: &Path, filter: Option<&str>) -> Result<(), String> {
    // A trailing `--json` on the filter argument, so the flag needs no
    // argument parser: `jigsaw-starts <dir> village --json`.
    let json = std::env::args().any(|a| a == "--json");
    let filter = filter.filter(|f| *f != "--json");
    let chunks = load_full(vanilla_dir)?;
    let mut found = 0usize;
    for chunk in chunks.values() {
        for start in &chunk.structure_starts {
            if let Some(f) = filter {
                if !start.id.contains(f) {
                    continue;
                }
            }
            // A non-jigsaw start has no `pool_element` on any piece, and
            // printing its boxes here would look like a village with no
            // templates.
            //
            // But a `TemplateStructurePiece` that is not a jigsaw piece —
            // the End city is the whole family — carries `Template`, `Rot`
            // and `TPX/TPY/TPZ` instead, which pin exactly the same three
            // decisions. Skipping those made this tool answer "no jigsaw
            // starts" for a world that demonstrably holds an end city, which
            // reads as "vanilla placed nothing" and is the worse failure.
            let jigsaw = start.pieces.iter().any(|p| p.pool_element.is_some());
            let templated = start.pieces.iter().any(|p| p.template.is_some());
            if !jigsaw && !templated {
                continue;
            }
            found += 1;
            if !jigsaw {
                println!(
                    "START {} chunk ({}, {}) — {} template pieces (not a jigsaw)",
                    start.id,
                    start.chunk_x,
                    start.chunk_z,
                    start.pieces.len()
                );
                for (i, piece) in start.pieces.iter().enumerate() {
                    let tp = piece.template_pos.map_or_else(
                        || "?".to_string(),
                        |p| format!("{},{},{}", p[0], p[1], p[2]),
                    );
                    let bb = piece.bounding_box;
                    println!(
                        "  {i:3}  {:<24} rot={:<20} TP={:<16} GD={:<4} BB=[{} {} {} .. {} {} {}]",
                        piece.template.as_deref().unwrap_or("-"),
                        piece.template_rotation.as_deref().unwrap_or("-"),
                        tp,
                        piece.depth,
                        bb[0],
                        bb[1],
                        bb[2],
                        bb[3],
                        bb[4],
                        bb[5],
                    );
                }
                continue;
            }
            // `--json` writes the same list as a golden vector, so the
            // assembler can be gated on a laptop with no vanilla world at
            // all. The reference is 181 pieces of one savanna village; the
            // regenerated-vanilla noise floor that bit this project on blocks
            // does not apply to it, because a piece list is a decision
            // sequence rather than a sampled world.
            if json {
                println!("{{");
                println!("  \"structure\": \"{}\",", start.id);
                println!("  \"chunk\": [{}, {}],", start.chunk_x, start.chunk_z);
                println!("  \"pieces\": [");
                for (i, piece) in start.pieces.iter().enumerate() {
                    let comma = if i + 1 == start.pieces.len() { "" } else { "," };
                    let pos = piece.pos.unwrap_or([0, 0, 0]);
                    let bb = piece.bounding_box;
                    println!(
                        "    {{\"element\": {}, \"type\": {}, \"projection\": {}, \
                         \"rotation\": {}, \"pos\": [{}, {}, {}], \
                         \"ground_level_delta\": {}, \"bb\": [{}, {}, {}, {}, {}, {}], \
                         \"junctions\": {}}}{comma}",
                        quoted(piece.pool_element.as_deref()),
                        quoted(piece.pool_element_type.as_deref()),
                        quoted(piece.projection.as_deref()),
                        quoted(piece.rotation.as_deref()),
                        pos[0],
                        pos[1],
                        pos[2],
                        piece.ground_level_delta.unwrap_or(0),
                        bb[0],
                        bb[1],
                        bb[2],
                        bb[3],
                        bb[4],
                        bb[5],
                        piece.junctions.len(),
                    );
                }
                println!("  ]");
                println!("}}");
                continue;
            }
            println!(
                "START {} chunk ({}, {}) — {} pieces",
                start.id,
                start.chunk_x,
                start.chunk_z,
                start.pieces.len()
            );
            for (i, piece) in start.pieces.iter().enumerate() {
                let pos = piece.pos.map_or_else(
                    || "?".to_string(),
                    |p| format!("{},{},{}", p[0], p[1], p[2]),
                );
                let bb = piece.bounding_box;
                println!(
                    "  {i:3}  {:<58} {:<20} pos={:<16} gld={:<4} proj={:<16} BB=[{} {} {} .. {} {} {}] junctions={}",
                    piece.pool_element.as_deref().unwrap_or("-"),
                    piece.rotation.as_deref().unwrap_or("-"),
                    pos,
                    piece.ground_level_delta.unwrap_or(i32::MIN),
                    piece.projection.as_deref().unwrap_or("-"),
                    bb[0],
                    bb[1],
                    bb[2],
                    bb[3],
                    bb[4],
                    bb[5],
                    piece.junctions.len(),
                );
            }
        }
    }
    if found == 0 {
        println!("no jigsaw starts in {}", vanilla_dir.display());
    }
    Ok(())
}

fn mineshaft_refs(seed: &str, vanilla_dir: &Path) -> Result<(), String> {
    use rustmc_worldgen::fastutil_long_set::{chunk_pos_as_long, LongOpenHashSet};
    use rustmc_worldgen::pipeline::Area;

    let seed: i64 = seed.parse().map_err(|e| format!("bad seed: {e}"))?;

    // Load every full chunk vanilla wrote, and the bbox they cover.
    let mut chunks: Vec<rustmc_world::anvil::ChunkColumn> = Vec::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(vanilla_dir)
        .map_err(|e| format!("{}: {e}", vanilla_dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "mca"))
        .collect();
    entries.sort();
    for path in entries {
        for chunk in rustmc_world::anvil::read_region(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?
        {
            if chunk.status == "minecraft:full" {
                chunks.push(chunk);
            }
        }
    }
    if chunks.is_empty() {
        return Err(format!("{}: no full chunks", vanilla_dir.display()));
    }
    chunks.sort_by_key(|c| (c.x, c.z));

    let min_x = chunks.iter().map(|c| c.x).min().unwrap_or(0);
    let max_x = chunks.iter().map(|c| c.x).max().unwrap_or(0);
    let min_z = chunks.iter().map(|c| c.z).min().unwrap_or(0);
    let max_z = chunks.iter().map(|c| c.z).max().unwrap_or(0);
    let area = Area {
        min_chunk_x: min_x,
        min_chunk_z: min_z,
        max_chunk_x: max_x,
        max_chunk_z: max_z,
    };
    // This caller only wants the layouts, and the variant does not change
    // them — only the woods. A constant plains biome therefore keeps every
    // start Normal, which is what this tool compared against before the
    // variant existed.
    let mines = worldgen_assembly::generate::mineshaft_starts(
        seed,
        area,
        &mut |_, _, _| "minecraft:plains".to_string(),
        |_, _| None,
    )?;
    println!(
        "chunks {} over {min_x},{min_z}..{max_x},{max_z}; our starts in range: {}",
        chunks.len(),
        mines.len()
    );

    let (mut compared, mut member_bad, mut order_bad, mut refused) = (0u32, 0u32, 0u32, 0u32);
    let mut shown = 0u32;
    // How many references there are to compare at all. A gate that reads an
    // absent field passes on nothing, which is the failure mode this counts
    // its way out of.
    let (mut want_total, mut got_total) = (0u64, 0u64);
    for chunk in &chunks {
        let want: Vec<i64> = chunk
            .structure_references
            .get("minecraft:mineshaft")
            .cloned()
            .unwrap_or_default();
        let (x0, z0) = (chunk.x * 16, chunk.z * 16);
        let (x1, z1) = (x0 + 15, z0 + 15);
        // `createReferences` scans chunk positions in x-major order and adds
        // the ones whose union box meets this chunk in x/z only.
        let mut set = LongOpenHashSet::new();
        let mut grew = false;
        for mine in &mines {
            let b = mine.union_box;
            if b.max_x >= x0
                && b.min_x <= x1
                && b.max_z >= z0
                && b.min_z <= z1
                && set
                    .add(chunk_pos_as_long(mine.start_chunk.0, mine.start_chunk.1))
                    .is_err()
            {
                grew = true;
            }
        }
        if grew {
            refused += 1;
            continue;
        }
        let got = set.iter_order();
        compared += 1;
        want_total += want.len() as u64;
        got_total += got.len() as u64;
        let mut want_sorted = want.clone();
        want_sorted.sort_unstable();
        let mut got_sorted = got.clone();
        got_sorted.sort_unstable();
        if want_sorted != got_sorted {
            member_bad += 1;
            if shown < 12 {
                shown += 1;
                println!(
                    "  MEMBERSHIP chunk ({}, {}): vanilla {:?} ours {:?}",
                    chunk.x,
                    chunk.z,
                    want_sorted.iter().map(unpack).collect::<Vec<_>>(),
                    got_sorted.iter().map(unpack).collect::<Vec<_>>()
                );
            }
        } else if want != got {
            order_bad += 1;
            if shown < 12 {
                shown += 1;
                println!(
                    "  ORDER chunk ({}, {}): vanilla {:?} ours {:?}",
                    chunk.x,
                    chunk.z,
                    want.iter().map(unpack).collect::<Vec<_>>(),
                    got.iter().map(unpack).collect::<Vec<_>>()
                );
            }
        }
    }
    println!(
        "MINESHAFT REFERENCES compared {compared} chunks holding {want_total} vanilla \
         references against {got_total} of ours: {member_bad} wrong membership, \
         {order_bad} wrong order, {refused} refused (set growth)"
    );

    // `Entrances` — the one mineshaft oracle the draw-count work could not
    // reach.
    //
    // `MineShaftRoom.postProcess` hollows its own box, then each entrance
    // box, then a dome, and takes **no** random values doing it. So a wrong
    // entrance is wrong blocks that cost nothing in the stream: every offset
    // downstream still agrees and `mineshaft-ceiling` sees nothing. The only
    // way to catch it is to read the boxes vanilla saved.
    //
    // Both sides are post-move. `MineShaftRoom.move` walks
    // `childEntranceBoxes` calling `BoundingBox.move`, which mutates in place
    // and returns `this` — so the `pop` on its result is harmless and the
    // saved boxes carry `moveBelowSeaLevel`'s drop, exactly as ours do.
    let mut van_rooms: BTreeMap<(i32, i32), Vec<[i32; 6]>> = BTreeMap::new();
    let mut room_disagreed_with_itself = 0u32;
    for chunk in &chunks {
        for start in &chunk.structure_starts {
            if start.id != "minecraft:mineshaft" {
                continue;
            }
            for piece in &start.pieces {
                if piece.id != "minecraft:msroom" {
                    continue;
                }
                // A start is serialised into every chunk it touches, so the
                // same room arrives many times. They must all agree; if two
                // copies differ the oracle is not what this assumes it is.
                let key = (start.chunk_x, start.chunk_z);
                match van_rooms.entry(key) {
                    std::collections::btree_map::Entry::Vacant(slot) => {
                        slot.insert(piece.entrances.clone());
                    }
                    std::collections::btree_map::Entry::Occupied(seen) => {
                        if *seen.get() != piece.entrances {
                            room_disagreed_with_itself += 1;
                        }
                    }
                }
            }
        }
    }

    let (mut rooms_compared, mut rooms_bad, mut boxes_compared) = (0u32, 0u32, 0u64);
    let mut shown = 0u32;
    for mine in &mines {
        let Some(want) = van_rooms.get(&mine.start_chunk) else {
            continue;
        };
        let got: Vec<[i32; 6]> = mine
            .entrances
            .iter()
            .map(|b| [b.min_x, b.min_y, b.min_z, b.max_x, b.max_y, b.max_z])
            .collect();
        rooms_compared += 1;
        boxes_compared += want.len() as u64;
        // Order matters as much as content: `postProcess` hollows them in
        // list order and `addChildren` appends in north/south/west/east
        // order, so a list that agrees as a set and not as a sequence is
        // still a port that built its children in the wrong order.
        if *want != got {
            rooms_bad += 1;
            if shown < 8 {
                shown += 1;
                println!(
                    "  ENTRANCES start ({}, {}): vanilla {want:?} ours {got:?}",
                    mine.start_chunk.0, mine.start_chunk.1
                );
            }
        }
    }
    println!(
        "MINESHAFT ENTRANCES compared {rooms_compared} rooms holding {boxes_compared} \
         vanilla boxes: {rooms_bad} wrong, {room_disagreed_with_itself} rooms whose \
         copies disagreed with each other"
    );

    // `hps` is saved per corridor, and it is the only piece of `postProcess`
    // output vanilla writes into the NBT. Counted here because a spider
    // corridor that placed its spawner and reaches more than one chunk is
    // exactly the case where treating `hasPlacedSpider` as call-local
    // redraws the spawner's `nextInt(3)`.
    let (mut spiders, mut spiders_placed, mut placed_multichunk) = (0u32, 0u32, 0u32);
    for chunk in &chunks {
        for start in &chunk.structure_starts {
            if start.id != "minecraft:mineshaft" {
                continue;
            }
            for piece in &start.pieces {
                if piece.spider != Some(true) {
                    continue;
                }
                spiders += 1;
                if piece.spider_placed == Some(true) {
                    spiders_placed += 1;
                    let bb = piece.bounding_box;
                    let chunks_spanned =
                        ((bb[3] >> 4) - (bb[0] >> 4) + 1) * ((bb[5] >> 4) - (bb[2] >> 4) + 1);
                    if chunks_spanned > 1 {
                        placed_multichunk += 1;
                    }
                }
            }
        }
    }
    println!(
        "MINESHAFT hasPlacedSpider: {spiders} spider corridors, {spiders_placed} placed a \
         spawner, {placed_multichunk} of those reach more than one chunk"
    );
    if member_bad == 0 && order_bad == 0 && refused == 0 {
        println!("REFERENCES 100.000000%");
    }
    // Reported separately, and only when there was something to compare: a
    // clean line printed over zero rooms is the failure this counts its way
    // out of, the same way the reference counters do.
    if rooms_bad == 0 && room_disagreed_with_itself == 0 && boxes_compared > 0 {
        println!("ENTRANCES 100.000000%");
    }
    Ok(())
}

/// `mineshaft-ceiling <seed> <vanilla-region-dir>` — read vanilla's draw
/// count out of its own blocks.
///
/// A corridor's first drawing step is
/// `generateMaybeBox(random, 0.8, 0,2,0, 2,2,length, CAVE_AIR, CAVE_AIR,
/// false, false)`: one `nextFloat` per position, taken **before** any test,
/// nesting y then x then z. It reads nothing. So the ceiling row is a
/// verbatim printout of `3 * (length + 1)` consecutive values from the
/// stream, thresholded at 0.8 — and vanilla saved the result.
///
/// That makes the region file a draw-counter. A position the draw *passed*
/// is `cave_air` afterwards, and nothing later in a mineshaft writes stone
/// there; so a position holding natural rock proves the draw at its index
/// **failed**. Sliding the stream against the rock gives the offset at which
/// vanilla entered this corridor, which is the number the port has to match
/// and the number nobody has ever had.
///
/// Reported per corridor, in the order `startsForStructure` hands them over
/// — which [`mineshaft_refs`] has already gated against the same files.
fn mineshaft_ceiling(
    seed: &str,
    vanilla_dir: &Path,
    blocks_dir: Option<&Path>,
) -> Result<(), String> {
    use rustmc_worldgen::rng::worldgen_random::WorldgenRandom;
    use rustmc_worldgen::rng::xoroshiro::XoroshiroRandomSource;
    use rustmc_worldgen::rng::RandomSource;
    use rustmc_worldgen::structure::pieces::{BoundingBox, Facing, Piece};

    let seed: i64 = seed.parse().map_err(|e| format!("bad seed: {e}"))?;
    let index: i32 = std::env::var("RUSTMC_STRUCTURE_INDEX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    // How far to slide. A corridor is at most 19 long, so 60 ceiling draws,
    // and a chunk's pass rarely runs more than a handful of pieces.
    let search: usize = std::env::var("RUSTMC_CEILING_SEARCH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4000);
    // How much rock a corridor has to show before its offset is believed.
    // Each rock position costs a wrong offset a factor of five, so six of
    // them leave about a quarter of one false survivor in a 4,000-wide
    // search; four leave six, and a verdict drawn from four was the first
    // thing this tool got wrong.
    let min_rock: usize = std::env::var("RUSTMC_CEILING_MIN_ROCK")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(6);

    let chunks = load_full(vanilla_dir)?;
    // The structures always come from vanilla; the *blocks* may come from
    // somewhere else. Pointing this at our own region with vanilla's piece
    // list is what validates the instrument: the offsets it recovers there
    // have to be the ones `RUSTMC_TRACE_MINESHAFT` printed while writing it.
    let blocks = match blocks_dir {
        Some(dir) => load_full(dir)?,
        None => BTreeMap::new(),
    };
    let (mut corridors, mut at_zero, mut unique, mut ambiguous, mut none_found) =
        (0u32, 0u32, 0u32, 0u32, 0u32);
    let mut unreadable = 0u32;
    let (mut refused, mut refused_compared, mut refused_agreed) = (0u32, 0u32, 0u32);
    let mut shown = 0u32;
    // When a second world is given, how the two answers line up. This is
    // the number the whole tool exists for.
    let (mut compared, mut agreed) = (0u32, 0u32);
    let mut deltas: BTreeMap<i64, u32> = BTreeMap::new();
    let show_limit: u32 = std::env::var("RUSTMC_CEILING_SHOW")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(40);
    let keys: Vec<(i32, i32)> = chunks.keys().copied().collect();
    for key in keys {
        let refs = chunks[&key]
            .structure_references
            .get("minecraft:mineshaft")
            .cloned()
            .unwrap_or_default();
        if refs.is_empty() {
            continue;
        }
        let (cx, cz) = key;
        // `ChunkGenerator.getWritableArea` — minY + 1 at the bottom.
        let clip = BoundingBox::new(cx * 16, -63, cz * 16, cx * 16 + 15, 319, cz * 16 + 15);

        let mut random = WorldgenRandom::new(XoroshiroRandomSource::from_seed(0));
        let population = random.set_decoration_seed(seed, cx * 16, cz * 16);
        random.set_feature_seed(population, index, 3);
        let stream: Vec<f32> = (0..search + 4096).map(|_| random.next_f32()).collect();

        // Every corridor this chunk's pass reaches, in order.
        let mut nth = 0u32;
        for packed in &refs {
            let (sx, sz) = unpack(packed);
            let Some(start) = chunks.get(&(sx, sz)).and_then(|c| {
                c.structure_starts
                    .iter()
                    .find(|s| s.id == "minecraft:mineshaft")
            }) else {
                continue;
            };
            for piece in &start.pieces {
                let bb = piece.bounding_box;
                let bbox = BoundingBox::new(bb[0], bb[1], bb[2], bb[3], bb[4], bb[5]);
                if !bbox.intersects(clip) || piece.id != "minecraft:mscorridor" {
                    continue;
                }
                let Some(sections) = piece.sections else {
                    continue;
                };
                let length = sections * 5 - 1;
                let frame = Piece {
                    id: "minecraft:mscorridor",
                    bounding_box: bbox,
                    facing: Facing::from_saved_value(piece.orientation),
                    depth: piece.depth,
                };
                corridors += 1;
                nth += 1;
                let want =
                    offset_from_blocks(&chunks, &frame, clip, length, &stream, search, min_rock);
                let got = blocks_dir.map(|_| {
                    offset_from_blocks(&blocks, &frame, clip, length, &stream, search, min_rock)
                });
                match want {
                    Verdict::Pinned(0) => {
                        at_zero += 1;
                        unique += 1;
                    }
                    Verdict::Pinned(_) => unique += 1,
                    Verdict::Ambiguous(_, _) => ambiguous += 1,
                    Verdict::NoFit => none_found += 1,
                    Verdict::Refused(_) => refused += 1,
                    Verdict::Unreadable(_) => unreadable += 1,
                }
                // A refusal is a claim about *both* sides, so it is only
                // agreement when ours refused the same piece. Counted apart
                // from the pinned offsets because no offset is involved.
                if let (Verdict::Refused(_), Some(g)) = (&want, &got) {
                    refused_compared += 1;
                    if matches!(g, Verdict::Refused(_)) {
                        refused_agreed += 1;
                    }
                }
                if let (Verdict::Pinned(a), Some(Verdict::Pinned(b))) = (&want, &got) {
                    compared += 1;
                    #[expect(clippy::cast_possible_wrap, reason = "an offset under 4096")]
                    let delta = *b as i64 - *a as i64;
                    if delta == 0 {
                        agreed += 1;
                    }
                    *deltas.entry(delta).or_insert(0) += 1;
                }
                // Only the lines that carry information: a corridor whose
                // two sides both pin and disagree, or one vanilla's own
                // blocks cannot explain at all. Everything else is a row
                // too sparse to read and saying so 600 times buries them.
                let interesting = matches!(&want, Verdict::NoFit)
                    || matches!(
                        (&want, &got),
                        (Verdict::Pinned(a), Some(Verdict::Pinned(b))) if a != b
                    )
                    // A refusal vanilla made and we did not is a real
                    // disagreement and has to be visible; one both sides made
                    // is the answer, and printing it once per corridor is
                    // what turns the assumption into a count.
                    || matches!(
                        (&want, &got),
                        (Verdict::Refused(_), Some(g)) if !matches!(g, Verdict::Refused(_))
                    );
                if shown < show_limit && interesting {
                    shown += 1;
                    let tail = match &got {
                        Some(g) => format!("{want}  ours {g}"),
                        None => format!("{want}"),
                    };
                    println!(
                        "  chunk ({cx}, {cz}) corridor #{nth} start ({sx}, {sz}) \
                         box {},{},{}..{},{},{} sections {sections} draws {} -> {tail}",
                        bb[0],
                        bb[1],
                        bb[2],
                        bb[3],
                        bb[4],
                        bb[5],
                        3 * (length + 1),
                    );
                }
            }
        }
    }
    if blocks_dir.is_some() {
        println!(
            "  offsets pinned on both sides: {compared}, agreeing: {agreed}, \
             disagreeing: {}",
            compared - agreed
        );
        for (delta, count) in &deltas {
            if *delta != 0 {
                println!("    delta {delta:+}  x{count}");
            }
        }
        println!(
            "  corridors vanilla refused: {refused_compared}, ours refused too: \
             {refused_agreed}, ours placed: {}",
            refused_compared - refused_agreed
        );
    }
    println!(
        "MINESHAFT CEILING corridors {corridors}: {at_zero} entered at offset 0, \
         {unique} pinned to one offset, {ambiguous} ambiguous, {refused} refused, \
         {none_found} unexplainable, {unreadable} unreadable"
    );
    Ok(())
}

/// What the ceiling row says about the offset a corridor entered on.
enum Verdict {
    /// One offset explains every rock position and the most cave_air.
    Pinned(usize),
    /// Several survive and score alike — the row is too sparse to choose.
    Ambiguous(usize, usize),
    /// No offset in the search window leaves every rock position unwritten,
    /// and the row is not all rock — so the corridor did run its pass and
    /// this tool cannot say where it entered. Either the search is too narrow
    /// or something upstream is wrong about this piece.
    NoFit,
    /// **Every** in-clip ceiling position is untouched natural rock, so the
    /// pass never ran: the piece was refused before it drew.
    ///
    /// This is not a judgement call. The pass writes `cave_air` wherever
    /// `nextFloat() < 0.8`, so a corridor that ran and left `n` positions
    /// untouched did so with probability `0.2^n`; at the 45 positions of the
    /// smallest corridor here that is 3e-32. A row with no hollow and no
    /// foreign block in it is a corridor `isInInvalidLocation` turned away.
    Refused(usize),
    /// Fewer than `min_rock` positions survived as natural rock, so there is
    /// not enough evidence to say anything. Counted, never guessed at.
    Unreadable(usize),
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pinned(o) => write!(f, "offset {o}"),
            Self::Ambiguous(n, first) => write!(f, "{n} offsets tie, first {first}"),
            Self::NoFit => write!(f, "NO OFFSET FITS"),
            Self::Refused(n) => write!(f, "REFUSED (all {n} ceiling cells untouched rock)"),
            Self::Unreadable(rock) => write!(f, "unreadable ({rock} rock)"),
        }
    }
}

/// Slide the stream against one corridor's ceiling row and see where it fits.
///
/// A position holding natural rock proves the draw at its index failed — the
/// pass writes `cave_air` wherever it passes and nothing in a mineshaft puts
/// stone back. That is the hard constraint; the `cave_air` positions are the
/// soft one, since a carver could have hollowed them out first, so they only
/// break ties.
fn offset_from_blocks(
    world: &BTreeMap<(i32, i32), rustmc_world::anvil::ChunkColumn>,
    frame: &rustmc_worldgen::structure::pieces::Piece,
    clip: rustmc_worldgen::structure::pieces::BoundingBox,
    length: i32,
    stream: &[f32],
    search: usize,
    min_rock: usize,
) -> Verdict {
    let mut rock: Vec<usize> = Vec::new();
    let mut hollow: Vec<usize> = Vec::new();
    // Everything in the clip that is neither: a block some other stage put
    // there. It is what separates "the pass never ran" from "the pass ran and
    // something painted over it".
    let mut foreign = 0usize;
    let mut draw = 0usize;
    for x in 0..=2 {
        for z in 0..=length {
            let (wx, wy, wz) = frame.world_pos(x, 2, z);
            if wx >= clip.min_x
                && wx <= clip.max_x
                && wz >= clip.min_z
                && wz <= clip.max_z
                && wy >= clip.min_y
                && wy <= clip.max_y
            {
                match block_at(world, wx, wy, wz) {
                    Some(state) if is_untouched_rock(&state) => rock.push(draw),
                    Some(state) if state.starts_with("minecraft:cave_air") => hollow.push(draw),
                    Some(_) => foreign += 1,
                    None => {}
                }
            }
            draw += 1;
        }
    }
    if rock.len() < min_rock {
        return Verdict::Unreadable(rock.len());
    }
    // Answered before the search, because no offset can fit an all-rock row
    // and calling that "NO OFFSET FITS" hides the reason.
    if hollow.is_empty() && foreign == 0 {
        return Verdict::Refused(rock.len());
    }
    // `RUSTMC_CEILING_FROM` narrows the window. A sparse ceiling cannot be
    // read against the whole stream, but it can be read against a handful
    // of candidates — and once the corridors either side of it are pinned,
    // a handful is all that is left.
    let from: usize = std::env::var("RUSTMC_CEILING_FROM")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut best: Vec<(usize, usize)> = Vec::new();
    for offset in from..from + search {
        if rock.iter().any(|&i| stream[offset + i] <= 0.8) {
            continue;
        }
        let explained = hollow
            .iter()
            .filter(|&&i| stream[offset + i] <= 0.8)
            .count();
        best.push((offset, explained));
    }
    let top = best.iter().map(|&(_, e)| e).max().unwrap_or(0);
    let winners: Vec<usize> = best
        .iter()
        .filter(|&&(_, e)| e == top)
        .map(|&(o, _)| o)
        .collect();
    match winners.len() {
        0 => Verdict::NoFit,
        1 => Verdict::Pinned(winners[0]),
        n => Verdict::Ambiguous(n, winners[0]),
    }
}

/// Every `full` chunk in a region directory, by chunk position.
fn load_full(dir: &Path) -> Result<BTreeMap<(i32, i32), rustmc_world::anvil::ChunkColumn>, String> {
    let mut out = BTreeMap::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "mca"))
        .collect();
    entries.sort();
    for path in entries {
        for chunk in rustmc_world::anvil::read_region(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?
        {
            if chunk.status == "minecraft:full" {
                out.insert((chunk.x, chunk.z), chunk);
            }
        }
    }
    Ok(out)
}

/// The block at a world position, or `None` if that chunk or section is
/// absent.
fn block_at(
    chunks: &BTreeMap<(i32, i32), rustmc_world::anvil::ChunkColumn>,
    x: i32,
    y: i32,
    z: i32,
) -> Option<String> {
    let chunk = chunks.get(&(x >> 4, z >> 4))?;
    #[expect(clippy::cast_possible_truncation, reason = "a section y fits in i8")]
    let sy = (y >> 4) as i8;
    let section = chunk.sections.iter().find(|s| s.y == sy)?;
    let i = ((y & 15) * 256 + (z & 15) * 16 + (x & 15)) as usize;
    let idx = section.blocks.get(i).map_or(0, |&v| v as usize);
    section.palette.get(idx).cloned()
}

/// Blocks a mineshaft never writes and a post-structure feature never puts
/// into air — so finding one at a ceiling position proves the draw failed.
///
/// Deliberately narrow. `moss_block`, `water` and `lava` are all things a
/// later step can drop into the hole a passing draw made, and counting them
/// as rock would turn a correct offset into an impossible one.
fn is_untouched_rock(state: &str) -> bool {
    let name = state.split_once('[').map_or(state, |(n, _)| n);
    name.ends_with("_ore")
        || matches!(
            name,
            "minecraft:stone"
                | "minecraft:deepslate"
                | "minecraft:tuff"
                | "minecraft:andesite"
                | "minecraft:diorite"
                | "minecraft:granite"
                | "minecraft:calcite"
                | "minecraft:smooth_basalt"
                | "minecraft:gravel"
                | "minecraft:sand"
                | "minecraft:dirt"
                | "minecraft:clay"
                | "minecraft:sandstone"
                | "minecraft:sulfur"
                | "minecraft:cinnabar"
        )
}

/// A packed `ChunkPos.asLong` back into a readable pair.
fn unpack(packed: &i64) -> (i32, i32) {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "ChunkPos.asLong's own halves"
    )]
    (
        (*packed as u64 as u32) as i32,
        ((*packed as u64) >> 32) as u32 as i32,
    )
}

/// `column <seed> <x> <z> <y-lo> [y-hi]` — one column, stage by stage.
///
/// A whole-world diff says *what* a block should be; this says *which stage
/// made it what it is*, which is the question that actually gets asked when
/// a row in that diff has to be attributed. Both `_WG` heightmap fossils are
/// printed alongside, because they are primed during the noise stage and
/// never updated, so they record what the column looked like before carving
/// — and a placement chain that hangs off `OCEAN_FLOOR_WG` reads that
/// fossil rather than the terrain that ends up in the file.
fn column(
    seed: &str,
    x: &str,
    z: &str,
    y_lo: &str,
    y_hi: Option<&str>,
    center_x: Option<&str>,
    center_z: Option<&str>,
) -> Result<(), String> {
    let seed: i64 = seed.parse().map_err(|e| format!("bad seed: {e}"))?;
    let x: i32 = x.parse().map_err(|e| format!("bad x: {e}"))?;
    let z: i32 = z.parse().map_err(|e| format!("bad z: {e}"))?;
    let lo: i32 = y_lo.parse().map_err(|e| format!("bad y-lo: {e}"))?;
    let hi: i32 = y_hi.unwrap_or("").parse().unwrap_or(lo + 16);

    // Radius 2 so the decoration stage has the neighbours it reads. The
    // earlier stages need fewer, but generating the same area for all of
    // them keeps the columns comparable.
    //
    // **The centre matters, and getting it wrong makes this tool lie.**
    // Decoration writes one chunk outwards, so the world is a function of
    // the *extent* generated, not only the seed — two squares covering the
    // same column at different centres put features in different places
    // (ORACLE.md rule 10b). Centred on the column's own chunk by default,
    // which is right for asking "what does this column look like" and wrong
    // for attributing a block in a dump made at some other centre: pass that
    // dump's centre and they will agree.
    //
    // Found by doing exactly that: this reported `cinnabar` from the surface
    // stage, unchanged afterwards, for a position the r2-at-(64,64) dump had
    // as `sulfur`.
    let (cx, cz) = match (center_x, center_z) {
        (Some(a), Some(b)) => (
            a.parse::<i32>().map_err(|e| format!("bad center-x: {e}"))?,
            b.parse::<i32>().map_err(|e| format!("bad center-z: {e}"))?,
        ),
        _ => (x >> 4, z >> 4),
    };
    let area = rustmc_worldgen::pipeline::Area::square(cx, cz, 2);
    let stages = [
        ("noise", rustmc_worldgen::pipeline::Through::Noise),
        ("surface", rustmc_worldgen::pipeline::Through::Surface),
        ("carvers", rustmc_worldgen::pipeline::Through::Carvers),
        ("decorated", rustmc_worldgen::pipeline::Through::Decoration),
    ];

    let mut columns: Vec<(&str, Vec<String>)> = Vec::new();
    let mut fossils: Vec<(i32, i32)> = Vec::new();
    for (name, through) in stages {
        let world = worldgen_assembly::generate::generate(seed, area, through)?;
        fossils.push((
            world.canvas.world_surface_wg(x, z),
            world.canvas.ocean_floor_wg(x, z),
        ));
        let blocks = (lo..=hi)
            .map(|y| world.canvas.block_at(x, y, z).replace("minecraft:", ""))
            .collect();
        columns.push((name, blocks));
    }

    println!("column ({x}, {z})  seed {seed}");
    // Per stage, because the `_WG` maps are live through carving and only
    // become fossils at decoration — a single number would hide the move
    // that matters.
    print!("  WORLD_SURFACE_WG ");
    for ((name, _), (ws, _)) in columns.iter().zip(&fossils) {
        print!(" {name}={ws}");
    }
    println!();
    print!("  OCEAN_FLOOR_WG   ");
    for ((name, _), (_, of)) in columns.iter().zip(&fossils) {
        print!(" {name}={of}");
    }
    println!();
    print!("  {:>5}", "y");
    for (name, _) in &columns {
        print!("  {name:>16}");
    }
    println!();
    for (i, y) in (lo..=hi).enumerate() {
        print!("  {y:>5}");
        let mut previous: Option<&str> = None;
        for (_, blocks) in &columns {
            let here = blocks[i].as_str();
            // Only the changes are worth reading; an unchanged cell is
            // noise in a table whose whole point is where something moved.
            let shown = if previous == Some(here) { "" } else { here };
            print!("  {shown:>16}");
            previous = Some(here);
        }
        println!();
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("bench") if args.len() == 3 => bench(&args[1], &args[2]),
        Some("scale") if args.len() == 3 || args.len() == 4 => {
            scale(&args[1], &args[2], args.get(3).map(String::as_str))
        }
        Some("region") if args.len() == 4 => region(&args[1], &args[2], Path::new(&args[3])),
        Some("column") if (5..=8).contains(&args.len()) => column(
            &args[1],
            &args[2],
            &args[3],
            &args[4],
            args.get(5).map(String::as_str),
            args.get(6).map(String::as_str),
            args.get(7).map(String::as_str),
        ),
        Some("village-pieces") if (4..=6).contains(&args.len()) => village_pieces(
            &args[1],
            &args[2],
            &args[3],
            args.get(4).map(String::as_str),
            args.get(5).map(String::as_str),
        ),
        Some("nether-fossil-starts") if (3..=5).contains(&args.len()) => {
            nether_fossil_starts_cmd(&args[1], &args[2], args.get(3), args.get(4))
        }
        Some("bastion-starts") if (3..=5).contains(&args.len()) => {
            bastion_starts_cmd(&args[1], &args[2], args.get(3), args.get(4))
        }
        Some("fortress-starts") if (3..=5).contains(&args.len()) => {
            fortress_starts_cmd(&args[1], &args[2], args.get(3), args.get(4))
        }
        Some("ruined-portal-starts") if (3..=5).contains(&args.len()) => {
            ruined_portal_starts_cmd(&args[1], &args[2], args.get(3), args.get(4))
        }
        Some("ruined-portal-apron") if args.len() == 3 || args.len() == 4 => {
            ruined_portal_apron(&args[1], Path::new(&args[2]), args.get(3).map(Path::new))
        }
        Some("scattered-starts") if args.len() == 5 => scattered_starts_cmd(&args[1..]),
        // `stronghold-starts <seed> <cx> <cz> <r>` — every stronghold our
        // ring placement puts within reach of the square, piece by piece in
        // the shape `region-differ starts` prints vanilla's.
        Some("stronghold-starts") if args.len() == 5 => {
            let parse = |s: &String| s.parse::<i64>().map_err(|e| e.to_string());
            (|| -> Result<(), String> {
                let seed = parse(&args[1])?;
                let (cx, cz, r) = (
                    i32::try_from(parse(&args[2])?).map_err(|e| e.to_string())?,
                    i32::try_from(parse(&args[3])?).map_err(|e| e.to_string())?,
                    i32::try_from(parse(&args[4])?).map_err(|e| e.to_string())?,
                );
                let area = rustmc_worldgen::pipeline::Area::square(cx, cz, r);
                for sh in worldgen_assembly::generate::stronghold_report(seed, area)? {
                    println!(
                        "START stronghold chunk ({}, {}) {} pieces",
                        sh.start_chunk.0,
                        sh.start_chunk.1,
                        sh.pieces.len()
                    );
                    for p in &sh.pieces {
                        let b = p.piece.bounding_box;
                        println!(
                            "  {} O={} GD={} BB=[{}, {}, {}, {}, {}, {}]",
                            p.piece.id,
                            p.piece.facing.map_or(-1, |f| f.saved_value()),
                            p.piece.depth,
                            b.min_x,
                            b.min_y,
                            b.min_z,
                            b.max_x,
                            b.max_y,
                            b.max_z
                        );
                    }
                }
                Ok(())
            })()
        }
        // `mansion-starts <seed> <cx> <cz> <r>` — every woodland mansion within
        // reach of the square, piece by piece: template, rotation, mirror,
        // template position and box, in `createMansion`'s order.
        Some("mansion-starts") if args.len() == 5 => {
            let parse = |s: &String| s.parse::<i64>().map_err(|e| e.to_string());
            (|| -> Result<(), String> {
                let seed = parse(&args[1])?;
                let (cx, cz, r) = (
                    i32::try_from(parse(&args[2])?).map_err(|e| e.to_string())?,
                    i32::try_from(parse(&args[3])?).map_err(|e| e.to_string())?,
                    i32::try_from(parse(&args[4])?).map_err(|e| e.to_string())?,
                );
                let area = rustmc_worldgen::pipeline::Area::square(cx, cz, r);
                for m in worldgen_assembly::generate::mansion_report(seed, area)? {
                    println!(
                        "START mansion chunk ({}, {}) {} pieces",
                        m.start_chunk.0,
                        m.start_chunk.1,
                        m.pieces.len()
                    );
                    for p in &m.pieces {
                        let b = p.bbox;
                        println!(
                            "  {} {:?} {:?} T=[{}, {}, {}] BB=[{}, {}, {}, {}, {}, {}]",
                            p.piece.name,
                            p.piece.rotation,
                            p.piece.mirror,
                            p.piece.pos.0,
                            p.piece.pos.1,
                            p.piece.pos.2,
                            b.min_x,
                            b.min_y,
                            b.min_z,
                            b.max_x,
                            b.max_y,
                            b.max_z
                        );
                    }
                }
                Ok(())
            })()
        }
        // `structure-spawns <overworld|end> <seed> <cx> <cz> <r>` — every
        // entity a mansion, monument or end city spawns in the square, one
        // `SPAWN` line each: structure, entity id, block, facing, count
        // (`?` for an allay group's 1..3).
        Some("structure-spawns") if args.len() == 6 => {
            let parse = |s: &String| s.parse::<i64>().map_err(|e| e.to_string());
            (|| -> Result<(), String> {
                let end = match args[1].as_str() {
                    "end" => true,
                    "overworld" => false,
                    other => return Err(format!("dimension {other}: want overworld or end")),
                };
                let seed = parse(&args[2])?;
                let (cx, cz, r) = (
                    i32::try_from(parse(&args[3])?).map_err(|e| e.to_string())?,
                    i32::try_from(parse(&args[4])?).map_err(|e| e.to_string())?,
                    i32::try_from(parse(&args[5])?).map_err(|e| e.to_string())?,
                );
                let area = rustmc_worldgen::pipeline::Area::square(cx, cz, r);
                for s in worldgen_assembly::generate::structure_spawns(seed, area, end)? {
                    println!(
                        "SPAWN {} {} {} {} {} {} {}",
                        s.structure,
                        s.id,
                        s.block.0,
                        s.block.1,
                        s.block.2,
                        s.facing.unwrap_or("-"),
                        s.count.map_or_else(|| "?".to_string(), |c| c.to_string())
                    );
                }
                Ok(())
            })()
        }
        // `end-city-starts <seed> <cx> <cz> <r>` — every End city start in
        // the square, piece by piece in `startHouseTower`'s order, in the
        // `PIECE` shape `region-differ starts` prints vanilla's in, plus the
        // template, rotation and GD. `ship` in a template column means the
        // tower bridge grew one.
        Some("end-city-starts") if args.len() == 5 => {
            let parse = |s: &String| s.parse::<i64>().map_err(|e| e.to_string());
            (|| -> Result<(), String> {
                let seed = parse(&args[1])?;
                let (cx, cz, r) = (
                    i32::try_from(parse(&args[2])?).map_err(|e| e.to_string())?,
                    i32::try_from(parse(&args[3])?).map_err(|e| e.to_string())?,
                    i32::try_from(parse(&args[4])?).map_err(|e| e.to_string())?,
                );
                let area = rustmc_worldgen::pipeline::Area::square(cx, cz, r);
                for city in worldgen_assembly::generate::end_city_report(seed, area)? {
                    let ship = city.pieces.iter().any(|p| p.template == "ship");
                    println!(
                        "START end_city chunk ({}, {}) {} pieces ship={ship}",
                        city.start_chunk.0,
                        city.start_chunk.1,
                        city.pieces.len()
                    );
                    for p in &city.pieces {
                        let b = p.bbox;
                        println!(
                            "PIECE minecraft:end_city {},{} minecraft:end_city_piece {} {} {} {} {} {} {} {:?} GD={}",
                            city.start_chunk.0,
                            city.start_chunk.1,
                            b.min_x,
                            b.min_y,
                            b.min_z,
                            b.max_x,
                            b.max_y,
                            b.max_z,
                            p.template,
                            p.rotation,
                            p.gen_depth
                        );
                    }
                }
                Ok(())
            })()
        }
        // `monument-starts <seed> <cx> <cz> <r>` — every ocean monument whose
        // building reaches the square: the saved building piece in the shape
        // `region-differ starts` prints it, then the child rooms vanilla
        // builds in memory and never saves.
        Some("monument-starts") if args.len() == 5 => {
            let parse = |s: &String| s.parse::<i64>().map_err(|e| e.to_string());
            (|| -> Result<(), String> {
                let seed = parse(&args[1])?;
                let (cx, cz, r) = (
                    i32::try_from(parse(&args[2])?).map_err(|e| e.to_string())?,
                    i32::try_from(parse(&args[3])?).map_err(|e| e.to_string())?,
                    i32::try_from(parse(&args[4])?).map_err(|e| e.to_string())?,
                );
                let area = rustmc_worldgen::pipeline::Area::square(cx, cz, r);
                for m in worldgen_assembly::generate::monument_report(seed, area)? {
                    let b = m.building.bounding_box;
                    println!(
                        "START monument chunk ({}, {}) O={} BB=[{}, {}, {}, {}, {}, {}] rooms {}",
                        m.start_chunk.0,
                        m.start_chunk.1,
                        m.building.facing.map_or(-1, |f| f.saved_value()),
                        b.min_x,
                        b.min_y,
                        b.min_z,
                        b.max_x,
                        b.max_y,
                        b.max_z,
                        m.rooms.len()
                    );
                    for room in &m.rooms {
                        let b = room.piece.bounding_box;
                        println!(
                            "  {:?} cell={} BB=[{}, {}, {}, {}, {}, {}]",
                            room.kind,
                            room.def.map_or(-1, |d| m.defs[d].index),
                            b.min_x,
                            b.min_y,
                            b.min_z,
                            b.max_x,
                            b.max_y,
                            b.max_z
                        );
                    }
                }
                Ok(())
            })()
        }
        // `jigsaw-set-oracle <outpost|trail|city> <seed> <vanilla-dir> [r cx cz]` —
        // the same piece-for-piece harness the chambers and villages get.
        Some("jigsaw-set-oracle") if (4..=7).contains(&args.len()) => {
            type Report = fn(
                i64,
                rustmc_worldgen::pipeline::Area,
            )
                -> Result<Vec<worldgen_assembly::generate::Village>, String>;
            let (title, id, noun, report): (&str, &str, &str, Report) = match args[1].as_str() {
                "outpost" => (
                    "OUTPOST ORACLE",
                    "pillager_outpost",
                    "outpost",
                    worldgen_assembly::generate::outpost_report,
                ),
                "trail" => (
                    "TRAIL RUINS ORACLE",
                    "trail_ruins",
                    "trail ruin",
                    worldgen_assembly::generate::trail_ruin_report,
                ),
                "city" => (
                    "ANCIENT CITY ORACLE",
                    "ancient_city",
                    "ancient city",
                    worldgen_assembly::generate::ancient_city_report,
                ),
                other => {
                    eprintln!("unknown jigsaw set `{other}` (outpost|trail|city)");
                    std::process::exit(2)
                }
            };
            jigsaw_oracle(
                title,
                id,
                noun,
                report,
                &args[2],
                Path::new(&args[3]),
                args.get(4),
                args.get(5),
                args.get(6),
            )
        }
        Some("jigsaw-starts") if (2..=4).contains(&args.len()) => {
            jigsaw_starts(Path::new(&args[1]), args.get(2).map(String::as_str))
        }
        Some("trial-chamber-oracle") if (3..=6).contains(&args.len()) => trial_chamber_oracle(
            &args[1],
            Path::new(&args[2]),
            args.get(3),
            args.get(4),
            args.get(5),
        ),
        Some("village-oracle") if (3..=6).contains(&args.len()) => village_oracle(
            &args[1],
            Path::new(&args[2]),
            args.get(3),
            args.get(4),
            args.get(5),
        ),
        Some("mineshaft-refs") if args.len() == 3 => mineshaft_refs(&args[1], Path::new(&args[2])),
        Some("mineshaft-mesa") if args.len() == 3 => mineshaft_mesa(&args[1], Path::new(&args[2])),
        Some("mineshaft-ceiling") if args.len() == 3 || args.len() == 4 => {
            mineshaft_ceiling(&args[1], Path::new(&args[2]), args.get(3).map(Path::new))
        }
        Some("full") if (4..=6).contains(&args.len()) => full(
            &args[1],
            &args[2],
            Path::new(&args[3]),
            args.get(4).map(String::as_str),
            args.get(5).map(String::as_str),
            Dimension::Overworld,
        ),
        Some("wg-heightmaps") if (4..=6).contains(&args.len()) => wg_heightmaps(
            &args[1],
            &args[2],
            &args[3],
            args.get(4).map(String::as_str),
            args.get(5).map(String::as_str),
        ),
        Some("nether-density") if args.len() == 6 => {
            nether_density(&args[1], &args[2], &args[3], &args[4], &args[5])
        }
        Some("full-nether") if (4..=6).contains(&args.len()) => full(
            &args[1],
            &args[2],
            Path::new(&args[3]),
            args.get(4).map(String::as_str),
            args.get(5).map(String::as_str),
            Dimension::Nether,
        ),
        Some("full-end") if (4..=6).contains(&args.len()) => full(
            &args[1],
            &args[2],
            Path::new(&args[3]),
            args.get(4).map(String::as_str),
            args.get(5).map(String::as_str),
            Dimension::End,
        ),
        _ => {
            eprint!("{USAGE}");
            std::process::exit(2);
        }
    };
    if let Err(message) = result {
        eprintln!("worldgen-dump: {message}");
        std::process::exit(1);
    }
}

/// Where the noise parameters come from.
///
/// The golden vector file already carries the whole registry, emitted from
/// the jar by goldengen. Reading it here keeps this tool working without an
/// extracted-data directory; it is not a claim that this is where a *server*
/// should get them.
fn noise_parameters() -> Result<OverworldNoises, String> {
    let path = std::env::var("RUSTMC_NOISE_JSON").unwrap_or_else(|_| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../crates/rustmc-worldgen/golden/router_vectors.json"
        )
        .to_string()
    });
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read noise parameters from {path}: {e}"))?;
    let doc: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{path} is not JSON: {e}"))?;

    let mut set = OverworldNoises::new();
    let entries = doc["parameters"]
        .as_array()
        .ok_or_else(|| format!("{path} has no `parameters` array"))?;
    for entry in entries {
        let amplitudes: Vec<f64> = entry["amplitudes"]
            .as_array()
            .ok_or("an entry has no amplitudes")?
            .iter()
            .map(|v| {
                let bits: i64 = v
                    .as_str()
                    .ok_or("amplitude is not a string")?
                    .parse()
                    .map_err(|e| format!("bad amplitude: {e}"))?;
                Ok(f64::from_bits(bits as u64))
            })
            .collect::<Result<_, String>>()?;
        set = set.with(
            entry["name"].as_str().ok_or("an entry has no name")?,
            NoiseParameters::new(
                entry["firstOctave"]
                    .as_i64()
                    .ok_or("an entry has no firstOctave")? as i32,
                amplitudes,
            ),
        );
    }
    Ok(set)
}

/// Build a fill for one chunk.
///
/// The router is rebuilt per chunk, which is what `NoiseChunk.forChunk` does
/// too — but note it is *not* free, so a future benchmark that wants to
/// isolate the fill should hoist it and say so.
/// The per-chunk clone of the router's graphs, on its own.
///
/// Measured separately because it is pure overhead we invented — vanilla
/// builds one router per world and only wraps its markers per chunk — and
/// because a hypothesis about where time goes has already been wrong twice
/// in this project.
fn clone_inputs(router: &OverworldRouter, seeding: &NoiseSeeding) -> FillInputs {
    FillInputs {
        final_density: router.final_density.clone(),
        aquifer_noises: AquiferNoises {
            barrier: router.barrier.clone(),
            fluid_level_floodedness: router.fluid_level_floodedness.clone(),
            fluid_level_spread: router.fluid_level_spread.clone(),
            lava: router.lava.clone(),
            erosion: router.erosion.clone(),
            depth: router.depth.clone(),
        },
        preliminary_surface_level: router.preliminary_surface_level.clone(),
        aquifer_random: seeding.source_for("minecraft:aquifer").fork_positional(),
        ore_veinifier: Some(OreVeinifier::new(
            router.vein_toggle.clone(),
            router.vein_ridged.clone(),
            router.vein_gap.clone(),
            seeding.source_for("minecraft:ore").fork_positional(),
        )),
        // The benchmark's isolated per-chunk router clone. Beardless, and
        // that is what it is timing: the cost of cloning the graph, not the
        // cost of a beard.
        beardifier: None,
    }
}

fn build_fill(
    router: &OverworldRouter,
    seeding: &NoiseSeeding,
    settings: FillSettings,
    chunk_x: i32,
    chunk_z: i32,
) -> NoiseFill {
    NoiseFill::new(
        FillInputs {
            final_density: router.final_density.clone(),
            aquifer_noises: AquiferNoises {
                barrier: router.barrier.clone(),
                fluid_level_floodedness: router.fluid_level_floodedness.clone(),
                fluid_level_spread: router.fluid_level_spread.clone(),
                lava: router.lava.clone(),
                erosion: router.erosion.clone(),
                depth: router.depth.clone(),
            },
            preliminary_surface_level: router.preliminary_surface_level.clone(),
            aquifer_random: seeding.source_for("minecraft:aquifer").fork_positional(),
            ore_veinifier: Some(OreVeinifier::new(
                router.vein_toggle.clone(),
                router.vein_ridged.clone(),
                router.vein_gap.clone(),
                seeding.source_for("minecraft:ore").fork_positional(),
            )),
            // Beardless. `ablate` and `generate` measure the fill in
            // isolation, with no structures stage above them to collect
            // pieces from.
            beardifier: None,
        },
        settings,
        chunk_x,
        chunk_z,
    )
}

/// Total fill time for `count` chunks under one settings variant.
///
/// The benchmark's breakdown of *setup* against *fill* is a wall-clock split;
/// this is how the fill half is broken down further. Timing inside the block
/// loop is not an option — an `Instant::now()` costs ~25 ns against a
/// ~190 ns block — so the fill's parts are isolated by turning them off and
/// taking the difference. Ablation measures what a feature costs *in place*,
/// including the work it causes elsewhere, which is the number that matters.
fn ablate(
    router: &OverworldRouter,
    seeding: &NoiseSeeding,
    settings: FillSettings,
    count: u32,
) -> f64 {
    let side = (count as f64).sqrt().ceil() as i32;
    let mut done = 0u32;
    let mut sink = 0u64;
    let mut secs = 0.0;
    for cz in 0..side {
        for cx in 0..side {
            if done == count {
                break;
            }
            let fill = build_fill(router, seeding, settings, cx - side / 2, cz - side / 2);
            let start = Instant::now();
            let chunk = fill.fill();
            secs += start.elapsed().as_secs_f64();
            sink += u64::from(chunk.get(0, 0, 0) != chunk.get(15, 100, 15));
            done += 1;
        }
    }
    std::hint::black_box(sink);
    secs
}

fn generate(
    router: &OverworldRouter,
    seeding: &NoiseSeeding,
    chunk_x: i32,
    chunk_z: i32,
) -> rustmc_worldgen::chunk_gen::FilledChunk {
    build_fill(router, seeding, FillSettings::overworld(), chunk_x, chunk_z).fill()
}

fn bench(seed: &str, chunks: &str) -> Result<(), String> {
    let seed: i64 = seed.parse().map_err(|e| format!("bad seed: {e}"))?;
    let count: u32 = chunks
        .parse()
        .map_err(|e| format!("bad chunk count: {e}"))?;
    if count == 0 {
        return Err("nothing to generate".into());
    }
    let noises = noise_parameters()?;

    // Router construction is measured separately: it is once per world in a
    // server, so folding it into the per-chunk number would flatter or
    // penalise depending only on how many chunks were asked for.
    let router_start = Instant::now();
    let router = OverworldRouter::new(seed, &noises, false);
    let seeding = NoiseSeeding::new(seed);
    let router_ms = router_start.elapsed().as_secs_f64() * 1000.0;

    // A square spiralling out from the origin, so the sample is not one
    // biome's worth of terrain.
    let side = (count as f64).sqrt().ceil() as i32;
    let mut done = 0u32;
    let mut blocks = 0u64;

    // Split, because the two are very different work and guessing which
    // dominates has been wrong twice. Setup builds the flat caches and every
    // interpolator's cell lattice; fill is the per-block loop over the
    // sampler, aquifer and ore veins.
    let mut clone_secs = 0.0;
    let mut setup_secs = 0.0;
    let mut fill_secs = 0.0;

    let start = Instant::now();
    for cz in 0..side {
        for cx in 0..side {
            if done == count {
                break;
            }
            let clone_start = Instant::now();
            let inputs = clone_inputs(&router, &seeding);
            clone_secs += clone_start.elapsed().as_secs_f64();

            let setup_start = Instant::now();
            let fill = NoiseFill::new(
                inputs,
                FillSettings::overworld(),
                cx - side / 2,
                cz - side / 2,
            );
            setup_secs += setup_start.elapsed().as_secs_f64();

            let fill_start = Instant::now();
            let chunk = fill.fill();
            fill_secs += fill_start.elapsed().as_secs_f64();

            // Touch the result so the fill cannot be optimised away.
            blocks += u64::from(chunk.get(0, 0, 0) != chunk.get(15, 100, 15));
            done += 1;
        }
    }
    let elapsed = start.elapsed().as_secs_f64();

    println!("router build      {router_ms:.1} ms (once per world)");
    println!("chunks            {done}");
    println!("total             {:.3} s", elapsed);
    println!(
        "per chunk         {:.2} ms",
        elapsed * 1000.0 / f64::from(done)
    );
    println!(
        "throughput        {:.1} chunks/s",
        f64::from(done) / elapsed
    );
    println!(
        "                  {:.1} M blocks/s",
        f64::from(done) * 98304.0 / elapsed / 1e6
    );
    println!(
        "  graph clone     {:.2} ms/chunk ({:.0}%) — pure overhead, vanilla has none",
        clone_secs * 1000.0 / f64::from(done),
        100.0 * clone_secs / elapsed
    );
    println!(
        "  setup           {:.2} ms/chunk ({:.0}%) — flat caches + cell lattices",
        setup_secs * 1000.0 / f64::from(done),
        100.0 * setup_secs / elapsed
    );
    println!(
        "  fill            {:.2} ms/chunk ({:.0}%) — per-block sampler + aquifer + veins",
        fill_secs * 1000.0 / f64::from(done),
        100.0 * fill_secs / elapsed
    );
    // Ablations, so the fill half is split by measurement rather than by
    // guesswork. Each variant is a real vanilla setting, not a benchmark
    // hack: several dimensions ship with aquifers or ore veins off.
    let full = ablate(&router, &seeding, FillSettings::overworld(), done);
    let no_veins = ablate(
        &router,
        &seeding,
        FillSettings {
            ore_veins_enabled: false,
            ..FillSettings::overworld()
        },
        done,
    );
    let no_aquifer = ablate(
        &router,
        &seeding,
        FillSettings {
            aquifers_enabled: false,
            ..FillSettings::overworld()
        },
        done,
    );
    let per = |secs: f64| secs * 1000.0 / f64::from(done);
    println!("fill ablation (ms/chunk, difference = what the feature costs in place)");
    println!("  full                    {:.2}", per(full));
    println!(
        "  without ore veins       {:.2}   (veins cost {:.2})",
        per(no_veins),
        per(full - no_veins)
    );
    println!(
        "  without the aquifer     {:.2}   (aquifer costs {:.2})",
        per(no_aquifer),
        per(full - no_aquifer)
    );
    println!(
        "  residual                {:.2}   (sampler + outer graph + writes)",
        per(no_aquifer - (full - no_veins).max(0.0))
    );
    println!("(noise stage only: density, aquifer, ore veins, fill — no surfaces,");
    println!(" carvers, features or structures, and no disk IO)");
    // Reported so the loop is visibly not dead code.
    eprintln!("(sanity: {blocks} chunks had differing sample blocks)");
    Ok(())
}

fn region(seed: &str, radius: &str, out: &Path) -> Result<(), String> {
    let seed: i64 = seed.parse().map_err(|e| format!("bad seed: {e}"))?;
    let radius: i32 = radius.parse().map_err(|e| format!("bad radius: {e}"))?;
    let noises = noise_parameters()?;
    let router = OverworldRouter::new(seed, &noises, false);
    let seeding = NoiseSeeding::new(seed);

    let mut columns = Vec::new();
    for cz in -radius..=radius {
        for cx in -radius..=radius {
            let chunk = generate(&router, &seeding, cx, cz);
            columns.push(to_chunk_column(&chunk, "minecraft:stone", None));
        }
    }

    let region_dir: PathBuf = out.join("region");
    rustmc_world::anvil::write_region(&region_dir, &columns)
        .map_err(|e| format!("writing regions to {}: {e}", region_dir.display()))?;
    println!("wrote {} chunks to {}", columns.len(), region_dir.display());
    println!("status is minecraft:noise — no surfaces, carvers or features yet");
    Ok(())
}

/// How chunk generation scales with threads.
///
/// Chunks are independent — nothing in a `NoiseFill` outlives it and the
/// router is immutable — so this is close to the cleanest parallelism a
/// server ever gets. It is also the one thing the per-chunk numbers above
/// say nothing about, and the reason a 24-thread box was worth setting up.
///
/// Each worker clones the router's graphs for itself, which is the 7% the
/// benchmark reports as `graph clone`. Removing that needs the graph to be
/// numbered once per world rather than once per chunk; until then it is a
/// per-thread constant, not a shared bottleneck.
///
/// Reported as **speedup against one thread on this same machine**, not
/// against vanilla. Vanilla parallelises chunk generation too, so a
/// single-threaded vanilla number next to a 24-thread rustmc one would be
/// meaningless.
fn scale(seed: &str, chunks: &str, max_threads: Option<&str>) -> Result<(), String> {
    let seed: i64 = seed.parse().map_err(|e| format!("bad seed: {e}"))?;
    let count: u32 = chunks
        .parse()
        .map_err(|e| format!("bad chunk count: {e}"))?;
    if count == 0 {
        return Err("nothing to generate".into());
    }
    let available = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1);
    let cap: usize = match max_threads {
        Some(v) => v.parse().map_err(|e| format!("bad thread count: {e}"))?,
        None => available,
    };

    let noises = noise_parameters()?;
    let router = OverworldRouter::new(seed, &noises, false);
    let seeding = NoiseSeeding::new(seed);

    println!("{count} chunks, seed {seed}, {available} hardware threads available");
    println!("threads   total      chunks/s    speedup   efficiency");

    let mut baseline = 0.0_f64;
    let mut threads = 1usize;
    loop {
        let elapsed = run_parallel(&router, &seeding, count, threads);
        let rate = f64::from(count) / elapsed;
        if threads == 1 {
            baseline = rate;
        }
        let speedup = rate / baseline;
        #[allow(clippy::cast_precision_loss)]
        let efficiency = 100.0 * speedup / threads as f64;
        println!(
            "{threads:>7}   {elapsed:>6.3} s   {rate:>8.1}    {speedup:>6.2}x   {efficiency:>5.0}%"
        );
        if threads >= cap {
            break;
        }
        threads = (threads * 2).min(cap);
    }
    println!("(speedup is against one thread on this machine, not against vanilla —");
    println!(" vanilla parallelises chunk generation too)");
    Ok(())
}

/// Generate `count` chunks across `threads` workers; returns wall-clock time.
fn run_parallel(
    router: &OverworldRouter,
    seeding: &NoiseSeeding,
    count: u32,
    threads: usize,
) -> f64 {
    let side = (f64::from(count).sqrt().ceil()) as i32;
    let next = std::sync::atomic::AtomicU32::new(0);
    let sink = std::sync::atomic::AtomicU64::new(0);

    let start = Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                let mut local = 0u64;
                loop {
                    // A shared counter rather than a fixed split: chunks
                    // differ in cost (an ocean column exits the
                    // aquifer early, a mountain does not), so a static partition
                    // would leave threads idle at the end.
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if index >= count {
                        break;
                    }
                    let cx = (index as i32) % side - side / 2;
                    let cz = (index as i32) / side - side / 2;
                    let chunk = generate(router, seeding, cx, cz);
                    local += u64::from(chunk.get(0, 0, 0) != chunk.get(15, 100, 15));
                }
                sink.fetch_add(local, std::sync::atomic::Ordering::Relaxed);
            });
        }
    });
    let elapsed = start.elapsed().as_secs_f64();
    std::hint::black_box(sink.load(std::sync::atomic::Ordering::Relaxed));
    elapsed
}

/// Generate through every stage and write the region files.
///
/// Only the **inner** square is written. The pipeline generates a one-chunk
/// margin so that features reaching across a border land, and those margin
/// chunks are themselves missing their own neighbours' features — writing
/// them would put a ring of half-decorated chunks in the output and any diff
/// taken over them would be measuring the margin.
/// Which dimension `full` generates. Not `rustmc_data::Dimension`: this tool
/// does not depend on that crate, and the end is not reachable from here.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dimension {
    Overworld,
    Nether,
    End,
}

fn full(
    seed: &str,
    radius: &str,
    out: &Path,
    center_x: Option<&str>,
    center_z: Option<&str>,
    dimension: Dimension,
) -> Result<(), String> {
    use rustmc_worldgen::pipeline::{Area, Through};

    let seed: i64 = seed.parse().map_err(|e| format!("bad seed: {e}"))?;
    let radius: i32 = radius.parse().map_err(|e| format!("bad radius: {e}"))?;
    let cx: i32 = center_x
        .unwrap_or("0")
        .parse()
        .map_err(|e| format!("bad center x: {e}"))?;
    let cz: i32 = center_z
        .unwrap_or("0")
        .parse()
        .map_err(|e| format!("bad center z: {e}"))?;

    let area = Area::square(cx, cz, radius);
    // The occupancy trace covers this whole function, not just the generator:
    // building the columns and writing the region files happens *after*
    // `generate_decorating` returns, so every stage number is blind to it and
    // an idle tail there is invisible to `RUSTMC_STAGE_CPU`. Off unless
    // `RUSTMC_OCCUPANCY_MS` is set.
    let _occupancy = worldgen_assembly::occupancy::Occupancy::start("worldgen-dump full");
    let started = Instant::now();
    // `RUSTMC_DECORATE_ONLY=x,z` decorates that chunk and no other, while
    // every earlier stage still runs over the whole area. That is what
    // `DecorateGolden` measures — one chunk against neighbours which are
    // generated but have not decorated — and without it the two sides are
    // answering different questions.
    let decorate_only = std::env::var("RUSTMC_DECORATE_ONLY").ok().and_then(|v| {
        let (x, z) = v.split_once(',')?;
        Some((x.trim().parse().ok()?, z.trim().parse().ok()?))
    });
    // `RUSTMC_THROUGH=noise|surface|carvers|decorated` stops the pipeline at a
    // stage. Defaults to `decorated`, which is what this subcommand has always
    // produced.
    //
    // Needed because a finished world cannot answer "is our *terrain* the same
    // as the oracle's" — decoration has written over it. Comparing a
    // carver-stage world against `SurfaceChunkGolden`'s pre-decoration dump is
    // the only way to separate a terrain difference from a feature placing
    // differently on identical terrain, and this session spent a long time
    // unable to make that distinction.
    let through = match std::env::var("RUSTMC_THROUGH").ok().as_deref() {
        Some("noise") => Through::Noise,
        // `biomes` exists so a profile can be taken of noise+biomes and
        // differenced against a noise-only one: the stages run in sequence and
        // a flat `perf` profile cannot otherwise separate them.
        Some("biomes") => Through::Biomes,
        Some("surface") => Through::Surface,
        Some("carvers") => Through::Carvers,
        None | Some("decorated") => Through::Decoration,
        Some(other) => return Err(format!("unknown RUSTMC_THROUGH stage `{other}`")),
    };
    let world = match dimension {
        Dimension::Overworld => {
            worldgen_assembly::generate::generate_decorating(seed, area, through, decorate_only)?
        }
        // `RUSTMC_THROUGH` defaults to `decorated`, which the nether path
        // cannot run. Clamp it to the furthest stage that exists rather than
        // erroring: the whole point of the subcommand is to produce a world
        // to measure, and a caller who asked for a later stage is told what
        // they got by the caveats.
        Dimension::Nether => worldgen_assembly::generate::generate_nether(seed, area, through)?,
        // Same clamping rule as the nether: the End path runs through the
        // surface stage and no further, so a caller who asked for
        // `decorated` is told what they got by the caveats instead of being
        // refused a world.
        Dimension::End => worldgen_assembly::generate::generate_end(seed, area, through)?,
    };
    let elapsed = started.elapsed();

    // Heightmaps go in the file when the block-property table is available.
    // Vanilla writes them, so a column that carries them is directly
    // diffable against a vanilla region — and they record stage history the
    // blocks have forgotten, which is how a mis-sited disk_sand was traced
    // back to a pre-carve terrain difference. Without the table they are
    // omitted rather than guessed: a *wrong* heightmap is worse than none,
    // because it looks authoritative.
    let columns: Vec<_> = area
        .chunks()
        .map(|(x, z)| {
            // 4000 was hardcoded here, a third value alongside the bridge's
            // 4325 and vanilla's real 4903. This is the dump the region
            // differ measures parity from, so it is also the one most likely
            // to be handed to vanilla.
            let mut column = rustmc_worldgen::anvil_bridge::column_from_canvas(
                &world.canvas,
                x,
                z,
                "minecraft:full",
                rustmc_worldgen::anvil_bridge::DATA_VERSION_26_2,
            );
            column.heightmaps = worldgen_assembly::heightmaps_from_canvas(&world.canvas, x, z);
            column
        })
        .collect();

    let region_dir = out.join("region");
    rustmc_world::anvil::write_region(&region_dir, &columns)
        .map_err(|e| format!("writing {}: {e}", region_dir.display()))?;

    println!(
        "wrote {} chunks to {} in {:.1}s",
        columns.len(),
        region_dir.display(),
        elapsed.as_secs_f64()
    );
    // The entities generation added (structure mobs, the ship's frame), into
    // `entities/`, the separate region store vanilla keeps them in. One chunk
    // record per chunk of the area, an empty list included, as vanilla writes
    // one for every chunk it has loaded. Only with the channel on.
    if std::env::var("RUSTMC_RECORD_BLOCK_ENTITIES").as_deref() == Ok("1") {
        let chunks: Vec<_> = area
            .chunks()
            .map(|(x, z)| rustmc_world::entitystore::EntityChunk {
                chunk_x: x,
                chunk_z: z,
                data_version: rustmc_worldgen::anvil_bridge::DATA_VERSION_26_2,
                entities: world.canvas.entity_records(x, z).to_vec(),
            })
            .collect();
        let count: usize = chunks.iter().map(|c| c.entities.len()).sum();
        let entity_dir = out.join("entities");
        rustmc_world::entitystore::write_region_merging(&entity_dir, &chunks)
            .map_err(|e| format!("writing {}: {e}", entity_dir.display()))?;
        println!("ENTITIES {count} written to {}", entity_dir.display());
    }
    // The block-entity emit, including what it THREW AWAY.
    //
    // `docs/DETERMINISM.md` §42.5 requires that a position needing a payload
    // and having none emits nothing and is counted. The counting existed one
    // commit before this line and nothing read it, which is the same defect as
    // a check that cannot go red: `column_from_canvas` computed the stats and
    // dropped them. Printed unconditionally, because the useful readings are
    // the surprising ones — `recorded 0` on a run with the channel on says the
    // producers never fired, and `orphaned` far above `recorded` says the
    // channel is keyed to positions no final block occupies. Neither is
    // distinguishable from a healthy run without these four numbers.
    let be = rustmc_worldgen::block_entity::take_emit_totals();
    println!(
        "BLOCK ENTITIES emitted {} (derived {} recorded {}), dropped: orphaned {} incomplete {}",
        be.derived + be.recorded,
        be.derived,
        be.recorded,
        be.orphaned,
        be.incomplete
    );
    if be.incomplete > 0 {
        println!(
            "  {} position(s) had a loot table and no seed behind it, so they emitted",
            be.incomplete
        );
        println!("  nothing rather than a payload wrong on one key — §43.5. The fix is to");
        println!("  thread the piece's RandomSource into structure::place::place_piece, and");
        println!("  RUSTMC_JIGSAW_STREAM=1 does exactly that for the jigsaw call site —");
        println!("  villages and trial chambers. §44 has what it costs in block states.");
    }
    // The caveats are printed with the output, every time, because a world
    // written to disk outlives the terminal that made it.
    println!("CAVEATS (this world is not vanilla-complete):");
    for caveat in world.caveats() {
        println!("  - {caveat}");
    }
    if !world.refused.is_empty() {
        let mut refusals: Vec<_> = world.refused.iter().collect();
        refusals.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
        // HOW MANY TO PRINT, and why this is not `take(10)` any more.
        //
        // It was, and the cap hid families from the overworld biome census on
        // the very windows that census exists to look at: an r=7 square in a
        // jungle spends its ten lines on `large_dripstone` and four tree
        // configs and never mentions `minecraft:bamboo`, and a reader who
        // takes the list as the answer concludes the port declines five
        // things there when it declines eight. A truncated tally reads
        // exactly like a complete one — there is no ellipsis in a count.
        //
        // So the cap is now the caller's, the default is generous, and the
        // line that follows says what was left out rather than leaving the
        // reader to wonder.
        let cap = std::env::var("RUSTMC_REFUSED_LINES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(40);
        let total: usize = refusals.iter().map(|(_, n)| **n).sum();
        println!(
            "REFUSED (features the port declined, and how often): \
             {} distinct, {total} declines in total",
            refusals.len()
        );
        for (reason, count) in refusals.iter().take(cap) {
            println!("  {count:5}x {reason}");
        }
        if refusals.len() > cap {
            let hidden: usize = refusals.iter().skip(cap).map(|(_, n)| **n).sum();
            println!(
                "  ... and {} more distinct reasons, {hidden} declines, \
                 not shown — RUSTMC_REFUSED_LINES raises the cap",
                refusals.len() - cap
            );
        }
    }
    Ok(())
}

/// `worldgen-dump nether-density <seed> <x> <z> <y0> <y1>` — our nether
/// `finalDensity` up one column, printed to line up with
/// `ci/nether-density-probe.sh`.
///
/// # Why this is a separate subcommand and not part of `column`
///
/// `column` prints *blocks*, stage by stage, and blocks are exactly what
/// stopped being informative about the nether. The residual there is
/// order-invariant, lives in the noise stage, and the three hypotheses left —
/// the density crosses zero at the wrong `y`, has the wrong sign, or is
/// merely offset — produce the same blocks and different numbers.
///
/// # It is deliberately the UN-interpolated value
///
/// [`FunctionContext::new`] leaves the interpolated-marker slice empty, which
/// is the port's spelling of vanilla's
/// `if (context != NoiseChunk.this) return noiseFiller.compute(context)`: an
/// `Interpolated` marker sampled outside a chunk fill just computes its
/// wrapped function. Vanilla's `SinglePointContext` does the same thing, so
/// the two sides are comparing the same quantity — the value the cell lattice
/// is built from, not the trilinear blend the filler uses.
///
/// So the comparison splits the remaining question in two: if the columns
/// agree the defect is the interpolation lattice, and if they differ it is
/// upstream of it.
///
/// # Errors
/// If an argument does not parse or the nether noise parameters are missing.
fn nether_density(seed: &str, x: &str, z: &str, y0: &str, y1: &str) -> Result<(), String> {
    let seed: i64 = seed.parse().map_err(|e| format!("seed: {e}"))?;
    let x: i32 = x.parse().map_err(|e| format!("x: {e}"))?;
    let z: i32 = z.parse().map_err(|e| format!("z: {e}"))?;
    let y0: i32 = y0.parse().map_err(|e| format!("y0: {e}"))?;
    let y1: i32 = y1.parse().map_err(|e| format!("y1: {e}"))?;

    let noises = worldgen_assembly::nether_noises()?;
    let router = rustmc_worldgen::density::router::OverworldRouter::nether(seed, &noises);

    // `worldgen/noise_settings/nether.json`: `size_vertical: 2` over a
    // 4-block base, and `min_y: 0`.
    const CELL_HEIGHT: i32 = 8;
    const MIN_Y: i32 = 0;

    println!("SEED {seed} COLUMN {x},{z} cell_height={CELL_HEIGHT} min_y={MIN_Y}");
    for y in y0..=y1 {
        let corner = (y - MIN_Y).rem_euclid(CELL_HEIGHT) == 0;
        let value = router
            .final_density
            .compute(rustmc_worldgen::density::FunctionContext::new(x, y, z));
        println!(
            "DENSITY y={y}{} value={value} bits={:x} solid={}",
            if corner { " CORNER" } else { "       " },
            value.to_bits(),
            value > 0.0,
        );
    }
    Ok(())
}

/// `worldgen-dump wg-heightmaps <seed> <cx> <cz> [centre-x centre-z]` — our
/// canvas `WORLD_SURFACE_WG` and `OCEAN_FLOOR_WG` for one chunk, all 256
/// columns, printed to line up with `ci/nether-real-fill-probe.sh --baseheight`.
///
/// # Why this exists
///
/// The structures stage reads those two maps off the canvas, which is what
/// stops it moving above the noise stage that produces them. Vanilla reads
/// `ChunkGenerator.getBaseHeight`, which walks a noise column against the
/// router and needs no filled chunk. Swapping our five canvas reads for
/// `getBaseHeight` is the unblock, and this is one half of the evidence that
/// the swap changes nothing: the other half is the probe above, and the two
/// must agree column for column BEFORE any beard exists.
///
/// After the beard exists they genuinely differ, because `iterateNoiseColumn`
/// builds its `NoiseChunk` with `BeardifierMarker.INSTANCE` — vanilla's
/// structure placement never sees the terrain its own structures raise. So
/// this gate has an expiry date and it is the reason the swap must land first.
///
/// # Errors
/// If an argument does not parse or generation fails.
fn wg_heightmaps(
    seed: &str,
    chunk_x: &str,
    chunk_z: &str,
    centre_x: Option<&str>,
    centre_z: Option<&str>,
) -> Result<(), String> {
    let seed: i64 = seed.parse().map_err(|e| format!("seed: {e}"))?;
    let chunk_x: i32 = chunk_x.parse().map_err(|e| format!("cx: {e}"))?;
    let chunk_z: i32 = chunk_z.parse().map_err(|e| format!("cz: {e}"))?;
    let cx = centre_x
        .map_or(Ok(chunk_x), str::parse)
        .map_err(|e| format!("centre-x: {e}"))?;
    let cz = centre_z
        .map_or(Ok(chunk_z), str::parse)
        .map_err(|e| format!("centre-z: {e}"))?;

    // Radius 2 around the centre: the fossils are a property of the chunk's
    // own noise column, so the smallest area that generates the target chunk
    // is enough, and a bigger one would only cost time.
    let area = rustmc_worldgen::pipeline::Area::square(cx, cz, 2);
    let world = worldgen_assembly::generate::generate(
        seed,
        area,
        rustmc_worldgen::pipeline::Through::Noise,
    )?;
    // `RUSTMC_WG_ROUTER=1` answers from `stage::noise::base_height` — the
    // router walk that needs no filled chunk — instead of the canvas fossil.
    // Same format either way, so the two runs `diff` directly, and that diff
    // is the gate for swapping the structures stage's five canvas reads.
    let router_mode = std::env::var_os("RUSTMC_WG_ROUTER").is_some();
    let router_bits = if router_mode {
        let params = worldgen_assembly::noise_parameters();
        let noises = worldgen_assembly::overworld_noises(&params);
        let router = rustmc_worldgen::density::router::OverworldRouter::new(seed, &noises, false);
        let seeding = rustmc_worldgen::noise::NoiseSeeding::new(seed);
        Some((router, seeding))
    } else {
        None
    };

    println!("SEED {seed} CHUNK {chunk_x},{chunk_z} BASEHEIGHT");
    for lx in 0..16 {
        for lz in 0..16 {
            let x = chunk_x * 16 + lx;
            let z = chunk_z * 16 + lz;
            let (ws, of) = match &router_bits {
                Some((router, seeding)) => {
                    let settings = rustmc_worldgen::chunk_gen::FillSettings::overworld();
                    (
                        rustmc_worldgen::stage::noise::base_height(
                            router,
                            seeding,
                            settings,
                            x,
                            z,
                            rustmc_worldgen::chunk_gen::Wg::WorldSurface,
                        ),
                        rustmc_worldgen::stage::noise::base_height(
                            router,
                            seeding,
                            settings,
                            x,
                            z,
                            rustmc_worldgen::chunk_gen::Wg::OceanFloor,
                        ),
                    )
                }
                None => (
                    world.canvas.world_surface_wg(x, z),
                    world.canvas.ocean_floor_wg(x, z),
                ),
            };
            println!("BASEHEIGHT {x} {z} world_surface_wg={ws} ocean_floor_wg={of}");
        }
    }
    Ok(())
}

/// `trial-chamber-oracle` — our trial chamber walk against vanilla's own
/// saved `Children[]`, field by field.
///
/// # Why a comparison command and not two listings
///
/// `nether-fossil-starts` prints ours in vanilla's format so a human can diff
/// the two. That works for a one-piece structure. A trial chamber is 220-240
/// pieces, and a text diff of two 240-line listings turns every real finding
/// into an argument about column alignment. This compares the four decisions
/// that a block diff cannot separate — **which element, which rotation, which
/// position, and the ground level delta** — plus the bounding box, and reports
/// the first mismatches with both sides spelled out.
///
/// It needs no generated world on our side and no JVM on either: vanilla's
/// answer is read out of the region files it already wrote, and ours comes
/// from `trial_chamber_report`, which builds its own router.
///
/// A structure vanilla placed and we did not is reported as a missing start
/// rather than skipped — **the denominator counts vanilla's starts**, so a
/// port that generates nothing scores 0 of 3 and not 0 of 0.
fn trial_chamber_oracle(
    seed: &str,
    vanilla_dir: &Path,
    radius: Option<&String>,
    center_x: Option<&String>,
    center_z: Option<&String>,
) -> Result<(), String> {
    jigsaw_oracle(
        "TRIAL CHAMBER ORACLE",
        "trial_chambers",
        "trial chamber",
        worldgen_assembly::generate::trial_chamber_report,
        seed,
        vanilla_dir,
        radius,
        center_x,
        center_z,
    )
}

/// `village-oracle` — our village walk against vanilla's own saved
/// `Children[]`, on exactly the harness the chambers get.
///
/// Shares [`jigsaw_oracle`] with the chamber command rather than copying it.
/// That is not tidiness: the chamber harness carries two hard-won rules — the
/// denominator counts **vanilla's** starts so a port that generates nothing
/// scores 0 and not 0/0, and a start of ours outside the reference's own
/// generated square is *unadjudicated* rather than SPURIOUS — and a
/// second copy would have had to relearn both.
fn village_oracle(
    seed: &str,
    vanilla_dir: &Path,
    radius: Option<&String>,
    center_x: Option<&String>,
    center_z: Option<&String>,
) -> Result<(), String> {
    jigsaw_oracle(
        "VILLAGE ORACLE",
        "village",
        "village",
        worldgen_assembly::generate::village_report,
        seed,
        vanilla_dir,
        radius,
        center_x,
        center_z,
    )
}

/// The shared body. `id_filter` is matched with `contains` against the saved
/// start id, so `village` reaches all five variants and would also catch a
/// variant vanilla placed and we chose differently — which is the failure this
/// is here to find, not one to filter out.
#[expect(
    clippy::too_many_arguments,
    reason = "one call site per structure; splitting it would only move the arguments"
)]
fn jigsaw_oracle(
    label: &str,
    id_filter: &str,
    noun: &str,
    report: fn(
        i64,
        rustmc_worldgen::pipeline::Area,
    ) -> Result<Vec<worldgen_assembly::generate::Village>, String>,
    seed: &str,
    vanilla_dir: &Path,
    radius: Option<&String>,
    center_x: Option<&String>,
    center_z: Option<&String>,
) -> Result<(), String> {
    let seed: i64 = seed.parse().map_err(|_| "seed must be an integer")?;
    let radius: i32 = radius
        .map_or(Ok(16), |v| v.parse())
        .map_err(|_| "radius must be an integer")?;
    let cx: i32 = center_x
        .map_or(Ok(64), |v| v.parse())
        .map_err(|_| "bad center-x")?;
    let cz: i32 = center_z
        .map_or(Ok(64), |v| v.parse())
        .map_err(|_| "bad center-z")?;

    // Vanilla's side.
    let chunks = load_full(vanilla_dir)?;
    let mut theirs: Vec<&rustmc_world::anvil::StructureStart> = Vec::new();
    for chunk in chunks.values() {
        for start in &chunk.structure_starts {
            if start.id.contains(id_filter) {
                theirs.push(start);
            }
        }
    }
    theirs.sort_by_key(|s| (s.chunk_x, s.chunk_z));

    // Ours.
    let area = rustmc_worldgen::pipeline::Area::square(cx, cz, radius);
    let mut ours = report(seed, area)?;
    ours.sort_by_key(|v| (v.start_chunk.0, v.start_chunk.1));

    println!(
        "{label} seed={seed} centre=({cx},{cz}) r={radius}  \
         vanilla starts={} ours={}",
        theirs.len(),
        ours.len()
    );

    let mut matched_starts = 0usize;
    let mut total_pieces = 0usize;
    let mut matched_pieces = 0usize;
    // Feature elements whose `feature` key was not a plain id string, so this
    // reader could not name what vanilla placed. Reported, never passed.
    let mut inline_features = 0usize;
    for start in &theirs {
        let Some(mine) = ours
            .iter()
            .find(|v| (v.start_chunk.0, v.start_chunk.1) == (start.chunk_x, start.chunk_z))
        else {
            println!(
                "  MISSING start at chunk ({}, {}) — vanilla has {} pieces, we produced none",
                start.chunk_x,
                start.chunk_z,
                start.pieces.len()
            );
            total_pieces += start.pieces.len();
            continue;
        };
        // `JIGSAW_ORACLE_ALL=1` lists our whole walk, so a divergence can be
        // read against vanilla's saved list rather than only at its first row.
        if std::env::var_os("JIGSAW_ORACLE_ALL").is_some() {
            for (i, u) in mine.pieces.iter().enumerate() {
                println!(
                    "  ours {i} {} rot={} pos={:?} bb=[{}, {}, {}, {}, {}, {}] gld={}",
                    u.element.describe(),
                    rotation_name(u.rotation),
                    u.pos,
                    u.bbox.min_x,
                    u.bbox.min_y,
                    u.bbox.min_z,
                    u.bbox.max_x,
                    u.bbox.max_y,
                    u.bbox.max_z,
                    u.ground_level_delta
                );
            }
        }
        let mut bad = 0usize;
        let n = start.pieces.len().max(mine.pieces.len());
        total_pieces += start.pieces.len();
        for i in 0..n {
            let them = start.pieces.get(i);
            let us = mine.pieces.get(i);
            match (them, us) {
                (Some(t), Some(u)) => {
                    // **Ask each element kind for the field vanilla actually
                    // writes about it.** A `feature_pool_element` has no
                    // `location`, so `pool_element` is `None` on every one of
                    // them; comparing our feature id against that `None` read
                    // all 14 feature elements of the (46,77) savanna village
                    // — and all 8 of (81,77)'s — as divergences, on a walk
                    // that was reproducing them exactly. The element_type is
                    // compared too, so a feature element answered with a
                    // single one is still caught.
                    let t_kind = t.pool_element_type.as_deref().unwrap_or("-");
                    let (t_element, u_element, u_kind) = match &u.element.kind {
                        rustmc_worldgen::structure::jigsaw::ElementKind::Single {
                            location,
                            legacy,
                            ..
                        } => (
                            t.pool_element.as_deref().unwrap_or("-"),
                            location.as_str(),
                            if *legacy {
                                "minecraft:legacy_single_pool_element"
                            } else {
                                "minecraft:single_pool_element"
                            },
                        ),
                        rustmc_worldgen::structure::jigsaw::ElementKind::Feature { feature } => (
                            // `None` here is an inline placed feature, which
                            // this reader cannot flatten to an id. Counted as
                            // unadjudicated below rather than silently passed.
                            t.pool_element_feature.as_deref().unwrap_or("?"),
                            feature.as_str(),
                            "minecraft:feature_pool_element",
                        ),
                        rustmc_worldgen::structure::jigsaw::ElementKind::Empty => (
                            t.pool_element.as_deref().unwrap_or("-"),
                            "-",
                            "minecraft:empty_pool_element",
                        ),
                        // Vanilla saves no `location` for a list element.
                        rustmc_worldgen::structure::jigsaw::ElementKind::List { .. } => (
                            t.pool_element.as_deref().unwrap_or("-"),
                            "-",
                            "minecraft:list_pool_element",
                        ),
                    };
                    if t_element == "?" {
                        inline_features += 1;
                    }
                    let t_rot = t.rotation.as_deref().unwrap_or("-");
                    let u_rot = rotation_name(u.rotation);
                    let t_pos = t.pos.unwrap_or([0, 0, 0]);
                    let u_pos = [u.pos.0, u.pos.1, u.pos.2];
                    let t_gld = t.ground_level_delta.unwrap_or(0);
                    let t_bb = t.bounding_box;
                    let u_bb = [
                        u.bbox.min_x,
                        u.bbox.min_y,
                        u.bbox.min_z,
                        u.bbox.max_x,
                        u.bbox.max_y,
                        u.bbox.max_z,
                    ];
                    let same = t_element == u_element
                        && t_kind == u_kind
                        && t_rot == u_rot
                        && t_pos == u_pos
                        && t_gld == u.ground_level_delta
                        && t_bb == u_bb;
                    if same {
                        matched_pieces += 1;
                    } else {
                        bad += 1;
                        if bad <= 6 {
                            let (sx, sz) = (start.chunk_x, start.chunk_z);
                            let u_gld = u.ground_level_delta;
                            println!(
                                "  chunk ({sx}, {sz}) piece {i}:\n    \
                                 vanilla {t_element} [{t_kind}] rot={t_rot} pos={t_pos:?} \
                                 gld={t_gld} bb={t_bb:?}\n    \
                                 ours    {u_element} [{u_kind}] rot={u_rot} pos={u_pos:?} \
                                 gld={u_gld} bb={u_bb:?}"
                            );
                        }
                    }
                }
                (Some(t), None) => {
                    bad += 1;
                    if bad <= 6 {
                        println!(
                            "  chunk ({}, {}) piece {i}: vanilla has {}, we ran out",
                            start.chunk_x,
                            start.chunk_z,
                            t.pool_element.as_deref().unwrap_or("-")
                        );
                    }
                }
                (None, Some(_)) => {
                    bad += 1;
                    if bad <= 6 {
                        println!(
                            "  chunk ({}, {}) piece {i}: we produced a piece vanilla did not",
                            start.chunk_x, start.chunk_z
                        );
                    }
                }
                (None, None) => {}
            }
        }
        if bad == 0 && start.pieces.len() == mine.pieces.len() {
            matched_starts += 1;
            println!(
                "  OK chunk ({}, {}) — {} of {} pieces identical",
                start.chunk_x,
                start.chunk_z,
                mine.pieces.len(),
                start.pieces.len()
            );
        } else {
            println!(
                "  BAD chunk ({}, {}) — {bad} differing, vanilla {} pieces, ours {}",
                start.chunk_x,
                start.chunk_z,
                start.pieces.len(),
                mine.pieces.len()
            );
        }
    }
    // A start of ours that vanilla does not have is as much a defect as a
    // missing one, and it would otherwise never appear: the loop above walks
    // vanilla's list.
    //
    // **But only inside the reference's own harvested extent.** Our search
    // reaches past the requested area, because a chamber's pieces span 116
    // blocks and a village's 80, and one starting outside can still write
    // inside. Vanilla
    // generated those chambers too — their `structures.starts` simply sit in
    // chunks nobody harvested, so the reference cannot speak to them either
    // way. Reporting them as SPURIOUS said "we invented four chambers" about a
    // seed where the truth was "the reference stops at chunk 80", which is the
    // denominator trap in its other direction: a comparison must not count
    // against you what its reference never looked at.
    // The bound is the **generated square**, `centre +/- radius`, and not the
    // extent of the region files. Those are not the same thing and the
    // difference is not academic: a reference harvested over chunks 48..80
    // ships region files covering 6..82, because a region file is 32x32 chunks
    // and the harvest does not align to one. The extra chunks exist on disk
    // and carry no structure starts, having never been force-loaded.
    //
    // Bounding by the file extent therefore accused this port of inventing a
    // chamber at chunk (48,42) on seed 12345 — and a reference generated
    // *centred on that chunk* then found vanilla's own, 209 pieces, identical
    // to ours. The file extent is what the reference *contains*; the square is
    // what it *knows*.
    let (rx0, rz0, rx1, rz1) = (cx - radius, cz - radius, cx + radius, cz + radius);
    let mut unadjudicated = 0usize;
    for mine in &ours {
        let (mx, mz) = mine.start_chunk;
        if theirs.iter().any(|s| (s.chunk_x, s.chunk_z) == (mx, mz)) {
            continue;
        }
        if mx < rx0 || mx > rx1 || mz < rz0 || mz > rz1 {
            unadjudicated += 1;
            println!(
                "  OUTSIDE REFERENCE chunk ({mx}, {mz}) — we produced {} pieces; the \
                 reference was generated over chunks {rx0}..{rx1} by {rz0}..{rz1} and \
                 cannot judge it",
                mine.pieces.len()
            );
            continue;
        }
        println!(
            "  SPURIOUS start at chunk ({mx}, {mz}) — we produced {} pieces, vanilla has none",
            mine.pieces.len()
        );
    }
    if unadjudicated > 0 {
        println!(
            "  ({unadjudicated} of our starts lie outside the reference and are neither \
             confirmed nor refuted here — point a reference at them to settle it)"
        );
    }
    if inline_features > 0 {
        println!(
            "  ({inline_features} feature elements carry an inline placed feature rather than \
             an id; this reader cannot name them, so their identity is UNADJUDICATED here \
             even where the row matched on every other field)"
        );
    }
    println!(
        "{label} {matched_starts}/{} starts, {matched_pieces}/{total_pieces} pieces",
        theirs.len()
    );
    if theirs.is_empty() {
        // Rule: a comparison whose denominator excludes what it cannot
        // generate is not a comparison. If the reference holds no chamber, the
        // run has measured nothing and must not read as a pass.
        println!("{label} VACUOUS — the reference holds no {noun}");
        return Err(format!("vacuous: no {noun} in the reference"));
    }
    Ok(())
}

/// The ramp `spreadNetherrack` indexes with `max(0, manhattan + offset)`.
const APRON_WEIGHTS: [f32; 14] = [
    1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.9, 0.9, 0.8, 0.7, 0.6, 0.4, 0.2,
];

/// One candidate stream position, scored against the blocks.
struct ApronTrial {
    leading: u32,
    offset: i32,
    scored: u32,
    wrong: u32,
    /// Positions the pass would have written that the blocks directory does
    /// not hold a chunk for. Reported, never charged.
    uncovered: u32,
}

/// `ruined-portal-apron <seed> <vanilla-region-dir> [blocks-dir]` — recover,
/// from vanilla's own netherrack, the draw index it entered `spreadNetherrack`
/// at. The draw counter this item was told to build before touching the code.
///
/// # Why a block diff could not answer this
///
/// `spreadNetherrack`'s first act is `nextInt(max(1, 8 - avgSpan / 2))`, and
/// that one integer sets the apron's radius: every position within Manhattan
/// `13 - offset` of the box centre spends a `nextDouble` and everything
/// further out is skipped without drawing. A port whose stream is one draw out
/// of step gets a different offset, so it spreads netherrack where vanilla has
/// none and leaves it out where vanilla has some — which is exactly what a
/// block diff reports for a wrong *rule* as well. The two are only separable
/// by counting draws.
///
/// # What makes the region file a draw counter
///
/// The pass reads nothing it has not already written, and everything it
/// writes is netherrack or magma:
///
/// * the walk order and the set of drawing positions are fixed by `offset`
///   alone, so a candidate `offset` predicts *which* draws happen;
/// * `canBlockBeReplacedByNetherrackOrMagma` refuses air, obsidian, lava and
///   `#features_cannot_replace` — and **both** blocks the pass writes are
///   replaceable, so a position's saved block answers the predicate the pass
///   asked, whether or not the pass wrote there;
/// * `placeNetherrackOrMagma` spends one `nextFloat` per block on a warm
///   portal and picks magma at 0.07, so the netherrack/magma *spelling* of
///   every apron block is a printout of that draw;
/// * `addNetherrackDripColumn` runs `i > 0 && nextFloat() < 0.5` and writes
///   unconditionally, so each column's **depth** is a unary printout of a run
///   of draws.
///
/// So a candidate leading-draw count `k` predicts every block the pass wrote,
/// and the wrong `k` disagrees with the saved world almost everywhere. The
/// answer is the `k` that scores zero.
///
/// Point `blocks-dir` at our own region with vanilla's piece list to validate
/// the instrument: the `k` it recovers there has to be the one our generator
/// actually spends. For a **ruined portal** that is unchanged by
/// `RUSTMC_JIGSAW_STREAM` — `ruined_portal.rs` is the one call site that has
/// always threaded its stream, and the knob only reaches the jigsaw one — so
/// the expected `k` here is still the count of containers the piece placed
/// before the pass, and 0 for a piece with none.
fn ruined_portal_apron(
    seed: &str,
    vanilla_dir: &Path,
    blocks_dir: Option<&Path>,
) -> Result<(), String> {
    use rustmc_worldgen::rng::worldgen_random::WorldgenRandom;
    use rustmc_worldgen::rng::xoroshiro::XoroshiroRandomSource;
    use rustmc_worldgen::rng::RandomSource;

    let seed: i64 = seed.parse().map_err(|e| format!("bad seed: {e}"))?;
    // How many leading draws to try. One per `RandomizableContainer` the
    // template places is the only thing known to spend the piece's stream
    // before the apron, and no portal template holds more than a handful.
    let search: u32 = std::env::var("RUSTMC_APRON_SEARCH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16);

    let refs = load_full(vanilla_dir)?;
    let blocks = match blocks_dir {
        Some(dir) => load_full(dir)?,
        None => BTreeMap::new(),
    };
    // **A `blocks-dir` that loaded nothing must not fall back to the
    // reference.** It did for one run of this tool, and the result was our
    // own world scoring a perfect zero because the tool was reading vanilla's
    // blocks: `load_full` walks `*.mca` in the directory it is given, and a
    // `worldgen-dump full` output puts them one level down in `region/`. A
    // silent fallback turns the argument that names what is being measured
    // into a no-op, which is the failure this project keeps paying for.
    if let Some(dir) = blocks_dir {
        if blocks.is_empty() {
            return Err(format!(
                "{} holds no full chunks — a `worldgen-dump full` output keeps \
                 them in its `region/` subdirectory, and this tool will not \
                 quietly measure the reference instead",
                dir.display()
            ));
        }
    }
    let world = if blocks.is_empty() { &refs } else { &blocks };
    let tags = worldgen_assembly::block_tags()?;
    let (step_table, _) = worldgen_assembly::structure_step_indices();
    let index = step_table
        .get("minecraft:ruined_portal")
        .map_or(10, |s| s.index);

    let (mut portals, mut resolved, mut refused, mut at_zero) = (0u32, 0u32, 0u32, 0u32);
    let mut recovered: BTreeMap<u32, u32> = BTreeMap::new();
    for chunk in refs.values() {
        for start in &chunk.structure_starts {
            if start.id != "minecraft:ruined_portal" {
                continue;
            }
            let Some(piece) = start.pieces.first() else {
                continue;
            };
            portals += 1;
            let b = piece.bounding_box;
            let (x_span, z_span) = (b[3] - b[0] + 1, b[5] - b[2] + 1);
            let (cx, cz) = (b[0] + x_span / 2, b[2] + z_span / 2);
            let bound = (8 - (x_span + z_span) / 2 / 2).max(1);
            let placement = piece.vertical_placement.clone().unwrap_or_default();
            let cold = piece.portal_cold.unwrap_or(false);
            let overgrown = piece.portal_overgrown.unwrap_or(false);
            let template = piece.template.clone().unwrap_or_default();
            println!(
                "APRON chunk ({}, {}) {template} {placement} centre ({cx},{cz}) \
                 span {x_span}x{z_span} -> nextInt({bound}) cold={cold} overgrown={overgrown}",
                start.chunk_x, start.chunk_z
            );

            // A surface portal's apron sits on `WORLD_SURFACE_WG`, which
            // vanilla does not save; the saved `WORLD_SURFACE` is the
            // post-decoration one and is not the same number. Rather than
            // guess a y that decides which positions spend draws, refuse.
            if placement == "on_land_surface" || placement == "on_ocean_floor" {
                println!("  REFUSED  a surface portal's apron y is WORLD_SURFACE_WG, not saved");
                refused += 1;
                continue;
            }
            // A buried portal decays at `min(box.minY, surface - 1)`, and
            // when the surface is far above that is `box.minY` exactly. The
            // margin is checked rather than assumed, because a y that is
            // wrong by one changes which positions are replaceable and
            // therefore how many draws the pass spends.
            let apron_y = b[1];
            let mut too_shallow = false;
            for x in (cx - 14)..=(cx + 14) {
                for z in (cz - 14)..=(cz + 14) {
                    if (x - cx).abs() + (z - cz).abs() > 14 {
                        continue;
                    }
                    let Some(column) = world.get(&(x >> 4, z >> 4)) else {
                        continue;
                    };
                    let Some(map) = column.heightmaps.get("WORLD_SURFACE") else {
                        continue;
                    };
                    let h = map[((z & 15) * 16 + (x & 15)) as usize];
                    if h - 1 < apron_y + 8 {
                        too_shallow = true;
                    }
                }
            }
            if too_shallow {
                println!(
                    "  REFUSED  the surface comes within 8 of the box floor, so \
                     min(minY, surface-1) is not decidable from the saved heightmap"
                );
                refused += 1;
                continue;
            }

            let origin = rustmc_worldgen::feature::decorate::ChunkOrigin {
                chunk_x: start.chunk_x,
                chunk_z: start.chunk_z,
                min_section_y: -4,
            };
            let population = rustmc_worldgen::feature::decorate::population_seed(origin, seed);

            let mut trials = Vec::new();
            for leading in 0..=search {
                let mut random = WorldgenRandom::new(XoroshiroRandomSource::from_seed(0));
                random.set_feature_seed(
                    population,
                    i32::try_from(index).unwrap_or(10),
                    /* SURFACE_STRUCTURES */ 4,
                );
                for _ in 0..leading {
                    let _ = random.next_i64();
                }
                let offset = random.next_i32_bound(bound);
                let (scored, wrong, uncovered) = score_apron(
                    &mut random,
                    world,
                    &tags,
                    b,
                    (cx, cz),
                    apron_y,
                    offset,
                    cold,
                    overgrown,
                );
                trials.push(ApronTrial {
                    leading,
                    offset,
                    scored,
                    wrong,
                    uncovered,
                });
            }
            let clean: Vec<&ApronTrial> = trials.iter().filter(|t| t.wrong == 0).collect();
            let best = trials.iter().min_by_key(|t| t.wrong).expect("search >= 0");
            for t in &trials {
                println!(
                    "  k={:<3} offset={} {:>6} wrong of {:>5} scored{}",
                    t.leading,
                    t.offset,
                    t.wrong,
                    t.scored,
                    if t.wrong == 0 { "   <-- exact" } else { "" }
                );
                if t.uncovered > 0 {
                    println!(
                        "         {} positions not covered by the blocks \
                         directory, excluded from both columns",
                        t.uncovered
                    );
                }
            }
            match clean.as_slice() {
                [] => println!(
                    "  NONE FOUND  best is k={} at {} wrong of {} — the stream is not \
                     a shift of ours, or a later feature has overwritten the apron",
                    best.leading, best.wrong, best.scored
                ),
                [one] => {
                    resolved += 1;
                    *recovered.entry(one.leading).or_default() += 1;
                    if one.leading == 0 {
                        at_zero += 1;
                    }
                    println!(
                        "  RECOVERED k={} offset={} over {} scored blocks",
                        one.leading, one.offset, one.scored
                    );
                }
                many => println!(
                    "  AMBIGUOUS  {} candidates score zero: {:?}",
                    many.len(),
                    many.iter().map(|t| t.leading).collect::<Vec<_>>()
                ),
            }
        }
    }
    println!(
        "APRON {portals} portals, {resolved} resolved, {refused} refused, \
         {at_zero} entered the pass at draw 0"
    );
    for (k, n) in &recovered {
        println!("APRON leading-draws k={k}: {n} portals");
    }
    if portals == 0 {
        println!("APRON VACUOUS — the reference holds no ruined portal");
        return Err("vacuous: no ruined_portal in the reference".to_string());
    }
    Ok(())
}

/// Replay `spreadNetherrack` for one candidate `offset` and count how many of
/// the blocks it predicts the saved world disagrees with.
///
/// The random is advanced exactly as the pass would advance it, so a candidate
/// that is wrong early stays wrong for the rest of the walk — which is what
/// makes the search separate one `k` from its neighbours instead of grading
/// them.
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is a separate piece of vanilla's own state and \
              bundling them would hide which of them the draw count depends on"
)]
fn score_apron<R: rustmc_worldgen::rng::RandomSource + ?Sized>(
    random: &mut R,
    world: &BTreeMap<(i32, i32), rustmc_world::anvil::ChunkColumn>,
    tags: &rustmc_worldgen::tags::BlockTags,
    b: [i32; 6],
    centre: (i32, i32),
    apron_y: i32,
    offset: i32,
    cold: bool,
    overgrown: bool,
) -> (u32, u32, u32) {
    let (cx, cz) = centre;
    let n = 14i32;
    let (mut scored, mut wrong, mut uncovered) = (0u32, 0u32, 0u32);
    // A position whose chunk the blocks directory does not hold cannot be
    // scored, and must not be scored as WRONG either. It read as 50 wrong
    // once, on an apron that reaches into chunk (47,64) from a dump whose
    // window starts at 48 — a tool that charges the port for a chunk nobody
    // generated is measuring its own window.
    let covered = |x: i32, z: i32| world.contains_key(&(x >> 4, z >> 4));
    let outside = |x: i32, z: i32| x < b[0] || x > b[3] || z < b[2] || z > b[5];
    // `addNetherrackDripColumnsBelowPortal` runs AFTER this pass over
    // `minX+1 .. <maxX`, writing into the same sub-columns, so what is under
    // the box rim is the union of two passes and cannot score either.
    let second_pass = |x: i32, z: i32| x > b[0] && x < b[3] && z > b[2] && z < b[5];
    let plain = |s: &str| s.split_once('[').map_or(s, |(h, _)| h).to_string();
    let is_netherrack_or_magma = |s: &str| {
        matches!(
            plain(s).as_str(),
            "minecraft:netherrack" | "minecraft:magma_block"
        )
    };

    for x in (cx - n)..=(cx + n) {
        for z in (cz - n)..=(cz + n) {
            let dist = (x - cx).abs() + (z - cz).abs();
            let index = (dist + offset).max(0);
            let here = block_at(world, x, apron_y, z).unwrap_or_default();
            if index >= n {
                // Skipped without drawing. Netherrack out here would have to
                // have come from an apron this candidate says never reached,
                // and nothing else writes netherrack in the overworld.
                if outside(x, z) {
                    if covered(x, z) {
                        scored += 1;
                        if is_netherrack_or_magma(&here) {
                            wrong += 1;
                        }
                    } else {
                        uncovered += 1;
                    }
                }
                continue;
            }
            let weight = APRON_WEIGHTS[usize::try_from(index).expect("0..14 is a usize")];
            if random.next_f64() >= f64::from(weight) {
                if outside(x, z) {
                    if covered(x, z) {
                        scored += 1;
                        if is_netherrack_or_magma(&here) {
                            wrong += 1;
                        }
                    } else {
                        uncovered += 1;
                    }
                }
                continue;
            }
            if (apron_y - b[1]).abs() > 3 {
                continue;
            }
            // `canBlockBeReplacedByNetherrackOrMagma`, evaluated on the saved
            // block. Sound because the only two blocks this pass writes are
            // themselves replaceable, so writing here cannot have changed the
            // predicate's answer. `is(Blocks.AIR)` is the exact block, not
            // `isAir()` — cave_air passes.
            let name = plain(&here);
            let replaceable = name != "minecraft:air"
                && name != "minecraft:obsidian"
                && name != "minecraft:lava"
                && !tags.contains("minecraft:features_cannot_replace", &name);
            if !replaceable {
                continue;
            }
            // `placeNetherrackOrMagma` — the draw is conditional on `!cold`,
            // and its outcome is legible in the saved block's own name.
            let place = |random: &mut R, x: i32, y: i32, z: i32| -> bool {
                let want = if !cold && random.next_f32() < 0.07 {
                    "minecraft:magma_block"
                } else {
                    "minecraft:netherrack"
                };
                plain(&block_at(world, x, y, z).unwrap_or_default()) == want
            };
            let ok = place(random, x, apron_y, z);
            if covered(x, z) {
                scored += 1;
                if !ok {
                    wrong += 1;
                }
            } else {
                uncovered += 1;
            }
            if overgrown {
                // `maybeAddLeavesAbove` draws unconditionally.
                let _ = random.next_f32();
            }
            // The drip column. It writes without asking, so every block in it
            // is a hard prediction; only its depth is random.
            let mut y = apron_y - 1;
            let ok = place(random, x, y, z);
            if !second_pass(x, z) {
                if covered(x, z) {
                    scored += 1;
                    if !ok {
                        wrong += 1;
                    }
                } else {
                    uncovered += 1;
                }
            }
            let mut i = 8;
            while i > 0 && random.next_f32() < 0.5 {
                y -= 1;
                i -= 1;
                let ok = place(random, x, y, z);
                if !second_pass(x, z) {
                    if covered(x, z) {
                        scored += 1;
                        if !ok {
                            wrong += 1;
                        }
                    } else {
                        uncovered += 1;
                    }
                }
            }
        }
    }
    (scored, wrong, uncovered)
}

/// `nether-fossil-starts` — ours, in vanilla's own listing format.
///
/// The format is copied from `jigsaw_starts`'s template-piece branch on
/// purpose: a diff is only a gate if both sides are spelled identically, and
/// re-deriving the layout on this side is how a mismatch becomes a
/// formatting argument instead of a finding.
fn nether_fossil_starts_cmd(
    seed: &str,
    radius: &str,
    center_x: Option<&String>,
    center_z: Option<&String>,
) -> Result<(), String> {
    let seed: i64 = seed.parse().map_err(|_| "seed must be an integer")?;
    let radius: i32 = radius.parse().map_err(|_| "radius must be an integer")?;
    let cx: i32 = center_x
        .map_or(Ok(64), |v| v.parse())
        .map_err(|_| "bad center-x")?;
    let cz: i32 = center_z
        .map_or(Ok(64), |v| v.parse())
        .map_err(|_| "bad center-z")?;
    let area = rustmc_worldgen::pipeline::Area::square(cx, cz, radius);
    let mut starts = worldgen_assembly::generate::nether_fossil_report(seed, area)?;
    // Vanilla's listing walks region files, so it comes out ordered by chunk.
    starts.sort_by_key(|f| (f.start_chunk.0, f.start_chunk.1));
    for fossil in &starts {
        println!(
            "START minecraft:nether_fossil chunk ({}, {}) — 1 template pieces (not a jigsaw)",
            fossil.start_chunk.0, fossil.start_chunk.1
        );
        let b = fossil.bbox;
        println!(
            "    0  minecraft:{} rot={:?} TP={},{},{} GD=0 BB=[{} {} {} .. {} {} {}]",
            fossil.start.template,
            fossil.start.rotation,
            fossil.start.pos.0,
            fossil.start.pos.1,
            fossil.start.pos.2,
            b.min_x,
            b.min_y,
            b.min_z,
            b.max_x,
            b.max_y,
            b.max_z
        );
    }
    eprintln!("{} nether_fossil starts", starts.len());
    Ok(())
}

/// OUR bastion starts, in the format `jigsaw-starts <ref> bastion_remnant`
/// prints VANILLA's saved ones.
///
/// The format is copied from `jigsaw_starts`'s jigsaw branch character for
/// character on purpose: a diff is only a gate if both sides are spelled
/// identically, and re-deriving the layout on this side is how a mismatch
/// becomes a formatting argument instead of a finding.
///
/// **Starts outside the reference's own square are printed and cannot be
/// diffed.** `bastion_starts` searches six chunks beyond `area` because a
/// 105-piece start spans seven chunks and its placement chunk is not the one
/// it is widest in; vanilla saved `Children[]` only for chunks it generated.
/// So a caller diffing this against a reference has to bound the comparison
/// to the reference's square, which `golden_bastion_start` does by chunk.
/// The placement chunks where `nether_complexes` puts a fortress, within the
/// square of `radius` chunks about the centre. Layout only: no piece is
/// assembled and nothing is generated.
fn fortress_starts_cmd(
    seed: &str,
    radius: &str,
    center_x: Option<&String>,
    center_z: Option<&String>,
) -> Result<(), String> {
    let seed: i64 = seed.parse().map_err(|_| "seed must be an integer")?;
    let radius: i32 = radius.parse().map_err(|_| "radius must be an integer")?;
    let cx: i32 = center_x
        .map_or(Ok(64), |v| v.parse())
        .map_err(|_| "bad center-x")?;
    let cz: i32 = center_z
        .map_or(Ok(64), |v| v.parse())
        .map_err(|_| "bad center-z")?;
    let area = rustmc_worldgen::pipeline::Area::square(cx, cz, radius);
    for (x, z) in worldgen_assembly::generate::fortress_report(seed, area, 0)? {
        println!("FORTRESS chunk ({x}, {z})");
    }
    Ok(())
}

fn bastion_starts_cmd(
    seed: &str,
    radius: &str,
    center_x: Option<&String>,
    center_z: Option<&String>,
) -> Result<(), String> {
    let seed: i64 = seed.parse().map_err(|_| "seed must be an integer")?;
    let radius: i32 = radius.parse().map_err(|_| "radius must be an integer")?;
    let cx: i32 = center_x
        .map_or(Ok(64), |v| v.parse())
        .map_err(|_| "bad center-x")?;
    let cz: i32 = center_z
        .map_or(Ok(64), |v| v.parse())
        .map_err(|_| "bad center-z")?;
    let area = rustmc_worldgen::pipeline::Area::square(cx, cz, radius);
    let mut starts = worldgen_assembly::generate::bastion_report(seed, area)?;
    // Vanilla's listing walks region files, so it comes out ordered by chunk.
    starts.sort_by_key(|b| (b.start_chunk.0, b.start_chunk.1));
    for bastion in &starts {
        println!(
            "START minecraft:{} chunk ({}, {}) — {} pieces",
            bastion.structure,
            bastion.start_chunk.0,
            bastion.start_chunk.1,
            bastion.pieces.len()
        );
        for (i, piece) in bastion.pieces.iter().enumerate() {
            let name = match &piece.element.kind {
                rustmc_worldgen::structure::jigsaw::ElementKind::Single { location, .. } => {
                    location.clone()
                }
                rustmc_worldgen::structure::jigsaw::ElementKind::Feature { feature } => {
                    feature.clone()
                }
                rustmc_worldgen::structure::jigsaw::ElementKind::Empty => "-".to_string(),
                rustmc_worldgen::structure::jigsaw::ElementKind::List { .. } => {
                    piece.element.describe()
                }
            };
            let b = piece.bbox;
            println!(
                "  {i:3}  {:<58} {:<20} pos={:<16} gld={:<4} proj={:<16} BB=[{} {} {} .. {} {} {}] junctions={}",
                name,
                // Vanilla's enum-constant name, which is what the NBT holds
                // and what `jigsaw-starts` prints on the other side.
                // `{:?}`.to_uppercase() gives `CLOCKWISE90` and the one
                // missing underscore is enough to make every line of a diff
                // that exists to be diffed come out different.
                match piece.rotation {
                    rustmc_worldgen::structure::template::Rotation::None => "NONE",
                    rustmc_worldgen::structure::template::Rotation::Clockwise90 => "CLOCKWISE_90",
                    rustmc_worldgen::structure::template::Rotation::Clockwise180 => "CLOCKWISE_180",
                    rustmc_worldgen::structure::template::Rotation::Counterclockwise90 => {
                        "COUNTERCLOCKWISE_90"
                    }
                },
                format!("{},{},{}", piece.pos.0, piece.pos.1, piece.pos.2),
                piece.ground_level_delta,
                format!("{:?}", piece.element.projection).to_lowercase(),
                b.min_x,
                b.min_y,
                b.min_z,
                b.max_x,
                b.max_y,
                b.max_z,
                piece.junctions.len(),
            );
        }
    }
    eprintln!("{} bastion_remnant starts", starts.len());
    Ok(())
}

/// OUR ruined-portal starts, in the format `jigsaw-starts <ref> ruined_portal`
/// prints VANILLA's, so the two diff line for line.
///
/// The field this exists for is `TP`. Vanilla's `RuinedPortalPiece` saves
/// `TPX/TPY/TPZ`, and for seed 20260722 chunk (48,64) that is `768,42,1024` —
/// the chunk origin, not the box centre. A port that takes the centre puts the
/// whole portal 4 east and 12 south, which reads in a block diff as "the
/// structure is wrong" and in a plan gate as nothing at all: the template, the
/// rotation, the mirror and the placement are all still right.
fn ruined_portal_starts_cmd(
    seed: &str,
    radius: &str,
    center_x: Option<&String>,
    center_z: Option<&String>,
) -> Result<(), String> {
    let seed: i64 = seed.parse().map_err(|_| "seed must be an integer")?;
    let radius: i32 = radius.parse().map_err(|_| "radius must be an integer")?;
    let cx: i32 = center_x
        .map_or(Ok(64), |v| v.parse())
        .map_err(|_| "bad center-x")?;
    let cz: i32 = center_z
        .map_or(Ok(64), |v| v.parse())
        .map_err(|_| "bad center-z")?;
    let area = rustmc_worldgen::pipeline::Area::square(cx, cz, radius);
    let mut starts = worldgen_assembly::generate::ruined_portal_report(seed, area)?;
    // Vanilla's listing walks region files, so it comes out ordered by chunk.
    starts.sort_by_key(|p| (p.start_chunk.0, p.start_chunk.1));
    for portal in &starts {
        println!(
            "START minecraft:{} chunk ({}, {}) — 1 template pieces (not a jigsaw)",
            portal.structure, portal.start_chunk.0, portal.start_chunk.1
        );
        let piece = &portal.piece;
        let b = piece.bounding_box(&portal.template);
        println!(
            "    0  minecraft:{:<24} rot={:?} mirror={:?} TP={},{},{} GD=0 \
             BB=[{} {} {} .. {} {} {}]",
            portal.template_name,
            piece.rotation,
            piece.mirror,
            piece.template_position.0,
            piece.template_position.1,
            piece.template_position.2,
            b.min_x,
            b.min_y,
            b.min_z,
            b.max_x,
            b.max_y,
            b.max_z
        );
    }
    eprintln!("{} ruined_portal starts", starts.len());
    Ok(())
}

// ---------------------------------------------------------------------------

/// **This target executed zero assertions until 2026-08-16.**
///
/// `worldgen-dump` is 3,100 lines and is where most of this project's
/// worldgen oracles are read out of vanilla. Its bin target is selected by
/// `ci/t0.sh`'s `--lib --bins` and reported `0 passed` on every gate ever
/// run here.
///
/// Almost everything in this file needs a generated world or the vanilla
/// extract, and that is why it is a T1/T2 tool rather than a unit-tested one.
/// What is tested here is the small set of **pure decoders** every one of
/// those dumps passes its data through: get one of these wrong and the dump
/// is confidently, silently about the wrong position or the wrong block, and
/// no downstream comparison can tell.
#[cfg(test)]
mod tests {
    use super::{is_untouched_rock, quoted, unpack};

    /// Vanilla's `ChunkPos.asLong`: x in the low 32 bits, z in the high 32.
    fn as_long(x: i32, z: i32) -> i64 {
        #[expect(
            clippy::cast_sign_loss,
            reason = "the packing is defined on the bit patterns"
        )]
        let packed = (u64::from(x as u32) | (u64::from(z as u32) << 32)) as i64;
        packed
    }

    /// **The sign is the whole difference.** `asLong` packs two 32-bit
    /// patterns into a 64-bit word; recovering them with anything that sign-
    /// extends the low half corrupts the high one, and a negative chunk x
    /// then reads as a chunk z one less than it should be. Every window this
    /// project measures is centred, so the negative quadrant is not an edge
    /// case, it is a quarter of the data.
    #[test]
    fn unpack_inverts_as_long_in_every_quadrant() {
        for (x, z) in [
            (0, 0),
            (1, 2),
            (-1, -1),
            (-1, 0),
            (0, -1),
            (-2048, 2048),
            (2048, -2048),
            (i32::MIN, i32::MAX),
            (i32::MAX, i32::MIN),
        ] {
            assert_eq!(
                unpack(&as_long(x, z)),
                (x, z),
                "asLong({x}, {z}) did not round-trip"
            );
        }
    }

    /// The chunk at (-1, 0) and the chunk at (-1, -1) must not decode to the
    /// same pair, which is what a sign-extending low half would do.
    #[test]
    fn adjacent_negative_chunks_stay_distinct() {
        assert_ne!(unpack(&as_long(-1, 0)), unpack(&as_long(-1, -1)));
    }

    /// A block state carries its properties, and the untouched-rock test is
    /// on the block *name*: `minecraft:deepslate[axis=y]` is deepslate. A
    /// comparison that forgot the split would classify every propertied rock
    /// as `foreign`, which is the verdict `offset_from_blocks` uses to
    /// distinguish "the pass never ran" from "something painted over it".
    #[test]
    fn untouched_rock_is_decided_on_the_name_not_the_state() {
        assert!(is_untouched_rock("minecraft:stone"));
        assert!(is_untouched_rock("minecraft:deepslate[axis=y]"));
        assert!(is_untouched_rock("minecraft:iron_ore"));
        assert!(is_untouched_rock(
            "minecraft:deepslate_diamond_ore[foo=bar]"
        ));
        assert!(!is_untouched_rock("minecraft:cave_air"));
        assert!(!is_untouched_rock("minecraft:oak_planks"));
        // The suffix test is on the name, so a property whose *value* ends in
        // `_ore` must not promote a plank to rock.
        assert!(!is_untouched_rock("minecraft:oak_planks[kind=iron_ore]"));
    }

    /// `null` is a JSON value and `"null"` is a JSON string; a piece with no
    /// template must produce the first.
    #[test]
    fn an_absent_field_is_json_null_and_a_present_one_is_a_string() {
        assert_eq!(quoted(None), "null");
        assert_eq!(
            quoted(Some("minecraft:village/plains/houses")),
            "\"minecraft:village/plains/houses\""
        );
    }
}
