//! The autosave, on vanilla's own trigger and in vanilla's own order.
//!
//! Reference (26.2), read from the jar with `javap -p -c -constants`:
//! `MinecraftServer.tickServer`, `MinecraftServer.autoSave`,
//! `MinecraftServer.computeNextAutosaveInterval`,
//! `MinecraftServer.saveEverything` and `MinecraftServer.saveAllChunks`.
//!
//! # What was here before, and the two things wrong with it
//!
//! `main.rs` spawned a thread that slept `--autosave-seconds` (default 300)
//! and then wrote `level.dat` and the chunks. Both halves of that are
//! divergences, and the second is a data-loss bug rather than a fidelity one.
//!
//! **One: the trigger is a tick count, not a wall clock.** `tickServer`
//! decrements `ticksUntilAutosave` once per tick and calls `autoSave()` when
//! it reaches zero, so on a server that cannot hold 20 Hz the interval
//! stretches with the world's own clock. A wall-clock thread instead fires on
//! schedule while the world falls behind, which is the one case where the two
//! measurably differ — and it is the case a saturated server is always in.
//!
//! **Two: the order was backwards, and its own comment said so.** The old
//! code wrote `level.dat` first, justified as "a world directory whose chunks
//! are newer than its `level.dat` is one vanilla opens at the wrong time of
//! day". That is a correct statement of the hazard and writing `level.dat`
//! first is what *creates* it: the file carrying `Time` and `DayTime` is then
//! the oldest thing in the directory. Vanilla writes it last —
//! `saveAllChunks` runs every level's `save` and only then calls
//! `storageSource.saveDataTag`.
//!
//! **Three, and it is the one that loses data: connected players were never
//! saved at all.** `saveEverything` opens with `getPlayerList().saveAll()`,
//! before it touches a chunk. This server saved a player only on disconnect
//! (`crate::status`, `ChunkMap.removeEntity`'s arm), so a `kill -9`, a power
//! cut or a crash put every connected player back to wherever they were when
//! they last logged out — which can be hours and a world away, in a world
//! whose chunks were saved five minutes ago. The autosave bounded the loss
//! for the terrain and not for the people standing on it.
//!
//! # The interval
//!
//! ```text
//! MinecraftServer.<init>            ticksUntilAutosave = 6000
//! AUTOSAVE_INTERVAL                 6000        (the field, and the initial value)
//! MIMINUM_AUTOSAVE_TICKS            100         (spelled that way in the jar)
//! computeNextAutosaveInterval():
//!     f = tickRateManager.isSprinting()
//!             ? NANOSECONDS_PER_SECOND / (getAverageTickTimeNanos() + 1)
//!             : tickRateManager.tickrate()
//!     return Math.max(100, (int)(f * 300.0f))
//! autoSave():
//!     ticksUntilAutosave = computeNextAutosaveInterval()
//!     LOGGER.debug("Autosave started")
//!     saveEverything(true, false, false)
//!     LOGGER.debug("Autosave finished")
//! ```
//!
//! So **6000 is not a constant this server may copy — it is the value at the
//! default tick rate.** `AUTOSAVE_INTERVAL = 6000` is the field's initial
//! value and nothing reads it again; every later interval comes out of
//! `computeNextAutosaveInterval`, which is 300 seconds' worth of ticks at
//! whatever rate the server is running, floored at 100. At the default 20.0
//! that is `max(100, 6000)` = 6000, which is why the number looks like a
//! constant from outside. A server at `/tick rate 5` autosaves every 1500
//! ticks — still every 300 seconds — and one at `/tick rate 0.3` autosaves
//! every 100, which is the floor doing its job.
//!
//! `ServerTickRateManager` is not modelled here, so `tickrate()` is the
//! default 20.0 and `isSprinting()` is false. That is a *value* this server
//! does not vary rather than a branch it does not have, so the function takes
//! the rate as an argument and the caller supplies 20.0; when `/tick rate`
//! lands it passes the real one and nothing here changes.
//!
//! # What this module does NOT do
//!
//! `saveAllChunks` also writes the scoreboard (`ScoreboardSaveData`) and the
//! `SavedDataStorage` set. Neither exists in this server — `crate::levelstore`
//! writes the nine `data/minecraft/*.dat` files as part of `level.dat`'s own
//! save, which is where the equivalent state lives here. `warnOnLowDiskSpace`
//! is not modelled: it is a chat message to operators and has no effect on
//! what lands on disk.

