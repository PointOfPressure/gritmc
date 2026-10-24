//! `net.minecraft.world.entity.ai.goal.PathfindToRaidGoal<T extends Raider>`,
//! from the 26.2 jar. Flag `MOVE`; `RECRUITMENT_SEARCH_TICK_DELAY = 20`,
//! `SPEED_MODIFIER = 1.0f`. Used by Raider.
//!
//! ```text
//! canUse()            getTarget() == null && !hasControllingPassenger() && hasActiveRaid()
//!                     && !getCurrentRaid().isOver() && !level.isVillage(blockPosition())
//! canContinueToUse()  hasActiveRaid() && !getCurrentRaid().isOver() && !level.isVillage(blockPosition())
//! tick()              if hasActiveRaid():
//!                         raid = getCurrentRaid()
//!                         if tickCount > recruitmentTick: recruitmentTick = tickCount + 20; recruitNearby(raid)
//!                         if !isPathFinding():
//!                             v = DefaultRandomPos.getPosTowards(mob, 15, 4, atBottomCenterOf(raid.getCenter()), (float) PI / 2)
//!                             if v != null: navigation.moveTo(v, 1.0)
//! recruitNearby(raid) if raid.isActive():
//!                         set = new HashSet(); set.addAll(getEntitiesOfClass(Raider.class, bb.inflate(16),
//!                                 r -> !r.hasActiveRaid() && Raids.canJoinRaid(r)))
//!                         for r in set: raid.joinRaid(level, raid.getGroupsSpawned(), r, null, true)
//! ```
//!
//! `Mob.isPathFinding()` is `!navigation.isDone()`. The `HashSet` walk is
//! by `Entity.hashCode()`, which is the id: bucket `id & (n - 1)` on a
//! table of 16 that doubles past 12, 24, ... entries, insertion order
//! within a bucket — reproduced by [`java_hash_set_order`].

use crate::ai::goal::{Flag, FlagSet, Goal};
use crate::ai::goals::move_towards_restriction_goal::HALF_PI_F32_AS_F64;
use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::default_get_pos_towards;
use crate::host::MobHost;
use crate::phys::Vec3;

/// `RECRUITMENT_SEARCH_TICK_DELAY`.
pub const RECRUITMENT_SEARCH_TICK_DELAY: i32 = 20;

/// The iteration order of a `java.util.HashSet<Entity>` filled by one
/// `addAll` of `ids` (hash = id, spread `h ^ (h >>> 16)`, table 16 doubling
/// when `size > 0.75 * n`, chains in insertion order; tree bins, which
/// need eight collisions in one bucket, are not modelled).
#[must_use]
pub fn java_hash_set_order(ids: &[i32]) -> Vec<i32> {
    let mut n: usize = 16;
    while ids.len() > n * 3 / 4 {
        n *= 2;
    }
    let mut buckets: Vec<Vec<i32>> = vec![Vec::new(); n];
    for id in ids {
        #[allow(clippy::cast_sign_loss)]
        let h = (*id as u32) ^ ((*id as u32) >> 16);
        buckets[(h as usize) & (n - 1)].push(*id);
    }
    buckets.into_iter().flatten().collect()
}

/// `PathfindToRaidGoal`.
#[derive(Debug, Clone, Default)]
pub struct PathfindToRaidGoal {
    recruitment_tick: i32,
}

impl PathfindToRaidGoal {
    /// `PathfindToRaidGoal(T)`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `recruitNearby(Raid)`.
    fn recruit_nearby<M: MobHost>(cx: &mut MobAi<M>) {
        if !cx.mob.raid_is_active() {
            return;
        }
        let area = cx.mob.bounding_box().inflate(16.0, 16.0, 16.0);
        let list = cx.mob.raid_recruits_in(&area);
        for r in java_hash_set_order(&list) {
            cx.mob.raid_join(r);
        }
    }

    fn in_a_live_raid_outside_a_village<M: MobHost>(cx: &MobAi<M>) -> bool {
        cx.mob.has_active_raid()
            && !cx.mob.raid_is_over()
            && !cx.mob.is_village(cx.mob.block_position())
    }
}

impl<M: MobHost> Goal<MobAi<M>> for PathfindToRaidGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        cx.mob.target().is_none()
            && !cx.mob.has_controlling_passenger()
            && Self::in_a_live_raid_outside_a_village(cx)
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        Self::in_a_live_raid_outside_a_village(cx)
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        if !cx.mob.has_active_raid() {
            return;
        }
        if cx.mob.tick_count() > self.recruitment_tick {
            self.recruitment_tick = cx.mob.tick_count() + RECRUITMENT_SEARCH_TICK_DELAY;
            Self::recruit_nearby(cx);
        }
        if cx.navigation.is_done() {
            let centre = Vec3::at_bottom_center_of(cx.mob.raid_center());
            if let Some(v) = default_get_pos_towards(cx, 15, 4, centre, HALF_PI_F32_AS_F64) {
                cx.navigation.move_to(&mut cx.mob, v.x, v.y, v.z, 1.0);
            }
        }
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of(Flag::Move)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn hash_set_order_is_by_id_bucket() {
        assert_eq!(
            java_hash_set_order(&[17, 3, 16, 1]),
            vec![16, 17, 1, 3],
            "17 and 1 share bucket 1, insertion order"
        );
        assert_eq!(
            java_hash_set_order(&[33, 1]),
            vec![33, 1],
            "same bucket: insertion order"
        );
        let many: Vec<i32> = (0..13).rev().collect();
        assert_eq!(
            java_hash_set_order(&many),
            (0..13).collect::<Vec<_>>(),
            "resized to 32"
        );
    }

    /// Recruits every 20 ticks in hash order, and paths toward the raid
    /// centre whenever the navigation is idle.
    #[test]
    fn recruits_then_walks_to_the_raid() {
        let mut g = GridLevel::new();
        g.floor(-40, -40, 40, 40, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 2);
        mob.active_raid = true;
        mob.raid_over = false;
        mob.raid_active = true;
        mob.raid_center = rustmc_data::BlockPos { x: 30, y: 64, z: 0 };
        mob.others
            .push((EntityClass::Mob, EntityFacts::living(17, 3.5, 64.0, 0.5)));
        mob.others
            .push((EntityClass::Mob, EntityFacts::living(2, 4.5, 64.0, 0.5)));
        mob.raid_recruits = vec![17, 2];
        mob.tick_count = 5;
        let mut ai = MobAi::new(mob);
        let mut goal = PathfindToRaidGoal::new();
        assert!(goal.can_use(&mut ai));
        goal.tick(&mut ai);
        assert_eq!(
            ai.mob.calls,
            vec!["joinRaid(17)", "joinRaid(2)"],
            "buckets 1 then 2"
        );
        // getPosTowards can miss all ten candidates; each idle tick re-draws.
        for _ in 0..50 {
            if ai.navigation.is_in_progress() {
                break;
            }
            goal.tick(&mut ai);
        }
        assert!(ai.navigation.is_in_progress());
        ai.mob.tick_count = 10;
        goal.tick(&mut ai);
        assert_eq!(ai.mob.calls.len(), 2, "not yet 20 ticks");
        ai.mob.village_sections.push((0, 4, 0));
        assert!(!goal.can_continue_to_use(&mut ai), "inside a village");
    }
}
