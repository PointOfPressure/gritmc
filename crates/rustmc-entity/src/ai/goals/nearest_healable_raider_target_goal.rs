//! `net.minecraft.world.entity.ai.goal.target.NearestHealableRaiderTargetGoal<T>`,
//! from the 26.2 jar. A `NearestAttackableTargetGoal` built with
//! `(raider, class, 500, mustSee, false, selector)`; `cooldown = 0`.
//! Used by Witch (healing other raiders).
//!
//! # `canUse()`, offsets 0..55
//!
//! ```text
//! if cooldown > 0 || !mob.getRandom().nextBoolean() return false    -- draw at offset 14
//! if !((Raider) mob).hasActiveRaid() return false
//! findTarget(); return target != null
//! ```
//!
//! `super.canUse()` is not called, so the 500-tick `randomInterval`
//! draw never happens; the gate is one `nextBoolean` per tick while the
//! cooldown is down. `start()`: `cooldown = reducedTickDelay(200)` (100),
//! then `super.start()`. `getCooldown` / `decrementCooldown` are the
//! Witch's to drive.

use rustmc_worldgen::rng::RandomSource;

use crate::ai::goal::{reduced_tick_delay, FlagSet, Goal};
use crate::ai::goals::target_goal::NearestAttackableTargetGoal;
use crate::ai::mob_ai::MobAi;
use crate::ai::targeting::Selector;
use crate::host::{EntityClass, MobHost};

/// `DEFAULT_COOLDOWN`.
pub const DEFAULT_COOLDOWN: i32 = 200;

/// `NearestHealableRaiderTargetGoal`.
pub struct NearestHealableRaiderTargetGoal<M: MobHost> {
    inner: NearestAttackableTargetGoal<M>,
    cooldown: i32,
}

impl<M: MobHost> NearestHealableRaiderTargetGoal<M> {
    /// `NearestHealableRaiderTargetGoal(Raider, Class<T>, boolean mustSee, Selector)`.
    #[must_use]
    pub fn new(
        cx: &MobAi<M>,
        target_type: EntityClass,
        must_see: bool,
        selector: Option<Selector<M>>,
    ) -> Self {
        NearestHealableRaiderTargetGoal {
            inner: NearestAttackableTargetGoal::with(
                cx,
                target_type,
                500,
                must_see,
                false,
                selector,
            ),
            cooldown: 0,
        }
    }

    /// `getCooldown()`.
    #[must_use]
    pub fn cooldown(&self) -> i32 {
        self.cooldown
    }

    /// `decrementCooldown()`: `--cooldown`.
    pub fn decrement_cooldown(&mut self) {
        self.cooldown -= 1;
    }

    /// The found target, for tests.
    #[must_use]
    pub fn found(&self) -> Option<i32> {
        self.inner.found()
    }
}

impl<M: MobHost> Goal<MobAi<M>> for NearestHealableRaiderTargetGoal<M> {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if self.cooldown > 0 || !cx.mob.random().next_bool() {
            return false;
        }
        if !cx.mob.has_active_raid() {
            return false;
        }
        self.inner.find_target(cx);
        self.inner.found().is_some()
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.inner.can_continue_to_use(cx)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        self.cooldown = reduced_tick_delay(DEFAULT_COOLDOWN);
        self.inner.start(cx);
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        self.inner.stop(cx);
    }

    fn flags(&self) -> FlagSet {
        self.inner.flags()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmc_worldgen::rng::LegacyRandomSource;

    use crate::host::EntityFacts;
    use crate::testing::{GridLevel, TestMob};

    /// The gate is one `nextBoolean` per call while the cooldown is
    /// down, no raid means no target, and `start` sets a 100-tick
    /// cooldown that the Witch counts down.
    #[test]
    fn one_coin_flip_per_tick_then_a_hundred_tick_cooldown() {
        let mut mob = TestMob::new(GridLevel::new(), 0.5, 64.0, 0.5, 21);
        mob.follow_range = 16.0;
        mob.others
            .push((EntityClass::Mob, EntityFacts::living(7, 4.5, 64.0, 0.5)));
        mob.visible.push(7);
        mob.active_raid = true;
        let mut ai = MobAi::new(mob);
        let mut goal = NearestHealableRaiderTargetGoal::new(&ai, EntityClass::Mob, true, None);
        let mut twin = LegacyRandomSource::new(21);
        let mut calls = 0;
        while !twin.next_bool() {
            calls += 1;
        }
        for _ in 0..calls {
            assert!(!goal.can_use(&mut ai));
        }
        assert!(goal.can_use(&mut ai));
        assert_eq!(goal.found(), Some(7));
        goal.start(&mut ai);
        assert_eq!(goal.cooldown(), 100);
        for _ in 0..50 {
            assert!(!goal.can_use(&mut ai), "cooldown holds without a draw");
        }
        for _ in 0..100 {
            goal.decrement_cooldown();
        }
        ai.mob.active_raid = false;
        let mut any = false;
        for _ in 0..40 {
            any |= goal.can_use(&mut ai);
        }
        assert!(!any, "no raid");
    }
}
