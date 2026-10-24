//! `net.minecraft.world.entity.ai.behavior.TryLaySpawnOnFluidNearLand`,
//! from the 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(absent(ATTACK_TARGET), present(WALK_TARGET), present(IS_PREGNANT))`.
//!
//! ```text
//! trigger (lambda$create$2, 0..198):
//!   if mob.isInWater() || !mob.onGround(): return false           <- 1..15, read carefully
//!   below = mob.blockPosition().below()
//!   for d in Direction.Plane.HORIZONTAL:
//!       q = below.relative(d)
//!       if !level.getBlockState(q).getCollisionShape(level, q).getFaceShape(UP).isEmpty(): continue
//!       if !(level.getFluidState(q).is(FluidTags.SUPPORTS_FROGSPAWN)
//!            || level.getBlockState(q).is(BlockTags.SUPPORTS_FROGSPAWN)): continue
//!       above = q.above()
//!       if level.getBlockState(above).isAir():
//!           state = block.defaultBlockState()
//!           level.setBlock(above, state, 3)
//!           level.gameEvent(GameEvent.BLOCK_PLACE, above, Context.of(mob, state))
//!           level.playSound(null, mob, SoundEvents.FROG_LAY_SPAWN, SoundSource.BLOCKS, 1.0f, 1.0f)
//!           isPregnant.erase()
//!           return true
//!   return true                                                   <- 197, ALSO true
//! ```
//!
//! **The entry guard is `!isInWater() && onGround()`.** The bytecode at
//! 1..15 is `isInWater(); ifne 14; onGround(); ifne 16; [14] iconst_0;
//! ireturn` — so being in water is an immediate refusal and being off the
//! ground is one too. A frog lays spawn standing on **land** beside water,
//! which is the opposite of what the class name suggests at a glance.
//!
//! **The trigger returns `true` whether or not it laid anything** (197),
//! so the one-shot always counts as having run; only the successful path
//! erases `IS_PREGNANT`, so an unsuccessful frog stays pregnant and tries
//! again next tick.
//!
//! The face-shape test is on the **up face of the neighbour block itself**
//! and must be **empty**, and the fluid/block `SUPPORTS_FROGSPAWN` tags
//! are an `||` — water satisfies the fluid tag, a waterlogged block the
//! block tag.
//!
//! `Direction.Plane.HORIZONTAL` iterates NORTH, SOUTH, WEST, EAST, and the
//! first neighbour that passes wins, so the search order is a fact about
//! where the spawn lands.

use rustmc_data::BlockPos;

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryStatus};
use crate::brain::BrainState;
use crate::host::{MobHost, PathLevel};

/// `FluidTags.SUPPORTS_FROGSPAWN` / `BlockTags.SUPPORTS_FROGSPAWN`.
pub const SUPPORTS_FROGSPAWN: &str = "supports_frogspawn";
/// `Direction.Plane.HORIZONTAL`, in its iteration order.
pub const HORIZONTAL: [(i32, i32); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];

/// `TryLaySpawnOnFluidNearLand`'s trigger.
#[derive(Debug, Clone, Copy)]
pub struct TryLaySpawnOnFluidNearLand {
    /// The `Block` to place, by registry name (`minecraft:` implied).
    pub block: &'static str,
}

impl TryLaySpawnOnFluidNearLand {
    /// `create(Block)`.
    #[must_use]
    pub fn create(block: &'static str) -> OneShot<Self> {
        OneShot::new(TryLaySpawnOnFluidNearLand { block })
    }
}

