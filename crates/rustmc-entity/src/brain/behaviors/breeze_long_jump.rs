//! `net.minecraft.world.entity.monster.breeze.LongJump`, from the 26.2 jar.
//!
//! ```text
//! constants: REQUIRED_AIR_BLOCKS_ABOVE = 4, JUMP_COOLDOWN_TICKS = 10,
//!            JUMP_COOLDOWN_WHEN_HURT_TICKS = 2, DEFAULT_FOLLOW_RANGE = 24.0f,
//!            DEFAULT_MAX_JUMP_VELOCITY = 1.4f,
//!            MAX_JUMP_VELOCITY_MULTIPLIER = 0.058333334f
//! <clinit>:  INHALING_DURATION_TICKS = Math.round(10.0f) = 10
//!            ALLOWED_ANGLES = ObjectArrayList[40, 55, 60, 75, 80]
//!
//! LongJump()   Behavior<Breeze>, Behavior(map, 200) -> (200, 200)
//!   entry Map.of(ATTACK_TARGET:VALUE_PRESENT, BREEZE_JUMP_COOLDOWN:VALUE_ABSENT,
//!         BREEZE_JUMP_INHALING:REGISTERED, BREEZE_JUMP_TARGET:REGISTERED,
//!         BREEZE_SHOOT:VALUE_ABSENT, WALK_TARGET:VALUE_ABSENT,
//!         BREEZE_LEAVING_WATER:REGISTERED)
//!
//!   canRun(level, breeze)  (0..190)   -- also checkExtraStartConditions
//!       0  if !onGround() && !isInWater(): return false
//!      16  if Swim.shouldSwim(breeze): return false
//!      25  if brain.checkMemory(BREEZE_JUMP_TARGET, VALUE_PRESENT): return true
//!      43  target = brain.getMemory(ATTACK_TARGET).orElse(null); if null: return false
//!      67  if outOfAggroRange(breeze, target): brain.eraseMemory(ATTACK_TARGET); return false
//!      87  if tooCloseForJump(breeze, target): return false
//!      97  if !canJumpFromCurrentPosition(level, breeze): return false
//!     107  pos = snapToSurface(breeze, BreezeUtil.randomPointBehindTarget(target, breeze.getRandom()))
//!     120  if pos == null: return false
//!     126  if breeze.getType().isBlockDangerous(level.getBlockState(pos.below())): return false
//!     150  if !hasLineOfSight(breeze, atCenterOf(pos))
//!             && !hasLineOfSight(breeze, atCenterOf(pos.above(4))): return false
//!     178  brain.setMemory(BREEZE_JUMP_TARGET, pos); return true
//!
//!   outOfAggroRange (0..20):  !target.closerThan(breeze, breeze.getAttributeValue(FOLLOW_RANGE))
//!   tooCloseForJump (0..19):  target.distanceTo(breeze) - 4.0f <= 0.0f
//!   canJumpFromCurrentPosition (0..74):
//!         pos = breeze.blockPosition()
//!         if level.getBlockState(pos).is(HONEY_BLOCK): return false
//!         for i in 1..=4:
//!             q = pos.relative(UP, i)
//!             if !level.getBlockState(q).isAir()
//!                && !level.getFluidState(q).is(FluidTags.WATER): return false
//!         return true
//!
//!   canStillUse (0..28):
//!         breeze.getPose() != STANDING && !brain.hasMemoryValue(BREEZE_JUMP_COOLDOWN)
//!   start (0..73):
//!        0  if brain.checkMemory(BREEZE_JUMP_INHALING, VALUE_ABSENT):
//!              brain.setMemoryWithExpiry(BREEZE_JUMP_INHALING, Unit.INSTANCE, 10L)
//!       33  breeze.setPose(INHALING)
//!       40  level.playSound(null, breeze, BREEZE_CHARGE, HOSTILE, 1.0f, 1.0f)
//!       54  brain.getMemory(BREEZE_JUMP_TARGET)
//!              .ifPresent(pos -> breeze.lookAt(EYES, Vec3.atCenterOf(pos)))
//!   tick (0..224):
//!        0  inWater = breeze.isInWater()
//!       11  if !inWater && brain.checkMemory(BREEZE_LEAVING_WATER, VALUE_PRESENT):
//!              brain.eraseMemory(BREEZE_LEAVING_WATER)
//!       37  if isFinishedInhaling(breeze):
//!       44     v = brain.getMemory(BREEZE_JUMP_TARGET)
//!                   .flatMap(pos -> calculateOptimalJumpVector(breeze, breeze.getRandom(),
//!                                       Vec3.atBottomCenterOf(pos))).orElse(null)
//!       72     if v == null: breeze.setPose(STANDING); return
//!       85     if inWater: brain.setMemory(BREEZE_LEAVING_WATER, Unit.INSTANCE)
//!      103     breeze.playSound(BREEZE_JUMP, 1.0f, 1.0f)
//!      112     breeze.setPose(LONG_JUMPING)
//!      119     breeze.setYRot(breeze.yBodyRot)
//!      127     breeze.setDiscardFriction(true)
//!      132     breeze.setDeltaMovement(v)
//!      141  else if isFinishedJumping(breeze):
//!      148     breeze.playSound(BREEZE_LAND, 1.0f, 1.0f)
//!      157     breeze.setPose(STANDING)
//!      164     breeze.setDiscardFriction(false)
//!      169     hurt = brain.hasMemoryValue(HURT_BY)
//!      181     brain.setMemoryWithExpiry(BREEZE_JUMP_COOLDOWN, Unit.INSTANCE, hurt ? 2L : 10L)
//!      208     brain.setMemoryWithExpiry(BREEZE_SHOOT, Unit.INSTANCE, 100L)
//!   stop (0..57):
//!        0  if pose == LONG_JUMPING || pose == INHALING: breeze.setPose(STANDING)
//!       27  erase BREEZE_JUMP_TARGET, BREEZE_JUMP_INHALING, BREEZE_LEAVING_WATER
//!
//!   isFinishedInhaling (0..31):
//!        brain.getMemory(BREEZE_JUMP_INHALING).isEmpty() && pose == INHALING
//!   isFinishedJumping (0..67):
//!        pose == LONG_JUMPING
//!        && (onGround() || (isInWater() && brain.checkMemory(BREEZE_LEAVING_WATER, VALUE_ABSENT)))
//!   calculateOptimalJumpVector(breeze, random, target)  (0..129):
//!        0  for angle in Util.shuffledCopy(ALLOWED_ANGLES, random):
//!       41     maxVel = 0.058333334f * (float) breeze.getAttributeValue(FOLLOW_RANGE)
//!       55     opt = LongJumpUtil.calculateJumpVectorForAngle(breeze, target, maxVel, angle, false)
//!       67     if opt.isPresent():
//!       75        if breeze.hasEffect(JUMP_BOOST):
//!       85           d = opt.get().normalize().y * breeze.getJumpBoostPower()
//!      107           return opt.map(v -> v.add(0.0, d, 0.0))
//!      120        return opt
//!      126  return Optional.empty()
//! ```
//!
//! **The shuffle here IS seeded, unlike `LongJumpToRandomPos`'s.**
//! `LongJumpToRandomPos.calculateOptimalJumpVector` uses
//! `Collections.shuffle(list)` with an unseeded `java.util.Random`, which
//! the fifth lane recorded as unverifiable by construction. This one uses
//! `Util.shuffledCopy(ALLOWED_ANGLES, random)` with the *breeze's own*
//! `RandomSource`, and `Util.shuffle` (offsets 0..56) is an ordinary
//! Fisher-Yates from the end down: for `i` from `size` while `i > 1`, draw
//! `j = random.nextInt(i)` and swap `i - 1` with `j`. Five angles means
//! four draws, always, before any jump vector is computed. That makes this
//! class fully deterministic from a seed, and the test replays it.
//!
//! **`canStillUse` wants the pose NOT to be standing.** A breeze that has
//! not yet reached the `INHALING` pose — or has been knocked back to
//! `STANDING` mid-jump — ends the behaviour, which is the opposite of the
//! usual shape and is why `start` sets the pose before anything else can
//! tick.
//!
//! **The water handling is a three-memory dance.** `BREEZE_LEAVING_WATER`
//! is set at the moment of a jump taken *from* water and erased on the
//! first tick after the breeze is out of it; `isFinishedJumping` treats
//! being in water *without* that memory as having landed. So a jump into
//! water ends on contact, and a jump out of water does not end until the
//! breeze is airborne and back down.
//!
//! **The hurt cooldown is 2 ticks against the usual 10** — `HURT_BY`
//! present at the moment of landing, not at the moment of jumping.
//!
//! **`REQUIRED_AIR_BLOCKS_ABOVE` is a `for` from 1 to 4 inclusive**, so the
//! block the breeze stands *in* is not one of the four; only the honey-block
//! test looks at it.
//!
//! **UNVERIFIABLE FROM THIS CRATE: `snapToSurface`.** It needs
//! `Level.clip`'s exact hit location; [`MobHost::snap_to_surface`] carries
//! the bytecode and the host owes it. Everything around it — the aggro
//! range, the 4-block gap, the dangerous block, the two lines of sight and
//! the memory it writes — is transcribed and tested.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::breeze_util;
use crate::brain::behaviors::long_jump_util;
use crate::brain::behaviors::swim::Swim;
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::{MobHost, PathLevel, Pose, SoundSpec};
use crate::phys::Vec3;
use rustmc_data::BlockPos;
use rustmc_worldgen::rng::RandomSource;

