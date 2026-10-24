//! `net.minecraft.world.entity.monster.breeze.Shoot`, from the 26.2 jar.
//!
//! ```text
//! constants: ATTACK_RANGE_MAX_SQRT = 256, UNCERTAINTY_BASE = 5,
//!            UNCERTAINTY_MULTIPLIER = 4, PROJECTILE_MOVEMENT_SCALE = 0.7f
//! <clinit>:  SHOOT_INITIAL_DELAY_TICKS = Math.round(15.0f) = 15
//!            SHOOT_RECOVER_DELAY_TICKS = Math.round( 4.0f) =  4
//!            SHOOT_COOLDOWN_TICKS      = Math.round(10.0f) = 10
//!
//! Shoot()   Behavior<Breeze>, Behavior(map, 15 + 1 + 4) -> (20, 20)
//!   entry ImmutableMap.of(ATTACK_TARGET:VALUE_PRESENT,
//!         BREEZE_SHOOT_COOLDOWN:VALUE_ABSENT, BREEZE_SHOOT_CHARGING:VALUE_ABSENT,
//!         BREEZE_SHOOT_RECOVERING:VALUE_ABSENT, BREEZE_SHOOT:VALUE_PRESENT,
//!         WALK_TARGET:VALUE_ABSENT, BREEZE_JUMP_TARGET:VALUE_ABSENT)
//!   checkExtraStartConditions (0..53):
//!        0  if breeze.getPose() != STANDING: return false
//!       12  return brain.getMemory(ATTACK_TARGET)
//!                     .map(lambda$checkExtraStartConditions$0)   // isTargetWithinRange
//!                     .map(lambda$checkExtraStartConditions$1)   // erase BREEZE_SHOOT if false
//!                     .orElse(false)
//!   isTargetWithinRange (0..25):
//!        breeze.position().distanceToSqr(target.position()) < 256.0d
//!   canStillUse (0..31):
//!        brain.hasMemoryValue(ATTACK_TARGET) && brain.hasMemoryValue(BREEZE_SHOOT)
//!   start (0..45):
//!        0  brain.getMemory(ATTACK_TARGET).ifPresent(lambda$start$0)  // setPose(SHOOTING)
//!       19  brain.setMemoryWithExpiry(BREEZE_SHOOT_CHARGING, Unit.INSTANCE, 15L)
//!       36  breeze.playSound(BREEZE_INHALE, 1.0f, 1.0f)
//!   stop (0..44):
//!        0  if breeze.getPose() == SHOOTING: breeze.setPose(STANDING)
//!       17  brain.setMemoryWithExpiry(BREEZE_SHOOT_COOLDOWN, Unit.INSTANCE, 10L)
//!       34  brain.eraseMemory(BREEZE_SHOOT)
//!   tick (0..185):
//!        0  target = brain.getMemory(ATTACK_TARGET).orElse(null); if null return
//!       29  breeze.lookAt(Anchor.EYES, target.position())
//!       41  if BREEZE_SHOOT_CHARGING present || BREEZE_SHOOT_RECOVERING present: return
//!       70  brain.setMemoryWithExpiry(BREEZE_SHOOT_RECOVERING, Unit.INSTANCE, 4L)
//!       85  dx = target.getX() - breeze.getX()
//!       97  dy = target.getY(target.isPassenger() ? 0.8d : 0.3d) - breeze.getFiringYPosition()
//!      126  dz = target.getZ() - breeze.getZ()
//!      138  Projectile.spawnProjectileUsingShoot(new BreezeWindCharge(breeze, level),
//!               level, ItemStack.EMPTY, dx, dy, dz, 0.7f,
//!               (float) (5 - level.getDifficulty().getId() * 4))
//!      175  breeze.playSound(BREEZE_SHOOT, 1.5f, 1.0f)
//! ```
//!
//! **`checkExtraStartConditions`'s second `map` is a side effect wearing a
//! `Function`.** `lambda$checkExtraStartConditions$1` takes the `Boolean`,
//! and when it is false erases `BREEZE_SHOOT` before returning it unchanged.
//! So a start refused *for being out of range* also disarms the memory that
//! asked for the shot, while a start refused for the pose (the early
//! `return false` at offset 10) does not, and neither does an absent attack
//! target, whose `orElse(false)` never runs either lambda. Three different
//! refusals with three different side effects; all three are tested.
//!
//! **The range is a squared comparison against 256** — `ATTACK_RANGE_MAX_SQRT`
//! names the *square*, despite the name, and the test is strict `<`.
//!
//! **`start`'s `ifPresent` ignores its argument.** `lambda$start$0(breeze,
//! livingEntity)` sets the pose and returns; the target it was handed is
//! unused. The pose is therefore set only when an attack target is present,
//! which after `checkExtraStartConditions` it always is — the guard is
//! redundant and faithfully kept.
//!
//! **`tick` is gated by two memories and fires at most once per charge.**
//! `BREEZE_SHOOT_CHARGING` runs 15 ticks from `start`, and the first tick
//! after it expires does the shot and immediately arms
//! `BREEZE_SHOOT_RECOVERING` for 4. Since the behaviour's whole duration is
//! 15 + 1 + 4 = 20, the recovery expiry and the timeout land together.
//!
//! **The vertical aim is the target's body, not its feet or its eyes:**
//! `getY(d)` is `position.y + getBbHeight() * d`, with `d` 0.8 for a
//! passenger and 0.3 otherwise — a rider is aimed at higher up its own
//! hitbox. The breeze's end of the line is `getFiringYPosition()`, not
//! `getY()`.
//!
//! **UNVERIFIABLE FROM THIS CRATE: the shot's own RNG.**
//! `Projectile.spawnProjectileUsingShoot` calls `shoot(...)`, which applies
//! the `inaccuracy` with three `nextGaussian()` draws from the
//! *projectile's* random, not the breeze's. Spawning a `BreezeWindCharge`
//! is outside this crate, so `MobHost::breeze_shoot_wind_charge` takes the
//! five arguments and the host takes those draws. What is transcribed and
//! testable here is the arguments: the three deltas, the 0.7f scale, and
//! the `5 - difficulty.getId() * 4` inaccuracy — which is 5 on peaceful,
//! 1 on easy, -3 on normal and -7 on hard, and yes, it goes negative.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::{Difficulty, MobHost, Pose, SoundSpec};

