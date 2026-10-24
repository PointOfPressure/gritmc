//! `InhabitedTime` — how long a chunk has had a player near it.
//!
//! Reference (26.2), read from the jar with `javap -p -c -constants`:
//! `ServerChunkCache.tickChunks`, `ServerChunkCache.tickSpawningChunk`,
//! `ChunkMap.collectSpawningChunks`, `ChunkMap.playerIsCloseEnoughForSpawning`,
//! `ChunkAccess.incrementInhabitedTime`.
//!
//! # What it is for, which is why an invented value is worse than a zero
//!
//! Vanilla derives **local difficulty** from it — `DifficultyInstance` scales
//! with `chunk.getInhabitedTime()` — and through that the mob equipment, the
//! enchantment chances and the zombie-reinforcement rate in that chunk. A
//! world carried to vanilla with a fabricated `InhabitedTime` would be harder
//! or easier than the play that produced it. That is why
//! `docs/cold-start/persistence.md` left it at 0 rather than guessing, and why
//! it names this as the item that closes the chunk-NBT work.
//!
//! # THE COLD START'S DESCRIPTION OF THE RULE IS WRONG, AND THE BYTECODE SAYS SO
//!
//! It says: *"a per-chunk counter incremented once per tick per resident
//! player, as `ServerChunkCache.tickChunks` does."* It is not per player.
//!
//! ```text
//! ServerChunkCache.tickChunks():
//!    0: gameTime = level.getGameTime()
//!    8: delta = gameTime - lastInhabitedUpdate
//!   15: lastInhabitedUpdate = gameTime
//!   24: if (level.isDebug()) return
//!  167: chunkMap.collectSpawningChunks(list)
//!  239: for each: tickSpawningChunk(chunk, delta, categories, spawnState)
//!
//! ServerChunkCache.tickSpawningChunk(chunk, delta, ...):
//!    6: chunk.incrementInhabitedTime(delta)      <- FIRST, before anything
//!
//! ChunkMap.collectSpawningChunks(list):
//!       for each spawn-candidate chunk with a ticking LevelChunk:
//!           if (anyPlayerCloseEnoughForSpawningInternal(pos)) list.add(chunk)
//!
//! ChunkMap.playerIsCloseEnoughForSpawning(player, pos):
//!    1: if (player.isSpectator()) return false
//!   14: return euclideanDistanceSquared(pos, player.position()) < 16384.0
//! ```
//!
//! So the increment is **once per tick per CHUNK**, by the game-time delta,
//! for every chunk that at least one non-spectator player is within
//! `sqrt(16384) = 128` blocks of. A chunk with three players standing in it
//! gains one tick per tick, not three. A per-player counter would run 3x fast
//! there and would be indistinguishable from a correct one in every
//! single-player test — which is the whole reason this paragraph exists rather
//! than the implementation simply being right.
//!
//! Two further details the same bytecode settles:
//!
//! * **the increment is `delta`, not 1.** `tickChunks` computes it as the game
//!   time since the last call and stores the new one. It is 1 on a server
//!   keeping up, and it is larger after a pause — `/tick freeze`, a long GC,
//!   a server that stopped ticking chunks — so the counter tracks WORLD time
//!   rather than counting calls. A hardcoded 1 diverges exactly when the
//!   server was struggling, which is when nobody is looking.
//! * **spectators do not count**, and the check is on the player rather than
//!   on the chunk, so a chunk whose only nearby player is a spectator gains
//!   nothing.
//!
//! # What this server does and does not have
//!
//! It has the two things the rule needs: a game clock (`crate::tick`) and
//! every connected player's position (`crate::entities`). It does not have
//! spectator mode — `crate::status::game_type` is survival or creative — so
//! the `isSpectator` arm is unreachable and is written as a filter that can
//! never fire today rather than omitted, because omitting it would make the
//! rule silently wrong on the day spectator lands.
//!
//! It also does not have vanilla's ticket system, so "spawn-candidate chunk
//! with a ticking `LevelChunk`" has no exact analogue. What it has is the
//! resident tile set, and this counts a chunk when it is **resident and within
//! 128 blocks of a player** — which is the same predicate minus the ticket
//! filter. The difference is chunks that are loaded, near a player, and not
//! ticket-ticking in vanilla; on this server every resident chunk near a
//! player is one it is serving. Stated rather than hidden: it is an
//! over-count in exactly the case this server cannot yet distinguish.