impl<M: MobHost> Trigger<M> for TryLaySpawnOnFluidNearLand {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if !brain.check_memory(MemoryModuleType::AttackTarget, MemoryStatus::ValueAbsent)
            || !brain.check_memory(MemoryModuleType::WalkTarget, MemoryStatus::ValuePresent)
            || !brain.check_memory(MemoryModuleType::IsPregnant, MemoryStatus::ValuePresent)
        {
            return false;
        }
        if cx.mob.is_in_water() || !cx.mob.on_ground() {
            return false;
        }
        let p = cx.mob.block_position();
        let below = BlockPos {
            x: p.x,
            y: p.y - 1,
            z: p.z,
        };
        for (dx, dz) in HORIZONTAL {
            let q = BlockPos {
                x: below.x + dx,
                y: below.y,
                z: below.z + dz,
            };
            if !cx.mob.collision_face_shape_up_empty(q) {
                continue;
            }
            if !(cx.mob.fluid_tag_is(q, SUPPORTS_FROGSPAWN)
                || cx.mob.block_tag_is(q, SUPPORTS_FROGSPAWN))
            {
                continue;
            }
            let above = BlockPos {
                x: q.x,
                y: q.y + 1,
                z: q.z,
            };
            if cx.mob.level().block_facts(above).is_air {
                cx.mob.lay_spawn_block(above, self.block);
                brain.erase_memory(MemoryModuleType::IsPregnant);
                return true;
            }
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::AttackTarget,
            MemoryModuleType::WalkTarget,
            MemoryModuleType::IsPregnant,
        ]
    }

    fn name(&self) -> &'static str {
        "TryLaySpawnOnFluidNearLand"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{MemoryModuleType as T, MemoryValue, WalkTarget};
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn it_lays_on_the_first_horizontal_neighbour_and_stays_pregnant_if_none_passes() {
        let mut level = GridLevel::new();
        level.floor(-4, -4, 4, 4, 0);
        let mut cx = MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, 1));
        cx.mob.on_ground = true;
        let mut brain =
            Brain::<TestMob>::new(&[T::AttackTarget, T::WalkTarget, T::IsPregnant], vec![]).state;
        brain.set_memory_value(
            T::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::at_block(BlockPos { x: 3, y: 1, z: 0 }, 1.0, 0)),
        );
        brain.set_memory_value(T::IsPregnant, MemoryValue::Unit);
        let mut b = TryLaySpawnOnFluidNearLand::create("frogspawn");
        // Nothing supports frogspawn: it runs, lays nothing and stays pregnant.
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(!cx.mob.calls.iter().any(|c| c.starts_with("lay_spawn")));
        assert!(brain.has_memory_value(T::IsPregnant));
        // Two neighbours qualify: EAST (+1, 0) and SOUTH (0, +1). HORIZONTAL
        // iterates NORTH, SOUTH, WEST, EAST, so SOUTH wins.
        let south = BlockPos { x: 0, y: 0, z: 1 };
        let east = BlockPos { x: 1, y: 0, z: 0 };
        for q in [south, east] {
            cx.mob.fluid_tags.push((q, SUPPORTS_FROGSPAWN));
        }
        assert!(b.try_start(&mut brain, &mut cx, 1));
        assert_eq!(cx.mob.calls, vec!["lay_spawn frogspawn 0 1 1".to_string()]);
        assert!(!brain.has_memory_value(T::IsPregnant));
    }

    #[test]
    fn water_underfoot_and_being_airborne_each_refuse_outright() {
        let mut level = GridLevel::new();
        level.floor(-4, -4, 4, 4, 0);
        let mut cx = MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, 1));
        let mut brain =
            Brain::<TestMob>::new(&[T::AttackTarget, T::WalkTarget, T::IsPregnant], vec![]).state;
        brain.set_memory_value(
            T::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::at_block(BlockPos { x: 3, y: 1, z: 0 }, 1.0, 0)),
        );
        brain.set_memory_value(T::IsPregnant, MemoryValue::Unit);
        cx.mob
            .fluid_tags
            .push((BlockPos { x: 0, y: 0, z: 1 }, SUPPORTS_FROGSPAWN));
        let mut b = TryLaySpawnOnFluidNearLand::create("frogspawn");
        cx.mob.on_ground = false;
        assert!(!b.try_start(&mut brain, &mut cx, 0), "airborne");
        cx.mob.on_ground = true;
        cx.mob.in_water = true;
        assert!(!b.try_start(&mut brain, &mut cx, 0), "swimming");
        cx.mob.in_water = false;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        // A non-air block above the fluid blocks the placement.
        cx.mob.calls.clear();
        brain.set_memory_value(T::IsPregnant, MemoryValue::Unit);
        cx.mob.level.set(0, 1, 1, crate::host::BlockFacts::STONE);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(!cx.mob.calls.iter().any(|c| c.starts_with("lay_spawn")));
        assert!(brain.has_memory_value(T::IsPregnant));
    }
}
