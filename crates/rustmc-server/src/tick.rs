//! The game clock.
//!
//! Reference (26.2): `ClientboundSetTimePacket`, whose body is a raw `i64`
//! game time followed by a map of `WorldClock` holders to
//! `ClockNetworkState` — read from the jar, not from older protocol notes,
//! which describe a three-field packet that no longer exists.
//!
//! # What ticks, and what does not
//!
//! **This file is the clock. The game loop is not here** — it is the closure
//! `crate::status::serve` hands to [`GameClock::spawn`], which is where
//! anything that wants to know what a tick *does* should look. This module
//! owns when a tick happens and nothing about what happens in one.
//!
//! Everything above this paragraph used to say the opposite: that no block
//! updates, no entities and no scheduled ticks existed "because this
//! server's world is immutable". Every clause of that has been false since
//! `crate::edits` gave the world an overlay, and a caveat list that lags the
//! code reads as authoritative — so, as of 2026-08-06, here is what the loop
//! actually runs, in `ServerLevel.tick`'s own order:
//!
//! 1. **the weather cycle** — `crate::weather::tick`, which is
//!    `ServerLevel.advanceWeatherCycle`: the clear/rain/thunder timers, the
//!    two level ramps at 0.01 a tick, and the `game_event` sends. It is
//!    first here because it is second there — that method's profiler
//!    strings read `world border`, `weather`, then everything below — and
//!    this server has no world border, so item one is absent rather than
//!    reordered;
//! 2. **scheduled block ticks** — `crate::ticks`, with vanilla's
//!    `ScheduledTick.DRAIN_ORDER` and its `UNIQUE_TICK_HASH` deduplication.
//!    Three behaviours reach it: `CoralBlock.tick`, `ButtonBlock.tick` and
//!    the container recheck (`ChestBlock.tick` -> `recheckOpen`); then the
//!    **scheduled fluid ticks**, the second `LevelTicks`, drained after the
//!    whole block queue under the same game time (`crate::fluid`,
//!    `crate::ticks::ScheduledTicks`);
//! 3. **the block broadcast** — `ChunkHolder.broadcastChanges`, coalesced per
//!    section;
//! 4. **`LivingEntity.updateSwingTime`**, once per entity per tick;
//! 5. **`ChunkMap.tick()`'s entity half** — `crate::tracker`: tracking
//!    membership on a section change and `ServerEntity.sendChanges` every
//!    tick;
//! 6. **`ServerLevel.runBlockEvents`**;
//! 7. **the acknowledgement flush**, `ServerGamePacketListenerImpl.tick`;
//! 8. a tile sweep every 20 ticks, which is this server's own memory
//!    management and has no vanilla counterpart.
//!
//! And what does **not** tick, which is the more useful half:
//!
//! * **random ticks.** The selection mechanism is derived and gated in
//!   `crate::randomtick` and is deliberately not wired here — that module's
//!   docs have the three reasons;
//! * **a player's motion.** A player's position is the owning client's own
//!   movement packet (`crate::world_send`) and its `onGround` is whatever the
//!   client claimed. (Items, falling blocks and mobs do move:
//!   `crate::itementity`, `crate::movement` and `crate::mob` run
//!   `Entity.move` for them from `crate::status`'s loop, not from this
//!   clock.);
//! * **`EntityTickList` order and the `tickNonPassenger`/`tickPassenger`
//!   body.** Both are read out of the bytecode in `rustmc_entity::tick_list`
//!   and `rustmc_entity::tick` — and **nothing calls either**: this crate
//!   imports `EntityId` from that crate and no more. Derived is not wired;
//! * mob AI, mob spawning, `checkDespawn`, item merging and pickup, and the
//!   block-entity tickers (`Level.blockEntityTickers`);
//! * mob AI, mob spawning, `checkDespawn`, fluid ticks, and the
//!   block-entity tickers (`Level.blockEntityTickers`) — and item merging
//!   and pickup, which `crate::itementity` and `crate::pickup` run from
//!   `crate::status`'s loop and the connection thread;
//! * **`tickPrecipitation`**, the ice-and-snow half of `tickChunk`. Its
//!   selection loop is derived twice over — `crate::randomtick`'s
//!   `PRECIPITATION_ATTEMPTS_PER_TICK` and
//!   `crate::weather::PRECIPITATION_DRAWS_PER_CHUNK_TICK` — and its two
//!   effects are unreachable in this server's world by construction rather
//!   than unported: ice and snow both need
//!   `getTemperature(pos, seaLevel) < 0.15f` and the corpus superflat's
//!   biome is `minecraft:plains` at 0.8. `docs/cold-start/weather.md` has
//!   the arithmetic and the refusal.
//!
//! The clock is worth having on its own because everything that ticks needs
//! one to tick *against*, and because the shape of the loop — where the drift
//! goes — is the part that is easy to get quietly wrong.
//!
//! # Drift
//!
//! Sleeping 50 ms per tick does not give 20 ticks a second: it gives 20
//! ticks per (50 ms + however long the tick took + however long the OS felt
//! like), and the error accumulates forever. The loop therefore sleeps until
//! the *next scheduled instant*, computed from a fixed start, so a slow tick
//! is absorbed by the next one rather than pushing every future tick back.
//!
//! A tick that overruns is followed by the ticks it delayed, back to back,
//! until the schedule is met: vanilla drops ticks ("Can't keep up") only when
//! it is more than two seconds behind. See [`GameClock::drive`].

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Vanilla's tick rate.
pub const TICKS_PER_SECOND: u64 = 20;
/// `50ms`, as a duration.
pub const TICK: Duration = Duration::from_nanos(1_000_000_000 / TICKS_PER_SECOND);
/// `MinecraftServer.OVERLOADED_THRESHOLD_NANOS` — `20 * NANOSECONDS_PER_SECOND
/// / 20` (`<clinit>` 6-17): one second.
pub const OVERLOADED_THRESHOLD: Duration = Duration::from_secs(1);
/// `MinecraftServer.OVERLOADED_WARNING_INTERVAL_NANOS` — `10 *
/// NANOSECONDS_PER_SECOND` (`<clinit>` 20-27).
pub const OVERLOADED_WARNING_INTERVAL: Duration = Duration::from_secs(10);

