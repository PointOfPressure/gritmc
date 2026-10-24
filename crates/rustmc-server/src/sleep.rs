//! Sleeping — the bed-entry ladder, the sleep counter, and the vote that skips
//! the night.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `net.minecraft.server.level.ServerPlayer.startSleepInBed` (offsets 0..341),
//! `.bedInRange`, `.bedBlocked`,
//! `net.minecraft.world.entity.player.Player.startSleepInBed`, `.tick`
//! (offsets 44..156), `.isSleepingLongEnough`, its `SLEEP_DURATION`,
//! `net.minecraft.server.players.SleepStatus` (all of it), and
//! `ServerLevel.tick` offsets 67..205 for the vote itself.
//!
//! # What main had before this file
//!
//! Nothing sleeps. There is no bed interaction, no sleep counter, no
//! `SleepStatus`, and `docs/SERVER.md` records the whole player as "a
//! position, a hotbar, a creative slot, swing, the click surface". The
//! serverbound packet that starts a sleep is `use_item_on` against a bed and
//! the one that ends it is `player_command` — **the packets lane owns that
//! decode**, so nothing here reads a packet; this module is the state machine
//! those handlers will call.
//!
//! # The bed ladder returns SIX different answers and the order matters
//!
//! `ServerPlayer.startSleepInBed` is a chain of early returns, and two of them
//! sit *after* a side effect:
//!
//! ```text
//!    18  isSleeping() || !isAlive()          -> OTHER_PROBLEM
//!    77  !canSetSpawn && !canSleep           -> bedRule.asProblem()
//!    95  !bedInRange(pos, facing)            -> TOO_FAR_AWAY
//!   111  bedBlocked(pos, facing)             -> OBSTRUCTED
//!   127  canSetSpawn                         -> setRespawnPosition(...)   [!]
//!   164  !canSleep                           -> bedRule.asProblem()
//!   177  !isCreative() && monsters nearby    -> NOT_SAFE
//!   296  super.startSleepInBed(pos)          -> Unit
//! ```
//!
//! The `[!]` is the reading a reimplementation loses. **The spawn point is set
//! before the "can you actually sleep here" test**, so clicking a bed in a
//! dimension where sleeping is impossible but spawn-setting is allowed sets
//! your spawn and *then* tells you no. Merging the two `bedRule.asProblem()`
//! returns into one early test would silently stop that happening.
//!
//! The monster scan is `8.0` blocks horizontally and `5.0` vertically around
//! `Vec3.atBottomCenterOf(pos)` — a box, not a sphere, and asymmetric — and it
//! is skipped entirely in creative.
//!
//! # `sleepersNeeded` is a percentage with a floor of one
//!
//! `max(1, ceil(activePlayers * percentage / 100.0f))`, where `percentage` is
//! the `players_sleeping_percentage` game rule. `activePlayers` counts
//! non-spectators. The `max(1, ...)` means a percentage of 0 still needs one
//! sleeper, and `Mth.ceil` is `(int) Math.ceil((double) f)` so the widening
//! happens before the ceiling.
//!
//! # The night is skipped only when enough are *deep* asleep
//!
//! `ServerLevel.tick` needs **both** `areEnoughSleeping` (which counts
//! `isSleeping()`) and `areEnoughDeepSleeping` (which counts
//! `isSleepingLongEnough()`, i.e. `sleepCounter >= 100`). So the earliest a
//! night can be skipped is 100 ticks after the last required player got into
//! bed, and a player who climbs out at tick 99 resets the whole thing.
//!
//! # The counter runs backwards too
//!
//! `Player.tick` offsets 125..156: once a player *stops* sleeping the counter
//! keeps **incrementing** to 110 and then snaps to 0. It is not reset on
//! waking. That tail is what the client's fade-out reads, and a port that
//! zeroed the counter on wake would be a different observable.

use crate::javamath;

/// `Player.SLEEP_DURATION` — the counter value at which a sleep counts as
/// deep, and the ceiling the counter is clamped to while asleep.
pub const SLEEP_DURATION: i32 = 100;
/// The value the post-wake counter tail runs to before snapping to zero,
/// `bipush 110` at `Player.tick` offset 146.
pub const SLEEP_COUNTER_TAIL: i32 = 110;
/// The half-width of the monster scan box, `ldc2_w 8.0d`.
pub const MONSTER_SCAN_HORIZONTAL: f64 = 8.0;
/// Its half-height, `ldc2_w 5.0d`.
pub const MONSTER_SCAN_VERTICAL: f64 = 5.0;