use std::sync::atomic::{AtomicI32, Ordering};

/// `MinecraftServer.AUTOSAVE_INTERVAL` — `sipush 6000` in the constructor,
/// which is the only place the value is used.
///
/// It is the **initial** `ticksUntilAutosave`, not the interval: the first
/// autosave of a server's life comes 6000 ticks after boot whatever the tick
/// rate, and every one after it comes from [`next_interval`].
pub const AUTOSAVE_INTERVAL: i32 = 6000;

/// `MinecraftServer.MIMINUM_AUTOSAVE_TICKS` — 100, and the misspelling is
/// vanilla's own, kept so a grep for the constant finds this line.
pub const MIMINUM_AUTOSAVE_TICKS: i32 = 100;

/// The `ldc 300.0f` in `computeNextAutosaveInterval` — five minutes of world
/// time, which is what the interval actually expresses.
pub const AUTOSAVE_SECONDS: f32 = 300.0;

/// `MinecraftServer.computeNextAutosaveInterval()`.
///
/// `Math.max(MIMINUM_AUTOSAVE_TICKS, (int)(rate * 300.0f))`. The cast is
/// Java's narrowing float-to-int, which truncates toward zero, and `as i32`
/// in Rust is the same for values in range — and saturates rather than
/// producing Java's `Integer.MAX_VALUE`/`MIN_VALUE` for values out of it,
/// which differs only for a tick rate above ~7.1 million.
///
/// The `sipush 300; istore_2` immediately before the `ldc 300.0f` is a dead
/// store in the bytecode; there is one 300 in this function, not two.
#[must_use]
pub fn next_interval(rate: f32) -> i32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Java's (int) narrowing cast, which truncates toward zero"
    )]
    let ticks = (rate * AUTOSAVE_SECONDS) as i32;
    ticks.max(MIMINUM_AUTOSAVE_TICKS)
}

/// `ServerTickRateManager.tickrate()`'s default — `20.0f`, the rate this
/// server runs at and does not yet vary.
pub const DEFAULT_TICKRATE: f32 = 20.0;

/// `MinecraftServer.ticksUntilAutosave`, and the decrement `tickServer` does
/// to it.
///
/// Held as an atomic because the tick closure is a `move` into another
/// thread; vanilla gets the same exclusivity from being single-threaded.
#[derive(Debug)]
pub struct AutosaveClock {
    ticks_until: AtomicI32,
    /// The interval to reload on each fire. Vanilla recomputes it from the
    /// tick rate every time; this server has one rate, so the value is
    /// computed once and stored — and an operator override (see
    /// `crate::main`'s `--autosave-seconds`) lands here rather than being
    /// re-derived, so the two cannot disagree.
    interval: i32,
}

impl AutosaveClock {
    /// A clock whose first fire is `min(AUTOSAVE_INTERVAL, interval)` ticks
    /// away and whose every later fire is `interval` ticks.
    ///
    /// **The first and the rest are deliberately different, and the `min` is
    /// vanilla's own.** The constructor stores a literal 6000 into
    /// `ticksUntilAutosave`; `autoSave` reloads it from
    /// `computeNextAutosaveInterval()`. Those coincide at the default tick
    /// rate and diverge at any other, and vanilla reconciles them in
    /// `onTickRateChanged`:
    ///
    /// ```text
    ///   0: i = computeNextAutosaveInterval()
    ///   5: if (i >= ticksUntilAutosave) goto 18
    ///  13: ticksUntilAutosave = i
    /// ```
    ///
    /// — a one-directional clamp: a **shorter** interval pulls the pending
    /// save in, a longer one leaves it alone. A server started at a rate
    /// whose interval is under 6000 therefore does not wait 6000 ticks for
    /// its first save, and this constructor is that same clamp applied once,
    /// at construction, because that is when this server learns its interval.
    ///
    /// It is also what makes `--autosave-seconds 2` mean what an operator
    /// reading the flag would expect. Without the clamp the first save would
    /// land 300 seconds in whatever the flag said, and
    /// `ci/t2-player-survives-a-restart.sh` — which passes exactly that —
    /// would be waiting on a save that had not been scheduled yet.
    #[must_use]
    pub const fn new(interval: i32) -> Self {
        let first = if interval < AUTOSAVE_INTERVAL {
            interval
        } else {
            AUTOSAVE_INTERVAL
        };
        Self {
            ticks_until: AtomicI32::new(first),
            interval,
        }
    }