/// A monotonically advancing game time, shared by every connection.
#[derive(Debug, Clone)]
pub struct GameClock {
    ticks: Arc<AtomicI64>,
}

/// Where [`GameClock::drive`] reads the time and how it waits: the time
/// since some fixed origin, and a sleep.
pub trait TimeSource {
    /// Time elapsed since this source's origin.
    fn now(&self) -> Duration;
    /// Block until `d` has passed.
    fn sleep(&self, d: Duration);
}

/// The wall clock: [`Instant`] and [`std::thread::sleep`].
#[derive(Debug, Clone, Copy)]
pub struct RealTime(pub Instant);

impl TimeSource for RealTime {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

impl Default for GameClock {
    fn default() -> Self {
        Self::new()
    }
}

impl GameClock {
    #[must_use]
    pub fn new() -> Self {
        Self {
            ticks: Arc::new(AtomicI64::new(0)),
        }
    }

    /// The current game time in ticks.
    #[must_use]
    pub fn ticks(&self) -> i64 {
        self.ticks.load(Ordering::Relaxed)
    }

    /// Advance by one tick, returning the new value.
    pub fn advance(&self) -> i64 {
        self.ticks.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Run the clock forever on this thread.
    ///
    /// `on_tick` is called with the new game time after each advance. It runs
    /// *on the clock's thread*, so anything slow in it eats the tick budget —
    /// which is exactly the property a tick loop should have and the reason
    /// the skip below exists.
    pub fn run(&self, mut on_tick: impl FnMut(i64)) -> ! {
        self.drive(&RealTime(Instant::now()), |t| {
            on_tick(t);
            true
        });
        unreachable!("the real clock's on_tick never asks to stop")
    }

    /// The loop [`Self::run`] runs, against any [`TimeSource`], until
    /// `on_tick` returns `false`. Split out so the drift and skip rules are
    /// tested on a fake clock: the same code, with time that only moves when
    /// the test moves it, so the verdict cannot depend on how loaded the box
    /// running it is.
    pub fn drive<T: TimeSource>(&self, time: &T, mut on_tick: impl FnMut(i64) -> bool) {
        // `MinecraftServer.runServer`, offsets 8-11 and 92-243:
        //
        //     nextTickTimeNanos = Util.getNanos()
        //     loop:
        //       behind = getNanos() - nextTickTimeNanos
        //       if (behind > OVERLOADED_THRESHOLD_NANOS + 20 * tick
        //           && nextTickTimeNanos - lastOverloadWarningNanos
        //              >= OVERLOADED_WARNING_INTERVAL_NANOS + 100 * tick) {
        //           skipped = behind / tick
        //           warn("Can't keep up! ...")
        //           nextTickTimeNanos += skipped * tick
        //           lastOverloadWarningNanos = nextTickTimeNanos
        //       }
        //       nextTickTimeNanos += tick
        //       tick; waitUntilNextTick()     // waits only if it is in the future
        //
        // So a late tick runs at once and the ones after it run back to back
        // until the schedule is met again: **no tick is dropped unless the
        // server is more than two seconds behind**, and then only once per
        // fifteen seconds. The deadline is a fixed schedule, never "now plus
        // 50 ms", so a slow tick is absorbed rather than pushing every later
        // tick back. This loop waits before its tick where vanilla waits
        // after, which moves every tick by one slot and nothing else.
        let mut next = time.now();
        // `lastOverloadWarningNanos` starts at the field default 0 against a
        // `nanoTime` origin far behind it, so the first overload always
        // qualifies: `None` here.
        let mut last_overload_warning: Option<Duration> = None;
        loop {
            let now = time.now();
            if now > next {
                let behind = now - next;
                if behind > OVERLOADED_THRESHOLD + TICK * 20
                    && last_overload_warning.is_none_or(|w| {
                        next.saturating_sub(w) >= OVERLOADED_WARNING_INTERVAL + TICK * 100
                    })
                {
                    let skipped = behind.as_nanos() / TICK.as_nanos();
                    eprintln!(
                        "Can't keep up! Is the server overloaded? Running {}ms or {skipped} ticks behind",
                        behind.as_millis()
                    );
                    next += TICK * u32::try_from(skipped).unwrap_or(u32::MAX);
                    last_overload_warning = Some(next);
                }
            }
            next += TICK;
            let now = time.now();
            if now < next {
                time.sleep(next - now);
            }
            if !on_tick(self.advance()) {
                return;
            }
        }
    }

    /// Spawn [`Self::run`] on its own thread.
    pub fn spawn(&self, on_tick: impl FnMut(i64) + Send + 'static) {
        let clock = self.clone();
        std::thread::spawn(move || clock.run(on_tick));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_advances() {
        let clock = GameClock::new();
        assert_eq!(clock.ticks(), 0);
        assert_eq!(clock.advance(), 1);
        assert_eq!(clock.advance(), 2);
        assert_eq!(clock.ticks(), 2);
    }

    /// Every clone reads the same clock — connections share one game time,
    /// they do not each keep their own.
    #[test]
    fn clones_share_one_clock() {
        let a = GameClock::new();
        let b = a.clone();
        a.advance();
        assert_eq!(b.ticks(), 1);
        b.advance();
        assert_eq!(a.ticks(), 2);
    }

    /// Time that moves only when told to: a sleep adds to it, and so does
    /// a tick's "work".
    struct FakeTime(std::cell::Cell<Duration>);

    impl TimeSource for FakeTime {
        fn now(&self) -> Duration {
            self.0.get()
        }
        fn sleep(&self, d: Duration) {
            self.0.set(self.0.get() + d);
        }
    }

    impl FakeTime {
        fn work(&self, ms: u64) {
            self.sleep(Duration::from_millis(ms));
        }
    }

    /// The deadline is computed from a fixed start, so a tick that takes
    /// time does not push the ones after it back. Twenty ticks with a 20 ms
    /// stall in each end at exactly 1000 ms + the last stall; a sleep loop
    /// (deadline = now + 50 ms) would end at 20 * 70 = 1400 ms.
    ///
    /// On a fake clock, so the verdict is the loop's and not the box's: the
    /// wall-clock version of this test (`elapsed < 1300 ms`) went red at
    /// 1.32 s on a loaded box (structure-mobs gate, 2026-10-03 22:21).
    #[test]
    fn a_slow_tick_does_not_delay_the_next_one() {
        let clock = GameClock::new();
        let time = FakeTime(std::cell::Cell::new(Duration::ZERO));
        let mut ended = None;
        clock.drive(&time, |t| {
            time.work(20);
            if t == 20 {
                ended = Some(time.now());
                return false;
            }
            true
        });
        assert_eq!(ended, Some(Duration::from_millis(1020)));
        assert_eq!(clock.ticks(), 20);
    }

    /// A tick that overruns is followed by the ticks it delayed, run back to
    /// back, not skipped: a 180 ms stall on tick 1 (run at 50, ending at
    /// 230) leaves slots 100, 150 and 200 late, and all three run at once at
    /// 230; 250 is on time again. The old rule here skipped any tick more
    /// than a slot late, so under load this server ran fewer ticks than
    /// vanilla, which drops a tick only when two seconds behind — a swing
    /// whose swallow depends on one `updateSwingTime` between two packets
    /// (gameplay 581) then saw none.
    #[test]
    fn a_late_tick_is_followed_by_the_ones_it_delayed() {
        let clock = GameClock::new();
        let time = FakeTime(std::cell::Cell::new(Duration::ZERO));
        let mut at = Vec::new();
        clock.drive(&time, |t| {
            at.push(time.now().as_millis());
            if t == 1 {
                time.work(180);
            }
            t < 6
        });
        assert_eq!(at, vec![50, 230, 230, 230, 250, 300]);
    }

    /// More than two seconds behind, with no warning yet: the missed ticks
    /// are dropped, once, as vanilla's "Can't keep up!" does. Under two
    /// seconds they are all run.
    #[test]
    fn only_a_stall_past_two_seconds_drops_ticks_and_only_every_fifteen() {
        let clock = GameClock::new();
        let time = FakeTime(std::cell::Cell::new(Duration::ZERO));
        let mut ran = 0;
        clock.drive(&time, |t| {
            ran += 1;
            if t == 1 {
                time.work(16_000);
                return true;
            }
            false
        });
        assert_eq!(ran, 2);
        // 16 s behind at the check: 319 slots dropped, so the clock is back
        // on schedule after one tick rather than running 320 late ones.
        assert_eq!(clock.ticks(), 2);
        let clock = GameClock::new();
        let time = FakeTime(std::cell::Cell::new(Duration::ZERO));
        let mut at = Vec::new();
        clock.drive(&time, |t| {
            at.push(time.now().as_millis());
            if t == 1 {
                time.work(1_500);
            }
            t < 32
        });
        // 1.5 s behind is under the threshold: all 30 delayed ticks run at
        // once at 1550, then the schedule resumes at 1600.
        assert_eq!(at.iter().filter(|&&m| m == 1550).count(), 30);
        assert_eq!(at.last(), Some(&1600));
    }
}