/// `SoundEvents.BREEZE_CHARGE`.
pub const BREEZE_CHARGE: SoundSpec = SoundSpec("entity.breeze.charge");
/// `SoundEvents.BREEZE_JUMP`.
pub const BREEZE_JUMP: SoundSpec = SoundSpec("entity.breeze.jump");
/// `SoundEvents.BREEZE_LAND`.
pub const BREEZE_LAND: SoundSpec = SoundSpec("entity.breeze.land");
/// `SoundSource.HOSTILE`.
pub const SOUND_SOURCE_HOSTILE: &str = "hostile";

/// `LongJump.REQUIRED_AIR_BLOCKS_ABOVE`.
pub const REQUIRED_AIR_BLOCKS_ABOVE: i32 = 4;
/// `LongJump.JUMP_COOLDOWN_TICKS`.
pub const JUMP_COOLDOWN_TICKS: i64 = 10;
/// `LongJump.JUMP_COOLDOWN_WHEN_HURT_TICKS`.
pub const JUMP_COOLDOWN_WHEN_HURT_TICKS: i64 = 2;
/// `LongJump.MAX_JUMP_VELOCITY_MULTIPLIER`.
pub const MAX_JUMP_VELOCITY_MULTIPLIER: f32 = 0.058_333_334;
/// `Math.round(10.0f)`.
pub const INHALING_DURATION_TICKS: i64 = 10;
/// The behaviour's duration, both min and max.
pub const LONG_JUMP_DURATION: i32 = 200;
/// The expiry the landing gives `BREEZE_SHOOT`.
pub const SHOOT_AFTER_LANDING_TICKS: i64 = 100;
/// The distance below which a target is too close to jump at.
pub const TOO_CLOSE_DISTANCE: f32 = 4.0;
/// `LongJump.ALLOWED_ANGLES`, in `<clinit>` order.
pub const ALLOWED_ANGLES: [i32; 5] = [40, 55, 60, 75, 80];