    /// The interval this clock reloads, for a caller that wants to say so.
    #[must_use]
    pub fn interval(&self) -> i32 {
        self.interval
    }

    /// Ticks remaining, for a test that wants to see the decrement.
    #[must_use]
    pub fn remaining(&self) -> i32 {
        self.ticks_until.load(Ordering::Relaxed)
    }

    /// `tickServer`'s two lines: decrement, and fire at or below zero.
    ///
    /// ```text
    /// 149: ticksUntilAutosave = ticksUntilAutosave - 1
    /// 159: if (ticksUntilAutosave > 0) goto 170
    /// 166: autoSave()
    /// ```
    ///
    /// The comparison is `ifgt`, so **zero fires** — an off-by-one here would
    /// be invisible in every test that only counts saves over a long run and
    /// visible in exactly one that counts ticks to the first.
    pub fn due(&self) -> bool {
        if self.ticks_until.fetch_sub(1, Ordering::Relaxed) - 1 > 0 {
            return false;
        }
        // `autoSave()`'s first statement, before the log line and before any
        // saving: the counter is reloaded whether or not the save that
        // follows succeeds. A reload placed after the save would let a
        // failing save retry every tick.
        self.ticks_until.store(self.interval, Ordering::Relaxed);
        true
    }
}

/// `MinecraftServer.saveEverything(suppressLogs, flush, forced)`, in its
/// order and reduced to the parts this server has.
///
/// ```text
/// isSaving = true
/// getPlayerList().saveAll()                     <- players FIRST
/// saveAllChunks(suppressLogs, flush, forced):
///     scoreboard.storeToSaveDataIfDirty(..)     <- no scoreboard here
///     for each level: level.save(..)            <- the chunks
///     storageSource.saveDataTag(worldData, ..)  <- level.dat LAST
///     savedDataStorage.saveAndJoin() | scheduleSave()
/// warnOnLowDiskSpace()                          <- chat only, not modelled
/// isSaving = false
/// ```
///
/// Returns `(players, chunks)` written. **The two counts are separate and
/// both are returned** because they fail independently: a world directory
/// with no `players/data` and 400 fresh chunks is the bug this function was
/// written to fix, and a single total would hide it behind the chunks.
///
/// Errors are reported and do not stop the rest: vanilla's `saveEverything`
/// has no failure path that skips a later level, and an autosave that gave up
/// on the chunks because one player's file was unwritable would turn a small
/// loss into a large one.
pub fn save_everything(
    store: &crate::levelstore::LevelStore,
    entities: &crate::entities::Entities,
    tiles: &crate::tiles::Tiles,
    state: &crate::levelstore::WorldState,
) -> (usize, usize) {
    // 1. the players, before anything else.
    let players = match store.save_all_players(entities) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("autosave: could not save players: {e}");
            0
        }
    };
    // 2. the chunks.
    let chunks = match tiles.save_all() {
        Ok(n) => n,
        Err(e) => {
            eprintln!("autosave: could not save chunks: {e}");
            0
        }
    };
    // 3. `level.dat` and the nine `data/minecraft/*.dat`, LAST — so the file
    //    carrying `Time` and `DayTime` is not older than the chunks it
    //    describes.
    if let Err(e) = store.save(state) {
        eprintln!("autosave: could not write level.dat: {e}");
    }
    (players, chunks)
}

/// The process-wide clock, armed by `main` when `--world` is given.
///
/// A `OnceLock` for the reason `crate::levelstore::SHARED` is one: the tick
/// closure in `crate::status::listen` is a `move` into another thread and is
/// built before `main` has finished parsing arguments in some orderings, so
/// the two cannot be wired by passing a value. `None` — the unarmed state —
/// is what a server without `--world` sees, and it makes [`due_now`] answer
/// `false` forever rather than saving into a directory that does not exist.
static SHARED: std::sync::OnceLock<Option<AutosaveClock>> = std::sync::OnceLock::new();

