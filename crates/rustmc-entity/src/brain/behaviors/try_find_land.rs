//! `net.minecraft.world.entity.ai.behavior.TryFindLand`,
//! `TryFindLandNearWater` and `TryFindWater`, from the 26.2 jar: three
//! `BehaviorBuilder` one-shots that walk `BlockPos.withinManhattan` and
//! share the same **shared mutable cooldown**.
//!
//! ```text
//! TryFindLand.create(range, speed)          group(absent(ATTACK_TARGET), absent(WALK_TARGET), registered(LOOK_TARGET))
//! TryFindLandNearWater.create(range, speed) group(absent(ATTACK_TARGET), absent(WALK_TARGET), registered(LOOK_TARGET))
//! TryFindWater.create(range, speed)         group(absent(ATTACK_TARGET), absent(WALK_TARGET), registered(LOOK_TARGET))
//!
//! all three, first two statements of the trigger:
//!   if level.getFluidState(mob.blockPosition()).is(FluidTags.WATER) != wantsWater: return false
//!       (TryFindLand needs water underfoot; the other two need NOT water)
//!   if time < cooldown.longValue(): cooldown.setValue(time + firstDelay); return true
//!
//! TryFindLand   (lambda$create$2, 43..274), firstDelay 60, refresh 60:
//!   for q in BlockPos.withinManhattan(p, range, range, range):
//!       if q.x == p.x && q.z == p.z: continue
//!       if state(q).is(Blocks.WATER):                       continue
//!       if !level.getFluidState(q).isEmpty():               continue
//!       if !state(q).getCollisionShape(...).isEmpty():      continue
//!       if !state(q.below()).isFaceSturdy(level, q.below(), UP): continue
//!       lookTarget = BlockPosTracker(q); walkTarget = WalkTarget(BlockPosTracker(q), speed, 1); break
//!   cooldown.setValue(time + 60); return true
//!
//! TryFindLandNearWater (lambda$create$2, 43..311), firstDelay 40, refresh 40:
//!   for q in withinManhattan(p, range, range, range):
//!       if q.x == p.x && q.z == p.z: continue
//!       if !state(q).getCollisionShape(...).isEmpty():      continue
//!       if state(q.below()).getCollisionShape(...).isEmpty(): continue      <- below must be SOLID
//!       for d in Direction.Plane.HORIZONTAL:
//!           n = q.relative(d)
//!           if state(n).isAir() && state(n.below()).is(Blocks.WATER):
//!               lookTarget = BlockPosTracker(q); walkTarget = WalkTarget(BlockPosTracker(q), speed, 0)
//!               break both
//!   cooldown.setValue(time + 40); return true
//!
//! TryFindWater (lambda$create$2, 47..273), firstDelay 20 + 2, refresh 40:
//!   found = null; fallback = null
//!   for q in withinManhattan(p, range, range, range):
//!       if q.x == p.x && q.z == p.z: continue
//!       if !state(q).is(Blocks.WATER): continue
//!       if state(q.above()).isAir(): found = q; break
//!       else if fallback == null && !q.closerToCenterThan(mob.position(), 1.5): fallback = q
//!   if found == null: found = fallback
//!   if found != null: lookTarget = BlockPosTracker(found); walkTarget = WalkTarget(..., speed, 0)
//!   cooldown.setValue(time + 40); return true
//! ```
//!
//! **The cooldown is a `MutableLong` captured by `create`, not a memory.**
//! It is created once per `BehaviorControl` and shared by every start of
//! that instance, so two mobs handed the *same* behaviour object would
//! share a cooldown — which is why `FrogAi`/`AxolotlAi` build one per mob.
//! Its first delay differs from its refresh in `TryFindWater` alone
//! (`20 + 2` at 34..41, then `40` at 268..273); the other two use the same
//! number twice.
//!
//! **All three return `true` on the cooldown path**, so a cooled-down
//! attempt still counts as the one-shot having run.
//!
//! **`TryFindWater`'s `closerToCenterThan(position, 1.5)` is a
//! `!`**: a water block whose centre is *within* 1.5 of the mob is not
//! taken as the fallback, so a mob standing beside water walks to a
//! further block rather than to its own feet.
//!
//! `withinManhattan` is a shell-by-shell iterator, so "the first match" is
//! the nearest by Manhattan distance and, within a shell, the iterator's
//! own order — reproduced by `phys::within_manhattan`.
//!
//! The `closeEnoughDist` differs: `TryFindLand` walks with **1** and the
//! other two with **0**.

