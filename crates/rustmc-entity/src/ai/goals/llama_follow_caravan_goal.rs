//! `net.minecraft.world.entity.ai.goal.LlamaFollowCaravanGoal`, from the
//! 26.2 jar. Flag `MOVE`; `CARAVAN_LIMIT = 8`. Used by Llama.
//!
//! # `canUse()`, offsets 0..289
//!
//! ```text
//! if llama.isLeashed() || llama.inCaravan() return false
//! list = level.getEntities(llama, bb.inflate(9, 4, 9), e -> e.is(LLAMA) || e.is(TRADER_LLAMA))
//! best = null; bestD = MAX_VALUE
//! for l in list: if l.inCaravan() && !l.hasCaravanTail(): d = distanceToSqr(l); if d <= bestD: bestD = d; best = l
//! if best == null: for l in list: if l.isLeashed() && !l.hasCaravanTail(): d = ...; if d <= bestD: best = l
//! if best == null return false
//! if bestD < 4.0 return false
//! if !best.isLeashed() && !firstIsLeashed(best, 1) return false
//! llama.joinCaravan(best); return true
//! ```
//!
//! `firstIsLeashed(l, depth)`: `depth > 8` is false; a caravan member whose
//! head is leashed is true, else recurse on the head with `depth + 1`.
//! `<=` on the distance keeps the LAST of equals.
//!
//! `canContinueToUse()`: `inCaravan() && getCaravanHead().isAlive() &&
//! firstIsLeashed(llama, 0)`, then `d = distanceToSqr(head)`: over 676
//! (26²) the speed is multiplied by 1.2 while under 3.0 and
//! `distCheckCounter = reducedTickDelay(40)`; otherwise a zero counter
//! ends the goal, a positive one counts down; true.
//! `stop()`: `leaveCaravan(); speedModifier = 2.1`. `tick()`: only in a
//! caravan and not held by a fence knot: `head = getCaravanHead(); d =
//! (double) distanceTo(head); v = (head - llama).normalize().scale(max(d
//! - 2.0, 0.0)); navigation.moveTo(llama + v, speedModifier)`.

