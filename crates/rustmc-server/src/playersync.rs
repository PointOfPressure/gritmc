//! `ServerPlayer.doTick`'s sync half — exactly when `set_health` and
//! `set_experience` go on the wire.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `net.minecraft.server.level.ServerPlayer.doTick` (offsets 145..532) and
//! `ServerPlayer.<init>` offsets 6..65 for the sentinels.
//!
//! # What main had before this file
//!
//! Neither packet has ever left this server. The recorder decodes both —
//! `tools/oracle-runner`'s `set_health` arm reads health, food and saturation,
//! and its `set_experience` arm reads progress, level and total — so the
//! instrument has been waiting on the mechanism, not the other way round.
//!
//! # This is a change detector, and it is the reason a state machine is not
//! enough
//!
//! `crate::health`, `crate::food` and `crate::experience` all move numbers.
//! None of them decides whether the client is told, and getting *that* wrong
//! is invisible to every unit test they have: a server that sent
//! `set_health` every tick would pass all of them and flood the wire, and a
//! server that sent it only on a health change would silently never tell a
//! client its food had run out.
//!
//! # The two triggers are different shapes and neither is "the fields moved"
//!
//! ```text
//!   145  getHealth() != lastSentHealth
//!     || lastSentFood != foodData.getFoodLevel()
//!     || (foodData.getSaturationLevel() == 0.0f) != lastFoodSaturationZero
//!          -> ClientboundSetHealthPacket(health, foodLevel, saturationLevel)
//!
//!   487  totalExperience != lastSentExp
//!          -> ClientboundSetExperiencePacket(experienceProgress,
//!                                            totalExperience,
//!                                            experienceLevel)
//! ```
//!
//! Two consequences a reimplementation gets wrong in opposite directions:
//!
//! * **`set_health` carries the saturation but is not triggered by it.**
//!   Only the *is it exactly zero* predicate is watched. Saturation falling
//!   5.0 -> 4.0 sends nothing, and the client keeps showing the old value
//!   until something else moves. That is why no hunger scenario can be
//!   written against this packet (see `docs/cold-start/player-systems.md`).
//! * **`set_experience` carries the level but is triggered by the *total*.**
//!   `giveExperienceLevels` on its own — `/xp levels`, an enchantment, a
//!   level penalty — moves `experienceLevel` and not `totalExperience`, so
//!   **nothing goes out** until something else moves the total.
//!
//! # Both fire once on join, and the sentinels are why
//!
//! `ServerPlayer.<init>` sets `lastSentHealth = -1.0E8f`,
//! `lastSentFood = -99999999`, `lastSentExp = -99999999` and
//! `lastFoodSaturationZero = true`. No reachable state equals any of them, so
//! the first `doTick` after a join sends both packets unconditionally. That
//! is the one observation of either packet a scenario on **any** substrate
//! can currently make.

/// `ServerPlayer.<init>` offset 43 — `ldc -1.0E8f`.
pub const LAST_SENT_HEALTH_SENTINEL: f32 = -1.0e8;
/// `ServerPlayer.<init>` offsets 49 and 60 — `ldc -99999999`.
pub const LAST_SENT_INT_SENTINEL: i32 = -99_999_999;

/// The player state `doTick` compares against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyncInputs {
    /// `getHealth()`.
    pub health: f32,
    /// `foodData.getFoodLevel()`.
    pub food_level: i32,
    /// `foodData.getSaturationLevel()`.
    pub saturation: f32,
    /// `experienceProgress`.
    pub experience_progress: f32,
    /// `totalExperience`.
    pub total_experience: i32,
    /// `experienceLevel`.
    pub experience_level: i32,
}

/// `ClientboundSetHealthPacket`'s three fields.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetHealth {
    /// `float health`.
    pub health: f32,
    /// `int food`, a varint on the wire.
    pub food: i32,
    /// `float saturation`.
    pub saturation: f32,
}

/// `ClientboundSetExperiencePacket`'s three fields, in the constructor's own
/// order `(F, I, I)` — progress, **total**, level.
///
/// The wire order is not this: `tools/oracle-runner`'s decoder reads
/// progress, then level, then total. The constructor argument order and the
/// stream order are different, and writing the packet from the constructor's
/// order is one of the three field-order traps `docs/SERVER.md` names for the
/// join packet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetExperience {
    /// `float experienceProgress`.
    pub progress: f32,
    /// `int totalExperience`.
    pub total: i32,
    /// `int experienceLevel`.
    pub level: i32,
}