use rustmc_data::BlockPos;

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{
    MemoryModuleType, MemoryStatus, MemoryValue, PositionTracker, WalkTarget,
};
use crate::brain::BrainState;
use crate::host::{MobHost, PathLevel};
use crate::phys::within_manhattan;

/// `TryFindLand.COOLDOWN_TICKS`.
pub const LAND_COOLDOWN_TICKS: i64 = 60;
/// `TryFindLandNearWater`'s cooldown.
pub const LAND_NEAR_WATER_COOLDOWN_TICKS: i64 = 40;
/// `TryFindWater`'s refresh cooldown.
pub const WATER_COOLDOWN_TICKS: i64 = 40;
/// `TryFindWater`'s **first** delay, `20 + 2`.
pub const WATER_FIRST_DELAY: i64 = 22;
/// `TryFindWater`'s fallback exclusion radius.
pub const WATER_FALLBACK_MIN_DISTANCE: f64 = 1.5;

/// Which of the three this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindKind {
    /// `TryFindLand`.
    Land,
    /// `TryFindLandNearWater`.
    LandNearWater,
    /// `TryFindWater`.
    Water,
}

/// The shared body of the three.
#[derive(Debug, Clone, Copy)]
pub struct TryFindLand {
    /// Which class this is.
    pub kind: FindKind,
    /// `range`, used for all three `withinManhattan` radii.
    pub range: i32,
    /// `speedModifier`.
    pub speed_modifier: f32,
    /// The captured `MutableLong`.
    pub cooldown: i64,
}

impl TryFindLand {
    /// `TryFindLand.create(int, float)`.
    #[must_use]
    pub fn create(range: i32, speed_modifier: f32) -> OneShot<Self> {
        OneShot::new(TryFindLand {
            kind: FindKind::Land,
            range,
            speed_modifier,
            cooldown: 0,
        })
    }

    /// `TryFindLandNearWater.create(int, float)`.
    #[must_use]
    pub fn create_near_water(range: i32, speed_modifier: f32) -> OneShot<Self> {
        OneShot::new(TryFindLand {
            kind: FindKind::LandNearWater,
            range,
            speed_modifier,
            cooldown: 0,
        })
    }

    /// `TryFindWater.create(int, float)`.
    #[must_use]
    pub fn create_water(range: i32, speed_modifier: f32) -> OneShot<Self> {
        OneShot::new(TryFindLand {
            kind: FindKind::Water,
            range,
            speed_modifier,
            cooldown: 0,
        })
    }

    fn first_delay(self) -> i64 {
        match self.kind {
            FindKind::Land => LAND_COOLDOWN_TICKS,
            FindKind::LandNearWater => LAND_NEAR_WATER_COOLDOWN_TICKS,
            FindKind::Water => WATER_FIRST_DELAY,
        }
    }

    fn refresh(self) -> i64 {
        match self.kind {
            FindKind::Land => LAND_COOLDOWN_TICKS,
            FindKind::LandNearWater => LAND_NEAR_WATER_COOLDOWN_TICKS,
            FindKind::Water => WATER_COOLDOWN_TICKS,
        }
    }

    fn close_enough_dist(self) -> i32 {
        i32::from(self.kind == FindKind::Land)
    }