/// `SoundEvents.BREEZE_INHALE`.
pub const BREEZE_INHALE: SoundSpec = SoundSpec("entity.breeze.inhale");
/// `SoundEvents.BREEZE_SHOOT`.
pub const BREEZE_SHOOT: SoundSpec = SoundSpec("entity.breeze.shoot");
/// `Shoot.ATTACK_RANGE_MAX_SQRT` — the squared range, despite the name.
pub const ATTACK_RANGE_MAX_SQRT: f64 = 256.0;
/// `Shoot.UNCERTAINTY_BASE`.
pub const UNCERTAINTY_BASE: i32 = 5;
/// `Shoot.UNCERTAINTY_MULTIPLIER`.
pub const UNCERTAINTY_MULTIPLIER: i32 = 4;
/// `Shoot.PROJECTILE_MOVEMENT_SCALE`.
pub const PROJECTILE_MOVEMENT_SCALE: f32 = 0.7;
/// `Math.round(15.0f)`.
pub const SHOOT_INITIAL_DELAY_TICKS: i32 = 15;
/// `Math.round(4.0f)`.
pub const SHOOT_RECOVER_DELAY_TICKS: i32 = 4;
/// `Math.round(10.0f)`.
pub const SHOOT_COOLDOWN_TICKS: i32 = 10;

/// `Difficulty.getId()`.
#[must_use]
pub fn difficulty_id(d: Difficulty) -> i32 {
    match d {
        Difficulty::Peaceful => 0,
        Difficulty::Easy => 1,
        Difficulty::Normal => 2,
        Difficulty::Hard => 3,
    }
}

/// `Shoot`'s body.
#[derive(Debug, Clone, Copy, Default)]
pub struct Shoot;