/// `Player.BedSleepingProblem`, plus the success case.
///
/// `bedRule.asProblem()` produces one of the problem values from the
/// dimension's `BedRule` environment attribute; it is carried as
/// [`BedResult::BedRuleProblem`] rather than resolved because the attribute
/// system is not modelled here and inventing which problem it names would be
/// a guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BedResult {
    /// Offset 32 — already asleep, or dead.
    OtherProblem,
    /// Offsets 87 and 169 — whatever `BedRule.asProblem()` answers. Reached
    /// from two different places, and the second is after the respawn point
    /// has already been set.
    BedRuleProblem,
    /// Offset 104.
    TooFarAway,
    /// Offset 120.
    Obstructed,
    /// Offset 289.
    NotSafe,
    /// Offset 296 — `Player.startSleepInBed`, which calls `startSleeping(pos)`
    /// and zeroes the sleep counter.
    Sleeping,
}

/// What `startSleepInBed` reads that this module cannot compute.
///
/// `bed_in_range` and `bed_blocked` are the answers of `ServerPlayer.bedInRange`
/// and `.bedBlocked` — both of which need the world and the player's reach —
/// and `monsters_nearby` is the `getEntitiesOfClass(Monster.class, ...)` scan.
/// They are inputs so that the *ladder* can be transcribed and tested exactly
/// while the world reads stay where the world is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BedContext {
    /// `isSleeping()`.
    pub already_sleeping: bool,
    /// `isAlive()`.
    pub alive: bool,
    /// `BedRule.canSleep(level)`.
    pub can_sleep: bool,
    /// `BedRule.canSetSpawn(level)`.
    pub can_set_spawn: bool,
    /// `bedInRange(pos, facing)`.
    pub bed_in_range: bool,
    /// `bedBlocked(pos, facing)`.
    pub bed_blocked: bool,
    /// `isCreative()`.
    pub creative: bool,
    /// Whether the 16x10x16 box around the bed holds a hostile the predicate
    /// accepts.
    pub monsters_nearby: bool,
}

/// The side effects `startSleepInBed` has on the way through the ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BedEffects {
    /// `setRespawnPosition(new RespawnConfig(...), true)` ran at offset 132.
    /// It runs **before** the `!canSleep` return, which is the whole point of
    /// reporting it separately from the result.
    pub respawn_set: bool,
    /// `level.updateSleepingPlayerList()` at offset 332. It runs on **every**
    /// path that reaches the end of the method, including the ones that
    /// returned a problem from `super`.
    pub update_sleeping_player_list: bool,
}

/// `ServerPlayer.startSleepInBed(BlockPos)`.
///
/// Returns the answer and what it did on the way. The overlay message at
/// offsets 312..332 (`sleep.not_possible`, when
/// `ServerLevel.canSleepThroughNights()` is false) is not modelled: it is a
/// chat component on a channel this branch does not touch.
#[must_use]
pub fn start_sleep_in_bed(ctx: BedContext) -> (BedResult, BedEffects) {
    let mut fx = BedEffects::default();
    if ctx.already_sleeping || !ctx.alive {
        return (BedResult::OtherProblem, fx);
    }
    if !ctx.can_set_spawn && !ctx.can_sleep {
        return (BedResult::BedRuleProblem, fx);
    }
    if !ctx.bed_in_range {
        return (BedResult::TooFarAway, fx);
    }
    if ctx.bed_blocked {
        return (BedResult::Obstructed, fx);
    }
    // 127..164 — before the canSleep test, deliberately.
    if ctx.can_set_spawn {
        fx.respawn_set = true;
    }
    if !ctx.can_sleep {
        return (BedResult::BedRuleProblem, fx);
    }
    if !ctx.creative && ctx.monsters_nearby {
        return (BedResult::NotSafe, fx);
    }
    // 296..341 — Player.startSleepInBed, then the list update.
    fx.update_sleeping_player_list = true;
    (BedResult::Sleeping, fx)
}

