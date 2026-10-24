//! The ocean-ruin placement, gated against a **server's own saved start**.
//!
//! # The seed labels are not to be trusted; terrain is
//!
//! The region directories carry no `level.dat`, so a saved world's seed is
//! recorded nowhere but its **directory name** — and the names are wrong.
//! `van-s2-c62` reads as "seed 2" and is in fact seed **31415926**; the ones
//! named `vanilla-r7` and `van-rows-c64` are the other seed. Refereed by
//! terrain, which is bit-exact-gated and owes nothing to placement:
//!
//! ```text
//!   worldgen-dump full 31415926 3 /tmp/ref-s1 60 64
//!   region-differ diff /tmp/ref-s1/region oracle-work/<dir> --bbox 58 62 62 66
//!
//!     van-s2-c62      99.995646%   biomes 38400/0        <- same seed
//!     vanilla-r7      76.354451%   biomes 38400/38400
//!     van-rows-c64    76.350952%   biomes 38400/38400
//! ```
//!
//! Every biome differing is the signature of a different seed, and 76% is what
//! two unrelated worlds of the same generator score by coincidence. Any future
//! comparison must referee the pairing this way before reading a number off it:
//! a diff between mismatched seeds is not a low parity score, it is nonsense.
//!
//! # How this stopped being a probe
//!
//! It was written as a probe with a wrong expectation. The census family of
//! `water -> sand/suspicious_sand/cut_sandstone/polished_granite/chest` blocks
//! had been attributed to chunks (63,67) and (63,68), so the probe printed
//! `DISAGREES` when the placement answered (67,65). Both halves of that were
//! checked and the *probe* was the one that was wrong:
//!
//!   * (63,67)/(63,68) is the **gravel-disk** family — a different open
//!     question, in a different chunk, at a different seed. Two unrelated
//!     families had been merged under one pair of coordinates.
//!   * `region-differ census --match suspicious --positions` over both
//!     servers of this seed finds exactly **5** suspicious_sand, both worlds
//!     agreeing, at (1067..1071, 48, 1041..1044) — which is chunk **(66,65)**,
//!     not (63,·).
//!
//! and the seed it was asked about was the wrong one, per the referee above.
//! This is the
//! third time a measurement here has been read against a belief rather than
//! against vanilla, which is what ORACLE rule 17 exists to prevent, and the
//! reason the fix is a gate rather than a corrected `println!`.
//!
//! # What is gated
//!
//! Vanilla serialises a chunk's structure starts into the region file, so a
//! server states in its own words which structure it chose and where:
//!
//! ```text
//!   minecraft:ocean_ruin_warm start (67, 65) 1 pieces,
//!                                   union 1066,48,1040..1072,54,1045
//! ```
//!
//! That is a stronger oracle than any harness. It is the server's decision,
//! not a re-derivation of it, and it pins three separate things at once:
//!
//!   1. the placement chunk — **(67,65)**, which this port already computes;
//!   2. the **warm** variant, which the biome tags independently predict;
//!   3. a **single** piece, so `addPieces` took the non-cluster path.
//!
//! Its union spans chunk x 66..67 at z 65, which is why the visible blocks sit
//! in (66,65) while the start is recorded in (67,65) — a structure is placed
//! from one chunk and written into every chunk it reaches. Reading the visible
//! blocks as the placement chunk is exactly the mistake above.
//!
//! Nothing here claims the ruin is *built* correctly. The port places no ocean
//! ruin at all yet. This gate fixes the one half that is already right, so that
//! when the building half lands it cannot silently move.

use rustmc_worldgen::structure::placement::StructurePlacement;

/// The seed whose server world carries the start quoted above — established
/// by the terrain referee, not by the directory name.
const SEED: i64 = 31_415_926;

/// Read off `worldgen-dump mineshaft-mesa <seed> oracle-work/van-s2-c62`,
/// which prints every structure start a server saved.
const SERVER_START: (i32, i32) = (67, 65);

/// The union the same server recorded, in blocks.
const SERVER_UNION: ((i32, i32, i32), (i32, i32, i32)) = ((1066, 48, 1040), (1072, 54, 1045));

