//! The `TamableAnimal` bodies the goals call that have their own draws:
//! `tryToTeleportToOwner` and what it reaches, from the 26.2 jar.
//!
//! ```text
//! tryToTeleportToOwner()                     (0..17)
//!   owner = getOwner(); if owner != null: teleportToAroundBlockPos(owner.blockPosition())
//!
//! teleportToAroundBlockPos(pos)              (0..102)
//!   for i in 0..10:
//!     dx = random.nextIntBetweenInclusive(-3, 3)      (9..20)
//!     dz = random.nextIntBetweenInclusive(-3, 3)      (22..33)
//!     if |dx| < 2 && |dz| < 2: continue              (35..52)   -- dy is NOT drawn
//!     dy = random.nextIntBetweenInclusive(-1, 1)      (56..66)
//!     if maybeTeleportTo(pos.x + dx, pos.y + dy, pos.z + dz): return
//!
//! maybeTeleportTo(x, y, z)                   (0..53)
//!   if !canTeleportTo(BlockPos(x, y, z)) return false
//!   snapTo(x + 0.5, y, z + 0.5, getYRot(), getXRot()); navigation.stop(); return true
//!
//! canTeleportTo(pos)                         (0..73)
//!   if WalkNodeEvaluator.getPathTypeStatic(this, pos) != WALKABLE return false
//!   below = level.getBlockState(pos.below())
//!   if !canFlyToOwner() && below.getBlock() instanceof LeavesBlock return false
//!   return level.noCollision(this, getBoundingBox().move(pos - blockPosition()))
//! ```
//!
//! The skipped-candidate branch draws two ints, the tried one three;
//! a replay must reproduce that.

use rustmc_data::BlockPos;
use rustmc_worldgen::rng::RandomSource;

use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::path_type_static_for;
use crate::host::{MobHost, PathLevel};
use crate::pathfinder::PathType;
use crate::phys::below;

/// `TamableAnimal.tryToTeleportToOwner()`.
pub fn try_to_teleport_to_owner<M: MobHost>(cx: &mut MobAi<M>) {
    let Some(owner) = cx.mob.owner().and_then(|id| cx.mob.entity(id)) else {
        return;
    };
    teleport_to_around_block_pos(cx, owner.block_position());
}

/// `TamableAnimal.teleportToAroundBlockPos(BlockPos)`.
pub fn teleport_to_around_block_pos<M: MobHost>(cx: &mut MobAi<M>, pos: BlockPos) {
    for _ in 0..10 {
        let dx = cx.mob.random().next_i32_between_inclusive(-3, 3);
        let dz = cx.mob.random().next_i32_between_inclusive(-3, 3);
        if dx.abs() < 2 && dz.abs() < 2 {
            continue;
        }
        let dy = cx.mob.random().next_i32_between_inclusive(-1, 1);
        if maybe_teleport_to(cx, pos.x + dx, pos.y + dy, pos.z + dz) {
            return;
        }
    }
}

/// `TamableAnimal.maybeTeleportTo(int, int, int)`.
pub fn maybe_teleport_to<M: MobHost>(cx: &mut MobAi<M>, x: i32, y: i32, z: i32) -> bool {
    if !can_teleport_to(&cx.mob, BlockPos { x, y, z }) {
        return false;
    }
    cx.mob
        .snap_to(f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5);
    cx.navigation.stop();
    true
}

/// `TamableAnimal.canTeleportTo(BlockPos)`.
#[must_use]
pub fn can_teleport_to<M: MobHost>(mob: &M, pos: BlockPos) -> bool {
    if path_type_static_for(mob, pos) != PathType::Walkable {
        return false;
    }
    let level = mob.level();
    if !mob.can_fly_to_owner() && level.block_facts(below(pos)).is_leaves {
        return false;
    }
    let here = mob.block_position();
    let moved = mob.bounding_box().moved(
        f64::from(pos.x - here.x),
        f64::from(pos.y - here.y),
        f64::from(pos.z - here.z),
    );
    level.no_collision(mob.id(), &moved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmc_worldgen::rng::LegacyRandomSource;

    use crate::host::{BlockFacts, EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    /// Replays the draws by seed: the first candidate that is not within
    /// 1 of the owner horizontally and stands on a walkable block wins,
    /// and the draw count is two per skipped candidate, three per tried.
    #[test]
    fn teleports_to_the_first_walkable_candidate_with_the_jar_draw_order() {
        let mut g = GridLevel::new();
        g.floor(-40, -40, 40, 40, 63);
        let mut mob = TestMob::new(g, 30.5, 64.0, 30.5, 42);
        mob.owner = Some(9);
        mob.others
            .push((EntityClass::Player, EntityFacts::player(9, 0.5, 64.0, 0.5)));
        let mut ai = MobAi::new(mob);
        // Predict with a twin RNG.
        let mut twin = LegacyRandomSource::new(42);
        let mut expect = None;
        for _ in 0..10 {
            let dx = twin.next_i32_between_inclusive(-3, 3);
            let dz = twin.next_i32_between_inclusive(-3, 3);
            if dx.abs() < 2 && dz.abs() < 2 {
                continue;
            }
            let dy = twin.next_i32_between_inclusive(-1, 1);
            if dy == 0 {
                expect = Some((f64::from(dx) + 0.5, 64.0, f64::from(dz) + 0.5));
                break;
            }
        }
        try_to_teleport_to_owner(&mut ai);
        assert_eq!(ai.mob.snaps.last().copied(), expect);
    }

    /// Leaves under the candidate refuse a walker and admit a flier;
    /// the flier check is `canFlyToOwner`, false in the base.
    #[test]
    fn leaves_below_refuse_a_walker() {
        let mut g = GridLevel::new();
        g.floor(-4, -4, 4, 4, 63);
        let mut leaves = BlockFacts::NONE;
        leaves.is_solid = true;
        leaves.is_leaves = true;
        g.set(2, 63, 0, leaves);
        let mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        assert!(can_teleport_to(&mob, BlockPos { x: 0, y: 64, z: 2 }));
        assert!(!can_teleport_to(&mob, BlockPos { x: 2, y: 64, z: 0 }));
        assert!(
            !can_teleport_to(&mob, BlockPos { x: 0, y: 63, z: 0 }),
            "inside the floor"
        );
    }
}
