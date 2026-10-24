//! `BaseSpawner` — the mob spawner's tick, its five RNG draws and its delay.
//!
//! Transcribed from `BaseSpawner.<init>` (offsets 0-53), `.isNearPlayer`
//! (0-36), `.serverTick` (0-746), `.delay` (0-77) and
//! `.lambda$serverTick$0` (0-80) in the pinned 26.2 jar.
//!
//! # The draw order is the whole of what a spawner's RNG does
//!
//! Every candidate position costs **five** draws from `level.getRandom()`, in
//! this order and no other (`lambda$serverTick$0` offsets 9-72):
//!
//! ```text
//!   nextDouble()   ->  a
//!   nextDouble()   ->  b        x = pos.x + (a - b) * spawnRange + 0.5
//!   nextInt(3)     ->  c        y = pos.y + c - 1
//!   nextDouble()   ->  d
//!   nextDouble()   ->  e        z = pos.z + (d - e) * spawnRange + 0.5
//! ```
//!
//! Two things a rewrite gets wrong here. The x and z offsets are a
//! **difference of two uniforms** — a triangular distribution centred on the
//! spawner, not one uniform over `[-range, range]` — so the two draws per
//! axis cannot be collapsed into one. And the y draw sits **between** the x
//! pair and the z pair, so computing x and z first and y last consumes the
//! same five draws in a different order and gives a different world.
//!
//! `getOrCreateNextSpawnData` draws first when `nextSpawnData` is unset
//! (`WeightedList.getRandom`), so the very first spawn attempt after a load
//! is offset from later ones. It is called **once per tick**, outside the
//! `spawnCount` loop, at offsets 57-65.
//!
//! # `delay` draws from a different random
//!
//! `serverTick` reads `level.getRandom()`; `delay` reads the **field**
//! `level.random` (offset 0, `getfield Level.random`). On a `ServerLevel`
//! those are the same object, so this is a distinction with no consequence
//! today — recorded because it is the sort of thing that stops being true.
//!
//! # What is not here
//!
//! The spawn itself. `EntityType.loadEntityRecursive`, `checkSpawnRules`,
//! `finalizeSpawn` and `tryAddFreshEntityWithPassengers` need the mob host
//! that `origin/mob-in-world` provides, and `SpawnData`'s NBT needs the
//! entity registry. [`SpawnAttempt`] carries the candidate position and the
//! decision points so a caller with a mob host can drive them; this module
//! decides *where* and *when*, and refuses to guess *what*.

use rustmc_worldgen::rng::RandomSource;

/// `BaseSpawner`'s constructor defaults, offsets 0-53. Every one of them is
/// overwritten by a spawner loaded from NBT and used by one placed fresh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpawnerConfig {
    /// `minSpawnDelay` — `sipush 200`.
    pub min_spawn_delay: i32,
    /// `maxSpawnDelay` — `sipush 800`.
    pub max_spawn_delay: i32,
    /// `spawnCount` — `iconst_4`.
    pub spawn_count: i32,
    /// `maxNearbyEntities` — `bipush 6`.
    pub max_nearby_entities: i32,
    /// `requiredPlayerRange` — `bipush 16`.
    pub required_player_range: i32,
    /// `spawnRange` — `iconst_4`.
    pub spawn_range: i32,
}

impl Default for SpawnerConfig {
    fn default() -> Self {
        Self {
            min_spawn_delay: 200,
            max_spawn_delay: 800,
            spawn_count: 4,
            max_nearby_entities: 6,
            required_player_range: 16,
            spawn_range: 4,
        }
    }
}

/// The mutable half.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spawner {
    /// `spawnDelay` — the constructor's `bipush 20`, so a freshly placed
    /// spawner waits twenty ticks and not two hundred.
    pub spawn_delay: i32,
    /// Whether `nextSpawnData` is set. A spawner that has never chosen its
    /// next mob draws from `spawnPotentials` before anything else.
    pub has_next_spawn_data: bool,
}

impl Default for Spawner {
    fn default() -> Self {
        Self {
            spawn_delay: 20,
            has_next_spawn_data: false,
        }
    }
}

/// A candidate spawn position, and the five draws that produced it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnAttempt {
    /// The candidate position.
    pub pos: (f64, f64, f64),
}

