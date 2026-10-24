//! `net.minecraft.world.entity.TamableAnimal$TamableAnimalPanicGoal`,
//! from the 26.2 jar: a `PanicGoal` whose `tick()` first does `if
//! !unableToMoveToOwner() && shouldTryTeleportToOwner():
//! tryToTeleportToOwner()` and then `super.tick()` (a bare return in
//! `PanicGoal`). Used by Cat, Parrot, Wolf.

use crate::ai::goal::{FlagSet, Goal};
use crate::ai::goals::panic_goal::PanicGoal;
use crate::ai::mob_ai::MobAi;
use crate::ai::tamable::try_to_teleport_to_owner;
use crate::host::MobHost;

/// `TamableAnimalPanicGoal`.
#[derive(Debug, Clone)]
pub struct TamableAnimalPanicGoal {
    pub inner: PanicGoal,
}

impl TamableAnimalPanicGoal {
    /// `TamableAnimalPanicGoal(TamableAnimal, double)` — the default
    /// panic-cause tag; the tag-taking constructor is the same goal with
    /// the host answering the other tag.
    #[must_use]
    pub fn new(speed_modifier: f64) -> Self {
        TamableAnimalPanicGoal {
            inner: PanicGoal::new(speed_modifier),
        }
    }
}

impl<M: MobHost> Goal<MobAi<M>> for TamableAnimalPanicGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.inner.can_use(cx)
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.inner.can_continue_to_use(cx)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        self.inner.start(cx);
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        self.inner.stop(cx);
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        if !cx.mob.unable_to_move_to_owner() && cx.mob.should_try_teleport_to_owner() {
            try_to_teleport_to_owner(cx);
        }
        self.inner.tick(cx);
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

    #[test]
    fn a_panicking_pet_far_from_its_owner_teleports_each_tick() {
        let mut g = GridLevel::new();
        g.floor(-40, -40, 40, 40, 63);
        let mut mob = TestMob::new(g, 30.5, 64.0, 30.5, 1);
        mob.panics = true;
        mob.owner = Some(9);
        mob.others
            .push((EntityClass::Player, EntityFacts::player(9, 0.5, 64.0, 0.5)));
        let mut ai = MobAi::new(mob);
        let mut goal = TamableAnimalPanicGoal::new(1.5);
        assert!(goal.can_use(&mut ai));
        goal.start(&mut ai);
        goal.tick(&mut ai);
        assert!(!ai.mob.snaps.is_empty(), "teleported toward the owner");
        ai.mob.ordered_to_sit = true;
        let n = ai.mob.snaps.len();
        goal.tick(&mut ai);
        assert_eq!(ai.mob.snaps.len(), n, "sitting: unable to move to owner");
    }
}