/// `net.minecraft.util.Util.shuffle(List, RandomSource)` (offsets 0..56):
/// for `i` from `list.size()` while `i > 1`, `j = random.nextInt(i)` then
/// `list.set(i - 1, list.set(j, list.get(i - 1)))` — a swap of `i - 1` and
/// `j`. `shuffledCopy` (0..15) is a copy followed by this.
pub fn util_shuffle<T>(list: &mut [T], random: &mut impl RandomSource) {
    let mut i = list.len();
    while i > 1 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let j = random.next_i32_bound(i as i32) as usize;
        list.swap(i - 1, j);
        i -= 1;
    }
}

/// `LongJump`'s body.
#[derive(Debug, Clone, Copy, Default)]
pub struct LongJump;

impl LongJump {
    /// `new LongJump()`.
    #[must_use]
    pub fn new() -> Behavior<LongJump> {
        Behavior::with_duration(
            LongJump,
            vec![
                (MemoryModuleType::AttackTarget, MemoryStatus::ValuePresent),
                (
                    MemoryModuleType::BreezeJumpCooldown,
                    MemoryStatus::ValueAbsent,
                ),
                (
                    MemoryModuleType::BreezeJumpInhaling,
                    MemoryStatus::Registered,
                ),
                (MemoryModuleType::BreezeJumpTarget, MemoryStatus::Registered),
                (MemoryModuleType::BreezeShoot, MemoryStatus::ValueAbsent),
                (MemoryModuleType::WalkTarget, MemoryStatus::ValueAbsent),
                (
                    MemoryModuleType::BreezeLeavingWater,
                    MemoryStatus::Registered,
                ),
            ],
            LONG_JUMP_DURATION,
        )
    }