use std::collections::HashMap;
use std::sync::Mutex;

/// `ChunkMap.playerIsCloseEnoughForSpawning`'s `ldc2_w 16384.0d`.
///
/// Squared, and compared with `<` (`dcmpg; ifge`), so a chunk at exactly 128
/// blocks does **not** count. Kept squared rather than as 128.0 so no square
/// root is taken per chunk per tick and so the constant in the code is the
/// constant in the bytecode.
pub const SPAWN_RANGE_SQUARED: f64 = 16384.0;

/// `ChunkMap.euclideanDistanceSquared(ChunkPos, Vec3)` — the chunk's centre
/// against a position, in three dimensions.
///
/// The chunk's x/z centre is `getMiddleBlockX/Z()`, which is `(cx << 4) + 8`,
/// and the y term is the player's own y against a chunk y of 0. That last part
/// is the one worth stating: the comparison is **not** flat, so a player at
/// y = 200 over a chunk directly below them is 200 blocks away by this measure
/// and outside the range, which is exactly what vanilla does.
#[must_use]
pub fn distance_squared(cx: i32, cz: i32, pos: (f64, f64, f64)) -> f64 {
    let mx = f64::from((cx << 4) + 8);
    let mz = f64::from((cz << 4) + 8);
    let dx = mx - pos.0;
    let dz = mz - pos.2;
    let dy = pos.1;
    dx * dx + dy * dy + dz * dz
}

/// Per-chunk `InhabitedTime`, and `ServerChunkCache.lastInhabitedUpdate`.
#[derive(Debug)]
pub struct Inhabited {
    /// `ChunkAccess.inhabitedTime`, per chunk. A `HashMap` and not a field on
    /// the chunk because this server's chunks are rebuilt from the canvas on
    /// every save (`World::save_to`), so a value living on the column would be
    /// discarded by the thing that is supposed to write it out.
    times: Mutex<HashMap<(i32, i32), i64>>,
    /// `ServerChunkCache.lastInhabitedUpdate`. The delta is game time since
    /// the previous sweep, so this is the previous sweep's game time.
    ///
    /// `i64::MIN` is the unset marker rather than 0: a server whose first
    /// sweep ran at game time 500 000 — a loaded world — would otherwise
    /// credit every chunk near a player with 500 000 ticks of habitation on
    /// its first tick.
    last_update: Mutex<i64>,
}

impl Default for Inhabited {
    fn default() -> Self {
        Self::new()
    }
}

impl Inhabited {
    #[must_use]
    pub fn new() -> Self {
        Self {
            times: Mutex::new(HashMap::new()),
            last_update: Mutex::new(i64::MIN),
        }
    }

