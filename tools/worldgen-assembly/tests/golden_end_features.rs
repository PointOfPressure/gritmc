//! The End's return gateways and the spikes' end crystals, against every
//! vanilla world of their windows.
//!
//! # Gateways
//!
//! `end_gateway_return` is `rarity_filter 700` in `end_highlands`, so the
//! measured windows held none and the feature went unported. A scan of sixteen
//! r=16 windows of our own generator found two at seed 20260722, in chunks
//! (-99,-5) and (116,109); each was harvested three times
//! (`van-end-20260722-c{-99_-5,116_109}-gw-{a,b,c}`). This compares the 9x9
//! chunks around each, every block over y 0..255, against all three, and the
//! gateway's block entity (`exit_portal`, `ExactTeleport`, `Age`) against each
//! reference's saved one.
//!
//! # Crystals
//!
//! `EndSpikeFeature.placeSpike` adds an `EndCrystal` per spike. The harvest
//! keeps `entities/` since 2026-10-04 (`van-end-{12345,31415926,20260722}-
//! c0_0-ent-{a,b,c}`), and the ten crystals of each seed are compared key for
//! key — every key but `UUID`, which vanilla draws from an unseeded random.
//! The yaw is the sharp field: it is the feature stream's `nextFloat()`, so
//! it is right only if every draw before it on that stream is.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rustmc_world::nbt::Nbt;
use rustmc_worldgen::pipeline::{Area, Through};

fn root() -> PathBuf {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../oracle-work")).to_path_buf()
}

fn refs(prefix: &str) -> Vec<PathBuf> {
    let refs: Vec<PathBuf> = ["a", "b", "c"]
        .iter()
        .map(|t| root().join(format!("{prefix}{t}")))
        .collect();
    for r in &refs {
        assert!(
            rustmc_data::fixture::available(r, "End reference", "ci/gen-vanilla-region.sh"),
            "missing {}",
            r.display()
        );
        let stamp = std::fs::read_to_string(r.join("HARVEST-ORDER")).expect("HARVEST-ORDER");
        assert!(
            stamp.lines().any(|l| l == "dump_order=zx"),
            "{} is not a rows harvest",
            r.display()
        );
    }
    refs
}

/// Read off the references, never asserted: every one stamps `zx`. Set once,
/// because the tests of this binary run on several threads.
fn env_once() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        std::env::set_var("RUSTMC_DECORATE_ORDER", "zx");
        std::env::set_var("RUSTMC_RECORD_BLOCK_ENTITIES", "1");
    });
}

fn columns(dir: &Path) -> BTreeMap<(i32, i32), rustmc_world::anvil::ChunkColumn> {
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(dir).expect("directory") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("mca") {
            continue;
        }
        for c in rustmc_world::anvil::read_region(&path).expect("region parses") {
            out.insert((c.x, c.z), c);
        }
    }
    out
}