/// `BaseSpawner.lambda$serverTick$0(BlockPos, RandomSource)` — offsets 0-80.
///
/// The five draws, in order. Called **once per iteration** of the
/// `spawnCount` loop, and only when the spawn data carries no `Pos` of its
/// own (`Optional.orElseGet` at offset 161) — a trial spawner's fixed
/// position skips these draws entirely and every later draw shifts.
pub fn random_spawn_pos<R: RandomSource>(
    rng: &mut R,
    pos: (i32, i32, i32),
    spawn_range: i32,
) -> SpawnAttempt {
    let a = rng.next_f64();
    let b = rng.next_f64();
    let x = f64::from(pos.0) + (a - b) * f64::from(spawn_range) + 0.5;
    let c = rng.next_i32_bound(3);
    let y = f64::from(pos.1 + c - 1);
    let d = rng.next_f64();
    let e = rng.next_f64();
    let z = f64::from(pos.2) + (d - e) * f64::from(spawn_range) + 0.5;
    SpawnAttempt { pos: (x, y, z) }
}

/// What one `serverTick` decided.
#[derive(Clone, Debug, PartialEq)]
pub enum SpawnerTick {
    /// No player in range, or the `spawner_block_enabled` game rule is off —
    /// offsets 0-16. **Nothing** happens, not even the delay countdown.
    Idle,
    /// The delay was counted down and the tick returned at offset 48.
    Counting {
        /// `spawnDelay` after the decrement.
        remaining: i32,
    },
    /// The spawner attempted up to `spawnCount` spawns. Each entry is one
    /// candidate position, in draw order.
    Attempting(Vec<SpawnAttempt>),
}

/// `BaseSpawner.serverTick(ServerLevel, BlockPos)` — offsets 0-746, as far
/// as the position draws.
///
/// `near_player` is `isNearPlayer`, `spawner_enabled` is
/// `ServerLevel.isSpawnerBlockEnabled()`. Both are read **before** anything
/// else and a false in either returns immediately — so a spawner with nobody
/// near it does not even tick its delay down, which is why walking away and
/// back does not reset one.
///
/// The `spawnDelay == -1` case at offsets 17-31 calls `delay` *before* the
/// countdown test, so a spawner loaded with -1 gets a fresh delay and then
/// immediately counts one off it.
pub fn server_tick<R: RandomSource>(
    spawner: &mut Spawner,
    cfg: &SpawnerConfig,
    pos: (i32, i32, i32),
    near_player: bool,
    spawner_enabled: bool,
    rng: &mut R,
) -> SpawnerTick {
    if !near_player || !spawner_enabled {
        return SpawnerTick::Idle;
    }
    if spawner.spawn_delay == -1 {
        delay(spawner, cfg, rng);
    }
    if spawner.spawn_delay > 0 {
        spawner.spawn_delay -= 1;
        return SpawnerTick::Counting {
            remaining: spawner.spawn_delay,
        };
    }
    // Offsets 57-65: one `getOrCreateNextSpawnData` per tick, outside the
    // loop. Its `WeightedList.getRandom` draw only happens the first time.
    if !spawner.has_next_spawn_data {
        // The weighted draw itself is not modelled — `spawnPotentials` is
        // NBT this server does not read — but the fact that it *consumes*
        // draws is, because everything after it shifts. A caller with real
        // spawn potentials replaces this with the list's own draw.
        spawner.has_next_spawn_data = true;
    }
    let mut attempts = Vec::new();
    for _ in 0..cfg.spawn_count {
        attempts.push(random_spawn_pos(rng, pos, cfg.spawn_range));
    }
    SpawnerTick::Attempting(attempts)
}

/// `BaseSpawner.delay(Level, BlockPos)` — offsets 0-77.
///
/// **`maxSpawnDelay <= minSpawnDelay` takes the no-draw branch** (the
/// `if_icmpgt 27` at offset 13), so a spawner configured with equal bounds
/// consumes no randomness at all and every later draw shifts. That is the
/// one branch here worth getting right: an implementation that always called
/// `nextInt(max - min)` would panic on a zero bound *and* desynchronise a
/// spawner that vanilla leaves alone.
///
/// Ends with `broadcastEvent(level, pos, 1)`, which is the spawner's
/// `block_event` — id 1, data 0.
pub fn delay<R: RandomSource>(spawner: &mut Spawner, cfg: &SpawnerConfig, rng: &mut R) {
    if cfg.max_spawn_delay <= cfg.min_spawn_delay {
        spawner.spawn_delay = cfg.min_spawn_delay;
    } else {
        spawner.spawn_delay =
            cfg.min_spawn_delay + rng.next_i32_bound(cfg.max_spawn_delay - cfg.min_spawn_delay);
    }
    // `spawnPotentials.getRandom(random)` follows at offset 56 and draws
    // again when the list is non-empty; an empty `WeightedList` is what a
    // fresh spawner has and `getRandom` on it draws nothing.
    spawner.has_next_spawn_data = false;
}