    /// The process-wide one, for the tick loop and the save path.
    #[must_use]
    pub fn shared() -> &'static Self {
        static SHARED: std::sync::OnceLock<Inhabited> = std::sync::OnceLock::new();
        SHARED.get_or_init(Self::new)
    }

    /// `ServerChunkCache.tickChunks`' habitation half: compute the delta, then
    /// credit every chunk in `resident` that a player is close enough to.
    ///
    /// `players` is every non-spectator player's position. Returns how many
    /// chunks were credited, which is what a test asserts on — a sweep that
    /// credited nothing and a sweep that ran are otherwise the same
    /// observation.
    ///
    /// The first call establishes `lastInhabitedUpdate` and credits nothing,
    /// which is not vanilla's behaviour (its field starts at 0 and a fresh
    /// world's game time is 0, so its first delta is 0 too) but is what makes
    /// a **loaded** world behave the same way. See `last_update`'s doc.
    pub fn sweep(
        &self,
        game_time: i64,
        resident: impl IntoIterator<Item = (i32, i32)>,
        players: &[(f64, f64, f64)],
    ) -> usize {
        let delta = {
            let Ok(mut last) = self.last_update.lock() else {
                return 0;
            };
            let previous = *last;
            *last = game_time;
            if previous == i64::MIN {
                return 0;
            }
            game_time - previous
        };
        // A delta of zero credits nothing and is not an error: two sweeps in
        // one game tick is what a paused clock produces, and vanilla's
        // `incrementInhabitedTime(0)` is likewise a no-op. A NEGATIVE delta is
        // a clock that went backwards — `/time set` does not move game time,
        // but a reloaded world whose `level.dat` is older than this process's
        // counter would — and crediting it would run the counter down.
        if delta <= 0 || players.is_empty() {
            return 0;
        }
        let Ok(mut times) = self.times.lock() else {
            return 0;
        };
        let mut credited = 0;
        for (cx, cz) in resident {
            if players
                .iter()
                .any(|p| distance_squared(cx, cz, *p) < SPAWN_RANGE_SQUARED)
            {
                *times.entry((cx, cz)).or_insert(0) += delta;
                credited += 1;
            }
        }
        credited
    }

    /// What to write into a chunk's `InhabitedTime`.
    ///
    /// `saved` is whatever the chunk already on disk carried, which for a
    /// world vanilla made is real habitation this server must not throw away.
    /// The answer is the sum: this counter holds only what happened while this
    /// process was running.
    #[must_use]
    pub fn saved_value(&self, cx: i32, cz: i32, saved: i64) -> i64 {
        let ours = self
            .times
            .lock()
            .ok()
            .and_then(|t| t.get(&(cx, cz)).copied())
            .unwrap_or(0);
        saved.saturating_add(ours)
    }

    /// Forget a chunk's accumulation, once it has been folded into a file.
    ///
    /// **Without this the counter double-credits on every save**, because
    /// `saved_value` adds this process's total to what is on disk and the next
    /// save would read that total back and add it again. Called by the save
    /// path immediately after the value is written.
    pub fn take(&self, cx: i32, cz: i32) {
        if let Ok(mut times) = self.times.lock() {
            times.remove(&(cx, cz));
        }
    }

    /// How many chunks are carrying unsaved habitation. For a test's
    /// population assertion.
    #[must_use]
    pub fn tracked(&self) -> usize {
        self.times.lock().map_or(0, |t| t.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The range, at the boundary and either side of it.
    ///
    /// 128 blocks is `sqrt(16384)`, and the comparison is `<`, so a chunk at
    /// exactly the range does not count. A `<=` passes every other test here.
    #[test]
    fn the_range_is_128_blocks_exclusive_and_is_not_flat() {
        // Chunk (0,0)'s centre is (8, 8). A player at (8, 0, 8) is at zero
        // distance.
        assert!((distance_squared(0, 0, (8.0, 0.0, 8.0)) - 0.0).abs() < 1e-9);
        // Exactly 128 blocks east: 128^2 = 16384, and `<` excludes it.
        let at_range = distance_squared(0, 0, (136.0, 0.0, 8.0));
        assert!((at_range - SPAWN_RANGE_SQUARED).abs() < 1e-9);
        assert!(
            at_range >= SPAWN_RANGE_SQUARED,
            "the comparison is `<`, so exactly 128 blocks is out of range"
        );
        assert!(distance_squared(0, 0, (135.0, 0.0, 8.0)) < SPAWN_RANGE_SQUARED);
        // The y term is real: a player 200 blocks up over the chunk is out of
        // range even though they are directly above it. A flat distance would
        // report 0 here, which is the plausible wrong answer.
        assert!(distance_squared(0, 0, (8.0, 200.0, 8.0)) > SPAWN_RANGE_SQUARED);
    }

    /// **Two players in one chunk credit it once, not twice.**
    ///
    /// This is the assertion the cold start's description would have failed:
    /// "once per tick per resident player" gives 2 here. Every single-player
    /// test gives the same answer either way, which is why this one exists.
    #[test]
    fn a_chunk_with_two_players_in_it_gains_one_tick_per_tick() {
        let inhabited = Inhabited::new();
        let here = [(8.0, 64.0, 8.0), (9.0, 64.0, 9.0)];
        assert_eq!(
            inhabited.sweep(100, [(0, 0)], &here),
            0,
            "the first sweep sets the clock"
        );
        assert_eq!(inhabited.sweep(101, [(0, 0)], &here), 1);
        assert_eq!(inhabited.saved_value(0, 0, 0), 1, "one tick, not two");
        assert_eq!(inhabited.sweep(102, [(0, 0)], &here), 1);
        assert_eq!(inhabited.saved_value(0, 0, 0), 2);
    }

    /// The increment is the game-time DELTA, not 1.
    ///
    /// A server that stopped ticking for 500 ticks and then resumed credits
    /// 500, because `tickChunks` differences the game clock. A hardcoded 1
    /// passes the test above and fails this one.
    #[test]
    fn a_gap_in_the_clock_credits_the_whole_gap() {
        let inhabited = Inhabited::new();
        let here = [(8.0, 64.0, 8.0)];
        inhabited.sweep(1000, [(0, 0)], &here);
        assert_eq!(inhabited.sweep(1500, [(0, 0)], &here), 1);
        assert_eq!(inhabited.saved_value(0, 0, 0), 500);
    }

    /// A chunk out of range gains nothing, and a sweep with no players at all
    /// gains nothing — which are different failures and are asserted apart.
    #[test]
    fn distance_and_emptiness_are_two_different_reasons_to_credit_nothing() {
        let inhabited = Inhabited::new();
        let far = [(1000.0, 64.0, 1000.0)];
        inhabited.sweep(100, [(0, 0)], &far);
        assert_eq!(inhabited.sweep(101, [(0, 0)], &far), 0, "out of range");
        assert_eq!(inhabited.saved_value(0, 0, 0), 0);
        assert_eq!(inhabited.tracked(), 0);

        assert_eq!(inhabited.sweep(102, [(0, 0)], &[]), 0, "nobody online");
    }

    /// **The saved value is the disk value plus ours, and taking it stops the
    /// next save adding ours again.**
    ///
    /// The double-credit this pins is not hypothetical: `saved_value` reads
    /// what is on disk, and the save path writes the sum back to that same
    /// place. Without `take` the second save reads its own first answer.
    #[test]
    fn a_second_save_does_not_credit_the_same_ticks_twice() {
        let inhabited = Inhabited::new();
        let here = [(8.0, 64.0, 8.0)];
        inhabited.sweep(0, [(0, 0)], &here);
        for t in 1..=10 {
            inhabited.sweep(t, [(0, 0)], &here);
        }
        // A world vanilla played on for 5000 ticks before we opened it.
        let first = inhabited.saved_value(0, 0, 5000);
        assert_eq!(first, 5010);
        inhabited.take(0, 0);
        assert_eq!(inhabited.tracked(), 0);
        assert_eq!(
            inhabited.saved_value(0, 0, first),
            5010,
            "the second save must not add the same ten ticks again"
        );
    }

    /// A clock that goes backwards credits nothing rather than running the
    /// counter down.
    #[test]
    fn a_backwards_clock_credits_nothing() {
        let inhabited = Inhabited::new();
        let here = [(8.0, 64.0, 8.0)];
        inhabited.sweep(1000, [(0, 0)], &here);
        assert_eq!(inhabited.sweep(900, [(0, 0)], &here), 0);
        assert_eq!(inhabited.saved_value(0, 0, 0), 0);
    }
}
