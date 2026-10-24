//! The spawn decision over a synthetic chunk, with a seeded generator.
//!
//! # WHAT THIS VERIFIES, AND WHAT IT DOES NOT
//!
//! It verifies that this transcription is **internally consistent**: that
//! the RNG draws happen where the doc comments say they happen, that each
//! gate refuses what it claims to refuse, and that the decision log and
//! the draw counter — two independent readings of the same run — agree.
//!
//! It does **not** verify that vanilla would decide the same thing. No
//! vanilla capture of natural spawning exists on this box and none can be
//! taken today: `ci/t2-gameplay.sh` boots `difficulty=peaceful`,
//! `spawn-monsters=false` and the `spawn_mobs` game rule off, and
//! `origin/corpus-dark-systems` measured lifting that rule through the
//! `[[gamerule]]` door as a NULL — 15/15 pairs over 6 runs, 6 distinct
//! projections. `docs/cold-start/natural-spawning.md` names the scenario
//! that would close this and what it needs.
//!
//! # The one check here that a wrong transcription cannot pass
//!
//! `draws_match_the_decision_log` recomputes the expected draw count from
//! the *events* and compares it with the counter the generator kept. The
//! two are derived differently — one from the control flow, one from the
//! generator — so a draw taken on the wrong branch moves one and not the
//! other. A test that only compared the log against a pinned copy of
//! itself would pass under exactly that defect.

use std::path::{Path, PathBuf};

use rustmc_data::{BlockPos, ChunkPos, Dimension};
use rustmc_entity::spawn::host::{RespawnData, SpawnBlockFacts, SpawnRandom};
use rustmc_entity::spawn::natural::{self, NoFilter, PositionGate, SpawnEvent};
use rustmc_entity::spawn::tables::SpawnTables;
use rustmc_entity::spawn::testing::{GridSpawnLevel, RecordingHost};
use rustmc_entity::spawn::MobCategory;
use rustmc_worldgen::rng::LegacyRandomSource;

/// A `SpawnRandom` that counts what it hands out.
struct Counting {
    inner: LegacyRandomSource,
    ints: u32,
    floats: u32,
}

impl Counting {
    fn new(seed: i64) -> Self {
        Self {
            inner: LegacyRandomSource::new(seed),
            ints: 0,
            floats: 0,
        }
    }
    fn total(&self) -> u32 {
        self.ints + self.floats
    }
}

impl SpawnRandom for Counting {
    fn next_i32_bound(&mut self, bound: i32) -> i32 {
        self.ints += 1;
        rustmc_worldgen::rng::RandomSource::next_i32_bound(&mut self.inner, bound)
    }
    fn next_f32(&mut self) -> f32 {
        self.floats += 1;
        rustmc_worldgen::rng::RandomSource::next_f32(&mut self.inner)
    }
}