use crate::ai::goal::{reduced_tick_delay, Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::{EntityFacts, MobHost};
use crate::mth::sqrt_f32;
use crate::phys::Vec3;

/// `CARAVAN_LIMIT`.
pub const CARAVAN_LIMIT: i32 = 8;

/// `LlamaFollowCaravanGoal`.
#[derive(Debug, Clone)]
pub struct LlamaFollowCaravanGoal {
    speed_modifier: f64,
    dist_check_counter: i32,
}

impl LlamaFollowCaravanGoal {
    /// `LlamaFollowCaravanGoal(Llama, double)`.
    #[must_use]
    pub fn new(speed_modifier: f64) -> Self {
        LlamaFollowCaravanGoal {
            speed_modifier,
            dist_check_counter: 0,
        }
    }

    /// `firstIsLeashed(Llama, int)` on another llama.
    fn first_is_leashed<M: MobHost>(cx: &MobAi<M>, llama: i32, depth: i32) -> bool {
        if depth > CARAVAN_LIMIT {
            return false;
        }
        if cx.mob.llama_in_caravan(llama) {
            let Some(head) = cx.mob.llama_caravan_head(llama) else {
                return false;
            };
            if cx.mob.llama_is_leashed(head) {
                return true;
            }
            return Self::first_is_leashed(cx, head, depth + 1);
        }
        false
    }

    /// `firstIsLeashed(this, 0)`: this llama's own caravan state.
    fn self_first_is_leashed<M: MobHost>(cx: &MobAi<M>) -> bool {
        if cx.mob.in_caravan() {
            let Some(head) = cx.mob.caravan_head() else {
                return false;
            };
            if cx.mob.llama_is_leashed(head) {
                return true;
            }
            return Self::first_is_leashed(cx, head, 1);
        }
        false
    }

    fn distance_to<M: MobHost>(mob: &M, f: &EntityFacts) -> f32 {
        let dx = (mob.x() - f.x) as f32;
        let dy = (mob.y() - f.y) as f32;
        let dz = (mob.z() - f.z) as f32;
        sqrt_f32(dx * dx + dy * dy + dz * dz)
    }

    /// `speedModifier`, for tests.
    #[must_use]
    pub fn speed_modifier(&self) -> f64 {
        self.speed_modifier
    }
}

impl<M: MobHost> Goal<MobAi<M>> for LlamaFollowCaravanGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if cx.mob.is_leashed() || cx.mob.in_caravan() {
            return false;
        }
        let area = cx.mob.bounding_box().inflate(9.0, 4.0, 9.0);
        let list = cx.mob.llamas_in(&area);
        let mut best: Option<i32> = None;
        let mut best_d = f64::MAX;
        for l in &list {
            if cx.mob.llama_in_caravan(*l) && !cx.mob.llama_has_caravan_tail(*l) {
                let Some(d) = cx.mob.distance_to_sqr_entity(*l) else {
                    continue;
                };
                if d > best_d {
                    continue;
                }
                best_d = d;
                best = Some(*l);
            }
        }
        if best.is_none() {
            for l in &list {
                if cx.mob.llama_is_leashed(*l) && !cx.mob.llama_has_caravan_tail(*l) {
                    let Some(d) = cx.mob.distance_to_sqr_entity(*l) else {
                        continue;
                    };
                    if d > best_d {
                        continue;
                    }
                    best_d = d;
                    best = Some(*l);
                }
            }
        }
        let Some(b) = best else {
            return false;
        };
        if best_d < 4.0 {
            return false;
        }
        if !cx.mob.llama_is_leashed(b) && !Self::first_is_leashed(cx, b, 1) {
            return false;
        }
        cx.mob.join_caravan(b);
        true
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if !cx.mob.in_caravan() {
            return false;
        }
        let Some(head) = cx.mob.caravan_head() else {
            return false;
        };
        if !cx.mob.entity(head).is_some_and(|f| f.alive) {
            return false;
        }
        if !Self::self_first_is_leashed(cx) {
            return false;
        }
        let d = cx.mob.distance_to_sqr_entity(head).unwrap_or(0.0);
        if d > 676.0 {
            if self.speed_modifier <= 3.0 {
                self.speed_modifier *= 1.2;
                self.dist_check_counter = reduced_tick_delay(40);
                return true;
            }
            if self.dist_check_counter == 0 {
                return false;
            }
        }
        if self.dist_check_counter > 0 {
            self.dist_check_counter -= 1;
        }
        true
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        cx.mob.leave_caravan();
        self.speed_modifier = 2.1;
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        if !cx.mob.in_caravan() {
            return;
        }
        if cx.mob.leash_holder_is_fence_knot() {
            return;
        }
        let Some(head) = cx.mob.caravan_head().and_then(|h| cx.mob.entity(h)) else {
            return;
        };
        let d = f64::from(Self::distance_to(&cx.mob, &head));
        let v = Vec3::new(
            head.x - cx.mob.x(),
            head.y - cx.mob.y(),
            head.z - cx.mob.z(),
        )
        .normalize()
        .scale(rustmc_worldgen::math::java_max(d - 2.0, 0.0));
        let (x, y, z) = (cx.mob.x() + v.x, cx.mob.y() + v.y, cx.mob.z() + v.z);
        let s = self.speed_modifier;
        cx.navigation.move_to(&mut cx.mob, x, y, z, s);
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of(Flag::Move)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::EntityClass;
    use crate::testing::{GridLevel, TestMob};

    /// Two caravan llamas without tails at 3 and 5 blocks: the nearer
    /// wins (`<=` keeps a later equal); it is leashed, so this llama
    /// joins. A candidate under 2 blocks refuses.
    #[test]
    fn joins_the_nearest_tailless_caravan_member() {
        let mut g = GridLevel::new();
        g.floor(-20, -20, 20, 20, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        mob.others
            .push((EntityClass::Animal, EntityFacts::living(5, 5.5, 64.0, 0.5)));
        mob.others
            .push((EntityClass::Animal, EntityFacts::living(3, 3.5, 64.0, 0.5)));
        mob.llamas.insert(5, (true, false, None, true));
        mob.llamas.insert(3, (true, false, None, true));
        let mut ai = MobAi::new(mob);
        let mut goal = LlamaFollowCaravanGoal::new(2.1);
        assert!(goal.can_use(&mut ai));
        assert_eq!(ai.mob.calls, vec!["joinCaravan(3)"]);
        assert!(!goal.can_use(&mut ai), "already in a caravan");
        goal.stop(&mut ai);
        ai.mob.others[1].1.x = 1.5;
        assert!(!goal.can_use(&mut ai), "the nearest is under 2 blocks");
    }

    /// Over 26 blocks from the head the speed climbs by 1.2 per check
    /// while under 3, and the goal walks toward `head - 2` along the line.
    #[test]
    fn speeds_up_when_the_head_is_far() {
        let mut g = GridLevel::new();
        g.floor(-40, -40, 40, 40, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        mob.others
            .push((EntityClass::Animal, EntityFacts::living(3, 30.5, 64.0, 0.5)));
        mob.llamas.insert(3, (false, false, None, true));
        mob.in_caravan = true;
        mob.caravan_head = Some(3);
        let mut ai = MobAi::new(mob);
        let mut goal = LlamaFollowCaravanGoal::new(2.1);
        assert!(goal.can_continue_to_use(&mut ai));
        assert!((goal.speed_modifier() - 2.52).abs() < 1e-12);
        goal.tick(&mut ai);
        assert!(ai.navigation.is_in_progress());
    }
}