    /// `outOfAggroRange(Breeze, LivingEntity)` (0..20).
    fn out_of_aggro_range<M: MobHost>(cx: &MobAi<M>, target: i32) -> bool {
        !cx.mob.entity_closer_than(target, cx.mob.follow_range())
    }

    /// `tooCloseForJump(Breeze, LivingEntity)` (0..19).
    fn too_close_for_jump<M: MobHost>(cx: &MobAi<M>, target: i32) -> bool {
        let Some(d2) = cx.mob.distance_to_sqr_entity(target) else {
            return false;
        };
        #[allow(clippy::cast_possible_truncation)]
        let d = d2.sqrt() as f32;
        d - TOO_CLOSE_DISTANCE <= 0.0
    }

    /// `canJumpFromCurrentPosition(ServerLevel, Breeze)` (0..74).
    fn can_jump_from_current_position<M: MobHost>(cx: &MobAi<M>) -> bool {
        let pos = cx.mob.block_position();
        if cx.mob.level().block_facts(pos).is_honey_block {
            return false;
        }
        for i in 1..=REQUIRED_AIR_BLOCKS_ABOVE {
            let q = BlockPos {
                x: pos.x,
                y: pos.y + i,
                z: pos.z,
            };
            let f = cx.mob.level().block_facts(q);
            if !f.is_air && !f.fluid_is_water {
                return false;
            }
        }
        true
    }

    /// `LongJump.canRun(ServerLevel, Breeze)` (0..190). Public because it
    /// is `public static` in the jar and `BreezeAi` calls it directly.
    pub fn can_run<M: MobHost>(brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        if !cx.mob.on_ground() && !cx.mob.is_in_water() {
            return false;
        }
        if Swim::should_swim(&cx.mob) {
            return false;
        }
        if brain.check_memory(
            MemoryModuleType::BreezeJumpTarget,
            MemoryStatus::ValuePresent,
        ) {
            return true;
        }
        let Some(target_id) = brain
            .get_memory(MemoryModuleType::AttackTarget)
            .and_then(MemoryValue::as_entity)
        else {
            return false;
        };
        if Self::out_of_aggro_range(cx, target_id) {
            brain.erase_memory(MemoryModuleType::AttackTarget);
            return false;
        }
        if Self::too_close_for_jump(cx, target_id) {
            return false;
        }
        if !Self::can_jump_from_current_position(cx) {
            return false;
        }
        let Some(target) = cx.mob.entity(target_id) else {
            return false;
        };
        let target_pos = target.position();
        let y_head_rot = cx.mob.entity_y_head_rot(target_id);
        let point = breeze_util::random_point_behind_target(cx, y_head_rot, target_pos);
        let Some(pos) = cx.mob.snap_to_surface(point) else {
            return false;
        };
        let below = BlockPos {
            x: pos.x,
            y: pos.y - 1,
            z: pos.z,
        };
        if cx.mob.entity_type_is_block_dangerous_at(below) {
            return false;
        }
        let above4 = BlockPos {
            x: pos.x,
            y: pos.y + 4,
            z: pos.z,
        };
        if !breeze_util::has_line_of_sight(cx, Vec3::at_center_of(pos))
            && !breeze_util::has_line_of_sight(cx, Vec3::at_center_of(above4))
        {
            return false;
        }
        brain.set_memory_value(
            MemoryModuleType::BreezeJumpTarget,
            MemoryValue::BlockPos(pos),
        );
        true
    }

