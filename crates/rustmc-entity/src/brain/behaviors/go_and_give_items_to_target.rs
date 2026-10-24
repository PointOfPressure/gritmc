//! `net.minecraft.world.entity.ai.behavior.GoAndGiveItemsToTarget` and its
//! `ItemThrower` callback, from the 26.2 jar.
//!
//! ```text
//! constants: CLOSE_ENOUGH_DISTANCE_TO_TARGET = 3,
//!            ITEM_PICKUP_COOLDOWN_AFTER_THROWING = 60
//!
//! GoAndGiveItemsToTarget(Function<LivingEntity, Optional<PositionTracker>>
//!                            targetPositionGetter,
//!                        float speedModifier, int duration,
//!                        ItemThrower<E> itemThrower)   (0..62)
//!   entry Map.of(LOOK_TARGET:REGISTERED, WALK_TARGET:REGISTERED,
//!                ITEM_PICKUP_COOLDOWN_TICKS:REGISTERED)
//!   Behavior(map, duration) -> (duration, duration)
//!   throwVelocity = new Vec3(0.20000000298023224d, 0.30000001192092896d,
//!                            0.20000000298023224d)
//!
//!   checkExtraStartConditions (0..5):  canThrowItemToTarget(mob)
//!   canStillUse (0..5):                canThrowItemToTarget(mob)   -- the same test
//!   start (0..23):
//!        targetPositionGetter.apply(mob).ifPresent(t ->
//!            BehaviorUtils.setWalkAndLookTargetMemories(mob, t, speedModifier, 3))
//!   tick (0..142):
//!        0   Optional<PositionTracker> t = targetPositionGetter.apply(mob)
//!       15   if t.isEmpty(): return
//!       24   PositionTracker tracker = t.get()
//!       34   Vec3 at = tracker.currentPosition()
//!       43   double d = at.distanceTo(mob.getEyePosition())
//!       54   if d < 3.0d:
//!       63      ItemStack stack = ((InventoryCarrier) mob).getInventory().removeItem(0, 1)
//!       79      if !stack.isEmpty():
//!       87         BehaviorUtils.throwItem(mob, stack, at.add(0.0, 1.0, 0.0),
//!                      throwVelocity, 0.2f)
//!      107         itemThrower.onItemThrown(level, mob, stack,
//!                      tracker.currentBlockPosition())
//!      127         mob.getBrain().setMemory(ITEM_PICKUP_COOLDOWN_TICKS, 60)
//!
//!   canThrowItemToTarget(E) (0..35):
//!        if ((InventoryCarrier) mob).getInventory().isEmpty(): return false
//!        return targetPositionGetter.apply(mob).isPresent()
//! ```
//!
//! **The throw velocity is three `double` literals, not `0.2` and `0.3`.**
//! `0.20000000298023224d` and `0.30000001192092896d` are the exact `double`
//! widenings of the `float`s `0.2f` and `0.3f`, written into the class file
//! as doubles. Transcribing them as `0.2` and `0.3` would be a different
//! vector, so the constants here are the literals the jar carries.
//!
//! **The distance is measured to the mob's EYES and the throw is aimed a
//! block ABOVE the tracker.** `at.distanceTo(mob.getEyePosition())` at
//! offset 43 against the constant 3.0, but the target handed to
//! `throwItem` is `at.add(0.0, 1.0, 0.0)`. Two different points from one
//! `currentPosition()`.
//!
//! **`removeItem(0, 1)` happens BEFORE the emptiness check** (offsets 63
//! and 79), so the inventory is mutated on every tick inside the 3-block
//! radius whether or not anything is thrown — an empty slot 0 yields an
//! empty stack and the rest of the branch is skipped, but the container
//! call has already been made.
//!
//! **`ITEM_PICKUP_COOLDOWN_TICKS` is set with `setMemory`, not
//! `setMemoryWithExpiry`** — the 60 is the memory's *value*, an
//! `Integer` counted down by a `CountDownCooldownTicks` elsewhere, not an
//! expiry. That is the difference between this and every `BREEZE_SHOOT`
//! style write in this crate.
//!
//! **`canStillUse` is literally `checkExtraStartConditions`**, so the
//! behaviour runs until the inventory empties or the target position stops
//! resolving, or until its constructor-supplied duration times out.
//!
//! **`ItemThrower` is a one-method interface**
//! (`onItemThrown(ServerLevel, E, ItemStack, BlockPos)`) and its whole
//! class file is 797 bytes with no body — a callback the provider supplies.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::behavior_utils;
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue, PositionTracker};
use crate::brain::BrainState;
use crate::host::{ItemSpec, MobHost};
use crate::phys::Vec3;