#[test]
fn the_return_gateways_match_every_vanilla_world() {
    env_once();
    for (chunk, prefix) in [
        ((-99, -5), "van-end-20260722-c-99_-5-gw-"),
        ((116, 109), "van-end-20260722-c116_109-gw-"),
    ] {
        let refs = refs(prefix);
        let ours = worldgen_assembly::generate::generate_end(
            20_260_722,
            Area::square(chunk.0, chunk.1, 4),
            Through::Decoration,
        )
        .expect("generates");
        let (x0, z0) = ((chunk.0 - 4) * 16, (chunk.1 - 4) * 16);
        let (x1, z1) = ((chunk.0 + 4) * 16 + 15, (chunk.1 + 4) * 16 + 15);
        let mut gateways = 0;
        for r in &refs {
            let vanilla = columns(r);
            let mut diff = 0usize;
            let mut first = None;
            for x in x0..=x1 {
                for z in z0..=z1 {
                    let col = &vanilla[&(x.div_euclid(16), z.div_euclid(16))];
                    for y in 0..256 {
                        let v = col.block_at(x, y, z).unwrap_or("minecraft:air");
                        let o = ours.canvas.block_at(x, y, z);
                        if v != o {
                            diff += 1;
                            first.get_or_insert((x, y, z, v.to_string(), o.to_string()));
                        }
                        if v == "minecraft:end_gateway" {
                            gateways += 1;
                            let saved = col
                                .block_entities
                                .iter()
                                .find(|be| {
                                    be.get("x").and_then(Nbt::as_i32) == Some(x)
                                        && be.get("y").and_then(Nbt::as_i32) == Some(y)
                                        && be.get("z").and_then(Nbt::as_i32) == Some(z)
                                })
                                .unwrap_or_else(|| {
                                    panic!("{}: no block entity at the gateway", r.display())
                                });
                            let rec = ours
                                .canvas
                                .block_entity_records(x.div_euclid(16), z.div_euclid(16))
                                .iter()
                                .find(|(p, _)| *p == (x, y, z))
                                .map(|(_, rec)| rec)
                                .expect("our gateway recorded its block entity");
                            let mine = rustmc_worldgen::block_entity::for_recorded(
                                "minecraft:end_gateway",
                                rec,
                                x,
                                y,
                                z,
                            )
                            .expect("an end_gateway payload");
                            assert_eq!(&mine, saved, "{}: the gateway's block entity", r.display());
                        }
                    }
                }
            }
            assert_eq!(
                diff,
                0,
                "{} vs ours around chunk {chunk:?}: first difference {first:?}",
                r.display()
            );
        }
        assert_eq!(
            gateways, 3,
            "one gateway in each reference around {chunk:?}"
        );
        println!(
            "GATEWAY {chunk:?}: 9x9 chunks x 256 equal to all 3 references, block entity equal"
        );
    }
}

fn vanilla_crystals(dir: &Path) -> Vec<Nbt> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir.join("entities")).expect("an entities/ directory") {
        let path = entry.expect("entry").path();
        // Vanilla leaves a zero-length `r.X.Z.mca` where every entity chunk
        // of a region was empty; it holds nothing to read.
        if std::fs::metadata(&path).map_or(0, |m| m.len()) == 0 {
            continue;
        }
        for record in rustmc_world::anvil::read_region_records(&path)
            .expect("entities region")
            .values()
        {
            let root = rustmc_world::anvil::decode_region_record(record).expect("decodes");
            for e in root.get("Entities").and_then(Nbt::as_list).unwrap_or(&[]) {
                if e.get("id").and_then(Nbt::as_str) == Some("minecraft:end_crystal") {
                    out.push(e.clone());
                }
            }
        }
    }
    out
}

fn without_uuid(e: &Nbt) -> BTreeMap<String, Nbt> {
    let mut m = e.as_compound().expect("compound").clone();
    m.remove("UUID");
    m
}

fn key(m: &BTreeMap<String, Nbt>) -> String {
    format!("{:?}", m.get("Pos"))
}

#[test]
fn the_end_crystals_match_every_vanilla_world_at_three_seeds() {
    env_once();
    for seed in [12_345_i64, 31_415_926, 20_260_722] {
        let refs = refs(&format!("van-end-{seed}-c0_0-ent-"));
        let ours = worldgen_assembly::generate::generate_end(
            seed,
            Area::square(0, 0, 3),
            Through::Decoration,
        )
        .expect("generates");
        let mut mine: Vec<BTreeMap<String, Nbt>> = Vec::new();
        for cx in -3..=3 {
            for cz in -3..=3 {
                for e in ours.canvas.entity_records(cx, cz) {
                    if e.get("id").and_then(Nbt::as_str) == Some("minecraft:end_crystal") {
                        mine.push(without_uuid(e));
                    }
                }
            }
        }
        mine.sort_by_key(key);
        assert_eq!(mine.len(), 10, "seed {seed}: ten spikes, ten crystals");
        for r in &refs {
            let mut theirs: Vec<BTreeMap<String, Nbt>> =
                vanilla_crystals(r).iter().map(without_uuid).collect();
            theirs.sort_by_key(key);
            assert_eq!(theirs.len(), 10, "{}: ten crystals saved", r.display());
            for (v, o) in theirs.iter().zip(&mine) {
                assert_eq!(v, o, "seed {seed}, {}: a crystal", r.display());
            }
        }
        println!("CRYSTALS seed {seed}: 10 of 10 equal, key for key but UUID, to all 3 references");
    }
}