/// Arm the autosave with an interval in ticks. Idempotent; the first call
/// wins, exactly as `open_shared` does.
pub fn arm(interval_ticks: i32) {
    let _ = SHARED.set(Some(AutosaveClock::new(interval_ticks)));
}

/// The clock, if one was armed.
#[must_use]
pub fn shared() -> Option<&'static AutosaveClock> {
    SHARED.get().and_then(Option::as_ref)
}

/// `tickServer`'s decrement, called once per tick from the game loop.
///
/// `false` when no clock is armed, which is a server with no `--world`: there
/// is nowhere to save to, so there is nothing for the counter to mean.
#[must_use]
pub fn due_now() -> bool {
    shared().is_some_and(AutosaveClock::due)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The interval at every rate the bytecode's two branches can produce,
    /// including both sides of the floor.
    ///
    /// 20.0 is the only one this server currently asks for, and it is
    /// deliberately not the only one pinned: a test that checked 6000 alone
    /// would pass on `fn next_interval(_) -> i32 { 6000 }`, which is exactly
    /// the constant this module's doc comment argues against.
    #[test]
    fn the_interval_is_three_hundred_seconds_of_ticks_floored_at_a_hundred() {
        assert_eq!(next_interval(20.0), 6000, "the default rate");
        assert_eq!(next_interval(5.0), 1500, "still 300 seconds");
        assert_eq!(next_interval(1.0), 300);
        assert_eq!(
            next_interval(0.3),
            MIMINUM_AUTOSAVE_TICKS,
            "0.3 * 300 = 90, under the floor"
        );
        assert_eq!(
            next_interval(0.0),
            MIMINUM_AUTOSAVE_TICKS,
            "the floor holds at zero"
        );
        // Java's (int) truncates toward zero rather than rounding.
        assert_eq!(next_interval(2.003), 600, "600.9 truncates to 600");
    }

    /// The first fire is `AUTOSAVE_INTERVAL` ticks away whatever the interval,
    /// and every later one is `interval` ticks.
    ///
    /// The two are equal at the default rate, so this uses a rate where they
    /// are not — otherwise the test passes on an implementation that only has
    /// one of the two numbers.
    #[test]
    fn a_shorter_interval_pulls_the_first_save_in_and_a_longer_one_does_not() {
        // 1500 < 6000, so `onTickRateChanged`'s clamp applies and the first
        // save is at 1500, not at 6000.
        let fast = AutosaveClock::new(next_interval(5.0));
        assert_eq!(fast.remaining(), 1500);
        let mut fired_at = Vec::new();
        for tick in 1..=4000 {
            if fast.due() {
                fired_at.push(tick);
            }
        }
        assert_eq!(fired_at, vec![1500, 3000], "every 1500 from the first");

        // 12000 > 6000, so the clamp does NOT apply: the constructor's own
        // 6000 stands for the first save and 12000 for every one after. A
        // `min` written as a `max`, or as a plain assignment, fails here and
        // passes the case above.
        let slow = AutosaveClock::new(12000);
        assert_eq!(slow.remaining(), AUTOSAVE_INTERVAL);
        let mut fired_at = Vec::new();
        for tick in 1..=20000 {
            if slow.due() {
                fired_at.push(tick);
            }
        }
        assert_eq!(fired_at, vec![6000, 18000], "6000 then 12000");
    }

    /// `ifgt` means zero fires. One tick either way changes which tick the
    /// first save lands on, and nothing else in the server would show it.
    #[test]
    fn the_counter_fires_at_zero_and_not_at_minus_one() {
        // The interval is the default's, so the clamp above is a no-op and
        // this is purely about which tick the counter reaches zero on.
        let clock = AutosaveClock::new(AUTOSAVE_INTERVAL);
        for tick in 1..AUTOSAVE_INTERVAL {
            assert!(!clock.due(), "fired early, at tick {tick}");
        }
        assert_eq!(clock.remaining(), 1, "one tick left after 5999");
        assert!(clock.due(), "the 6000th tick takes it to 0, which fires");
        assert_eq!(clock.remaining(), AUTOSAVE_INTERVAL, "reloaded");
    }
}
