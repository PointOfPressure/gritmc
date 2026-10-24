//! `net.minecraft.world.entity.ai.behavior.RingBell`, from the 26.2 jar. A
//! `BehaviorBuilder` one-shot over `group(present(MEETING_POINT))`.
//! `BELL_RING_CHANCE = 0.95f`, `RING_BELL_FROM_DISTANCE = 3`.
//!
//! ```text
//! lambda$create$2 (0..86):
//!   if level.getRandom().nextFloat() <= 0.95f: return false       <- LEVEL random; fcmpg/ifgt at 11..12
//!   pos = meetingPoint.pos()
//!   if pos.closerThan(mob.blockPosition(), 3.0):                   <- Vec3i.distSqr < 9.0
//!     state = level.getBlockState(pos)
//!     if state.is(BELL): ((BellBlock) state.getBlock()).attemptToRing(mob, level, pos, null)   <- result popped
//!   return true
//! ```
//!
//! The constant is named as a chance to ring, but the comparison is
//! `nextFloat() > 0.95f`: the bell is rung on 5 % of triggers, not 95 %.
//! One level `nextFloat` per trigger.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::MemoryModuleType;
use crate::brain::BrainState;
use crate::host::MobHost;
use crate::phys::dist_sqr;

/// `BELL_RING_CHANCE`.
pub const BELL_RING_CHANCE: f32 = 0.95;
/// `RING_BELL_FROM_DISTANCE`.
pub const RING_BELL_FROM_DISTANCE: i32 = 3;

/// `RingBell`'s trigger.
#[derive(Debug, Clone, Copy, Default)]
pub struct RingBell;

impl RingBell {
    /// `create()`.
    #[must_use]
    pub fn create() -> OneShot<Self> {
        OneShot::new(RingBell)
    }
}

impl<M: MobHost> Trigger<M> for RingBell {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let Some(Some(v)) = brain.memory_internal(MemoryModuleType::MeetingPoint) else {
            return false;
        };
        let Some(gp) = v.as_global_pos() else {
            return false;
        };
        let pos = gp.pos;
        if cx.mob.level_random_next_float() <= BELL_RING_CHANCE {
            return false;
        }
        let d = f64::from(RING_BELL_FROM_DISTANCE);
        if dist_sqr(pos, cx.mob.block_position()) < d * d && cx.mob.is_bell_at(pos) {
            cx.mob.attempt_to_ring_bell(pos);
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::MeetingPoint]
    }

    fn name(&self) -> &'static str {
        "RingBell"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{GlobalPos, MemoryModuleType as T, MemoryValue};
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::{BlockPos, Dimension};
    use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};

    #[test]
    fn rings_on_the_five_percent_within_three_blocks() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 5));
        let mut twin = LegacyRandomSource::new(5 ^ 0x5DEECE66D);
        let bell = BlockPos { x: 2, y: 1, z: 2 };
        cx.mob.bells.push(bell);
        let mut brain = Brain::<TestMob>::new(&[T::MeetingPoint], vec![]).state;
        let mut b = RingBell::create();
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "no meeting point: no draw"
        );
        brain.set_memory_value(
            T::MeetingPoint,
            MemoryValue::GlobalPos(GlobalPos {
                dimension: Dimension::Overworld,
                pos: bell,
            }),
        );
        let mut rung = 0;
        for t in 0..400 {
            let f = twin.next_f32();
            let r = b.try_start(&mut brain, &mut cx, t);
            assert_eq!(r, f > 0.95, "tick {t}");
            if r {
                rung += 1;
            }
        }
        assert!(rung > 0, "seed 5 passes the 5 % gate within 400 draws");
        assert_eq!(cx.mob.calls.len(), rung);
        assert_eq!(cx.mob.calls[0], "ring_bell 2 1 2");
        // (2,1,2) from (3,1,0): distSqr = 1+0+4 = 5 < 9; from (5,1,0): 9+0+4 = 13, too far.
        cx.mob.x = 5.5;
        cx.mob.calls.clear();
        for t in 0..400 {
            let f = twin.next_f32();
            assert_eq!(b.try_start(&mut brain, &mut cx, t), f > 0.95);
        }
        assert!(
            cx.mob.calls.is_empty(),
            "out of range: triggers but never rings"
        );
    }
}
