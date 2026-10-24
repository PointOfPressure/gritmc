//! `net.minecraft.world.entity.ai.behavior.warden.Digging`, from the 26.2
//! jar. `Behavior<Warden>` with entry `{ATTACK_TARGET: VALUE_ABSENT,
//! WALK_TARGET: VALUE_ABSENT}` and the caller's duration
//! (`WardenAi.DIGGING_DURATION` = `Mth.ceil(100.0f)` = 100).
//!
//! ```text
//! canStillUse (0..12):                warden.getRemovalReason() == null
//! checkExtraStartConditions (0..26):  warden.onGround() || warden.isInWater() || warden.isInLava()
//! start (0..44):
//!   if warden.onGround(): warden.setPose(Pose.DIGGING); warden.playSound(WARDEN_DIG, 5.0f, 1.0f)
//!   else:                 warden.playSound(WARDEN_AGITATED, 5.0f, 1.0f); this.stop(level, warden, time)
//! stop (0..14):           if warden.getRemovalReason() == null: warden.remove(RemovalReason.DISCARDED)
//! ```
//!
//! **`start` calls `stop` on itself** in the not-on-ground branch (41),
//! which discards the warden immediately — so a warden that starts digging
//! while swimming or in lava is removed on the same tick it plays
//! `WARDEN_AGITATED`, without the 100 ticks elapsing. The base class then
//! still has the behaviour `RUNNING`, and its own `stop` at the duration
//! finds `getRemovalReason() != null` and does nothing.
//!
//! `canStillUse` is the removal test, so the behaviour also ends the tick
//! after anything else removed the warden.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::memory::{MemoryModuleType, MemoryStatus};
use crate::brain::BrainState;
use crate::host::{MobHost, Pose, SoundSpec};

/// `WardenAi.DIGGING_DURATION` = `Mth.ceil(100.0f)`.
pub const DIGGING_DURATION: i32 = 100;
/// `WardenAi.DIGGING_COOLDOWN`.
pub const DIGGING_COOLDOWN: i32 = 1200;

/// `warden.Digging`'s body.
#[derive(Debug, Clone, Copy, Default)]
pub struct Digging;

impl Digging {
    /// `Digging(int)`.
    #[must_use]
    pub fn new(duration: i32) -> Behavior<Digging> {
        Behavior::with_duration(
            Digging,
            vec![
                (MemoryModuleType::AttackTarget, MemoryStatus::ValueAbsent),
                (MemoryModuleType::WalkTarget, MemoryStatus::ValueAbsent),
            ],
            duration,
        )
    }
}

impl<M: MobHost> BehaviorBody<M> for Digging {
    fn check_extra_start_conditions(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        cx.mob.on_ground() || cx.mob.is_in_water() || cx.mob.is_in_lava()
    }

    fn can_still_use(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        !cx.mob.is_removed()
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        if cx.mob.on_ground() {
            cx.mob.set_pose(Pose::Digging);
            cx.mob.play_sound(&SoundSpec("warden.dig"), 5.0, 1.0);
        } else {
            cx.mob.play_sound(&SoundSpec("warden.agitated"), 5.0, 1.0);
            BehaviorBody::<M>::stop(self, brain, cx, time);
        }
    }

    fn stop(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if !cx.mob.is_removed() {
            cx.mob.remove_discarded();
        }
    }

    fn name(&self) -> &'static str {
        "Digging"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn digging_in_lava_discards_the_warden_on_the_start_tick() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(&[T::AttackTarget, T::WalkTarget], vec![]).state;
        let mut b = Digging::new(DIGGING_DURATION);
        cx.mob.on_ground = false;
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "neither on ground nor in a fluid"
        );
        cx.mob.in_lava = true;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(cx
            .mob
            .calls
            .contains(&"playSound(warden.agitated, 5, 1)".to_string()));
        assert!(cx.mob.removed, "start's else branch calls stop");
        assert_ne!(cx.mob.pose, Pose::Digging);
        // The duration's own stop then finds it already removed.
        cx.mob.calls.clear();
        b.do_stop(&mut brain, &mut cx, 100);
        assert!(!cx.mob.calls.contains(&"remove_discarded".to_string()));
    }

    #[test]
    fn on_the_ground_it_digs_for_its_duration_and_is_discarded_by_stop() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(&[T::AttackTarget, T::WalkTarget], vec![]).state;
        let mut b = Digging::new(DIGGING_DURATION);
        cx.mob.on_ground = true;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(cx.mob.pose, Pose::Digging);
        assert!(cx
            .mob
            .calls
            .contains(&"playSound(warden.dig, 5, 1)".to_string()));
        assert!(!cx.mob.removed);
        assert_eq!(b.end_timestamp(), i64::from(DIGGING_DURATION));
        b.tick_or_stop(&mut brain, &mut cx, 100);
        assert!(!cx.mob.removed, "timedOut is `time > endTimestamp`");
        b.tick_or_stop(&mut brain, &mut cx, 101);
        assert!(cx.mob.removed);
    }
}