fn placement() -> Option<StructurePlacement> {
    let path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .join("oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/structure_set/ocean_ruins.json");
    if !rustmc_data::fixture::available(&path, "datagen extract", "ci/vanilla-server.sh data") {
        return None;
    }
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("json");
    Some(StructurePlacement::parse(&doc["placement"]).expect("ocean_ruins placement"))
}

/// The port's placement must select the chunk the server actually used, and
/// must not select any *other* chunk in the window the server left empty.
#[test]
fn the_placement_chunk_is_the_one_the_server_saved() {
    let Some(placement) = placement() else { return };

    let mut hits = Vec::new();
    for z in 59..=69 {
        for x in 59..=69 {
            if placement.is_placement_chunk(SEED, x, z).expect("supported") {
                hits.push((x, z));
            }
        }
    }

    println!("ocean-ruin placement chunks in 59..=69 at seed {SEED}: {hits:?}");
    println!("the server saved its start at {SERVER_START:?}");

    assert!(
        hits.contains(&SERVER_START),
        "the server placed an ocean_ruin_warm start at {SERVER_START:?} and this \
         placement does not select that chunk; it selected {hits:?}"
    );

    // The stronger half. A placement that fired everywhere would also contain
    // the server's chunk, so containment alone proves very little — the window
    // has to be *exactly* as sparse as the server's.
    assert_eq!(
        hits,
        vec![SERVER_START],
        "the server saved exactly one ocean-ruin start in this window, so any \
         extra placement chunk here is a ruin vanilla does not have"
    );
}

/// The visible blocks are in a different chunk from the start, and that is
/// not a discrepancy — it is what a structure union means.
///
/// This exists because misreading the two as the same thing is what made the
/// original probe print the wrong verdict, and a comment would not have caught
/// it a second time.
#[test]
fn the_saved_union_reaches_out_of_its_own_placement_chunk() {
    let ((x1, _, z1), (x2, _, z2)) = SERVER_UNION;

    let start_chunk_x = SERVER_START.0;
    assert_eq!(
        x2 >> 4,
        start_chunk_x,
        "the union's far edge is in the start chunk"
    );
    assert_ne!(
        x1 >> 4,
        start_chunk_x,
        "and its near edge is not — so the ruin's blocks appear in a chunk that \
         is not the one the start is recorded in"
    );

    // Where the 5 suspicious_sand actually are, per both servers.
    let sand_chunk = (1067 >> 4, 1041 >> 4);
    assert_eq!(
        sand_chunk,
        (66, 65),
        "the suspicious_sand sits in chunk (66,65), one west of the start"
    );
    assert!(
        (x1..=x2).contains(&1067) && (z1..=z2).contains(&1041),
        "and inside the union the server recorded, which is what ties the \
         blocks to this start rather than to a feature"
    );
}

/// The draws, held to what the server's union already proved.
///
/// `plan` is pure arithmetic once the seeding is fixed, and the seeding is
/// `GenerationContext`'s `setLargeFeatureSeed(seed, chunkX, chunkZ)` — the same
/// call the mineshaft start is gated on against 659 real pieces, so it is not a
/// fresh assumption.
///
/// The server's 7x7x6 union is a small warm template turned 90 or 270 degrees.
/// Nothing here was tuned to produce that: the arrays, the order and the
/// guarded cluster coin all came out of bytecode before this ran.
#[test]
fn the_draws_agree_with_the_union_the_server_saved() {
    use rustmc_worldgen::rng::legacy::LegacyRandomSource;
    use rustmc_worldgen::rng::worldgen_random::WorldgenRandom;
    use rustmc_worldgen::structure::ocean_ruin::{plan, WARM_RUINS};
    use rustmc_worldgen::structure::template::Rotation;

    // ocean_ruin_warm.json
    const LARGE_PROBABILITY: f32 = 0.3;
    const CLUSTER_PROBABILITY: f32 = 0.9;

    let (cx, cz) = SERVER_START;
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
    random.set_large_feature_seed(SEED, cx, cz);
    let p = plan(&mut random, cx, cz, LARGE_PROBABILITY, CLUSTER_PROBABILITY);

    println!("plan at {SERVER_START:?}, seed {SEED}: {p:?}");

    assert!(
        !p.large,
        "every big warm template is 16x16x16 and the server's union is 7x7x6, \
         so this ruin is small; a large plan means the draw order is wrong"
    );
    assert_eq!(p.integrity, 0.8, "small ruins rot at 0.8, not 0.9");
    assert!(
        !p.cluster_coin_spent,
        "the cluster nextFloat is guarded by `large`; spending it here would \
         shift every later structure in this chunk"
    );
    assert!(WARM_RUINS.contains(&p.template));
    assert!(
        matches!(
            p.rotation,
            Rotation::Clockwise90 | Rotation::Counterclockwise90
        ),
        "a 6x7x7 template only reaches the server's 7x7x6 union with x and z \
         swapped, so the rotation must be a quarter turn; got {:?}",
        p.rotation
    );
    assert_eq!(p.origin, (cx * 16, 90, cz * 16));
}

