//! `net.minecraft.world.entity.ai.behavior.FollowTemptation`, from the 26.2
//! jar. `Behavior<PathfinderMob>` with entry `{LOOK_TARGET: REGISTERED,
//! WALK_TARGET: REGISTERED, TEMPTATION_COOLDOWN_TICKS: VALUE_ABSENT,
//! IS_TEMPTED: VALUE_ABSENT, TEMPTING_PLAYER: VALUE_PRESENT, BREED_TARGET:
//! VALUE_ABSENT, IS_PANICKING: VALUE_ABSENT}` (in that `ImmutableMap.Builder`
//! order), the default `(60, 60)` duration with `timedOut` overridden to
//! `false`.
//!
//! ```text
//! FollowTemptation(speed)                    closeEnough = e -> 2.5, lookInTheEyes = false
//! FollowTemptation(speed, closeEnough)       lookInTheEyes = false
//! canStillUse (0..42):   TEMPTING_PLAYER present && !BREED_TARGET present && !IS_PANICKING present
//! start (0..14):         IS_TEMPTED = true
//! stop (0..43):          TEMPTATION_COOLDOWN_TICKS = 100; erase IS_TEMPTED, WALK_TARGET, LOOK_TARGET
//! tick (0..119):         p = TEMPTING_PLAYER; LOOK_TARGET = EntityTracker(p, true); d = closeEnough(mob)
//!                        if mob.distanceToSqr(p) < d*d: erase WALK_TARGET
//!                        else WALK_TARGET = WalkTarget(EntityTracker(p, lookInTheEyes, lookInTheEyes), speed(mob), 2)
//! ```
//!
//! `TEMPTATION_COOLDOWN = 100`, `DEFAULT_CLOSE_ENOUGH_DIST = 2.5`,
//! `BACKED_UP_CLOSE_ENOUGH_DIST = 3.5`.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::MobSpeed;
use crate::brain::memory::{
    MemoryModuleType, MemoryStatus, MemoryValue, PositionTracker, WalkTarget,
};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `TEMPTATION_COOLDOWN`.
pub const TEMPTATION_COOLDOWN: i32 = 100;
/// `DEFAULT_CLOSE_ENOUGH_DIST`.
pub const DEFAULT_CLOSE_ENOUGH_DIST: f64 = 2.5;
/// `BACKED_UP_CLOSE_ENOUGH_DIST`.
pub const BACKED_UP_CLOSE_ENOUGH_DIST: f64 = 3.5;

/// `Function<LivingEntity, Double>`.
pub type MobDistance<M> = Box<dyn FnMut(&mut MobAi<M>) -> f64>;

/// `FollowTemptation`'s body; build one with the constructors below.
pub struct FollowTemptation<M: MobHost> {
    pub speed_modifier: MobSpeed<M>,
    pub close_enough_distance: MobDistance<M>,
    pub look_in_the_eyes: bool,
}

impl<M: MobHost + 'static> FollowTemptation<M> {
    /// `FollowTemptation(Function<LivingEntity, Float>)`.
    #[must_use]
    pub fn new(speed_modifier: MobSpeed<M>) -> Behavior<Self> {
        Self::with_distance(speed_modifier, Box::new(|_| DEFAULT_CLOSE_ENOUGH_DIST))
    }

    /// `FollowTemptation(Function, Function)`.
    #[must_use]
    pub fn with_distance(
        speed_modifier: MobSpeed<M>,
        close_enough_distance: MobDistance<M>,
    ) -> Behavior<Self> {
        Self::full(speed_modifier, close_enough_distance, false)
    }

    /// `FollowTemptation(Function, Function, boolean)`.
    #[must_use]
    pub fn full(
        speed_modifier: MobSpeed<M>,
        close_enough_distance: MobDistance<M>,
        look_in_the_eyes: bool,
    ) -> Behavior<Self> {
        Behavior::new(
            FollowTemptation {
                speed_modifier,
                close_enough_distance,
                look_in_the_eyes,
            },
            vec![
                (MemoryModuleType::LookTarget, MemoryStatus::Registered),
                (MemoryModuleType::WalkTarget, MemoryStatus::Registered),
                (
                    MemoryModuleType::TemptationCooldownTicks,
                    MemoryStatus::ValueAbsent,
                ),
                (MemoryModuleType::IsTempted, MemoryStatus::ValueAbsent),
                (MemoryModuleType::TemptingPlayer, MemoryStatus::ValuePresent),
                (MemoryModuleType::BreedTarget, MemoryStatus::ValueAbsent),
                (MemoryModuleType::IsPanicking, MemoryStatus::ValueAbsent),
            ],
        )
    }

    fn tempting_player(brain: &BrainState) -> Option<i32> {
        brain
            .get_memory(MemoryModuleType::TemptingPlayer)
            .and_then(MemoryValue::as_entity)
    }
}

