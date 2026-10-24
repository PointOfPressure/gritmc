//! `net.minecraft.world.entity.ai.behavior.warden.Emerging` and
//! `warden.Sniffing`, from the 26.2 jar: two pose behaviours with the same
//! shape — `canStillUse` is the constant `true`, so the duration alone
//! ends them, and `stop` restores `Pose.STANDING` **only if the pose is
//! still the one `start` set**.
//!
//! ```text
//! Emerging(duration)   entry {IS_EMERGING: VALUE_PRESENT, WALK_TARGET: VALUE_ABSENT, LOOK_TARGET: REGISTERED}
//!   canStillUse: true
//!   start (0..14):  warden.setPose(Pose.EMERGING); warden.playSound(WARDEN_EMERGE, 5.0f, 1.0f)
//!   stop  (0..17):  if warden.hasPose(EMERGING): warden.setPose(STANDING)
//!
//! Sniffing(duration)   entry {IS_SNIFFING: VALUE_PRESENT, ATTACK_TARGET: VALUE_ABSENT,
//!                             WALK_TARGET: VALUE_ABSENT, LOOK_TARGET: REGISTERED,
//!                             NEAREST_ATTACKABLE: REGISTERED, DISTURBANCE_LOCATION: REGISTERED,
//!                             SNIFF_COOLDOWN: REGISTERED}
//!   canStillUse: true
//!   start (0..7):   warden.playSound(WARDEN_SNIFF, 5.0f, 1.0f)          <- no pose here; TryToSniff set it
//!   stop  (0..57):  if warden.hasPose(SNIFFING): warden.setPose(STANDING)
//!                   brain.eraseMemory(IS_SNIFFING)
//!                   brain.getMemory(NEAREST_ATTACKABLE).filter(warden::canTargetEntity).ifPresent(e -> {
//!                       if warden.closerThan(e, 6.0, 20.0): warden.increaseAngerAt(e)
//!                       if !brain.hasMemoryValue(DISTURBANCE_LOCATION):
//!                           WardenAi.setDisturbanceLocation(warden, e.blockPosition())
//!                   })
//! ```
//!
//! **`Sniffing.start` sets no pose.** `TryToSniff` set `Pose.SNIFFING`
//! before this behaviour could start, and this one only plays the sound —
//! so a `Sniffing` that starts without `TryToSniff` having run leaves the
//! warden standing and its `stop` restores nothing.
//!
//! The `closerThan(e, 6.0, 20.0)` in `stop` is the two-radius form:
//! `Mth.lengthSquared(dx, dz) < 6*6` and `Mth.square(dy) < 20*20` — a
//! horizontal disc with a vertical limit rather than a sphere
//! (`ANGER_FROM_SNIFFING_MAX_DISTANCE_XZ` and `_Y`). The anger and the disturbance are independent: a target inside
//! the box is angered, and a `DISTURBANCE_LOCATION` is written whether or
//! not it was, so long as none is set.
//!
//! `EMERGE_DURATION` is `Mth.ceil(133.59999f) = 134` and
//! `SNIFFING_DURATION` is `Mth.ceil(83.2f) = 84`, from `WardenAi`'s static
//! initialiser — neither is a round number and neither is written as an
//! integer literal anywhere in the jar.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::{MobHost, Pose, SoundSpec};

/// `WardenAi.EMERGE_DURATION` = `Mth.ceil(133.59999f)`.
pub const EMERGE_DURATION: i32 = 134;
/// `WardenAi.SNIFFING_DURATION` = `Mth.ceil(83.2f)`.
pub const SNIFFING_DURATION: i32 = 84;
/// `ANGER_FROM_SNIFFING_MAX_DISTANCE_XZ`.
pub const ANGER_FROM_SNIFFING_MAX_DISTANCE_XZ: f64 = 6.0;
/// `ANGER_FROM_SNIFFING_MAX_DISTANCE_Y`.
pub const ANGER_FROM_SNIFFING_MAX_DISTANCE_Y: f64 = 20.0;

/// Which of the two pose behaviours this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WardenPoseKind {
    /// `warden.Emerging`.
    Emerging,
    /// `warden.Sniffing`.
    Sniffing,
}

/// `warden.Emerging` / `warden.Sniffing`.
#[derive(Debug, Clone, Copy)]
pub struct Emerging {
    /// Which class this is.
    pub kind: WardenPoseKind,
}

impl Emerging {
    /// `Emerging(int)`.
    #[must_use]
    pub fn new(duration: i32) -> Behavior<Emerging> {
        Behavior::with_duration(
            Emerging {
                kind: WardenPoseKind::Emerging,
            },
            vec![
                (MemoryModuleType::IsEmerging, MemoryStatus::ValuePresent),
                (MemoryModuleType::WalkTarget, MemoryStatus::ValueAbsent),
                (MemoryModuleType::LookTarget, MemoryStatus::Registered),
            ],
            duration,
        )
    }

    /// `Sniffing(int)`.
    #[must_use]
    pub fn sniffing(duration: i32) -> Behavior<Emerging> {
        Behavior::with_duration(
            Emerging {
                kind: WardenPoseKind::Sniffing,
            },
            vec![
                (MemoryModuleType::IsSniffing, MemoryStatus::ValuePresent),
                (MemoryModuleType::AttackTarget, MemoryStatus::ValueAbsent),
                (MemoryModuleType::WalkTarget, MemoryStatus::ValueAbsent),
                (MemoryModuleType::LookTarget, MemoryStatus::Registered),
                (
                    MemoryModuleType::NearestAttackable,
                    MemoryStatus::Registered,
                ),
                (
                    MemoryModuleType::DisturbanceLocation,
                    MemoryStatus::Registered,
                ),
                (MemoryModuleType::SniffCooldown, MemoryStatus::Registered),
            ],
            duration,
        )
    }