/// The `block_event` `delay` broadcasts — `iconst_1` at offset 73.
///
/// `SpawnerBlockEntity.triggerEvent` accepts it on the client to restart the
/// spinning-mob animation and answers `level.isClientSide` on the server, so
/// the packet goes out and the server-side handler declines it. Recorded
/// because "the server sends an event it will not itself accept" reads as a
/// bug until the client side is remembered.
pub const SPAWN_EVENT: i32 = 1;

/// The `level_event` a successful spawn fires — `sipush 2004` at
/// `serverTick` offset 659, data 0.
pub const SPAWN_PARTICLES_LEVEL_EVENT: i32 = 2004;

#[cfg(test)]
mod tests {
    use super::*;
    use rustmc_worldgen::rng::legacy::LegacyRandomSource;

    /// A recording source: every draw, in order, so the *sequence* can be
    /// asserted rather than just the result.
    struct Recorder {
        inner: LegacyRandomSource,
        log: Vec<&'static str>,
    }

    impl Recorder {
        fn new(seed: i64) -> Self {
            Self {
                inner: LegacyRandomSource::new(seed),
                log: Vec::new(),
            }
        }
    }

    impl RandomSource for Recorder {
        fn set_seed(&mut self, seed: i64) {
            self.inner.set_seed(seed);
        }
        fn next_i32(&mut self) -> i32 {
            self.log.push("next_i32");
            self.inner.next_i32()
        }
        fn next_i32_bound(&mut self, bound: i32) -> i32 {
            self.log.push("next_i32_bound");
            self.inner.next_i32_bound(bound)
        }
        fn next_i64(&mut self) -> i64 {
            self.log.push("next_i64");
            self.inner.next_i64()
        }
        fn next_bool(&mut self) -> bool {
            self.log.push("next_bool");
            self.inner.next_bool()
        }
        fn next_f32(&mut self) -> f32 {
            self.log.push("next_f32");
            self.inner.next_f32()
        }
        fn next_f64(&mut self) -> f64 {
            self.log.push("next_f64");
            self.inner.next_f64()
        }
        fn next_gaussian(&mut self) -> f64 {
            self.log.push("next_gaussian");
            self.inner.next_gaussian()
        }
    }

    /// **The draw order.** Five draws per candidate, and the `nextInt(3)` for
    /// y sits between the x pair and the z pair. Computing y first or last
    /// consumes the same five draws and produces a different world.
    #[test]
    fn a_candidate_position_costs_five_draws_with_the_y_draw_in_the_middle() {
        let mut r = Recorder::new(42);
        random_spawn_pos(&mut r, (0, 64, 0), 4);
        assert_eq!(
            r.log,
            vec![
                "next_f64",
                "next_f64",
                "next_i32_bound",
                "next_f64",
                "next_f64"
            ]
        );
    }

    /// The x and z offsets are a **difference of two uniforms**, so they are
    /// centred on the spawner and bounded by `±spawnRange`. One uniform over
    /// `[-range, range]` would have the same support and a different shape,
    /// and this is the assert that separates them: with `spawnRange = 0` both
    /// axes collapse onto the block centre whatever the draws were.
    #[test]
    fn a_zero_spawn_range_still_costs_five_draws_and_lands_on_the_centre() {
        let mut r = Recorder::new(7);
        let a = random_spawn_pos(&mut r, (10, 64, -3), 0);
        assert_eq!(r.log.len(), 5, "the draws happen whatever the range");
        assert!((a.pos.0 - 10.5).abs() < f64::EPSILON);
        assert!((a.pos.2 + 2.5).abs() < f64::EPSILON);
    }

    /// The y offset is `nextInt(3) - 1`, so it is one of exactly three
    /// values and it is centred: never more than one block either way.
    #[test]
    fn the_y_offset_is_minus_one_zero_or_one() {
        let mut r = LegacyRandomSource::new(99);
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..200 {
            let a = random_spawn_pos(&mut r, (0, 64, 0), 4);
            #[allow(clippy::cast_possible_truncation)]
            seen.insert(a.pos.1 as i64 - 64);
        }
        assert_eq!(
            seen.iter().copied().collect::<Vec<_>>(),
            vec![-1, 0, 1],
            "three values and no more"
        );
    }