/// What `BedBlock.useWithoutItem` decided.
///
/// Reference (26.2): `net.minecraft.world.level.block.BedBlock.useWithoutItem`
/// offsets 0..240.
///
/// # Why this is hand-ported rather than read from `use_without_item.json`
///
/// **The golden refuses every bed head state.** `ci/use-golden.sh` probes
/// `useWithoutItem` against a fake level, and offset 58 asks that level for
/// `environmentAttributes()` — so all sixteen `BedBlock` rows carry
/// `"refused": "UnsupportedOperationException: UseGolden's level was asked
/// for environmentAttributes"` on `part=head`, and the `part=foot` rows
/// record `CONSUME`, which is offset 53's "the block at the head is not this
/// bed" arm and not a sleep at all. So the table cannot answer for a bed and
/// [`crate::useblock::UseTable`] will never carry one: a caller that goes
/// through it gets `Pass` and falls into the placement path, which is a
/// right-click on a bed placing a block.
///
/// That makes this the same case as [`crate::useblock::hand_ported`]'s two
/// classes — a `useWithoutItem` the extractor cannot reach — and it is
/// recorded here rather than left as a gap in the table's own count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BedUse {
    /// Offset 53 — the click was on the foot and the block at the head is not
    /// the same bed. `InteractionResult.CONSUME`: the click is eaten and
    /// nothing happens.
    Consume,
    /// Offsets 73..180 — `BedRule.explodes()`. Both halves are removed and a
    /// `5.0f` block explosion is raised at `Vec3.atCenterOf(head)`.
    ///
    /// **The removal happens before the explosion and in this order**: the
    /// head first (offset 101), then the foot but only if the block there is
    /// still this bed (offsets 108..147). A port that explodes first destroys
    /// the bed twice.
    Explodes,
    /// Offsets 181..219 — `OCCUPIED` is true. `kickVillagerOutOfBed` is tried
    /// first and, when it fails, the player gets the `block.minecraft.bed.
    /// occupied` overlay. Either way the result is `SUCCESS_SERVER` and no
    /// sleep starts.
    Occupied,
    /// Offset 220 — `player.startSleepInBed(pos)`, where `pos` is the **head**
    /// position, not the clicked one.
    StartSleep,
}

/// What `BedBlock.useWithoutItem` reads about the world.
///
/// Every field is one bytecode read, named for it. There is no default: a
/// bed with all-false fields is a legal state (`part=foot` clicked with air
/// at the head) and would silently be the answer for a caller that forgot to
/// fill one in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BedUseContext {
    /// `state.getValue(PART) == BedPart.HEAD` — offsets 11..21.
    pub clicked_head: bool,
    /// `level.getBlockState(pos.relative(FACING)).is(this)` — offsets 39..50.
    /// Read only when the click was on the foot, and `true` when the click
    /// was on the head, because that branch is skipped.
    pub head_is_same_bed: bool,
    /// `BedRule.explodes()` — offset 75.
    pub bed_rule_explodes: bool,
    /// `state.getValue(OCCUPIED)` — offsets 181..194, read from the **head**
    /// state when the click was on the foot, because offset 44 reassigned it.
    pub occupied: bool,
}

/// `BedBlock.useWithoutItem(state, level, pos, player, hit)`, server side.
///
/// The `level.isClientSide` arm at offsets 0..10 is not modelled: this server
/// has no client side.
#[must_use]
pub const fn bed_use_without_item(ctx: BedUseContext) -> BedUse {
    // 11..56 — the foot redirects to the head, and only then.
    if !ctx.clicked_head && !ctx.head_is_same_bed {
        return BedUse::Consume;
    }
    // 73..180
    if ctx.bed_rule_explodes {
        return BedUse::Explodes;
    }
    // 181..219
    if ctx.occupied {
        return BedUse::Occupied;
    }
    // 220..240
    BedUse::StartSleep
}

/// The `sleepCounter` on `Player`, and the two tick rules that move it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SleepCounter {
    counter: i32,
    sleeping: bool,
}