/// The strongest form of the same check: the transformed footprint must be the
/// server's union *by coordinate*, not merely by extent.
///
/// `postProcess` builds the far corner as
///
/// ```text
///   corner = transform(BlockPos(size.x - 1, 0, size.z - 1),
///                      Mirror.NONE, rotation, BlockPos.ZERO)
///            .offset(templatePosition)
/// ```
///
/// so origin and corner together are the piece's horizontal span. This ties
/// four independent things to one number the server wrote down: the placement
/// chunk, the rotation draw, the template's size, and `transform`'s handedness
/// — the last of which could not be read off the switch and had to be fixed
/// geometrically.
#[test]
fn the_transformed_footprint_is_the_servers_union() {
    use rustmc_worldgen::rng::legacy::LegacyRandomSource;
    use rustmc_worldgen::rng::worldgen_random::WorldgenRandom;
    use rustmc_worldgen::structure::ocean_ruin::plan;
    use rustmc_worldgen::structure::template::transform;

    let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .join("oracle-work/vanilla-26.2/generated-data/data/minecraft/structure/underwater_ruin");
    if !rustmc_data::fixture::available(
        &dir,
        "underwater_ruin templates",
        "ci/vanilla-server.sh data",
    ) {
        return;
    }

    let (cx, cz) = SERVER_START;
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
    random.set_large_feature_seed(SEED, cx, cz);
    let p = plan(&mut random, cx, cz, 0.3, 0.9);

    let file = std::fs::File::open(dir.join(format!("{}.nbt", p.template))).expect("open");
    let mut reader = flate2::read::GzDecoder::new(file);
    let (_n, root) = rustmc_world::nbt::read_named_root(&mut reader).expect("gzipped NBT");
    let template =
        rustmc_worldgen::structure::template::Template::from_nbt(&root).expect("template");

    let (sx, _sy, sz) = template.size;
    let far = transform(
        (sx - 1, 0, sz - 1),
        rustmc_worldgen::structure::template::Mirror::None,
        p.rotation,
        (0, 0),
    );
    let corner = (p.origin.0 + far.0, far.1, p.origin.2 + far.2);

    let (x_lo, x_hi) = (p.origin.0.min(corner.0), p.origin.0.max(corner.0));
    let (z_lo, z_hi) = (p.origin.2.min(corner.2), p.origin.2.max(corner.2));
    println!(
        "{} rotated {:?} from {:?} spans x {x_lo}..{x_hi}, z {z_lo}..{z_hi}",
        p.template, p.rotation, p.origin
    );

    let ((ux1, _, uz1), (ux2, _, uz2)) = SERVER_UNION;
    assert_eq!(
        (x_lo, x_hi),
        (ux1, ux2),
        "x span must be the server's union"
    );
    assert_eq!(
        (z_lo, z_hi),
        (uz1, uz2),
        "z span must be the server's union"
    );
}