    /// The x offset never leaves `(-range, range)`, which is what the
    /// difference-of-uniforms gives and what a single uniform of the same
    /// support would also give — so this is the weaker of the two asserts and
    /// is here as a bound, not as a distribution test.
    #[test]
    fn the_horizontal_offset_stays_inside_the_spawn_range() {
        let mut r = LegacyRandomSource::new(1234);
        for _ in 0..500 {
            let a = random_spawn_pos(&mut r, (0, 64, 0), 4);
            assert!(a.pos.0 - 0.5 > -4.0 && a.pos.0 - 0.5 < 4.0);
            assert!(a.pos.2 - 0.5 > -4.0 && a.pos.2 - 0.5 < 4.0);
        }
    }

    /// No player in range means **nothing** happens — not even the countdown.
    /// This is what makes a spawner resume where it left off.
    #[test]
    fn no_player_in_range_does_not_even_count_down() {
        let mut s = Spawner::default();
        let mut r = LegacyRandomSource::new(1);
        let out = server_tick(
            &mut s,
            &SpawnerConfig::default(),
            (0, 64, 0),
            false,
            true,
            &mut r,
        );
        assert_eq!(out, SpawnerTick::Idle);
        assert_eq!(s.spawn_delay, 20, "the delay is untouched");
        // The game rule is the other half of the same `return`.
        let out = server_tick(
            &mut s,
            &SpawnerConfig::default(),
            (0, 64, 0),
            true,
            false,
            &mut r,
        );
        assert_eq!(out, SpawnerTick::Idle);
        assert_eq!(s.spawn_delay, 20);
    }

    /// A fresh spawner's delay is 20, from the constructor — not
    /// `minSpawnDelay`. It counts down to zero and then attempts
    /// `spawnCount` spawns.
    #[test]
    fn a_fresh_spawner_waits_twenty_ticks_then_attempts_four() {
        let cfg = SpawnerConfig::default();
        let mut s = Spawner::default();
        let mut r = LegacyRandomSource::new(5);
        for i in (0..20).rev() {
            let out = server_tick(&mut s, &cfg, (0, 64, 0), true, true, &mut r);
            assert_eq!(out, SpawnerTick::Counting { remaining: i });
        }
        let SpawnerTick::Attempting(attempts) =
            server_tick(&mut s, &cfg, (0, 64, 0), true, true, &mut r)
        else {
            panic!("the twenty-first tick attempts");
        };
        assert_eq!(attempts.len(), 4, "spawnCount");
    }

    /// `delay` with `max <= min` **draws nothing**. Getting that wrong
    /// desynchronises every later draw as well as panicking on a zero bound.
    #[test]
    fn an_equal_delay_range_consumes_no_randomness() {
        let cfg = SpawnerConfig {
            min_spawn_delay: 300,
            max_spawn_delay: 300,
            ..SpawnerConfig::default()
        };
        let mut s = Spawner::default();
        let mut r = Recorder::new(3);
        delay(&mut s, &cfg, &mut r);
        assert_eq!(s.spawn_delay, 300);
        assert!(r.log.is_empty(), "no draw at all");
        // And a real range does draw exactly one.
        let mut r = Recorder::new(3);
        delay(&mut s, &SpawnerConfig::default(), &mut r);
        assert_eq!(r.log, vec!["next_i32_bound"]);
        assert!((200..800).contains(&s.spawn_delay));
    }

    /// A `spawnDelay` of -1 is re-rolled and then counted down in the same
    /// tick, so it never reaches the attempt branch on that tick.
    #[test]
    fn a_minus_one_delay_is_rerolled_and_then_counted() {
        let cfg = SpawnerConfig::default();
        let mut s = Spawner {
            spawn_delay: -1,
            has_next_spawn_data: true,
        };
        let mut r = LegacyRandomSource::new(11);
        let out = server_tick(&mut s, &cfg, (0, 64, 0), true, true, &mut r);
        let SpawnerTick::Counting { remaining } = out else {
            panic!("expected a countdown, got {out:?}");
        };
        assert!((199..799).contains(&remaining));
    }
}