impl SleepCounter {
    /// A player who is awake with no tail running.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            counter: 0,
            sleeping: false,
        }
    }

    /// `Player.sleepCounter`.
    #[must_use]
    pub const fn counter(&self) -> i32 {
        self.counter
    }

    /// `Entity.isSleeping()` — here, the flag the sleeping pose sets.
    #[must_use]
    pub const fn is_sleeping(&self) -> bool {
        self.sleeping
    }

    /// `Player.startSleepInBed` -> `startSleeping(pos); this.sleepCounter = 0`.
    pub const fn start(&mut self) {
        self.sleeping = true;
        self.counter = 0;
    }

    /// `stopSleeping()` — clears the pose. **It does not reset the counter**;
    /// the tail in [`SleepCounter::tick`] does that, twenty ticks later.
    pub const fn stop(&mut self) {
        self.sleeping = false;
    }

    /// `Player.isSleepingLongEnough()` — `isSleeping() && sleepCounter >= 100`.
    #[must_use]
    pub const fn is_sleeping_long_enough(&self) -> bool {
        self.sleeping && self.counter >= SLEEP_DURATION
    }

    /// `Player.tick` offsets 44..156, the counter half only.
    ///
    /// While asleep the counter climbs and is clamped at 100 (`if_icmple 76`,
    /// so 101 becomes 100 and 100 stays). While awake it climbs *only if it is
    /// already above zero*, and snaps to 0 on reaching 110.
    pub const fn tick(&mut self) {
        if self.sleeping {
            self.counter += 1;
            if self.counter > SLEEP_DURATION {
                self.counter = SLEEP_DURATION;
            }
        } else if self.counter > 0 {
            self.counter += 1;
            if self.counter >= SLEEP_COUNTER_TAIL {
                self.counter = 0;
            }
        }
    }
}

/// `net.minecraft.server.players.SleepStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SleepStatus {
    active_players: i32,
    sleeping_players: i32,
}

impl SleepStatus {
    /// A fresh status — both counters zero, as the implicit constructor
    /// leaves them.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            active_players: 0,
            sleeping_players: 0,
        }
    }

    /// `sleepersNeeded(int)` — `max(1, ceil(activePlayers * pct / 100.0f))`.
    #[must_use]
    pub fn sleepers_needed(&self, percentage: i32) -> i32 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "vanilla's own i2f at sleepersNeeded offset 7"
        )]
        let scaled = (self.active_players * percentage) as f32 / 100.0;
        javamath::max_i(1, mth_ceil(scaled))
    }

    /// `areEnoughSleeping(int)` — `sleepingPlayers >= sleepersNeeded(pct)`.
    #[must_use]
    pub fn are_enough_sleeping(&self, percentage: i32) -> bool {
        self.sleeping_players >= self.sleepers_needed(percentage)
    }

    /// `areEnoughDeepSleeping(int, List<ServerPlayer>)` — the same threshold
    /// against a count of players whose `isSleepingLongEnough()` is true.
    ///
    /// It does **not** read `sleepingPlayers`: the caller counts the list
    /// afresh, which is why a player who has just got into bed satisfies
    /// `areEnoughSleeping` and not this.
    #[must_use]
    pub fn are_enough_deep_sleeping(&self, percentage: i32, deep_sleepers: i32) -> bool {
        deep_sleepers >= self.sleepers_needed(percentage)
    }

    /// `amountSleeping()`.
    #[must_use]
    pub const fn amount_sleeping(&self) -> i32 {
        self.sleeping_players
    }

    /// `activePlayers` — non-spectators, as of the last [`SleepStatus::update`].
    #[must_use]
    pub const fn active_players(&self) -> i32 {
        self.active_players
    }

    /// `removeAllSleepers()` — zeroes the sleeping count and **not** the
    /// active one.
    pub const fn remove_all_sleepers(&mut self) {
        self.sleeping_players = 0;
    }

    /// `update(List<ServerPlayer>)`, given `(is_spectator, is_sleeping)` per
    /// player.
    ///
    /// Returns whether the client-visible sleeping status changed. The
    /// predicate at offsets 89..121 is the interesting part:
    ///
    /// ```text
    ///   (oldSleeping > 0 || sleepingPlayers > 0)
    ///       && (oldActive != activePlayers || oldSleeping != sleepingPlayers)
    /// ```
    ///
    /// so a change from nobody-sleeping to nobody-sleeping never reports, and
    /// a change in the *active* count reports only while somebody is or was
    /// asleep.
    pub fn update(&mut self, players: impl IntoIterator<Item = (bool, bool)>) -> bool {
        let old_active = self.active_players;
        let old_sleeping = self.sleeping_players;
        self.active_players = 0;
        self.sleeping_players = 0;
        for (spectator, sleeping) in players {
            if spectator {
                continue;
            }
            self.active_players += 1;
            if sleeping {
                self.sleeping_players += 1;
            }
        }
        (old_sleeping > 0 || self.sleeping_players > 0)
            && (old_active != self.active_players || old_sleeping != self.sleeping_players)
    }
}