    /// `isFinishedInhaling(Breeze)` (0..31).
    fn is_finished_inhaling<M: MobHost>(brain: &BrainState, cx: &MobAi<M>) -> bool {
        !brain.has_memory_value(MemoryModuleType::BreezeJumpInhaling)
            && cx.mob.has_pose(Pose::Inhaling)
    }

    /// `isFinishedJumping(Breeze)` (0..67).
    fn is_finished_jumping<M: MobHost>(brain: &BrainState, cx: &MobAi<M>) -> bool {
        let jumping = cx.mob.has_pose(Pose::LongJumping);
        let on_ground = cx.mob.on_ground();
        let leaving_water_absent = cx.mob.is_in_water()
            && brain.check_memory(
                MemoryModuleType::BreezeLeavingWater,
                MemoryStatus::ValueAbsent,
            );
        jumping && (on_ground || leaving_water_absent)
    }

    /// `calculateOptimalJumpVector(Breeze, RandomSource, Vec3)` (0..129).
    fn calculate_optimal_jump_vector<M: MobHost>(cx: &mut MobAi<M>, target: Vec3) -> Option<Vec3> {
        let mut angles = ALLOWED_ANGLES;
        util_shuffle(&mut angles, cx.mob.random());
        for angle in angles {
            #[allow(clippy::cast_possible_truncation)]
            let max_velocity = MAX_JUMP_VELOCITY_MULTIPLIER * (cx.mob.follow_range() as f32);
            if let Some(v) = long_jump_util::calculate_jump_vector_for_angle(
                cx,
                target,
                max_velocity,
                angle,
                false,
            ) {
                if cx.mob.has_effect("jump_boost") {
                    let d = v.normalize().y * f64::from(cx.mob.jump_boost_power());
                    return Some(v.add3(0.0, d, 0.0));
                }
                return Some(v);
            }
        }
        None
    }
}

