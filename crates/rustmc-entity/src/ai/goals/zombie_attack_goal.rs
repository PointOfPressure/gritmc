//! `net.minecraft.world.entity.ai.goal.ZombieAttackGoal`, from the 26.2
//! jar: a `MeleeAttackGoal` with `raiseArmTicks`. `start()`: super then
//! `raiseArmTicks = 0`; `stop()`: super then `setAggressive(false)`;
//! `tick()`: super, `++raiseArmTicks`, then
//! `setAggressive(raiseArmTicks >= 5 && getTicksUntilNextAttack() < getAttackInterval() / 2)`.
//! Used by Zombie.

use crate::ai::goal::{FlagSet, Goal};
use crate::ai::goals::melee_attack_goal::MeleeAttackGoal;
use crate::ai::mob_ai::MobAi;
use crate::host::MobHost;

/// `ZombieAttackGoal`.
pub struct ZombieAttackGoal {
    inner: MeleeAttackGoal,
    raise_arm_ticks: i32,
}

impl ZombieAttackGoal {
    /// `ZombieAttackGoal(Zombie, double, boolean)`.
    #[must_use]
    pub fn new(speed_modifier: f64, following_target_even_if_not_seen: bool) -> Self {
        ZombieAttackGoal {
            inner: MeleeAttackGoal::new(speed_modifier, following_target_even_if_not_seen),
            raise_arm_ticks: 0,
        }
    }
}

impl<M: MobHost> Goal<MobAi<M>> for ZombieAttackGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.inner.can_use(cx)
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.inner.can_continue_to_use(cx)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        self.inner.start(cx);
        self.raise_arm_ticks = 0;
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        self.inner.stop(cx);
        cx.mob.set_aggressive(false);
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        self.inner.tick(cx);
        self.raise_arm_ticks += 1;
        if self.raise_arm_ticks >= 5
            && self.inner.ticks_until_next_attack() < self.inner.attack_interval() / 2
        {
            cx.mob.set_aggressive(true);
        } else {
            cx.mob.set_aggressive(false);
        }
    }

    fn flags(&self) -> FlagSet {
        Goal::<MobAi<M>>::flags(&self.inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    /// The arm goes up from tick 5 on when the cooldown has fallen under
    /// 10, and `stop` always lowers it.
    #[test]
    fn raises_the_arm_after_five_ticks_in_the_second_half_of_the_cooldown() {
        let mut g = GridLevel::new();
        g.floor(-10, -10, 10, 10, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        mob.on_ground = true;
        mob.others
            .push((EntityClass::Player, EntityFacts::player(9, 1.5, 64.0, 0.5)));
        mob.target_raw = Some(9);
        mob.visible.push(9);
        mob.melee_reach.push(9);
        let mut ai = MobAi::new(mob);
        let mut goal = ZombieAttackGoal::new(1.0, false);
        ai.mob.level.game_time = 100;
        assert!(goal.can_use(&mut ai));
        goal.start(&mut ai);
        for _ in 0..4 {
            goal.tick(&mut ai);
            assert!(!ai.mob.aggressive);
        }
        // Tick 5: the attack at tick 1 set a 20-tick cooldown, now 16: not yet.
        goal.tick(&mut ai);
        assert!(!ai.mob.aggressive);
        for _ in 0..6 {
            goal.tick(&mut ai);
        }
        assert!(!ai.mob.aggressive, "tick 11: cooldown 10 is not under 10");
        goal.tick(&mut ai);
        assert!(ai.mob.aggressive, "tick 12: cooldown 9");
        goal.stop(&mut ai);
        assert!(!ai.mob.aggressive);
    }
}