impl Shoot {
    /// `new Shoot()`.
    #[must_use]
    pub fn new() -> Behavior<Shoot> {
        Behavior::with_duration(
            Shoot,
            vec![
                (MemoryModuleType::AttackTarget, MemoryStatus::ValuePresent),
                (
                    MemoryModuleType::BreezeShootCooldown,
                    MemoryStatus::ValueAbsent,
                ),
                (
                    MemoryModuleType::BreezeShootCharging,
                    MemoryStatus::ValueAbsent,
                ),
                (
                    MemoryModuleType::BreezeShootRecovering,
                    MemoryStatus::ValueAbsent,
                ),
                (MemoryModuleType::BreezeShoot, MemoryStatus::ValuePresent),
                (MemoryModuleType::WalkTarget, MemoryStatus::ValueAbsent),
                (
                    MemoryModuleType::BreezeJumpTarget,
                    MemoryStatus::ValueAbsent,
                ),
            ],
            SHOOT_INITIAL_DELAY_TICKS + 1 + SHOOT_RECOVER_DELAY_TICKS,
        )
    }
}

impl<M: MobHost> BehaviorBody<M> for Shoot {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        if !cx.mob.has_pose(Pose::Standing) {
            return false;
        }
        let Some(target) = brain
            .get_memory(MemoryModuleType::AttackTarget)
            .and_then(MemoryValue::as_entity)
            .and_then(|id| cx.mob.entity(id))
        else {
            // `Optional.map` on an empty optional runs neither lambda, so
            // `BREEZE_SHOOT` survives an absent attack target.
            return false;
        };
        let within = cx.mob.position().distance_to_sqr(target.position()) < ATTACK_RANGE_MAX_SQRT;
        if !within {
            brain.erase_memory(MemoryModuleType::BreezeShoot);
        }
        within
    }

    fn can_still_use(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) -> bool {
        brain.has_memory_value(MemoryModuleType::AttackTarget)
            && brain.has_memory_value(MemoryModuleType::BreezeShoot)
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if brain.has_memory_value(MemoryModuleType::AttackTarget) {
            cx.mob.set_pose(Pose::Shooting);
        }
        brain.set_memory_with_expiry(
            MemoryModuleType::BreezeShootCharging,
            MemoryValue::Unit,
            i64::from(SHOOT_INITIAL_DELAY_TICKS),
        );
        cx.mob.play_sound(&BREEZE_INHALE, 1.0, 1.0);
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if cx.mob.has_pose(Pose::Shooting) {
            cx.mob.set_pose(Pose::Standing);
        }
        brain.set_memory_with_expiry(
            MemoryModuleType::BreezeShootCooldown,
            MemoryValue::Unit,
            i64::from(SHOOT_COOLDOWN_TICKS),
        );
        brain.erase_memory(MemoryModuleType::BreezeShoot);
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        let Some(target) = brain
            .get_memory(MemoryModuleType::AttackTarget)
            .and_then(MemoryValue::as_entity)
            .and_then(|id| cx.mob.entity(id))
        else {
            return;
        };
        cx.mob.look_at_eyes(target.position());
        if brain.has_memory_value(MemoryModuleType::BreezeShootCharging)
            || brain.has_memory_value(MemoryModuleType::BreezeShootRecovering)
        {
            return;
        }
        brain.set_memory_with_expiry(
            MemoryModuleType::BreezeShootRecovering,
            MemoryValue::Unit,
            i64::from(SHOOT_RECOVER_DELAY_TICKS),
        );
        let dx = target.x - cx.mob.x();
        let scale = if target.vehicle.is_some() { 0.8 } else { 0.3 };
        let bb_height = target.bb.max_y - target.bb.min_y;
        let dy = (target.y + bb_height * scale) - cx.mob.breeze_firing_y_position();
        let dz = target.z - cx.mob.z();
        let inaccuracy =
            (UNCERTAINTY_BASE - difficulty_id(cx.mob.difficulty()) * UNCERTAINTY_MULTIPLIER) as f32;
        cx.mob
            .breeze_shoot_wind_charge(dx, dy, dz, PROJECTILE_MOVEMENT_SCALE, inaccuracy);
        cx.mob.play_sound(&BREEZE_SHOOT, 1.5, 1.0);
    }

    fn name(&self) -> &'static str {
        "Shoot"
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

    fn setup(target_at: f64) -> (MobAi<TestMob>, BrainState) {
        let mut mob = TestMob::new(GridLevel::new(), 0.0, 1.0, 0.0, 3);
        mob.others.push((
            EntityClass::Living,
            EntityFacts::living(9, target_at, 1.0, 0.0),
        ));
        let cx = MobAi::new(mob);
        let mut brain = Brain::<TestMob>::new(
            &[
                T::AttackTarget,
                T::BreezeShootCooldown,
                T::BreezeShootCharging,
                T::BreezeShootRecovering,
                T::BreezeShoot,
                T::WalkTarget,
                T::BreezeJumpTarget,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(T::AttackTarget, MemoryValue::Entity(9));
        brain.set_memory_value(T::BreezeShoot, MemoryValue::Unit);
        (cx, brain)
    }

    #[test]
    fn the_duration_is_fifteen_plus_one_plus_four() {
        let (mut cx, mut brain) = setup(4.0);
        let mut b = Shoot::new();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(b.end_timestamp(), 20);
    }

    #[test]
    fn only_the_out_of_range_refusal_erases_the_shoot_memory() {
        // out of range: 16 blocks away is 256 exactly, and the test is `<`.
        let (mut cx, mut brain) = setup(16.0);
        let mut b = Shoot::new();
        assert!(!b.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.has_memory_value(T::BreezeShoot), "erased at range");

        // wrong pose: the early return runs neither lambda.
        let (mut cx, mut brain) = setup(4.0);
        cx.mob.pose = Pose::Sliding;
        let mut b = Shoot::new();
        assert!(!b.try_start(&mut brain, &mut cx, 0));
        assert!(brain.has_memory_value(T::BreezeShoot), "kept on the pose");

        // just inside the range starts.
        let (mut cx, mut brain) = setup(15.9);
        let mut b = Shoot::new();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(cx.mob.pose, Pose::Shooting);
    }

    #[test]
    fn charging_gates_the_shot_and_recovering_gates_the_next_one() {
        let (mut cx, mut brain) = setup(4.0);
        cx.mob.difficulty = Difficulty::Normal;
        let mut b = Shoot::new();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(brain.time_until_expiry(T::BreezeShootCharging), 15);
        cx.mob.calls.clear();

        // While charging: it only looks.
        b.tick_or_stop(&mut brain, &mut cx, 1);
        assert!(cx.mob.calls.iter().all(|c| c.starts_with("look_at_eyes")));

        brain.erase_memory(T::BreezeShootCharging);
        cx.mob.calls.clear();
        b.tick_or_stop(&mut brain, &mut cx, 16);
        assert_eq!(
            cx.mob.calls,
            vec![
                "look_at_eyes 4 1 0".to_string(),
                // dy: (target.y 1.0 + bbHeight 1.8 * 0.3) - firing y 1.0.
                "shoot_wind_charge 4 0.54 0 0.7 -3".to_string(),
                "playSound(entity.breeze.shoot, 1.5, 1)".to_string(),
            ]
        );
        assert_eq!(brain.time_until_expiry(T::BreezeShootRecovering), 4);

        // A second tick while recovering shoots nothing more.
        cx.mob.calls.clear();
        b.tick_or_stop(&mut brain, &mut cx, 17);
        assert_eq!(cx.mob.calls, vec!["look_at_eyes 4 1 0".to_string()]);
    }

    #[test]
    fn the_inaccuracy_goes_negative_on_normal_and_hard() {
        for (d, expect) in [
            (Difficulty::Peaceful, 5),
            (Difficulty::Easy, 1),
            (Difficulty::Normal, -3),
            (Difficulty::Hard, -7),
        ] {
            assert_eq!(
                UNCERTAINTY_BASE - difficulty_id(d) * UNCERTAINTY_MULTIPLIER,
                expect
            );
        }
    }

    #[test]
    fn stop_resets_only_the_shooting_pose_and_always_arms_the_cooldown() {
        let (mut cx, mut brain) = setup(4.0);
        let mut b = Shoot::new();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        BehaviorControl::do_stop(&mut b, &mut brain, &mut cx, 21);
        assert_eq!(cx.mob.pose, Pose::Standing);
        assert_eq!(brain.time_until_expiry(T::BreezeShootCooldown), 10);
        assert!(!brain.has_memory_value(T::BreezeShoot));

        // A breeze already knocked out of the shooting pose keeps it.
        let (mut cx, mut brain) = setup(4.0);
        let mut b = Shoot::new();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        cx.mob.pose = Pose::LongJumping;
        BehaviorControl::do_stop(&mut b, &mut brain, &mut cx, 21);
        assert_eq!(cx.mob.pose, Pose::LongJumping);
        assert_eq!(brain.time_until_expiry(T::BreezeShootCooldown), 10);
    }
}