/// `Mth.ceil(float)` — `(int) Math.ceil((double) f)`, so the widening happens
/// before the ceiling and the narrowing is a `d2i`.
fn mth_ceil(v: f32) -> i32 {
    let ceiled = f64::from(v).ceil();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "vanilla's own d2i; the values here are player counts"
    )]
    {
        ceiled as i32
    }
}

#[cfg(test)]
mod tests {
    use super::{
        start_sleep_in_bed, BedContext, BedResult, SleepCounter, SleepStatus, SLEEP_COUNTER_TAIL,
        SLEEP_DURATION,
    };

    fn ok() -> BedContext {
        BedContext {
            already_sleeping: false,
            alive: true,
            can_sleep: true,
            can_set_spawn: true,
            bed_in_range: true,
            bed_blocked: false,
            creative: false,
            monsters_nearby: false,
        }
    }

    #[test]
    fn the_happy_path_sleeps_sets_spawn_and_updates_the_list() {
        let (r, fx) = start_sleep_in_bed(ok());
        assert_eq!(r, BedResult::Sleeping);
        assert!(fx.respawn_set);
        assert!(fx.update_sleeping_player_list);
    }

    #[test]
    fn already_sleeping_and_dead_are_the_same_answer() {
        let (a, _) = start_sleep_in_bed(BedContext {
            already_sleeping: true,
            ..ok()
        });
        let (b, _) = start_sleep_in_bed(BedContext {
            alive: false,
            ..ok()
        });
        assert_eq!(a, BedResult::OtherProblem);
        assert_eq!(b, BedResult::OtherProblem);
    }

    #[test]
    fn the_spawn_is_set_before_the_can_sleep_test_and_that_is_observable() {
        // The nether-bed shape: you may set spawn there and you may not sleep.
        // Vanilla sets the spawn and THEN refuses, and the two are reported
        // separately here so a caller cannot collapse them.
        let (r, fx) = start_sleep_in_bed(BedContext {
            can_sleep: false,
            can_set_spawn: true,
            ..ok()
        });
        assert_eq!(r, BedResult::BedRuleProblem);
        assert!(fx.respawn_set, "the respawn point must already be set");
        assert!(!fx.update_sleeping_player_list);
    }

    #[test]
    fn neither_sleeping_nor_spawn_refuses_before_touching_anything() {
        let (r, fx) = start_sleep_in_bed(BedContext {
            can_sleep: false,
            can_set_spawn: false,
            ..ok()
        });
        assert_eq!(r, BedResult::BedRuleProblem);
        assert!(!fx.respawn_set, "this arm is BEFORE the respawn write");
    }

    #[test]
    fn range_is_tested_before_obstruction_and_both_before_the_spawn_write() {
        let (far, fx1) = start_sleep_in_bed(BedContext {
            bed_in_range: false,
            bed_blocked: true,
            ..ok()
        });
        assert_eq!(far, BedResult::TooFarAway, "range is the earlier test");
        assert!(!fx1.respawn_set);

        let (blocked, fx2) = start_sleep_in_bed(BedContext {
            bed_blocked: true,
            ..ok()
        });
        assert_eq!(blocked, BedResult::Obstructed);
        assert!(!fx2.respawn_set);
    }

    #[test]
    fn monsters_stop_a_survival_sleep_and_not_a_creative_one() {
        let (survival, _) = start_sleep_in_bed(BedContext {
            monsters_nearby: true,
            ..ok()
        });
        assert_eq!(survival, BedResult::NotSafe);
        let (creative, fx) = start_sleep_in_bed(BedContext {
            monsters_nearby: true,
            creative: true,
            ..ok()
        });
        assert_eq!(creative, BedResult::Sleeping);
        assert!(fx.update_sleeping_player_list);
    }

    #[test]
    fn a_sleep_becomes_deep_on_the_hundredth_tick_and_not_before() {
        let mut c = SleepCounter::new();
        c.start();
        for tick in 1..=99 {
            c.tick();
            assert!(!c.is_sleeping_long_enough(), "deep at tick {tick}");
        }
        c.tick();
        assert!(c.is_sleeping_long_enough());
        assert_eq!(c.counter(), SLEEP_DURATION);
    }

    #[test]
    fn the_counter_is_clamped_at_a_hundred_while_asleep() {
        let mut c = SleepCounter::new();
        c.start();
        for _ in 0..500 {
            c.tick();
        }
        assert_eq!(c.counter(), SLEEP_DURATION);
    }