    /// The per-candidate test, minus the `x/z == mob's` skip.
    fn accepts<M: MobHost>(self, cx: &MobAi<M>, q: BlockPos) -> bool {
        let level = cx.mob.level();
        let below = BlockPos {
            x: q.x,
            y: q.y - 1,
            z: q.z,
        };
        match self.kind {
            FindKind::Land => {
                !level.block_facts(q).is_water_block
                    && cx.mob.fluid_is_empty_at(q)
                    && level.collision_shape_max_y(q).is_none()
                    && cx.mob.is_face_sturdy_up(below)
            }
            FindKind::LandNearWater => {
                if level.collision_shape_max_y(q).is_some()
                    || level.collision_shape_max_y(below).is_none()
                {
                    return false;
                }
                // `Direction.Plane.HORIZONTAL` iterates NORTH, SOUTH, WEST, EAST.
                [(0, -1), (0, 1), (-1, 0), (1, 0)].iter().any(|(dx, dz)| {
                    let n = BlockPos {
                        x: q.x + dx,
                        y: q.y,
                        z: q.z + dz,
                    };
                    let nb = BlockPos {
                        x: n.x,
                        y: n.y - 1,
                        z: n.z,
                    };
                    level.block_facts(n).is_air && level.block_facts(nb).is_water_block
                })
            }
            FindKind::Water => level.block_facts(q).is_water_block,
        }
    }

    fn set_targets(self, brain: &mut BrainState, target: BlockPos) {
        brain.set_memory_value(
            MemoryModuleType::LookTarget,
            MemoryValue::PositionTracker(PositionTracker::block(target)),
        );
        brain.set_memory_value(
            MemoryModuleType::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::at_block(
                target,
                self.speed_modifier,
                self.close_enough_dist(),
            )),
        );
    }
}

impl<M: MobHost> Trigger<M> for TryFindLand {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) -> bool {
        if !brain.check_memory(MemoryModuleType::AttackTarget, MemoryStatus::ValueAbsent)
            || !brain.check_memory(MemoryModuleType::WalkTarget, MemoryStatus::ValueAbsent)
        {
            return false;
        }
        let p = cx.mob.block_position();
        let in_water = cx.mob.level().block_facts(p).fluid_is_water;
        let wants_water = self.kind == FindKind::Land;
        if in_water != wants_water {
            return false;
        }
        if time < self.cooldown {
            self.cooldown = time + self.first_delay();
            return true;
        }
        let mut found: Option<BlockPos> = None;
        let mut fallback: Option<BlockPos> = None;
        for q in within_manhattan(p, self.range, self.range, self.range) {
            if q.x == p.x && q.z == p.z {
                continue;
            }
            if !self.accepts(cx, q) {
                continue;
            }
            if self.kind == FindKind::Water {
                let above = BlockPos {
                    x: q.x,
                    y: q.y + 1,
                    z: q.z,
                };
                if cx.mob.level().block_facts(above).is_air {
                    found = Some(q);
                    break;
                }
                if fallback.is_none() && !closer_to_center_than(q, cx, WATER_FALLBACK_MIN_DISTANCE)
                {
                    fallback = Some(q);
                }
                continue;
            }
            found = Some(q);
            break;
        }
        if found.is_none() {
            found = fallback;
        }
        if let Some(target) = found {
            self.set_targets(brain, target);
        }
        self.cooldown = time + self.refresh();
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::AttackTarget,
            MemoryModuleType::WalkTarget,
            MemoryModuleType::LookTarget,
        ]
    }

    fn name(&self) -> &'static str {
        match self.kind {
            FindKind::Land => "TryFindLand",
            FindKind::LandNearWater => "TryFindLandNearWater",
            FindKind::Water => "TryFindWater",
        }
    }
}