/// `CLOSE_ENOUGH_DISTANCE_TO_TARGET`.
pub const CLOSE_ENOUGH_DISTANCE_TO_TARGET: f64 = 3.0;
/// `ITEM_PICKUP_COOLDOWN_AFTER_THROWING`.
pub const ITEM_PICKUP_COOLDOWN_AFTER_THROWING: i32 = 60;
/// The `throwVelocity` x and z, verbatim from the class file.
pub const THROW_VELOCITY_XZ: f64 = 0.200_000_002_980_232_24;
/// The `throwVelocity` y, verbatim from the class file.
pub const THROW_VELOCITY_Y: f64 = 0.300_000_011_920_928_96;
/// The `throwItem` spread.
pub const THROW_SPREAD: f32 = 0.2;

/// A `Function<LivingEntity, Optional<PositionTracker>>` over the mob.
pub type TargetPositionGetter<M> = Box<dyn FnMut(&mut MobAi<M>) -> Option<PositionTracker>>;

/// `GoAndGiveItemsToTarget`'s body.
pub struct GoAndGiveItemsToTarget<M: MobHost> {
    /// `targetPositionGetter`.
    pub target_position_getter: TargetPositionGetter<M>,
    /// `speedModifier`.
    pub speed_modifier: f32,
    /// `throwVelocity`.
    pub throw_velocity: Vec3,
    /// Every `itemThrower.onItemThrown(level, mob, stack, pos)`, recorded
    /// here because the callback is the provider's and this crate has no
    /// provider yet.
    pub thrown: Vec<(ItemSpec, rustmc_data::BlockPos)>,
}

impl<M: MobHost + 'static> GoAndGiveItemsToTarget<M> {
    /// `new GoAndGiveItemsToTarget(getter, speedModifier, duration, thrower)`.
    #[must_use]
    pub fn new(
        target_position_getter: TargetPositionGetter<M>,
        speed_modifier: f32,
        duration: i32,
    ) -> Behavior<GoAndGiveItemsToTarget<M>> {
        Behavior::with_duration(
            GoAndGiveItemsToTarget {
                target_position_getter,
                speed_modifier,
                throw_velocity: Vec3::new(THROW_VELOCITY_XZ, THROW_VELOCITY_Y, THROW_VELOCITY_XZ),
                thrown: Vec::new(),
            },
            vec![
                (MemoryModuleType::LookTarget, MemoryStatus::Registered),
                (MemoryModuleType::WalkTarget, MemoryStatus::Registered),
                (
                    MemoryModuleType::ItemPickupCooldownTicks,
                    MemoryStatus::Registered,
                ),
            ],
            duration,
        )
    }

    /// `canThrowItemToTarget(E)` (0..35).
    fn can_throw_item_to_target(&mut self, cx: &mut MobAi<M>) -> bool {
        if cx.mob.inventory_items().iter().all(Option::is_none) {
            return false;
        }
        (self.target_position_getter)(cx).is_some()
    }
}