    #[test]
    fn waking_leaves_the_counter_running_for_ten_more_ticks() {
        let mut c = SleepCounter::new();
        c.start();
        for _ in 0..SLEEP_DURATION {
            c.tick();
        }
        c.stop();
        assert_eq!(c.counter(), SLEEP_DURATION, "waking does not reset it");
        assert!(!c.is_sleeping_long_enough(), "but it is no longer deep");
        for tick in 1..SLEEP_COUNTER_TAIL - SLEEP_DURATION {
            c.tick();
            assert_eq!(c.counter(), SLEEP_DURATION + tick);
        }
        c.tick();
        assert_eq!(c.counter(), 0, "and snaps to zero at 110");
    }

    #[test]
    fn an_awake_player_at_zero_never_starts_the_tail() {
        let mut c = SleepCounter::new();
        for _ in 0..200 {
            c.tick();
        }
        assert_eq!(c.counter(), 0);
    }

    #[test]
    fn sleepers_needed_has_a_floor_of_one_at_any_percentage() {
        let mut s = SleepStatus::new();
        s.update([(false, false); 8]);
        assert_eq!(s.active_players(), 8);
        assert_eq!(s.sleepers_needed(0), 1);
        assert_eq!(s.sleepers_needed(1), 1);
        assert_eq!(s.sleepers_needed(100), 8);
    }

    #[test]
    fn sleepers_needed_rounds_up() {
        let mut s = SleepStatus::new();
        s.update([(false, false); 3]);
        // 3 * 50 / 100 = 1.5 -> 2
        assert_eq!(s.sleepers_needed(50), 2);
        // 3 * 34 / 100 = 1.02 -> 2, which is the case a truncating
        // implementation gets wrong.
        assert_eq!(s.sleepers_needed(34), 2);
        assert_eq!(s.sleepers_needed(33), 1);
    }

    #[test]
    fn spectators_do_not_count_as_active() {
        let mut s = SleepStatus::new();
        // three real players, two spectators
        s.update([
            (false, false),
            (false, false),
            (false, false),
            (true, false),
            (true, true),
        ]);
        assert_eq!(s.active_players(), 3);
        assert_eq!(
            s.amount_sleeping(),
            0,
            "a sleeping spectator is not counted"
        );
    }

    #[test]
    fn update_reports_a_change_only_while_somebody_is_or_was_asleep() {
        let mut s = SleepStatus::new();
        // Nobody asleep before or after: no report, even though the active
        // count moved from 0 to 4.
        assert!(!s.update([(false, false); 4]));
        // One goes to bed: report.
        assert!(s.update([
            (false, true),
            (false, false),
            (false, false),
            (false, false)
        ]));
        // Nothing moves: no report.
        assert!(!s.update([
            (false, true),
            (false, false),
            (false, false),
            (false, false)
        ]));
        // The active count moves while somebody is asleep: report.
        assert!(s.update([(false, true), (false, false), (false, false)]));
        // Everybody gets up: report, because oldSleeping was 1.
        assert!(s.update([(false, false); 3]));
        // And now nothing again.
        assert!(!s.update([(false, false); 2]));
    }

    #[test]
    fn remove_all_sleepers_keeps_the_active_count() {
        let mut s = SleepStatus::new();
        s.update([(false, true), (false, true), (false, false)]);
        assert_eq!(s.amount_sleeping(), 2);
        s.remove_all_sleepers();
        assert_eq!(s.amount_sleeping(), 0);
        assert_eq!(s.active_players(), 3);
    }

    #[test]
    fn the_night_needs_both_votes_and_the_deep_one_lags_by_a_hundred_ticks() {
        // One player, percentage 100. `areEnoughSleeping` is satisfied the
        // instant they are in bed; `areEnoughDeepSleeping` is not, for 100
        // ticks. `ServerLevel.tick` requires both.
        let mut s = SleepStatus::new();
        let mut c = SleepCounter::new();
        c.start();
        s.update([(false, c.is_sleeping())]);
        assert!(s.are_enough_sleeping(100));
        for _ in 0..99 {
            c.tick();
            assert!(!s.are_enough_deep_sleeping(100, i32::from(c.is_sleeping_long_enough())));
        }
        c.tick();
        assert!(s.are_enough_deep_sleeping(100, i32::from(c.is_sleeping_long_enough())));
    }

