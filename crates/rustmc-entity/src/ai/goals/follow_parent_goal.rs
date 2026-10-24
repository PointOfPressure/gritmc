//! `net.minecraft.world.entity.ai.goal.FollowParentGoal`, from the 26.2
//! jar. No flags (`setFlags` is never called: an empty set).
//!
//! `canUse`, offsets 0..148: a baby (`getAge() < 0`) scans
//! `getEntitiesOfClass(own class, bb.inflate(8, 4, 8))` for the nearest
//! adult (`getAge() >= 0`), keeping a strictly nearer one
//! (`dcmpl; ifle` at 110: a farther-or-equal candidate is skipped), and
//! refuses when none or when the nearest is within nine blocks squared.
//!
//! `canContinueToUse`: still a baby, parent alive, distance in
//! `[9, 256]`. `tick`: `--timeToRecalcPath > 0` returns, else
//! `adjustedTickDelay(10)` and a path to the parent.

use crate::ai::goal::{adjusted_tick_delay, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::{EntityClass, MobHost};

/// `HORIZONTAL_SCAN_RANGE`, `VERTICAL_SCAN_RANGE`, `DONT_FOLLOW_IF_CLOSER_THAN`.
pub const HORIZONTAL_SCAN_RANGE: i32 = 8;
pub const VERTICAL_SCAN_RANGE: i32 = 4;
pub const DONT_FOLLOW_IF_CLOSER_THAN: i32 = 3;

/// `FollowParentGoal`.
#[derive(Debug, Clone)]
pub struct FollowParentGoal {
    parent: Option<i32>,
    speed_modifier: f64,
    time_to_recalc_path: i32,
}

impl FollowParentGoal {
    /// `FollowParentGoal(Animal, double)`.
    #[must_use]
    pub fn new(speed_modifier: f64) -> Self {
        FollowParentGoal {
            parent: None,
            speed_modifier,
            time_to_recalc_path: 0,
        }
    }

    /// The parent, for tests.
    #[must_use]
    pub fn parent(&self) -> Option<i32> {
        self.parent
    }
}

impl<M: MobHost> Goal<MobAi<M>> for FollowParentGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if cx.mob.age() >= 0 {
            return false;
        }
        let area = cx.mob.bounding_box().inflate(8.0, 4.0, 8.0);
        let ids = cx.mob.entities_of_class_in(EntityClass::SameKind, &area);
        let mut best: Option<i32> = None;
        let mut best_d = f64::MAX;
        for id in ids {
            let Some(f) = cx.mob.entity(id) else {
                continue;
            };
            if f.age < 0 {
                continue;
            }
            let d = cx.mob.position().distance_to_sqr3(f.x, f.y, f.z);
            if d > best_d {
                continue;
            }
            best_d = d;
            best = Some(id);
        }
        let Some(b) = best else {
            return false;
        };
        if best_d < 9.0 {
            return false;
        }
        self.parent = Some(b);
        true
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if cx.mob.age() >= 0 {
            return false;
        }
        let Some(p) = self.parent.and_then(|id| cx.mob.entity(id)) else {
            return false;
        };
        if !p.alive {
            return false;
        }
        let d = cx.mob.position().distance_to_sqr3(p.x, p.y, p.z);
        (9.0..=256.0).contains(&d)
    }

    fn start(&mut self, _cx: &mut MobAi<M>) {
        self.time_to_recalc_path = 0;
    }

    fn stop(&mut self, _cx: &mut MobAi<M>) {
        self.parent = None;
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        self.time_to_recalc_path -= 1;
        if self.time_to_recalc_path > 0 {
            return;
        }
        self.time_to_recalc_path = adjusted_tick_delay(false, 10);
        if let Some(p) = self.parent.and_then(|id| cx.mob.entity(id)) {
            cx.navigation
                .move_to_entity_at(&mut cx.mob, p.block_position(), self.speed_modifier);
        }
    }

    fn flags(&self) -> FlagSet {
        FlagSet::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::EntityFacts;
    use crate::testing::{GridLevel, TestMob};

    /// A baby follows the nearest adult of its kind unless already within
    /// three blocks; a tie keeps the earlier (`> best` skips, equal does
    /// not replace... equal replaces: `dcmpl ifle` skips only strictly
    /// farther), and the path is recomputed every 5 ticks.
    #[test]
    fn a_baby_follows_the_nearest_adult() {
        let mut g = GridLevel::new();
        g.floor(-20, -20, 20, 20, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        mob.age = -1000;
        let mut baby = EntityFacts::living(21, 2.5, 64.0, 0.5);
        baby.age = -500;
        mob.others = vec![
            (
                EntityClass::SameKind,
                EntityFacts::living(22, 6.5, 64.0, 0.5),
            ),
            (EntityClass::SameKind, baby),
            (
                EntityClass::SameKind,
                EntityFacts::living(23, 4.5, 64.0, 0.5),
            ),
        ];
        let mut ai = MobAi::new(mob);
        let mut goal = FollowParentGoal::new(1.1);
        assert!(goal.can_use(&mut ai));
        assert_eq!(goal.parent(), Some(23));
        goal.start(&mut ai);
        assert!(goal.can_continue_to_use(&mut ai));
        goal.tick(&mut ai);
        assert!(!ai.navigation.is_done());
        // Too close: refused.
        ai.mob.others[2].1.x = 2.5;
        let mut g2 = FollowParentGoal::new(1.1);
        assert!(!g2.can_use(&mut ai));
        // Grown up: neither starts nor continues.
        ai.mob.age = 0;
        assert!(!goal.can_continue_to_use(&mut ai));
        assert!(!g2.can_use(&mut ai));
    }
}