impl<M: MobHost> BehaviorBody<M> for LongJump {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        LongJump::can_run(brain, cx)
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        !cx.mob.has_pose(Pose::Standing)
            && !brain.has_memory_value(MemoryModuleType::BreezeJumpCooldown)
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if brain.check_memory(
            MemoryModuleType::BreezeJumpInhaling,
            MemoryStatus::ValueAbsent,
        ) {
            brain.set_memory_with_expiry(
                MemoryModuleType::BreezeJumpInhaling,
                MemoryValue::Unit,
                INHALING_DURATION_TICKS,
            );
        }
        cx.mob.set_pose(Pose::Inhaling);
        cx.mob
            .level_play_sound(&BREEZE_CHARGE, SOUND_SOURCE_HOSTILE, 1.0, 1.0);
        if let Some(MemoryValue::BlockPos(pos)) =
            brain.get_memory(MemoryModuleType::BreezeJumpTarget)
        {
            let at = Vec3::at_center_of(*pos);
            cx.mob.look_at_eyes(at);
        }
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        let in_water = cx.mob.is_in_water();
        if !in_water
            && brain.check_memory(
                MemoryModuleType::BreezeLeavingWater,
                MemoryStatus::ValuePresent,
            )
        {
            brain.erase_memory(MemoryModuleType::BreezeLeavingWater);
        }
        if LongJump::is_finished_inhaling(brain, cx) {
            let target = match brain.get_memory(MemoryModuleType::BreezeJumpTarget) {
                Some(MemoryValue::BlockPos(p)) => Some(Vec3::at_bottom_center_of(*p)),
                _ => None,
            };
            let v = target.and_then(|t| LongJump::calculate_optimal_jump_vector(cx, t));
            let Some(v) = v else {
                cx.mob.set_pose(Pose::Standing);
                return;
            };
            if in_water {
                brain.set_memory_value(MemoryModuleType::BreezeLeavingWater, MemoryValue::Unit);
            }
            cx.mob.play_sound(&BREEZE_JUMP, 1.0, 1.0);
            cx.mob.set_pose(Pose::LongJumping);
            let y_body_rot = cx.mob.y_body_rot();
            cx.mob.set_y_rot(y_body_rot);
            cx.mob.set_discard_friction(true);
            cx.mob.set_delta_movement(v.x, v.y, v.z);
        } else if LongJump::is_finished_jumping(brain, cx) {
            cx.mob.play_sound(&BREEZE_LAND, 1.0, 1.0);
            cx.mob.set_pose(Pose::Standing);
            cx.mob.set_discard_friction(false);
            let hurt = brain.has_memory_value(MemoryModuleType::HurtBy);
            brain.set_memory_with_expiry(
                MemoryModuleType::BreezeJumpCooldown,
                MemoryValue::Unit,
                if hurt {
                    JUMP_COOLDOWN_WHEN_HURT_TICKS
                } else {
                    JUMP_COOLDOWN_TICKS
                },
            );
            brain.set_memory_with_expiry(
                MemoryModuleType::BreezeShoot,
                MemoryValue::Unit,
                SHOOT_AFTER_LANDING_TICKS,
            );
        }
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if cx.mob.has_pose(Pose::LongJumping) || cx.mob.has_pose(Pose::Inhaling) {
            cx.mob.set_pose(Pose::Standing);
        }
        brain.erase_memory(MemoryModuleType::BreezeJumpTarget);
        brain.erase_memory(MemoryModuleType::BreezeJumpInhaling);
        brain.erase_memory(MemoryModuleType::BreezeLeavingWater);
    }

    fn name(&self) -> &'static str {
        "LongJump"
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
    use rustmc_worldgen::rng::LegacyRandomSource;

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let mut mob = TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 5);
        mob.on_ground = true;
        mob.follow_range = 24.0;
        mob.others
            .push((EntityClass::Living, EntityFacts::living(9, 10.5, 1.0, 0.5)));
        let cx = MobAi::new(mob);
        let mut brain = Brain::<TestMob>::new(
            &[
                T::AttackTarget,
                T::BreezeJumpCooldown,
                T::BreezeJumpInhaling,
                T::BreezeJumpTarget,
                T::BreezeShoot,
                T::WalkTarget,
                T::BreezeLeavingWater,
                T::HurtBy,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(T::AttackTarget, MemoryValue::Entity(9));
        (cx, brain)
    }

    #[test]
    fn the_angle_shuffle_is_seeded_and_takes_four_draws() {
        let mut random = LegacyRandomSource::new(5);
        let mut angles = ALLOWED_ANGLES;
        util_shuffle(&mut angles, &mut random);
        // A second, independent replay of `Util.shuffle` from the same seed —
        // a cross-check on the algorithm, not a jar golden.
        let mut twin = LegacyRandomSource::new(5);
        let mut expect = vec![40, 55, 60, 75, 80];
        let mut i = expect.len();
        while i > 1 {
            let j = twin.next_i32_bound(i as i32) as usize;
            expect.swap(i - 1, j);
            i -= 1;
        }
        assert_eq!(angles.to_vec(), expect);
        // Four draws for five elements, and the multiset is preserved.
        let mut sorted = angles;
        sorted.sort_unstable();
        assert_eq!(sorted, [40, 55, 60, 75, 80]);
    }

    #[test]
    fn an_existing_jump_target_short_circuits_can_run_before_any_draw() {
        let (mut cx, mut brain) = setup();
        brain.set_memory_value(
            T::BreezeJumpTarget,
            MemoryValue::BlockPos(BlockPos { x: 3, y: 1, z: 3 }),
        );
        let before = cx.mob.random;
        assert!(LongJump::can_run(&mut brain, &mut cx));
        assert_eq!(
            format!("{:?}", cx.mob.random),
            format!("{before:?}"),
            "the early return at offset 41 draws nothing"
        );
    }

    #[test]
    fn a_target_out_of_the_follow_range_is_forgotten() {
        let (mut cx, mut brain) = setup();
        cx.mob.others.clear();
        cx.mob
            .others
            .push((EntityClass::Living, EntityFacts::living(9, 100.5, 1.0, 0.5)));
        assert!(!LongJump::can_run(&mut brain, &mut cx));
        assert!(
            !brain.has_memory_value(T::AttackTarget),
            "outOfAggroRange erases it"
        );
    }

    #[test]
    fn a_target_within_four_blocks_is_too_close_and_the_target_is_kept() {
        let (mut cx, mut brain) = setup();
        cx.mob.others.clear();
        cx.mob
            .others
            .push((EntityClass::Living, EntityFacts::living(9, 3.5, 1.0, 0.5)));
        assert!(!LongJump::can_run(&mut brain, &mut cx));
        assert!(brain.has_memory_value(T::AttackTarget), "not forgotten");
    }

    #[test]
    fn four_clear_blocks_are_needed_above_and_the_standing_block_is_not_one() {
        let (mut cx, _brain) = setup();
        assert!(LongJump::can_jump_from_current_position(&cx));
        // The block the breeze stands in is only tested for honey.
        cx.mob.level.set(
            0,
            5,
            0,
            crate::host::BlockFacts {
                is_air: false,
                ..crate::host::BlockFacts::AIR
            },
        );
        assert!(
            !LongJump::can_jump_from_current_position(&cx),
            "the fourth block above is required"
        );
    }

    #[test]
    fn the_landing_cooldown_is_two_ticks_when_hurt_and_ten_otherwise() {
        for (hurt, expect) in [(false, 10), (true, 2)] {
            let (mut cx, mut brain) = setup();
            cx.mob.pose = Pose::LongJumping;
            if hurt {
                brain.set_memory_value(
                    T::HurtBy,
                    MemoryValue::DamageSource(crate::brain::memory::DamageSourceFacts {
                        entity: None,
                        direct_entity: None,
                        kind: "arrow",
                    }),
                );
            }
            let mut b = LongJump::new();
            b.body.tick(&mut brain, &mut cx, 1);
            assert_eq!(brain.time_until_expiry(T::BreezeJumpCooldown), expect);
            assert_eq!(brain.time_until_expiry(T::BreezeShoot), 100);
            assert_eq!(cx.mob.pose, Pose::Standing);
            assert!(!cx.mob.discard_friction);
        }
    }

    #[test]
    fn can_still_use_ends_it_the_moment_the_pose_is_standing_again() {
        let (mut cx, mut brain) = setup();
        let mut b = LongJump::new();
        cx.mob.pose = Pose::Inhaling;
        assert!(b.body.can_still_use(&mut brain, &mut cx, 1));
        cx.mob.pose = Pose::Standing;
        assert!(!b.body.can_still_use(&mut brain, &mut cx, 1));
        cx.mob.pose = Pose::Inhaling;
        brain.set_memory_value(T::BreezeJumpCooldown, MemoryValue::Unit);
        assert!(!b.body.can_still_use(&mut brain, &mut cx, 1));
    }

    #[test]
    fn stop_resets_only_the_two_jump_poses_and_erases_all_three_memories() {
        for (pose, expect) in [
            (Pose::LongJumping, Pose::Standing),
            (Pose::Inhaling, Pose::Standing),
            (Pose::Sliding, Pose::Sliding),
        ] {
            let (mut cx, mut brain) = setup();
            cx.mob.pose = pose;
            brain.set_memory_value(
                T::BreezeJumpTarget,
                MemoryValue::BlockPos(BlockPos { x: 1, y: 1, z: 1 }),
            );
            brain.set_memory_value(T::BreezeJumpInhaling, MemoryValue::Unit);
            brain.set_memory_value(T::BreezeLeavingWater, MemoryValue::Unit);
            let mut b = LongJump::new();
            BehaviorControl::do_stop(&mut b, &mut brain, &mut cx, 1);
            assert_eq!(cx.mob.pose, expect, "from {pose:?}");
            assert!(!brain.has_memory_value(T::BreezeJumpTarget));
            assert!(!brain.has_memory_value(T::BreezeJumpInhaling));
            assert!(!brain.has_memory_value(T::BreezeLeavingWater));
        }
    }
}