/// The `lastSent*` fields, which are the whole of the change detector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyncState {
    last_sent_health: f32,
    last_sent_food: i32,
    last_food_saturation_zero: bool,
    last_sent_exp: i32,
}

impl Default for SyncState {
    /// `ServerPlayer.<init>` — the four sentinels, which is what makes a join
    /// send both packets.
    fn default() -> Self {
        Self {
            last_sent_health: LAST_SENT_HEALTH_SENTINEL,
            last_sent_food: LAST_SENT_INT_SENTINEL,
            last_food_saturation_zero: true,
            last_sent_exp: LAST_SENT_INT_SENTINEL,
        }
    }
}

impl SyncState {
    /// A freshly constructed `ServerPlayer`'s sync state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `doTick` offsets 145..267.
    ///
    /// Returns the packet to send, or `None`. The three `lastSent*` fields are
    /// updated **only** when it sends, exactly as offsets 227..264 do.
    pub fn tick_health(&mut self, inputs: SyncInputs) -> Option<SetHealth> {
        let saturation_zero = inputs.saturation == 0.0;
        // `fcmpl; ifne` on the health, so a NaN health is "different" from
        // anything, including a NaN `lastSentHealth`, and would resend every
        // tick. That is vanilla's behaviour and not a hazard here: `setHealth`
        // clamps through `Mth.clamp(v, 0.0f, maxHealth)`, whose `fcmpg` sends
        // NaN down the `Math.min` branch, so a NaN can only arrive from a NaN
        // maximum.
        let changed = inputs.health != self.last_sent_health
            || self.last_sent_food != inputs.food_level
            || saturation_zero != self.last_food_saturation_zero;
        if !changed {
            return None;
        }
        self.last_sent_health = inputs.health;
        self.last_sent_food = inputs.food_level;
        self.last_food_saturation_zero = saturation_zero;
        Some(SetHealth {
            health: inputs.health,
            food: inputs.food_level,
            saturation: inputs.saturation,
        })
    }