impl<M: MobHost + 'static> BehaviorBody<M> for FollowTemptation<M> {
    fn timed_out(&self, _end_timestamp: i64, _time: i64) -> bool {
        false
    }

    fn can_still_use(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) -> bool {
        Self::tempting_player(brain).is_some()
            && !brain.has_memory_value(MemoryModuleType::BreedTarget)
            && !brain.has_memory_value(MemoryModuleType::IsPanicking)
    }

    fn start(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) {
        brain.set_memory_value(MemoryModuleType::IsTempted, MemoryValue::Bool(true));
    }

    fn stop(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) {
        brain.set_memory_value(
            MemoryModuleType::TemptationCooldownTicks,
            MemoryValue::Int(TEMPTATION_COOLDOWN),
        );
        brain.erase_memory(MemoryModuleType::IsTempted);
        brain.erase_memory(MemoryModuleType::WalkTarget);
        brain.erase_memory(MemoryModuleType::LookTarget);
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        let p = Self::tempting_player(brain).expect("present while running");
        brain.set_memory_value(
            MemoryModuleType::LookTarget,
            MemoryValue::PositionTracker(PositionTracker::entity(p, true)),
        );
        let d = (self.close_enough_distance)(cx);
        let sq = cx.mob.distance_to_sqr_entity(p).unwrap_or(f64::INFINITY);
        if sq < d * d {
            brain.erase_memory(MemoryModuleType::WalkTarget);
        } else {
            let speed = (self.speed_modifier)(cx);
            brain.set_memory_value(
                MemoryModuleType::WalkTarget,
                MemoryValue::WalkTarget(WalkTarget::new(
                    PositionTracker::entity_full(p, self.look_in_the_eyes, self.look_in_the_eyes),
                    speed,
                    2,
                )),
            );
        }
    }

    fn name(&self) -> &'static str {
        "FollowTemptation"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::{BehaviorControl, Status};
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn follows_the_tempting_player_until_close_and_cools_down_on_stop() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![(EntityClass::Player, EntityFacts::player(8, 6.5, 1.0, 0.5))];
        let mut brain = Brain::<TestMob>::new(
            &[
                T::LookTarget,
                T::WalkTarget,
                T::TemptationCooldownTicks,
                T::IsTempted,
                T::TemptingPlayer,
                T::BreedTarget,
                T::IsPanicking,
            ],
            vec![],
        )
        .state;
        let mut b = FollowTemptation::<TestMob>::new(Box::new(|_| 1.25));
        assert!(!b.try_start(&mut brain, &mut cx, 0));
        brain.set_memory_value(T::TemptingPlayer, MemoryValue::Entity(8));
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain.get_memory(T::IsTempted),
            Some(&MemoryValue::Bool(true))
        );
        b.tick_or_stop(&mut brain, &mut cx, 1);
        assert_eq!(
            brain.get_memory(T::WalkTarget).unwrap().as_walk_target(),
            Some(&WalkTarget::new(
                PositionTracker::entity_full(8, false, false),
                1.25,
                2
            ))
        );
        cx.mob.others[0].1.x = 2.5;
        b.tick_or_stop(&mut brain, &mut cx, 1000);
        assert_eq!(
            BehaviorControl::<TestMob>::status(&b),
            Status::Running,
            "never times out"
        );
        assert!(
            !brain.has_memory_value(T::WalkTarget),
            "4 < 6.25: close enough"
        );
        brain.erase_memory(T::TemptingPlayer);
        b.tick_or_stop(&mut brain, &mut cx, 1001);
        assert_eq!(BehaviorControl::<TestMob>::status(&b), Status::Stopped);
        assert_eq!(
            brain.get_memory(T::TemptationCooldownTicks),
            Some(&MemoryValue::Int(100))
        );
        assert!(!brain.has_memory_value(T::IsTempted));
        assert!(!brain.has_memory_value(T::LookTarget));
    }
}