    /// The positive control. Three misreadings, each written out and shown to
    /// disagree with the transcription on a named input.
    #[test]
    fn each_misreading_of_the_sleep_path_is_separable() {
        // 1. The two `bedRule.asProblem()` returns merged into one early test.
        //    The nether-bed case then never sets a respawn point.
        let (_, fx) = start_sleep_in_bed(BedContext {
            can_sleep: false,
            can_set_spawn: true,
            ..ok()
        });
        assert!(fx.respawn_set);
        let merged_early_return_would_set = false;
        assert_ne!(fx.respawn_set, merged_early_return_would_set);

        // 2. `sleepersNeeded` written with integer division rather than
        //    `ceil` on a float: 3 players at 34% would need 1, not 2.
        let mut s = SleepStatus::new();
        s.update([(false, false); 3]);
        assert_eq!(s.sleepers_needed(34), 2);
        assert_ne!(s.sleepers_needed(34), (3 * 34) / 100);

        // 3. The sleep counter reset on waking rather than left to run out.
        //    At the tick after waking vanilla's counter is 100, not 0.
        let mut c = SleepCounter::new();
        c.start();
        for _ in 0..SLEEP_DURATION {
            c.tick();
        }
        c.stop();
        c.tick();
        assert_eq!(c.counter(), SLEEP_DURATION + 1);
        assert_ne!(c.counter(), 0);
    }
    #[test]
    fn the_bed_ladder_takes_its_four_arms_in_vanillas_order() {
        use super::{bed_use_without_item, BedUse, BedUseContext};
        let head = BedUseContext {
            clicked_head: true,
            head_is_same_bed: true,
            bed_rule_explodes: false,
            occupied: false,
        };
        assert_eq!(bed_use_without_item(head), BedUse::StartSleep);

        // The foot with a real head behind it reaches the same answer; the
        // foot with air behind it is eaten at offset 53.
        assert_eq!(
            bed_use_without_item(BedUseContext {
                clicked_head: false,
                ..head
            }),
            BedUse::StartSleep
        );
        assert_eq!(
            bed_use_without_item(BedUseContext {
                clicked_head: false,
                head_is_same_bed: false,
                ..head
            }),
            BedUse::Consume
        );
        // And a HEAD click never reads `head_is_same_bed` — offsets 11..21
        // jump straight past it.
        assert_eq!(
            bed_use_without_item(BedUseContext {
                head_is_same_bed: false,
                ..head
            }),
            BedUse::StartSleep,
            "the head branch is skipped for a head click"
        );

        // Explodes wins over occupied, which wins over sleeping.
        assert_eq!(
            bed_use_without_item(BedUseContext {
                bed_rule_explodes: true,
                occupied: true,
                ..head
            }),
            BedUse::Explodes
        );
        assert_eq!(
            bed_use_without_item(BedUseContext {
                occupied: true,
                ..head
            }),
            BedUse::Occupied
        );
    }

    #[test]
    fn the_bed_ladder_and_the_sleep_ladder_are_two_gates_and_both_have_to_open() {
        use super::{
            bed_use_without_item, start_sleep_in_bed, BedContext, BedResult, BedUse, BedUseContext,
        };
        // The joint: `BedBlock.useWithoutItem` deciding `StartSleep` is not
        // the same as `ServerPlayer.startSleepInBed` answering `Sleeping`.
        // An unoccupied bed clicked by a player standing too far away passes
        // the first and fails the second, and a port that collapses the two
        // into one predicate cannot express that.
        let use_ctx = BedUseContext {
            clicked_head: true,
            head_is_same_bed: true,
            bed_rule_explodes: false,
            occupied: false,
        };
        assert_eq!(bed_use_without_item(use_ctx), BedUse::StartSleep);

        let sleep_ctx = BedContext {
            already_sleeping: false,
            alive: true,
            can_sleep: true,
            can_set_spawn: true,
            bed_in_range: false,
            bed_blocked: false,
            creative: false,
            monsters_nearby: false,
        };
        let (result, fx) = start_sleep_in_bed(sleep_ctx);
        assert_eq!(result, BedResult::TooFarAway);
        assert!(!fx.respawn_set, "and the respawn point was NOT set");

        // The positive control: in range, the same click sleeps.
        let (result, fx) = start_sleep_in_bed(BedContext {
            bed_in_range: true,
            ..sleep_ctx
        });
        assert_eq!(result, BedResult::Sleeping);
        assert!(fx.respawn_set);
        assert!(fx.update_sleeping_player_list);
    }
}