/// The two end cities WITH a ship at seed 20260722 (start chunks (81,3) and
/// (107,-37), harvested three times each as `van-end-20260722-c{81_3,
/// 107_-37}-ship-{a,b,c}`), over the 9x9 chunks around each start.
///
/// The three vanilla worlds do not agree everywhere — at (107,-37) one of
/// them grows a three-block ladder the other two lack — so the bar is the
/// **consensus**: wherever all three references hold the same block, ours must
/// too. Before the End ran `updateFromNeighbourShapes` over an end city's
/// pieces (`EndCityPiece.makeSettings` never sets `knownShape`), one
/// `purpur_stairs` at (1713,72,-581) kept its template's `outer_right` where
/// all three hold `straight`. The block entities (chests, banners, the
/// ship's ender chest and brewing stand) are compared to each reference's.
#[test]
fn the_ship_cities_match_vanillas_consensus() {
    env_once();
    for (seed, chunk, prefix) in [
        (20_260_722, (81, 3), "van-end-20260722-c81_3-ship-"),
        (20_260_722, (107, -37), "van-end-20260722-c107_-37-ship-"),
        // Four magenta wall banners hang on a block that is not `isSolid`
        // here; all three vanilla worlds drop them through
        // `WallBannerBlock.updateShape` and ours kept them until 2026-10-04.
        (12_345, (-16, 83), "van-end-12345-c-16_83-ship-"),
        // Far out (x -50,000, z 100,000): the ship of the city started at
        // chunk (-3136, 6243), five chunks west of this window. Until
        // 2026-10-04 the city's corner heights were read off the canvas, a
        // start outside it was skipped, and its ship (950 blocks) was air.
        (12_345, (-3127, 6247), "van-end-12345-c-3125_6250-far-"),
    ] {
        let refs = refs(prefix);
        let ours = worldgen_assembly::generate::generate_end(
            seed,
            Area::square(chunk.0, chunk.1, 4),
            Through::Decoration,
        )
        .expect("generates");
        let vanilla: Vec<_> = refs.iter().map(|r| columns(r)).collect();
        let (x0, z0) = ((chunk.0 - 4) * 16, (chunk.1 - 4) * 16);
        let (x1, z1) = ((chunk.0 + 4) * 16 + 15, (chunk.1 + 4) * 16 + 15);
        let mut agreed = 0usize;
        let mut wrong = Vec::new();
        for x in x0..=x1 {
            for z in z0..=z1 {
                let key = (x.div_euclid(16), z.div_euclid(16));
                for y in 0..256 {
                    let v: Vec<&str> = vanilla
                        .iter()
                        .map(|cols| cols[&key].block_at(x, y, z).unwrap_or("minecraft:air"))
                        .collect();
                    if v.iter().any(|b| *b != v[0]) {
                        continue;
                    }
                    agreed += 1;
                    let o = ours.canvas.block_at(x, y, z);
                    if o != v[0] {
                        wrong.push((x, y, z, v[0].to_string(), o.to_string()));
                    }
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "ship city {chunk:?}: {} of {agreed} consensus positions differ, first {:?}",
            wrong.len(),
            wrong.first()
        );
        let mut bes = 0;
        for (r, cols) in refs.iter().zip(&vanilla) {
            for cx in chunk.0 - 4..=chunk.0 + 4 {
                for cz in chunk.1 - 4..=chunk.1 + 4 {
                    let saved = &cols[&(cx, cz)].block_entities;
                    let mine: Vec<Nbt> = ours
                        .canvas
                        .block_entity_records(cx, cz)
                        .iter()
                        .filter_map(|((x, y, z), rec)| {
                            let name = ours.canvas.block_at(*x, *y, *z);
                            let plain = name.split_once('[').map_or(name, |(n, _)| n);
                            rustmc_worldgen::block_entity::for_recorded(plain, rec, *x, *y, *z)
                        })
                        .collect();
                    for be in saved {
                        bes += 1;
                        assert!(
                            mine.contains(be),
                            "{} chunk ({cx},{cz}): vanilla's block entity {be:?} is not ours",
                            r.display()
                        );
                    }
                }
            }
        }
        assert!(bes > 0, "ship city {chunk:?}: no block entity compared");
        // The entities: shulkers, the ship's elytra frame. Compared key for
        // key but `UUID` and a shulker's `Rotation`: `LivingEntity`'s
        // constructor sets `yRot = this.random.nextFloat() * 6.2831855f` off
        // the entity's own unseeded random, so the three vanilla worlds
        // disagree with each other on all 60 shulker yaws at (107,-37).
        let strip = |e: &Nbt| {
            let mut m = without_uuid(e);
            if m.get("id").and_then(Nbt::as_str) == Some("minecraft:shulker") {
                m.remove("Rotation");
            }
            m
        };
        let mut mine: Vec<BTreeMap<String, Nbt>> = Vec::new();
        for cx in chunk.0 - 4..=chunk.0 + 4 {
            for cz in chunk.1 - 4..=chunk.1 + 4 {
                mine.extend(ours.canvas.entity_records(cx, cz).iter().map(strip));
            }
        }
        mine.sort_by_key(key);
        let inside = |m: &BTreeMap<String, Nbt>| match m.get("Pos") {
            Some(Nbt::List(p)) => match (p.first(), p.get(2)) {
                (Some(Nbt::Double(x)), Some(Nbt::Double(z))) => {
                    #[allow(clippy::cast_possible_truncation)]
                    let (bx, bz) = (x.floor() as i32, z.floor() as i32);
                    (x0..=x1).contains(&bx) && (z0..=z1).contains(&bz)
                }
                _ => false,
            },
            _ => false,
        };
        mine.retain(inside);
        assert!(!mine.is_empty(), "ship city {chunk:?}: no entity recorded");
        for r in &refs {
            let mut theirs: Vec<BTreeMap<String, Nbt>> = Vec::new();
            for entry in std::fs::read_dir(r.join("entities")).expect("an entities/ directory") {
                let path = entry.expect("entry").path();
                // Vanilla leaves a zero-length `r.X.Z.mca` where every entity chunk
                // of a region was empty; it holds nothing to read.
                if std::fs::metadata(&path).map_or(0, |m| m.len()) == 0 {
                    continue;
                }
                for rec in rustmc_world::anvil::read_region_records(&path)
                    .expect("entities")
                    .values()
                {
                    let root = rustmc_world::anvil::decode_region_record(rec).expect("decodes");
                    theirs.extend(
                        root.get("Entities")
                            .and_then(Nbt::as_list)
                            .unwrap_or(&[])
                            .iter()
                            .map(strip),
                    );
                }
            }
            theirs.retain(inside);
            theirs.sort_by_key(key);
            assert_eq!(theirs.len(), mine.len(), "{}: entity count", r.display());
            for (v, o) in theirs.iter().zip(&mine) {
                assert_eq!(v, o, "{}: an entity", r.display());
            }
        }
        println!(
            "SHIP CITY {chunk:?}: {} entities equal but UUID and shulker yaw",
            mine.len()
        );
        println!(
            "SHIP CITY {chunk:?}: {agreed} consensus positions equal, {bes} block entities equal"
        );
    }
}

/// The exit portal where the dragon fight puts it.
///
/// `EnderDragonFight` builds the podium only once a player is in the arena,
/// so these references are harvested with one: `ci/harvest-end-podium.sh`
/// generates chunks -8..7, joins an offline client, sends it to the End and
/// waits for the fight's "dragon has not yet been killed" scan
/// (`van-end-{20260722,12345}-podium-{a,b,c}`).
///
/// Ours: the End generated through decoration, `exit_portal_origin` off its
/// `MOTION_BLOCKING_NO_LEAVES` height at (0,0), then `podium_writes(origin,
/// false)` applied. Compared over chunks -2..1, every block y 0..255,
/// wherever all three references agree.
#[test]
fn the_exit_portal_stands_where_the_dragon_fight_puts_it() {
    use rustmc_worldgen::canvas::LiveHeightmap;
    use rustmc_worldgen::feature::end::{exit_portal_origin, podium_writes};
    env_once();
    for seed in [20_260_722_i64, 12_345] {
        let refs = refs_any_order(&format!("van-end-{seed}-podium-"));
        let ours = worldgen_assembly::generate::generate_end(
            seed,
            Area::square(0, 0, 3),
            Through::Decoration,
        )
        .expect("generates");
        let mut canvas = ours.canvas;
        let h = canvas.height_of(LiveHeightmap::MotionBlockingNoLeaves, 0, 0);
        let origin = exit_portal_origin(
            h,
            |x, y, z| canvas.block_at(x, y, z).to_string(),
            canvas.min_y(),
        );
        for ((x, y, z), state) in podium_writes(origin, false) {
            canvas.set_block(x, y, z, state);
        }
        let vanilla: Vec<_> = refs.iter().map(|r| columns(r)).collect();
        let mut agreed = 0usize;
        let mut wrong = Vec::new();
        for x in -32_i32..32 {
            for z in -32_i32..32 {
                let key = (x.div_euclid(16), z.div_euclid(16));
                for y in 0..256 {
                    let v: Vec<&str> = vanilla
                        .iter()
                        .map(|cols| cols[&key].block_at(x, y, z).unwrap_or("minecraft:air"))
                        .collect();
                    if v.iter().any(|b| *b != v[0]) {
                        continue;
                    }
                    agreed += 1;
                    let o = canvas.block_at(x, y, z);
                    if o != v[0] {
                        wrong.push((x, y, z, v[0].to_string(), o.to_string()));
                    }
                }
            }
        }
        let torches = wrong.is_empty()
            && vanilla[0][&(0, -1)].block_at(0, origin.1 + 2, -1)
                == Some("minecraft:wall_torch[facing=north]");
        assert!(
            wrong.is_empty(),
            "seed {seed}: exit portal at {origin:?}, {} of {agreed} consensus positions differ, first {:?}",
            wrong.len(),
            wrong.first()
        );
        assert!(
            torches,
            "seed {seed}: the reference holds no podium at {origin:?}"
        );
        println!("EXIT PORTAL seed {seed}: origin {origin:?}, {agreed} consensus positions equal");
    }
}

/// Like [`refs`], for references stamped by `ci/harvest-end-podium.sh`.
fn refs_any_order(prefix: &str) -> Vec<PathBuf> {
    let refs: Vec<PathBuf> = ["a", "b", "c"]
        .iter()
        .map(|t| root().join(format!("{prefix}{t}")))
        .collect();
    for r in &refs {
        assert!(
            rustmc_data::fixture::available(r, "End podium reference", "ci/harvest-end-podium.sh"),
            "missing {}",
            r.display()
        );
    }
    refs
}

/// End city PLACEMENT: every `minecraft:end_city` start vanilla saved in an
/// r=24 square (2,401 chunks) at three seeds, against `end_city_report` over
/// the same square, start chunk for start chunk and piece bounding box for
/// piece bounding box in vanilla's saved order. One harvest per seed: a start
/// is decided by the seed alone (`RandomSpreadStructurePlacement`, then the
/// city's own `WorldgenRandom`), so the runs that disagree on blocks cannot
/// disagree here. References: `ci/gen-vanilla-region.sh <seed> 24 <dir> <port>
/// <cx> <cz> end` with `GEN_ORDER=rows`.
#[test]
fn the_end_city_starts_match_vanilla_over_three_r24_squares() {
    let cases: [(i64, i32, i32, &str); 3] = [
        (12345, 130, -70, "van-end-12345-c130_-70-r24-a"),
        (31415926, 130, -70, "van-end-31415926-c130_-70-r24-a"),
        (20260722, 95, -20, "van-end-20260722-c95_-20-r24-a"),
    ];
    let r = 24;
    let mut total = (0usize, 0usize);
    for (seed, cx, cz, dir) in cases {
        let dir = root().join(dir);
        assert!(
            rustmc_data::fixture::available(&dir, "End r=24 reference", "ci/gen-vanilla-region.sh"),
            "missing {}",
            dir.display()
        );
        let inside = |x: i32, z: i32| (x - cx).abs() <= r && (z - cz).abs() <= r;
        let mut vanilla: BTreeMap<(i32, i32), Vec<[i32; 6]>> = BTreeMap::new();
        for c in columns(&dir).values() {
            for s in &c.structure_starts {
                if s.id == "minecraft:end_city" && inside(s.chunk_x, s.chunk_z) {
                    vanilla.insert(
                        (s.chunk_x, s.chunk_z),
                        s.pieces.iter().map(|p| p.bounding_box).collect(),
                    );
                }
            }
        }
        let mut ours: BTreeMap<(i32, i32), Vec<[i32; 6]>> = BTreeMap::new();
        for city in worldgen_assembly::generate::end_city_report(seed, Area::square(cx, cz, r))
            .expect("end city report")
        {
            if inside(city.start_chunk.0, city.start_chunk.1) {
                ours.insert(
                    city.start_chunk,
                    city.pieces
                        .iter()
                        .map(|p| {
                            let b = p.bbox;
                            [b.min_x, b.min_y, b.min_z, b.max_x, b.max_y, b.max_z]
                        })
                        .collect(),
                );
            }
        }
        assert!(
            !vanilla.is_empty(),
            "seed {seed}: the reference holds no end city"
        );
        assert_eq!(
            ours.keys().collect::<Vec<_>>(),
            vanilla.keys().collect::<Vec<_>>(),
            "seed {seed}: start chunks differ"
        );
        for (chunk, pieces) in &vanilla {
            assert_eq!(
                &ours[chunk], pieces,
                "seed {seed}: city {chunk:?} pieces differ"
            );
            total.1 += pieces.len();
        }
        total.0 += vanilla.len();
        eprintln!(
            "END CITY STARTS seed {seed}: {} starts equal",
            vanilla.len()
        );
    }
    eprintln!(
        "END CITY STARTS: {} starts, {} pieces equal",
        total.0, total.1
    );
}

/// What `EnderDragonFight.setDragonKilled` builds, against vanilla worlds
/// whose dragon was killed (`KILL_DRAGON=1 ci/harvest-end-podium.sh`,
/// `van-end-{20260722,12345}-killed-{a,b,c}`):
///
/// * `spawnExitPortal(true)`: the ACTIVE podium at the saved
///   `exit_portal_location`, which must be ours (`exit_portal_origin`);
/// * `spawnNewGateway()`: the LAST index of the shuffled ring
///   (`gateway_spawn_order(seed)[0]`) at `gateway_ring_position`, a delayed
///   gateway (`gateway_writes`), and the fight's saved `gateways` list must be
///   what is left (`gateway_spawn_order(seed)[1..]`, reversed);
/// * the DRAGON_EGG at `getHeightmapPos(MOTION_BLOCKING, (0,0))`.
///
/// Compared over x, z in -112..112 (chunks -7..6), y 0..255, wherever all
/// three references agree, and the gateway, the egg and a portal block
/// checked in every reference.
#[test]
fn the_dragon_kill_builds_what_vanilla_builds() {
    use rustmc_world::nbt::Nbt;
    use rustmc_worldgen::canvas::LiveHeightmap;
    use rustmc_worldgen::feature::end::{
        exit_portal_origin, gateway_ring_position, gateway_spawn_order, gateway_writes,
        podium_writes,
    };
    env_once();
    for seed in [20_260_722_i64, 12_345] {
        let refs = refs_any_order(&format!("van-end-{seed}-killed-"));
        let ours = worldgen_assembly::generate::generate_end(
            seed,
            Area::square(0, 0, 7),
            Through::Decoration,
        )
        .expect("generates");
        let mut canvas = ours.canvas;
        let h = canvas.height_of(LiveHeightmap::MotionBlockingNoLeaves, 0, 0);
        let origin = exit_portal_origin(
            h,
            |x, y, z| canvas.block_at(x, y, z).to_string(),
            canvas.min_y(),
        );
        for ((x, y, z), state) in podium_writes(origin, true) {
            canvas.set_block(x, y, z, state);
        }
        let order = gateway_spawn_order(seed);
        let gateway = gateway_ring_position(order[0]);
        for ((x, y, z), state) in gateway_writes(gateway) {
            canvas.set_block(x, y, z, state);
        }
        // The live maps follow decoration only; `getHeightmapPos` reads the
        // level's, which every `setBlock` above has kept current. Re-prime
        // chunk (0,0) off the written world, as `primeHeightmaps` would.
        struct Pred(worldgen_assembly::facts::TableFacts);
        impl rustmc_worldgen::canvas::HeightmapPredicates for Pred {
            fn matches(&self, state: &str, map: LiveHeightmap) -> bool {
                rustmc_worldgen::stage::decorate::BlockFacts::matches_heightmap(&self.0, state, map)
            }
        }
        let pred = Pred(worldgen_assembly::facts::TableFacts(
            worldgen_assembly::block_props().expect("block props"),
        ));
        canvas.prime_heightmaps(0, 0, &pred);
        let egg_y = canvas.height_of(LiveHeightmap::MotionBlocking, 0, 0);
        canvas.set_block(0, egg_y, 0, "minecraft:dragon_egg");

        let mut left: Vec<i32> = order[1..].to_vec();
        left.reverse();
        for r in &refs {
            let path =
                r.join("data/dimensions/minecraft/the_end/data/minecraft/ender_dragon_fight.dat");
            let mut gz =
                flate2::read::GzDecoder::new(std::fs::File::open(&path).expect("fight data"));
            let (_n, root) = rustmc_world::nbt::read_named_root(&mut gz).expect("nbt");
            let data = root.get("data").expect("data");
            let saved: Vec<i32> = match data.get("gateways") {
                Some(Nbt::IntArray(v)) => v.clone(),
                Some(Nbt::List(v)) => v
                    .iter()
                    .map(|n| match n {
                        Nbt::Int(i) => *i,
                        other => panic!("gateway entry {other:?}"),
                    })
                    .collect(),
                other => panic!("{}: gateways {other:?}", r.display()),
            };
            assert_eq!(
                saved,
                left,
                "seed {seed}: {} saved gateway order",
                r.display()
            );
            assert_eq!(
                data.get("exit_portal_location"),
                Some(&Nbt::IntArray(vec![origin.0, origin.1, origin.2])),
                "seed {seed}: {} exit portal location",
                r.display()
            );
        }

        let vanilla: Vec<_> = refs.iter().map(|r| columns(r)).collect();
        let mut agreed = 0usize;
        let mut wrong = Vec::new();
        for x in -112_i32..112 {
            for z in -112_i32..112 {
                let key = (x.div_euclid(16), z.div_euclid(16));
                for y in 0..256 {
                    let v: Vec<&str> = vanilla
                        .iter()
                        .map(|cols| cols[&key].block_at(x, y, z).unwrap_or("minecraft:air"))
                        .collect();
                    if v.iter().any(|b| *b != v[0]) {
                        continue;
                    }
                    agreed += 1;
                    let o = canvas.block_at(x, y, z);
                    if o != v[0] {
                        wrong.push((x, y, z, v[0].to_string(), o.to_string()));
                    }
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "seed {seed}: {} of {agreed} consensus positions differ, first {:?}",
            wrong.len(),
            wrong.first()
        );
        for cols in &vanilla {
            let key = (gateway.0.div_euclid(16), gateway.2.div_euclid(16));
            assert_eq!(
                cols[&key].block_at(gateway.0, gateway.1, gateway.2),
                Some("minecraft:end_gateway")
            );
            assert_eq!(
                cols[&(0, 0)].block_at(0, egg_y, 0),
                Some("minecraft:dragon_egg")
            );
            assert_eq!(
                cols[&(0, 0)].block_at(0, origin.1, 1),
                Some("minecraft:end_portal")
            );
        }
        println!(
            "DRAGON KILL seed {seed}: portal {origin:?}, gateway {} at {gateway:?}, egg y {egg_y}, \
             {agreed} consensus positions equal, saved order of {} equal in {} worlds",
            order[0],
            left.len(),
            refs.len()
        );
    }
}

/// Our End, decorated chunk by chunk on demand, as the exit search sees it.
struct LazyEnd {
    seed: i64,
    chunks: BTreeMap<(i32, i32), rustmc_worldgen::canvas::Canvas>,
    props: rustmc_behavior::block_props::BlockPropsTable,
}

impl LazyEnd {
    fn chunk(&mut self, cx: i32, cz: i32) -> &rustmc_worldgen::canvas::Canvas {
        let seed = self.seed;
        self.chunks.entry((cx, cz)).or_insert_with(|| {
            worldgen_assembly::generate::generate_end(
                seed,
                Area::square(cx, cz, 0),
                Through::Decoration,
            )
            .expect("generates")
            .canvas
        })
    }
}

impl rustmc_worldgen::feature::end::GatewayExitWorld for LazyEnd {
    fn highest_filled_section_y(&mut self, cx: i32, cz: i32) -> Option<i32> {
        let c = self.chunk(cx, cz);
        (0..16).rev().find(|s| {
            (s * 16..s * 16 + 16).any(|y| {
                (cx * 16..cx * 16 + 16).any(|x| {
                    (cz * 16..cz * 16 + 16).any(|z| c.block_at(x, y, z) != "minecraft:air")
                })
            })
        })
    }
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
        if !(0..256).contains(&y) {
            return "minecraft:air".to_string();
        }
        self.chunk(x.div_euclid(16), z.div_euclid(16))
            .block_at(x, y, z)
            .to_string()
    }
    fn is_collision_full_block(&mut self, state: &str) -> bool {
        self.props
            .get(state)
            .unwrap_or_else(|| panic!("no block props for {state}"))
            .collision_full_faces
            .iter()
            .all(|f| *f)
    }
    fn min_y(&self) -> i32 {
        0
    }
    fn max_y(&self) -> i32 {
        255
    }
}

/// `TheEndGatewayBlockEntity.getPortalPosition` on first entry, against
/// vanilla worlds whose player walked into the first gateway after the kill
/// (`KILL_DRAGON=1 GATEWAY="x y z" ci/harvest-end-podium.sh`,
/// `van-end-{seed}-exit-{a,b,c}`): the entered gateway saves our exit as
/// `exit_portal`, and the exit gateway stands there (`gateway_writes`) and
/// saves the entered gateway as its own `exit_portal`, not exact.
#[test]
fn the_first_gateway_exit_matches_vanilla() {
    use rustmc_world::nbt::Nbt;
    use rustmc_worldgen::feature::end::{
        gateway_exit, gateway_ring_position, gateway_spawn_order, gateway_writes,
    };
    env_once();
    for seed in [20_260_722_i64, 12_345] {
        let refs = refs_any_order(&format!("van-end-{seed}-exit-"));
        let gateway = gateway_ring_position(gateway_spawn_order(seed)[0]);
        let mut world = LazyEnd {
            seed,
            chunks: BTreeMap::new(),
            props: worldgen_assembly::block_props().expect("block props"),
        };
        let exit = gateway_exit(&mut world, gateway);
        assert!(
            exit.island.is_none(),
            "seed {seed}: our search spawned an island at {:?}; its shape is unseeded in vanilla",
            exit.island
        );
        let be_at = |cols: &BTreeMap<(i32, i32), rustmc_world::anvil::ChunkColumn>,
                     p: (i32, i32, i32)| {
            cols[&(p.0.div_euclid(16), p.2.div_euclid(16))]
                .block_entities
                .iter()
                .find(|be| {
                    be.get("x") == Some(&Nbt::Int(p.0))
                        && be.get("y") == Some(&Nbt::Int(p.1))
                        && be.get("z") == Some(&Nbt::Int(p.2))
                })
                .cloned()
        };
        for r in &refs {
            let cols = columns(r);
            let entered = be_at(&cols, gateway).expect("the entered gateway's block entity");
            assert_eq!(
                entered.get("exit_portal"),
                Some(&Nbt::IntArray(vec![exit.exit.0, exit.exit.1, exit.exit.2])),
                "seed {seed}: {} entered gateway's exit (ours {exit:?})",
                r.display()
            );
            let built = be_at(&cols, exit.exit).expect("the exit gateway's block entity");
            assert_eq!(
                built.get("exit_portal"),
                Some(&Nbt::IntArray(vec![gateway.0, gateway.1, gateway.2])),
                "seed {seed}: {} exit gateway's exit",
                r.display()
            );
            assert_ne!(built.get("ExactTeleport"), Some(&Nbt::Byte(1)));
            for ((x, y, z), state) in gateway_writes(exit.exit) {
                let col = &cols[&(x.div_euclid(16), z.div_euclid(16))];
                assert_eq!(
                    col.block_at(x, y, z).unwrap_or("minecraft:air"),
                    state,
                    "seed {seed}: {} exit gateway block at {:?}",
                    r.display(),
                    (x, y, z)
                );
            }
        }
        println!(
            "GATEWAY EXIT seed {seed}: gateway {gateway:?} -> exit {:?} (spawn {:?}, tentative {:?}), {} worlds",
            exit.exit,
            exit.spawn,
            exit.tentative,
            refs.len()
        );
    }
}