fn repo(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn tables() -> Option<SpawnTables> {
    let biomes = repo("data/vanilla/biome_spawners.json");
    if !rustmc_data::fixture::available(&biomes, "biome spawner table", "ci/dump-biome-spawners.sh")
    {
        return None;
    }
    Some(
        SpawnTables::load(
            &biomes,
            &repo("data/vanilla/spawn_placements.json"),
            &repo("data/vanilla/entity_spawn_facts.json"),
        )
        .expect("the three spawn tables load"),
    )
}

/// A level a pig can stand in: solid ground below, air at and above, a
/// player far enough away for the 24-block rule and near enough for the
/// 128-block one.
fn plains_level() -> GridSpawnLevel {
    GridSpawnLevel {
        biome: "plains".into(),
        rough_biome: "plains".into(),
        // 40 blocks away in x: 1600 > 576, and well inside 128² = 16384.
        players: vec![(40.5, 64.0, 8.5)],
        respawn: RespawnData {
            dimension: Dimension::Overworld,
            // Far from the chunk, so the world-spawn rule never fires.
            pos: BlockPos {
                x: 5000,
                y: 64,
                z: 5000,
            },
        },
        default_facts: SpawnBlockFacts {
            fluid_is_empty: true,
            animals_spawnable_on: true,
            ..SpawnBlockFacts::default()
        },
        raw_brightness: 15,
        ..GridSpawnLevel::default()
    }
}

/// The draw count the events imply, derived from the control flow rather
/// than from the generator.
fn expected_draws(events: &[SpawnEvent], level: &GridSpawnLevel) -> u32 {
    let mut n = 0u32;
    for e in events {
        n += match e {
            // getRandomPosWithin: nextInt(16), nextInt(16),
            // randomBetweenInclusive -> one nextInt.
            SpawnEvent::ChunkPos { .. } => 3,
            // Mth.ceil(nextFloat() * 4.0f).
            SpawnEvent::Attempt { .. } => 1,
            // nextInt(6) - nextInt(6), twice.
            SpawnEvent::Position { .. } => 4,
            // WeightedList.getRandom, then nextInt(1 + max - min).
            SpawnEvent::Picked { .. } => 2,
            // nextFloat() * 360.0f, drawn after create_mob succeeds.
            SpawnEvent::Spawned { .. } => 1,
            // Monster.isDarkEnoughToSpawn: nextInt(32), then the
            // IntProvider sample if the first two gates pass. The pig's
            // predicate is checkAnimalSpawnRules, which draws nothing, so
            // this fixture's only light draws come from a monster pass.
            SpawnEvent::BadPositionForType {
                gate: PositionGate::SpawnRules,
            } if level.sky_brightness > 0 => 1,
            _ => 0,
        };
    }
    n
}

#[test]
fn get_random_pos_within_takes_three_draws_and_stays_in_the_chunk() {
    let level = plains_level();
    for seed in 0..64i64 {
        let mut rng = Counting::new(seed);
        let pos = natural::random_pos_within(&level, ChunkPos { x: 3, z: -2 }, &mut rng);
        assert_eq!(rng.total(), 3, "seed {seed}: draws in getRandomPosWithin");
        assert_eq!(
            rng.floats, 0,
            "seed {seed}: getRandomPosWithin draws no float"
        );
        assert!(
            (48..64).contains(&pos.x),
            "seed {seed}: x {} outside chunk 3",
            pos.x
        );
        assert!(
            (-32..-16).contains(&pos.z),
            "seed {seed}: z {} outside chunk -2",
            pos.z
        );
        // y is uniform over [minY, surface + 1], both inclusive.
        assert!(
            (level.min_y..=level.surface_height + 1).contains(&pos.y),
            "seed {seed}: y {}",
            pos.y
        );
    }
}

#[test]
fn a_position_below_min_y_plus_one_ends_the_call_before_any_other_draw() {
    let Some(t) = tables() else { return };
    let mut level = plains_level();
    // A world whose only column is at the bottom: every drawn y is minY,
    // which is < minY + 1.
    level.min_y = 0;
    level.surface_height = -1;
    let mut rng = Counting::new(7);
    let mut host = RecordingHost::new(4, 4);
    let mut log = Vec::new();
    natural::spawn_category_for_chunk(
        MobCategory::Creature,
        &level,
        ChunkPos { x: 0, z: 0 },
        &mut NoFilter,
        &mut host,
        &mut rng,
        &t,
        &mut log,
    );
    assert!(matches!(log.last(), Some(SpawnEvent::BelowMinY)), "{log:?}");
    assert_eq!(rng.total(), 3, "only getRandomPosWithin's three draws");
    assert!(host.placed.is_empty());
}

#[test]
fn a_redstone_conductor_at_the_drawn_position_ends_the_call() {
    let Some(t) = tables() else { return };
    let mut level = plains_level();
    level.default_facts.is_redstone_conductor = true;
    let mut rng = Counting::new(11);
    let mut host = RecordingHost::new(4, 4);
    let mut log = Vec::new();
    natural::spawn_category_for_chunk(
        MobCategory::Creature,
        &level,
        ChunkPos { x: 0, z: 0 },
        &mut NoFilter,
        &mut host,
        &mut rng,
        &t,
        &mut log,
    );
    assert!(log.contains(&SpawnEvent::RedstoneConductor), "{log:?}");
    assert_eq!(rng.total(), 3, "no attempt is entered");
}

#[test]
fn no_player_costs_the_walk_but_never_the_weighted_draw() {
    let Some(t) = tables() else { return };
    let mut level = plains_level();
    level.players.clear();
    let mut rng = Counting::new(3);
    let mut host = RecordingHost::new(4, 4);
    let mut log = Vec::new();
    natural::spawn_category_for_chunk(
        MobCategory::Creature,
        &level,
        ChunkPos { x: 0, z: 0 },
        &mut NoFilter,
        &mut host,
        &mut rng,
        &t,
        &mut log,
    );
    assert!(!log.iter().any(|e| matches!(e, SpawnEvent::Picked { .. })));
    assert!(host.placed.is_empty());
    assert_eq!(rng.total(), expected_draws(&log, &level));
}

#[test]
fn a_player_column_inside_twenty_four_blocks_refuses_every_position() {
    let Some(t) = tables() else { return };
    let mut level = plains_level();
    // `getRandomPosWithin` draws y uniformly over the whole column, so a
    // single player at one y is only close to some of the drawn
    // positions. A column of players eight blocks apart keeps the nearest
    // one within 4 in y and ~11.5 in x and z of every position the walk
    // can reach: dy² + dx² + dz² <= 16 + 132 + 132 = 280, well under 576.
    level.players = (-64..=72)
        .step_by(8)
        .map(|y| (8.5, f64::from(y), 8.5))
        .collect();
    let mut refused = 0;
    for seed in 0..32i64 {
        let mut rng = Counting::new(seed);
        let mut host = RecordingHost::new(4, 4);
        let mut log = Vec::new();
        natural::spawn_category_for_chunk(
            MobCategory::Creature,
            &level,
            ChunkPos { x: 0, z: 0 },
            &mut NoFilter,
            &mut host,
            &mut rng,
            &t,
            &mut log,
        );
        assert!(host.placed.is_empty(), "seed {seed}: {log:?}");
        if log
            .iter()
            .any(|e| matches!(e, SpawnEvent::WrongDistance { .. }))
        {
            refused += 1;
        }
    }
    // The control: without it, "nothing spawned" would also be satisfied
    // by a run that never reached the distance gate at all.
    assert!(
        refused > 24,
        "only {refused} of 32 seeds reached the distance gate"
    );
}

/// `isRightDistanceToPlayerAndSpawnPoint`, driven directly, because the
/// two 24s it applies are the same constant on different quantities and a
/// whole-chunk pass cannot separate them: the y of the drawn position is
/// itself a draw.
#[test]
fn the_distance_gate_separates_the_player_rule_from_the_world_spawn_rule() {
    let mut level = plains_level();
    level.respawn = RespawnData {
        dimension: Dimension::Overworld,
        pos: BlockPos { x: 0, y: 64, z: 0 },
    };
    let chunk = ChunkPos { x: 0, z: 0 };
    let at = |x, y, z| BlockPos { x, y, z };

    // Gate 1 is `> 576.0`, strictly. 576.0 exactly is refused. The
    // position is 236 blocks above the respawn point so that gate 2
    // cannot be what answers.
    assert!(!natural::is_right_distance_to_player_and_spawn_point(
        &level,
        chunk,
        at(8, 300, 8),
        576.0
    ));
    assert!(natural::is_right_distance_to_player_and_spawn_point(
        &level,
        chunk,
        at(8, 300, 8),
        576.000_000_1
    ));

    // Gate 2, with the player distance well clear: the respawn point is
    // at (0, 64, 0), and `closerToCenterThan` adds +0.5 to all three of
    // ITS coordinates while the vector it is given carries +0.5 on x and
    // z only. So the y term is `(64.5 - y)²` where the x and z terms are
    // `(0.5 - (x + 0.5))²` — the half-block asymmetry is vanilla's.
    // 23 blocks up: (64.5 - 87)² = 506.25 < 576 -> refused.
    assert!(!natural::is_right_distance_to_player_and_spawn_point(
        &level,
        chunk,
        at(0, 87, 0),
        100_000.0
    ));
    // 25 blocks up: (64.5 - 89)² = 600.25 > 576 -> allowed.
    assert!(natural::is_right_distance_to_player_and_spawn_point(
        &level,
        chunk,
        at(0, 89, 0),
        100_000.0
    ));
    // The asymmetry itself: 24 blocks DOWN from the respawn block is
    // (64.5 - 40)² = 600.25 and passes, while 24 blocks UP is
    // (64.5 - 88)² = 552.25 and does not. A port that put +0.5 on the
    // position's y as well would answer the same for both.
    assert!(natural::is_right_distance_to_player_and_spawn_point(
        &level,
        chunk,
        at(0, 40, 0),
        100_000.0
    ));
    assert!(!natural::is_right_distance_to_player_and_spawn_point(
        &level,
        chunk,
        at(0, 88, 0),
        100_000.0
    ));

    // Gate 2 is skipped entirely in another dimension.
    level.dimension = Dimension::Nether;
    assert!(natural::is_right_distance_to_player_and_spawn_point(
        &level,
        chunk,
        at(0, 64, 0),
        100_000.0
    ));
    level.dimension = Dimension::Overworld;
    assert!(!natural::is_right_distance_to_player_and_spawn_point(
        &level,
        chunk,
        at(0, 64, 0),
        100_000.0
    ));

    // Gate 3: a position outside the chunk being spawned for needs
    // `canSpawnEntitiesInChunk`. The walk can leave the chunk, so this is
    // reachable rather than defensive.
    level.chunk_spawn_allowed = false;
    assert!(!natural::is_right_distance_to_player_and_spawn_point(
        &level,
        chunk,
        at(-3, 200, -3),
        100_000.0
    ));
    // ... and the same position inside the chunk is allowed without it.
    assert!(natural::is_right_distance_to_player_and_spawn_point(
        &level,
        chunk,
        at(3, 200, 3),
        100_000.0
    ));
}

#[test]
fn draws_match_the_decision_log_over_sixty_four_seeds() {
    let Some(t) = tables() else { return };
    let level = plains_level();
    let mut spawned_any = 0;
    for seed in 0..64i64 {
        let mut rng = Counting::new(seed);
        let mut host = RecordingHost::new(4, 4);
        let mut log = Vec::new();
        natural::spawn_category_for_chunk(
            MobCategory::Creature,
            &level,
            ChunkPos { x: 0, z: 0 },
            &mut NoFilter,
            &mut host,
            &mut rng,
            &t,
            &mut log,
        );
        assert_eq!(
            rng.total(),
            expected_draws(&log, &level),
            "seed {seed}: the generator and the event log disagree about the \
             draw count. log = {log:?}"
        );
        if !host.placed.is_empty() {
            spawned_any += 1;
        }
    }
    // A run in which nothing ever spawns would satisfy the assertion
    // above for free — the vacuous case this project keeps paying for.
    assert!(
        spawned_any > 8,
        "only {spawned_any} of 64 seeds spawned anything; the draw-count \
         check would be measuring an empty log"
    );
}

#[test]
fn the_pack_size_is_reassigned_after_the_weighted_draw() {
    let Some(t) = tables() else { return };
    let level = plains_level();
    let mut disagreed = 0;
    for seed in 0..256i64 {
        let mut rng = Counting::new(seed);
        let mut host = RecordingHost::new(64, 64);
        let mut log = Vec::new();
        natural::spawn_category_for_chunk(
            MobCategory::Creature,
            &level,
            ChunkPos { x: 0, z: 0 },
            &mut NoFilter,
            &mut host,
            &mut rng,
            &t,
            &mut log,
        );
        let mut provisional = None;
        for e in &log {
            match e {
                SpawnEvent::Attempt { pack_size, .. } => provisional = Some(*pack_size),
                SpawnEvent::Picked { pack_size, .. } => {
                    if provisional != Some(*pack_size) {
                        disagreed += 1;
                    }
                    // minCount..=maxCount, from the drawn SpawnerData.
                    if let SpawnEvent::Picked { data, .. } = e {
                        assert!(
                            (data.min_count..=data.max_count).contains(pack_size),
                            "seed {seed}: pack size {pack_size} outside {}..={}",
                            data.min_count,
                            data.max_count
                        );
                    }
                }
                _ => {}
            }
        }
    }
    // `Mth.ceil(nextFloat() * 4)` is 1..=4 and the plains CREATURE list
    // has entries as wide as 2..=6, so the two must differ sometimes; if
    // they never did, the reassignment would be unobservable and this
    // module could not be said to have transcribed it.
    assert!(
        disagreed > 0,
        "the provisional pack size never differed from the drawn one over \
         256 seeds — the reassignment at offset 295-323 is untested"
    );
}

#[test]
fn an_empty_category_list_costs_the_walk_and_no_weighted_draw() {
    let Some(t) = tables() else { return };
    let mut level = plains_level();
    // `the_void` has empty spawner lists in every category.
    level.biome = "the_void".into();
    level.rough_biome = "the_void".into();
    let mut rng = Counting::new(9);
    let mut host = RecordingHost::new(4, 4);
    let mut log = Vec::new();
    natural::spawn_category_for_chunk(
        MobCategory::Creature,
        &level,
        ChunkPos { x: 0, z: 0 },
        &mut NoFilter,
        &mut host,
        &mut rng,
        &t,
        &mut log,
    );
    assert!(log.contains(&SpawnEvent::NoMob), "{log:?}");
    assert!(!log.iter().any(|e| matches!(e, SpawnEvent::Picked { .. })));
    // The counted total must still match, which is the statement that the
    // empty list drew nothing: `Picked` contributes 2 and never appears.
    assert_eq!(rng.total(), expected_draws(&log, &level));
}

#[test]
fn an_uncreatable_type_ends_the_call_before_the_yaw_is_drawn() {
    let Some(t) = tables() else { return };
    let level = plains_level();
    let mut ended = 0;
    for seed in 0..64i64 {
        let mut rng = Counting::new(seed);
        let mut host = RecordingHost::new(4, 4);
        // Every CREATURE the plains list can yield.
        host.uncreatable = vec![
            "minecraft:sheep".into(),
            "minecraft:pig".into(),
            "minecraft:chicken".into(),
            "minecraft:cow".into(),
            "minecraft:horse".into(),
            "minecraft:donkey".into(),
        ];
        let mut log = Vec::new();
        natural::spawn_category_for_chunk(
            MobCategory::Creature,
            &level,
            ChunkPos { x: 0, z: 0 },
            &mut NoFilter,
            &mut host,
            &mut rng,
            &t,
            &mut log,
        );
        assert!(host.placed.is_empty(), "seed {seed}");
        if let Some(at) = log.iter().position(|e| *e == SpawnEvent::CreateFailed) {
            ended += 1;
            // Offset 383 is a bare `return`: nothing follows, not even the
            // next of the three attempts.
            assert_eq!(at, log.len() - 1, "seed {seed}: {log:?}");
        }
        // THE ORDERING ASSERTION. `expected_draws` charges a `nextFloat`
        // only for a `Spawned` event, so this equality holds exactly when
        // the yaw is drawn AFTER `create_mob` succeeds. Moving the draw
        // above the `create_mob` call — which is the natural way to write
        // it, and what a single fused host call would force — makes the
        // generator one ahead of the log on every seed that reaches here.
        assert_eq!(
            rng.total(),
            expected_draws(&log, &level),
            "seed {seed}: {log:?}"
        );
    }
    assert!(
        ended > 8,
        "only {ended} of 64 seeds reached getMobForSpawn; the ordering \
         assertion above is measuring nothing"
    );
}