/// `BlockPos.closerToCenterThan(Position, double)`.
fn closer_to_center_than<M: MobHost>(q: BlockPos, cx: &MobAi<M>, d: f64) -> bool {
    let dx = f64::from(q.x) + 0.5 - cx.mob.x();
    let dy = f64::from(q.y) + 0.5 - cx.mob.y();
    let dz = f64::from(q.z) + 0.5 - cx.mob.z();
    dx * dx + dy * dy + dz * dz < d * d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    /// `getFluidState(pos).is(WATER)` and `getBlockState(pos).is(WATER)`
    /// together, which is what a water block answers.
    fn set_water(cx: &mut MobAi<TestMob>, p: BlockPos) {
        let mut f = crate::host::BlockFacts::AIR;
        f.fluid_is_water = true;
        f.is_water_block = true;
        cx.mob.level.set(p.x, p.y, p.z, f);
    }

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let mut level = GridLevel::new();
        level.floor(-8, -8, 8, 8, 0);
        let cx = MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, 1));
        let brain =
            Brain::<TestMob>::new(&[T::AttackTarget, T::WalkTarget, T::LookTarget], vec![]).state;
        (cx, brain)
    }

    #[test]
    fn the_shared_cooldown_burns_one_run_and_uses_a_different_first_delay_for_water() {
        let (mut cx, mut brain) = setup();
        // TryFindWater wants the mob NOT in water.
        let mut b = TryFindLand::create_water(4, 1.0);
        assert!(b.try_start(&mut brain, &mut cx, 0), "cooldown starts at 0");
        assert_eq!(b.trigger.cooldown, 40, "the refresh, not the first delay");
        // Now a run inside the cooldown takes the FIRST-delay branch.
        assert!(b.try_start(&mut brain, &mut cx, 10));
        assert_eq!(b.trigger.cooldown, 10 + WATER_FIRST_DELAY, "20 + 2");
        assert!(!brain.has_memory_value(T::WalkTarget), "it never searched");
        // TryFindLand's two numbers are the same 60.
        let mut b = TryFindLand::create(4, 1.0);
        set_water(&mut cx, BlockPos { x: 0, y: 1, z: 0 });
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(b.trigger.cooldown, LAND_COOLDOWN_TICKS);
        assert!(b.try_start(&mut brain, &mut cx, 5));
        assert_eq!(b.trigger.cooldown, 5 + LAND_COOLDOWN_TICKS);
    }

    #[test]
    fn the_water_underfoot_test_points_the_opposite_way_for_land_and_water() {
        let (mut cx, mut brain) = setup();
        let mut land = TryFindLand::create(4, 1.0);
        let mut water = TryFindLand::create_water(4, 1.0);
        assert!(
            !land.try_start(&mut brain, &mut cx, 0),
            "dry: no land search"
        );
        assert!(water.try_start(&mut brain, &mut cx, 0));
        set_water(&mut cx, BlockPos { x: 0, y: 1, z: 0 });
        assert!(land.try_start(&mut brain, &mut cx, 100));
        assert!(
            !water.try_start(&mut brain, &mut cx, 100),
            "wet: no water search"
        );
    }

    #[test]
    fn try_find_land_needs_no_fluid_no_collision_and_a_sturdy_block_below() {
        let (mut cx, mut brain) = setup();
        set_water(&mut cx, BlockPos { x: 0, y: 1, z: 0 });
        let mut b = TryFindLand::create(3, 0.7);
        // Nothing is sturdy yet: the walk target stays unset.
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.has_memory_value(T::WalkTarget));
        let spot = BlockPos { x: 2, y: 1, z: 0 };
        cx.mob.sturdy_up.push(BlockPos { x: 2, y: 0, z: 0 });
        assert!(b.try_start(&mut brain, &mut cx, 200));
        assert_eq!(
            brain.get_memory(T::WalkTarget).unwrap().as_walk_target(),
            Some(&WalkTarget::at_block(spot, 0.7, 1)),
            "closeEnoughDist is 1 for TryFindLand alone"
        );
        assert_eq!(
            brain.get_memory(T::LookTarget),
            Some(&MemoryValue::PositionTracker(PositionTracker::block(spot)))
        );
        // A fluid at the spot disqualifies it.
        brain.erase_memory(T::WalkTarget);
        cx.mob.wet_blocks.push(spot);
        assert!(b.try_start(&mut brain, &mut cx, 400));
        assert!(!brain.has_memory_value(T::WalkTarget));
    }
}