    /// `doTick` offsets 487..532.
    ///
    /// Note the order inside vanilla's `if`: `lastSentExp` is assigned at
    /// offset 503, **before** the packet is constructed at 510. Nothing here
    /// observes the difference, and it is written the same way so that a later
    /// change that does cannot silently reorder it.
    pub fn tick_experience(&mut self, inputs: SyncInputs) -> Option<SetExperience> {
        if inputs.total_experience == self.last_sent_exp {
            return None;
        }
        self.last_sent_exp = inputs.total_experience;
        Some(SetExperience {
            progress: inputs.experience_progress,
            total: inputs.total_experience,
            level: inputs.experience_level,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{SetExperience, SetHealth, SyncInputs, SyncState};

    fn fresh() -> SyncInputs {
        SyncInputs {
            health: 20.0,
            food_level: 20,
            saturation: 5.0,
            experience_progress: 0.0,
            total_experience: 0,
            experience_level: 0,
        }
    }

    #[test]
    fn a_join_sends_both_packets_because_of_the_sentinels() {
        let mut s = SyncState::new();
        let i = fresh();
        assert_eq!(
            s.tick_health(i),
            Some(SetHealth {
                health: 20.0,
                food: 20,
                saturation: 5.0
            })
        );
        assert_eq!(
            s.tick_experience(i),
            Some(SetExperience {
                progress: 0.0,
                total: 0,
                level: 0
            })
        );
    }

    #[test]
    fn a_second_tick_with_nothing_moving_sends_nothing() {
        let mut s = SyncState::new();
        let i = fresh();
        s.tick_health(i);
        s.tick_experience(i);
        for _ in 0..100 {
            assert_eq!(s.tick_health(i), None);
            assert_eq!(s.tick_experience(i), None);
        }
    }

    #[test]
    fn saturation_moving_off_zero_does_not_resend_but_reaching_zero_does() {
        let mut s = SyncState::new();
        let mut i = fresh();
        s.tick_health(i);

        // 5.0 -> 4.0: the packet CARRIES saturation and is not triggered by
        // it. This is the reason no hunger scenario can be written.
        i.saturation = 4.0;
        assert_eq!(s.tick_health(i), None);

        // 4.0 -> 0.0 flips the predicate, so it sends.
        i.saturation = 0.0;
        assert!(s.tick_health(i).is_some());
        // ...and back off zero flips it again.
        i.saturation = 0.5;
        assert!(s.tick_health(i).is_some());
        // ...but a further rise does not.
        i.saturation = 3.0;
        assert_eq!(s.tick_health(i), None);
    }

    #[test]
    fn the_saturation_that_is_sent_is_the_current_one_not_the_last_synced_one() {
        // The trigger is the food level here, and the saturation that rides
        // along is 4.0 — the value at the moment of sending, which the
        // detector never stored.
        let mut s = SyncState::new();
        let mut i = fresh();
        s.tick_health(i);
        i.saturation = 4.0;
        assert_eq!(s.tick_health(i), None);
        i.food_level = 19;
        assert_eq!(
            s.tick_health(i),
            Some(SetHealth {
                health: 20.0,
                food: 19,
                saturation: 4.0
            })
        );
    }

    #[test]
    fn health_and_food_each_trigger_on_their_own() {
        let mut s = SyncState::new();
        let mut i = fresh();
        s.tick_health(i);
        i.health = 19.0;
        assert!(s.tick_health(i).is_some());
        assert_eq!(s.tick_health(i), None);
        i.food_level = 19;
        assert!(s.tick_health(i).is_some());
        assert_eq!(s.tick_health(i), None);
    }

    #[test]
    fn a_level_change_with_no_total_change_sends_nothing() {
        // The case `/xp levels`, an enchantment and a level penalty all take.
        let mut s = SyncState::new();
        let mut i = fresh();
        s.tick_experience(i);
        i.experience_level = 30;
        i.experience_progress = 0.5;
        assert_eq!(
            s.tick_experience(i),
            None,
            "the trigger is totalExperience, not the level"
        );
    }

    #[test]
    fn a_total_change_sends_the_level_and_the_progress_with_it() {
        let mut s = SyncState::new();
        let mut i = fresh();
        s.tick_experience(i);
        i.experience_level = 30;
        i.experience_progress = 0.5;
        i.total_experience = 1395;
        assert_eq!(
            s.tick_experience(i),
            Some(SetExperience {
                progress: 0.5,
                total: 1395,
                level: 30
            })
        );
    }

    #[test]
    fn a_total_going_back_down_sends_too() {
        // `!=`, not `>`. Spending experience resends.
        let mut s = SyncState::new();
        let mut i = fresh();
        i.total_experience = 100;
        s.tick_experience(i);
        i.total_experience = 40;
        assert!(s.tick_experience(i).is_some());
    }

    /// The positive control. Three misreadings, each written out and shown to
    /// disagree with the transcription on a named input.
    #[test]
    fn each_misreading_of_the_sync_rules_is_separable() {
        // 1. `set_health` triggered on the saturation VALUE rather than on the
        //    is-it-zero predicate. 5.0 -> 4.0 would then send.
        let mut real = SyncState::new();
        let mut i = fresh();
        real.tick_health(i);
        i.saturation = 4.0;
        assert_eq!(real.tick_health(i), None);
        let value_triggered = 5.0_f32 != 4.0;
        assert!(value_triggered, "the value did move; the predicate did not");

        // 2. `set_experience` triggered on the LEVEL. A level change with no
        //    total change would then send.
        let mut e = SyncState::new();
        let mut j = fresh();
        e.tick_experience(j);
        j.experience_level = 30;
        assert_eq!(e.tick_experience(j), None);
        assert_ne!(j.experience_level, 0, "the level did move");

        // 3. The sentinels replaced by zeroes — the obvious way to write a
        //    "last sent" cache. A player joining at 20 health and 20 food
        //    would still send `set_health`, so the defect hides there; but a
        //    player joining with 0 experience would NOT send
        //    `set_experience`, and that is the one observation of that packet
        //    a scenario on any substrate can currently make.
        let k = fresh();
        let mut sentinel = SyncState::new();
        assert!(
            sentinel.tick_experience(k).is_some(),
            "the sentinel is what makes a join observable"
        );
        let zeroed = SyncState {
            last_sent_health: 0.0,
            last_sent_food: 0,
            last_food_saturation_zero: true,
            last_sent_exp: 0,
        };
        let mut zeroed = zeroed;
        assert_eq!(
            zeroed.tick_experience(k),
            None,
            "a zero-initialised cache swallows the join packet"
        );
    }
}