impl<M: MobHost + 'static> BehaviorBody<M> for GoAndGiveItemsToTarget<M> {
    fn check_extra_start_conditions(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        self.can_throw_item_to_target(cx)
    }

    fn can_still_use(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        self.can_throw_item_to_target(cx)
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if let Some(tracker) = (self.target_position_getter)(cx) {
            behavior_utils::set_walk_and_look_target_memories(
                brain,
                tracker,
                self.speed_modifier,
                #[allow(clippy::cast_possible_truncation)]
                {
                    CLOSE_ENOUGH_DISTANCE_TO_TARGET as i32
                },
            );
        }
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        let Some(tracker) = (self.target_position_getter)(cx) else {
            return;
        };
        let Some(at) = tracker.current_position(&cx.mob) else {
            return;
        };
        let eyes = Vec3::new(cx.mob.x(), cx.mob.eye_y(), cx.mob.z());
        if at.distance_to(eyes) >= CLOSE_ENOUGH_DISTANCE_TO_TARGET {
            return;
        }
        // `removeItem(0, 1)` runs before the emptiness test.
        let stack = cx.mob.inventory_items().first().cloned().flatten();
        cx.mob.shrink_inventory_slot(0, 1);
        let Some(stack) = stack else {
            return;
        };
        cx.mob.throw_item(stack.clone(), at.add3(0.0, 1.0, 0.0));
        if let Some(pos) = tracker.current_block_position(&cx.mob) {
            self.thrown.push((stack, pos));
        }
        brain.set_memory_value(
            MemoryModuleType::ItemPickupCooldownTicks,
            MemoryValue::Int(ITEM_PICKUP_COOLDOWN_AFTER_THROWING),
        );
    }

    fn name(&self) -> &'static str {
        "GoAndGiveItemsToTarget"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::BlockPos;

    fn item() -> ItemSpec {
        ItemSpec {
            id: "poppy",
            count: 1,
            component: None,
        }
    }

    fn behavior(at: Vec3) -> Behavior<GoAndGiveItemsToTarget<TestMob>> {
        GoAndGiveItemsToTarget::new(
            Box::new(move |_| {
                Some(PositionTracker::block(
                    crate::phys::block_pos_containing_vec(at),
                ))
            }),
            0.6,
            100,
        )
    }

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let mut mob = TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 31);
        mob.inventory = vec![Some(item())];
        let cx = MobAi::new(mob);
        let brain = Brain::<TestMob>::new(
            &[T::LookTarget, T::WalkTarget, T::ItemPickupCooldownTicks],
            vec![],
        )
        .state;
        (cx, brain)
    }

    #[test]
    fn the_throw_velocity_is_the_double_widening_of_two_floats() {
        let b = behavior(Vec3::new(1.5, 1.0, 0.5));
        assert_eq!(b.body.throw_velocity.x, f64::from(0.2f32));
        assert_eq!(b.body.throw_velocity.y, f64::from(0.3f32));
        assert_eq!(b.body.throw_velocity.z, f64::from(0.2f32));
        assert_ne!(b.body.throw_velocity.x, 0.2, "not the double 0.2");
    }

    #[test]
    fn an_empty_inventory_refuses_it_and_a_full_one_walks_and_looks() {
        let (mut cx, mut brain) = setup();
        cx.mob.inventory = vec![None];
        let mut b = behavior(Vec3::new(1.5, 1.0, 0.5));
        assert!(!b.try_start(&mut brain, &mut cx, 0));

        let (mut cx, mut brain) = setup();
        let mut b = behavior(Vec3::new(1.5, 1.0, 0.5));
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(b.end_timestamp(), 100);
        assert!(brain.has_memory_value(T::WalkTarget));
        assert!(brain.has_memory_value(T::LookTarget));
    }

    #[test]
    fn inside_three_blocks_it_throws_and_writes_the_pickup_cooldown_as_a_value() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior(Vec3::new(1.5, 1.0, 0.5));
        assert!(b.try_start(&mut brain, &mut cx, 0));
        cx.mob.calls.clear();
        b.body.tick(&mut brain, &mut cx, 1);
        let calls = cx.mob.calls.join(" | ");
        assert!(calls.contains("throw 1xpoppy"), "{calls}");
        // The aim is a block above the tracker's own position.
        assert!(calls.contains("2.5"), "one block above y = 1.5: {calls}");
        assert_eq!(
            brain.get_memory(T::ItemPickupCooldownTicks),
            Some(&MemoryValue::Int(60)),
            "setMemory, not setMemoryWithExpiry"
        );
        assert_eq!(
            brain.time_until_expiry(T::ItemPickupCooldownTicks),
            i64::MAX,
            "no expiry was set"
        );
        assert_eq!(b.body.thrown, vec![(item(), BlockPos { x: 1, y: 1, z: 0 })]);
    }

    #[test]
    fn outside_three_blocks_nothing_is_removed_from_the_inventory() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior(Vec3::new(20.5, 1.0, 0.5));
        assert!(b.try_start(&mut brain, &mut cx, 0));
        cx.mob.calls.clear();
        b.body.tick(&mut brain, &mut cx, 1);
        assert!(cx.mob.calls.is_empty());
        assert!(!brain.has_memory_value(T::ItemPickupCooldownTicks));
    }

    #[test]
    fn the_slot_is_removed_before_the_emptiness_test() {
        let (mut cx, mut brain) = setup();
        cx.mob.inventory = vec![None, Some(item())];
        let mut b = behavior(Vec3::new(1.5, 1.0, 0.5));
        // The entry condition passes on slot 1, but the throw reads slot 0.
        assert!(b.try_start(&mut brain, &mut cx, 0));
        cx.mob.calls.clear();
        b.body.tick(&mut brain, &mut cx, 1);
        let calls = cx.mob.calls.join(" | ");
        // `shrink_inventory_slot` on an empty slot is a no-op that logs
        // nothing, so the observable is that nothing was thrown and the
        // full slot 1 was left alone.
        assert_eq!(cx.mob.inventory[1], Some(item()), "slot 1 untouched");
        assert!(!calls.contains("throw 1xpoppy"), "{calls}");
        assert!(!brain.has_memory_value(T::ItemPickupCooldownTicks));
    }
}