/// The suspicious sand, against a server's own blocks.
///
/// This is the first check on the ruin that reads *contents* rather than
/// geometry. Both seed-31415926 worlds agree on exactly five, at
///
/// ```text
///   1068 48 1041   1068 48 1042   1071 48 1042   1071 48 1043   1067 48 1044
/// ```
///
/// found with `region-differ census --match suspicious --positions`. Five is
/// also the processor's cap (`iconst_5`), so a port that shuffled the wrong
/// list length, or counted visits instead of changes, would still produce five
/// — just five of the wrong ones. Only the positions separate those cases,
/// which is why this asserts the set and not the count.
///
/// `y` is taken from the server rather than from `settle_height`, so a failure
/// here is a failure of the processor chain and cannot be blamed on the height
/// walk. That check is separate and still owed.
#[test]
fn the_suspicious_sand_lands_where_the_server_put_it() {
    use rustmc_worldgen::rng::legacy::LegacyRandomSource;
    use rustmc_worldgen::rng::worldgen_random::WorldgenRandom;
    use rustmc_worldgen::structure::ocean_ruin::{place_blocks, plan};

    let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .join("oracle-work/vanilla-26.2/generated-data/data/minecraft/structure/underwater_ruin");
    if !rustmc_data::fixture::available(
        &dir,
        "underwater_ruin templates",
        "ci/vanilla-server.sh data",
    ) {
        return;
    }

    let (cx, cz) = SERVER_START;
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
    random.set_large_feature_seed(SEED, cx, cz);
    let p = plan(&mut random, cx, cz, 0.3, 0.9);

    let file = std::fs::File::open(dir.join(format!("{}.nbt", p.template))).expect("open");
    let mut reader = flate2::read::GzDecoder::new(file);
    let (_n, root) = rustmc_world::nbt::read_named_root(&mut reader).expect("gzipped NBT");
    let template =
        rustmc_worldgen::structure::template::Template::from_nbt(&root).expect("template");

    // The y both servers put the sand at.
    let blocks = place_blocks(&template, &p, 48, SEED);

    let mut ours: Vec<(i32, i32, i32)> = blocks
        .iter()
        .filter(|(_, s)| s.starts_with("minecraft:suspicious_sand["))
        .map(|(pos, _)| *pos)
        .collect();
    ours.sort_unstable();

    let mut want = vec![
        (1068, 48, 1041),
        (1068, 48, 1042),
        (1071, 48, 1042),
        (1071, 48, 1043),
        (1067, 48, 1044),
    ];
    want.sort_unstable();

    println!("survivors {} blocks", blocks.len());
    println!("ours  {ours:?}");
    println!("server {want:?}");
    assert_eq!(ours, want, "the suspicious sand must be the server's");
}

/// The chest, from the template's data marker, against the server's block.
///
/// `warm_7` carries one marker, `chest` at template-local (3,0,3). Rotated
/// CW90 about the origin that becomes (1069, 48, 1043), and the server has
/// exactly one chest in these two chunks:
///
/// ```text
///   1069 48 1043 minecraft:chest[facing=north,type=single,waterlogged=true]
/// ```
///
/// `facing=north` is the part that had to come from the server. The piece is
/// rotated a quarter turn, so a chest that inherited the piece's rotation would
/// read `facing=east`; `handleDataMarker` uses `defaultBlockState()` and does
/// not rotate it. Reading that off the bytecode alone would have been a
/// judgement call, and this is a measurement.
#[test]
fn the_chest_marker_matches_the_servers_chest() {
    use rustmc_worldgen::rng::legacy::LegacyRandomSource;
    use rustmc_worldgen::rng::worldgen_random::WorldgenRandom;
    use rustmc_worldgen::structure::ocean_ruin::{marker_blocks, plan};

    let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .join("oracle-work/vanilla-26.2/generated-data/data/minecraft/structure/underwater_ruin");
    if !rustmc_data::fixture::available(
        &dir,
        "underwater_ruin templates",
        "ci/vanilla-server.sh data",
    ) {
        return;
    }

    let (cx, cz) = SERVER_START;
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
    random.set_large_feature_seed(SEED, cx, cz);
    let p = plan(&mut random, cx, cz, 0.3, 0.9);

    let file = std::fs::File::open(dir.join(format!("{}.nbt", p.template))).expect("open");
    let mut reader = flate2::read::GzDecoder::new(file);
    let (_n, root) = rustmc_world::nbt::read_named_root(&mut reader).expect("gzipped NBT");
    let template =
        rustmc_worldgen::structure::template::Template::from_nbt(&root).expect("template");

    // The ruin sits on the sea floor and the server's chest is waterlogged, so
    // the fluid answer is water. Passed as a closure rather than hardcoded in
    // the port so that a dry ruin would still be expressible.
    let blocks = marker_blocks(&template, &p, 48, |_| true);

    println!("markers -> {blocks:?}");
    assert_eq!(
        blocks,
        vec![(
            (1069, 48, 1043),
            "minecraft:chest[facing=north,type=single,waterlogged=true]".to_string()
        )],
        "the chest must be the server's, position and state"
    );
}