    fn pose(self) -> Pose {
        match self.kind {
            WardenPoseKind::Emerging => Pose::Emerging,
            WardenPoseKind::Sniffing => Pose::Sniffing,
        }
    }
}

impl<M: MobHost> BehaviorBody<M> for Emerging {
    fn can_still_use(&mut self, _brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) -> bool {
        true
    }

    fn start(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        match self.kind {
            WardenPoseKind::Emerging => {
                cx.mob.set_pose(Pose::Emerging);
                cx.mob.play_sound(&SoundSpec("warden.emerge"), 5.0, 1.0);
            }
            // No pose: `TryToSniff` set `Pose.SNIFFING` already.
            WardenPoseKind::Sniffing => {
                cx.mob.play_sound(&SoundSpec("warden.sniff"), 5.0, 1.0);
            }
        }
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if cx.mob.has_pose(self.pose()) {
            cx.mob.set_pose(Pose::Standing);
        }
        if self.kind == WardenPoseKind::Emerging {
            return;
        }
        brain.erase_memory(MemoryModuleType::IsSniffing);
        let Some(target) = brain
            .get_memory(MemoryModuleType::NearestAttackable)
            .and_then(MemoryValue::as_entity)
            .filter(|id| cx.mob.can_target_entity(*id))
        else {
            return;
        };
        let Some(f) = cx.mob.entity(target) else {
            return;
        };
        let dx = f.x - cx.mob.x();
        let dy = f.y - cx.mob.y();
        let dz = f.z - cx.mob.z();
        if dx * dx + dz * dz
            < ANGER_FROM_SNIFFING_MAX_DISTANCE_XZ * ANGER_FROM_SNIFFING_MAX_DISTANCE_XZ
            && dy * dy < ANGER_FROM_SNIFFING_MAX_DISTANCE_Y * ANGER_FROM_SNIFFING_MAX_DISTANCE_Y
        {
            cx.mob.warden_increase_anger_at(target);
        }
        if !brain.has_memory_value(MemoryModuleType::DisturbanceLocation) {
            cx.mob.warden_set_disturbance_location(f.block_position());
        }
    }

    fn name(&self) -> &'static str {
        match self.kind {
            WardenPoseKind::Emerging => "Emerging",
            WardenPoseKind::Sniffing => "Sniffing",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn emerging_sets_and_restores_the_pose_only_while_it_still_holds_it() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain =
            Brain::<TestMob>::new(&[T::IsEmerging, T::WalkTarget, T::LookTarget], vec![]).state;
        brain.set_memory_value(T::IsEmerging, MemoryValue::Unit);
        let mut b = Emerging::new(EMERGE_DURATION);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(cx.mob.pose, Pose::Emerging);
        assert!(cx
            .mob
            .calls
            .contains(&"playSound(warden.emerge, 5, 1)".to_string()));
        assert_eq!(b.end_timestamp(), i64::from(EMERGE_DURATION));
        b.do_stop(&mut brain, &mut cx, 1);
        assert_eq!(cx.mob.pose, Pose::Standing);
        // Something else moved the pose: stop leaves it alone.
        assert!(b.try_start(&mut brain, &mut cx, 2));
        cx.mob.pose = Pose::Roaring;
        b.do_stop(&mut brain, &mut cx, 3);
        assert_eq!(cx.mob.pose, Pose::Roaring);
    }

    #[test]
    fn sniffing_angers_inside_the_box_and_records_a_disturbance_independently() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(9, 5.5, 25.0, 0.5))];
        cx.mob.targetable = vec![9];
        let mut brain = Brain::<TestMob>::new(
            &[
                T::IsSniffing,
                T::AttackTarget,
                T::WalkTarget,
                T::LookTarget,
                T::NearestAttackable,
                T::DisturbanceLocation,
                T::SniffCooldown,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(T::IsSniffing, MemoryValue::Unit);
        brain.set_memory_value(T::NearestAttackable, MemoryValue::Entity(9));
        let mut b = Emerging::sniffing(SNIFFING_DURATION);
        cx.mob.pose = Pose::Sniffing;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(cx.mob.pose, Pose::Sniffing, "start sets no pose");
        assert!(cx
            .mob
            .calls
            .contains(&"playSound(warden.sniff, 5, 1)".to_string()));
        b.do_stop(&mut brain, &mut cx, 1);
        assert_eq!(cx.mob.pose, Pose::Standing);
        assert!(!brain.has_memory_value(T::IsSniffing));
        assert!(
            !cx.mob.calls.iter().any(|c| c.starts_with("increase_anger")),
            "dy = 24 is outside the 20 box"
        );
        assert!(
            cx.mob.calls.contains(&"set_disturbance 5 25 0".to_string()),
            "but the disturbance is written regardless"
        );
        // Inside the box: angered, and now a disturbance exists so no second write.
        cx.mob.calls.clear();
        cx.mob.others[0].1.y = 4.0;
        // `setDisturbanceLocation` is a host call; on the real warden it
        // writes the memory, so the test does that half itself.
        brain.set_memory_value(
            T::DisturbanceLocation,
            MemoryValue::BlockPos(rustmc_data::BlockPos { x: 5, y: 25, z: 0 }),
        );
        brain.set_memory_value(T::IsSniffing, MemoryValue::Unit);
        assert!(b.try_start(&mut brain, &mut cx, 2));
        b.do_stop(&mut brain, &mut cx, 3);
        assert!(cx.mob.calls.contains(&"increase_anger 9".to_string()));
        assert!(!cx
            .mob
            .calls
            .iter()
            .any(|c| c.starts_with("set_disturbance")));
    }
}
