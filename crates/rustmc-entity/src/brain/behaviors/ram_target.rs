//! `net.minecraft.world.entity.ai.behavior.RamTarget`, the goat's charge,
//! from the 26.2 jar.
//!
//! ```text
//! constants: TIME_OUT_DURATION = 200, RAM_SPEED_FORCE_FACTOR = 1.65f
//!
//! RamTarget(Function<Goat,UniformInt> getTimeBetweenRams,
//!           TargetingConditions ramTargeting, float speed,
//!           ToDoubleFunction<Goat> getKnockbackForce,
//!           Function<Goat,SoundEvent> getImpactSound,
//!           Function<Goat,SoundEvent> getHornBreakSound)   (0..62)
//!   entry ImmutableMap.of(RAM_COOLDOWN_TICKS:VALUE_ABSENT, RAM_TARGET:VALUE_PRESENT)
//!   Behavior(map, 200) -> (200, 200);  ramDirection = Vec3.ZERO
//!
//!   checkExtraStartConditions (0..10):  brain.hasMemoryValue(RAM_TARGET)
//!   canStillUse (0..10):                brain.hasMemoryValue(RAM_TARGET)   -- the same test
//!   start (0..89):
//!        0  BlockPos here = goat.blockPosition()
//!       12  Vec3 ram = brain.getMemory(RAM_TARGET).get()
//!       28  ramDirection = new Vec3(here.getX() - ram.x(), 0.0, here.getZ() - ram.z()).normalize()
//!       67  brain.setMemory(WALK_TARGET, new WalkTarget(ram, speed, 0))
//!   tick (0..447):
//!        0  List<LivingEntity> hit = level.getNearbyEntities(
//!               LivingEntity.class, ramTargeting, goat, goat.getBoundingBox())
//!       23  if !hit.isEmpty():
//!       33     LivingEntity target = hit.get(0)
//!       46     DamageSource noAggro = level.damageSources().noAggroMobAttack(goat)
//!       56     float attack = (float) goat.getAttributeValue(ATTACK_DAMAGE)
//!       66     if target.hurtServer(level, noAggro, attack):
//!       79        EnchantmentHelper.doPostAttackEffects(level, target, noAggro)
//!       87     int speedAmp    = hasEffect(SPEED)    ? getEffect(SPEED).getAmplifier()    + 1 : 0
//!      115     int slownessAmp = hasEffect(SLOWNESS) ? getEffect(SLOWNESS).getAmplifier() + 1 : 0
//!      143     float bonus = 0.25f * (speedAmp - slownessAmp)
//!      154     float force = Mth.clamp(goat.getSpeed() * 1.65f, 0.2f, 3.0f) + bonus
//!      173     DamageSource aggro = level.damageSources().mobAttack(goat)
//!      183     float blocked = target.applyItemBlocking(level, aggro, attack)
//!      195     float factor = blocked > 0.0f ? 0.5f : 1.0f
//!      210     target.knockback(factor * force * getKnockbackForce.applyAsDouble(goat),
//!                               ramDirection.x(), ramDirection.z(), aggro, attack)
//!      250     finishRam(level, goat)
//!      256     level.playSound(null, goat, getImpactSound.apply(goat), NEUTRAL, 1.0f, 1.0f)
//!      283  else if hasRammedHornBreakingBlock(level, goat):
//!      292     level.playSound(null, goat, getImpactSound.apply(goat), NEUTRAL, 1.0f, 1.0f)
//!      316     if goat.dropHorn():
//!      327        level.playSound(null, goat, getHornBreakSound.apply(goat), NEUTRAL, 1.0f, 1.0f)
//!      351     finishRam(level, goat)
//!      360  else:
//!      360     Optional<WalkTarget> walk = brain.getMemory(WALK_TARGET)
//!      370     Optional<Vec3>       ram  = brain.getMemory(RAM_TARGET)
//!      380     boolean done = walk.isEmpty() || ram.isEmpty()
//!                 || walk.get().getTarget().currentPosition().closerThan(ram.get(), 0.25d)
//!      436     if done: finishRam(level, goat)
//!
//!   hasRammedHornBreakingBlock(level, goat) (0..65):
//!        0  Vec3 dir = goat.getDeltaMovement().multiply(1.0, 0.0, 1.0).normalize()
//!       14  BlockPos ahead = BlockPos.containing(goat.position().add(dir))
//!       27  return level.getBlockState(ahead).is(SNAPS_GOAT_HORN)
//!               || level.getBlockState(ahead.above()).is(SNAPS_GOAT_HORN)
//!
//!   finishRam(level, goat) (0..50):
//!        0  level.broadcastEntityEvent(goat, (byte) 59)
//!        7  brain.setMemory(RAM_COOLDOWN_TICKS,
//!               getTimeBetweenRams.apply(goat).sample(level.getRandom()))
//!       40  brain.eraseMemory(RAM_TARGET)
//! ```
//!
//! **The cooldown draw is from the LEVEL's random, not the goat's** —
//! `finishRam` at offset 28 is `level.getRandom()`. That is
//! [`UniformInt::sample_level`], the same source `Behavior.tryStart`'s own
//! duration draw uses, and it is the only draw the class makes.
//!
//! **THE 0.25 PROXIMITY TEST IN THE `else` BRANCH CAN NEVER FIRE.**
//! `start` sets `WALK_TARGET = new WalkTarget(ramTarget, speed, 0)`, and
//! `WalkTarget(Vec3, float, int)` is `BlockPosTracker(BlockPos.containing(v))`
//! — the *block* constructor, whose `centerPosition` is
//! `Vec3.atCenterOf(pos)` and so has `y = blockY + 0.5`. Every `RAM_TARGET`
//! is written by `PrepareRamNearestTarget.getEdgeOfBlock`, which is
//! `Vec3.atBottomCenterOf(to).add(x, 0.0, z)` and so has `y = blockY`
//! exactly. The two points therefore differ by 0.5 on the y axis alone, and
//! `closerThan(ram, 0.25)` is false for every ram target the game can
//! produce. Read from `WalkTarget`'s and `BlockPosTracker`'s own bytecode,
//! not inferred: the `Vec3` constructor of `BlockPosTracker` keeps its
//! argument verbatim, but `WalkTarget` does not call that one.
//!
//! So the `else` at offset 360 is governed entirely by its two `isEmpty()`
//! disjuncts, and a goat that simply arrives without hitting anything ends
//! its ram when the walk target goes, when the ram target goes, or at the
//! 200-tick timeout — never by proximity. A test pins the 0.5 gap.
//!
//! **`ramDirection` points BACKWARDS along the charge.** `start` computes
//! `here - ramTarget` (offsets 33..57, the goat's block position minus the
//! target vector), normalised, so the knockback vector handed to
//! `target.knockback(strength, x, z, ...)` is from the ram target toward
//! where the goat began. `LivingEntity.knockback` subtracts, so this is the
//! right sign; the point is that the field is not the direction of travel.
//!
//! **The impact sound plays AFTER `finishRam`** (offsets 250 then 256), so
//! it is heard with `RAM_TARGET` already erased and the cooldown already
//! drawn. The horn-break branch is the other way round: sound, then horn,
//! then `finishRam`.
//!
//! **The speed/slowness bonus is `0.25f * (speedAmp - slownessAmp)` where
//! each amp is `amplifier + 1`, so it cancels for equal levels** and the
//! base force is `clamp(getSpeed() * 1.65f, 0.2f, 3.0f)` before the bonus is
//! added — the clamp does not see the bonus.
//!
//! **`applyItemBlocking` decides only a factor of a half.** Any positive
//! blocked amount halves the knockback; the damage itself was already
//! applied at offset 66 with the *no-aggro* source, while the knockback and
//! the blocking test use the *aggro* source. Two different damage sources
//! in one hit, and the port keeps them apart by name.

use crate::ai::mob_ai::MobAi;
use crate::ai::targeting::{nearby_entities, TargetingConditions};
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::set_entity_look_target_sometimes::UniformInt;
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue, WalkTarget};
use crate::brain::BrainState;
use crate::host::{EntityClass, MobHost, SoundSpec};
use crate::mth;
use crate::phys::{block_pos_containing_vec, Vec3};
use rustmc_data::BlockPos;

/// `RamTarget.TIME_OUT_DURATION`.
pub const TIME_OUT_DURATION: i32 = 200;
/// `RamTarget.RAM_SPEED_FORCE_FACTOR`.
pub const RAM_SPEED_FORCE_FACTOR: f32 = 1.65;
/// The `DamageSources` method the damage itself uses.
pub const NO_AGGRO_MOB_ATTACK: &str = "no_aggro_mob_attack";
/// The `DamageSources` method the blocking test and knockback use.
pub const MOB_ATTACK: &str = "mob_attack";
/// `BlockTags.SNAPS_GOAT_HORN`.
pub const SNAPS_GOAT_HORN: &str = "snaps_goat_horn";
/// The `broadcastEntityEvent` id `finishRam` sends.
pub const RAM_FINISHED_EVENT: u8 = 59;

/// `RamTarget`'s body.
pub struct RamTarget<M: MobHost> {
    /// `getTimeBetweenRams`.
    pub time_between_rams: UniformInt,
    /// `ramTargeting`.
    pub ram_targeting: TargetingConditions<M>,
    /// `speed`.
    pub speed: f32,
    /// `getKnockbackForce`.
    pub knockback_force: f64,
    /// `getImpactSound`.
    pub impact_sound: SoundSpec,
    /// `getHornBreakSound`.
    pub horn_break_sound: SoundSpec,
    /// `ramDirection`, `Vec3.ZERO` at construction.
    pub ram_direction: Vec3,
}

impl<M: MobHost + 'static> RamTarget<M> {
    /// `new RamTarget(...)`. The four `Function`s the jar takes are constant
    /// per mob at every call site, so they are their values here.
    #[must_use]
    pub fn new(
        time_between_rams: UniformInt,
        ram_targeting: TargetingConditions<M>,
        speed: f32,
        knockback_force: f64,
        impact_sound: SoundSpec,
        horn_break_sound: SoundSpec,
    ) -> Behavior<RamTarget<M>> {
        Behavior::with_duration(
            RamTarget {
                time_between_rams,
                ram_targeting,
                speed,
                knockback_force,
                impact_sound,
                horn_break_sound,
                ram_direction: Vec3::new(0.0, 0.0, 0.0),
            },
            vec![
                (
                    MemoryModuleType::RamCooldownTicks,
                    MemoryStatus::ValueAbsent,
                ),
                (MemoryModuleType::RamTarget, MemoryStatus::ValuePresent),
            ],
            TIME_OUT_DURATION,
        )
    }

    /// `hasRammedHornBreakingBlock(ServerLevel, Goat)` (0..65).
    fn has_rammed_horn_breaking_block(cx: &mut MobAi<M>) -> bool {
        let dm = cx.mob.delta_movement();
        let dir = Vec3::new(dm.x, 0.0, dm.z).normalize();
        let ahead = block_pos_containing_vec(cx.mob.position() + dir);
        let above = BlockPos {
            x: ahead.x,
            y: ahead.y + 1,
            z: ahead.z,
        };
        cx.mob.block_tag_is(ahead, SNAPS_GOAT_HORN) || cx.mob.block_tag_is(above, SNAPS_GOAT_HORN)
    }

    /// `finishRam(ServerLevel, Goat)` (0..50). The cooldown is drawn from
    /// the LEVEL's random.
    fn finish_ram(&self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        cx.mob.broadcast_entity_event(RAM_FINISHED_EVENT);
        let n = self.time_between_rams.sample_level(&mut cx.mob);
        brain.set_memory_value(MemoryModuleType::RamCooldownTicks, MemoryValue::Int(n));
        brain.erase_memory(MemoryModuleType::RamTarget);
    }
}

impl<M: MobHost + 'static> BehaviorBody<M> for RamTarget<M> {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>) -> bool {
        brain.has_memory_value(MemoryModuleType::RamTarget)
    }

    fn can_still_use(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) -> bool {
        brain.has_memory_value(MemoryModuleType::RamTarget)
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        let here = cx.mob.block_position();
        let Some(MemoryValue::Vec3(ram)) = brain.get_memory(MemoryModuleType::RamTarget) else {
            return;
        };
        let ram = *ram;
        self.ram_direction =
            Vec3::new(f64::from(here.x) - ram.x, 0.0, f64::from(here.z) - ram.z).normalize();
        brain.set_memory_value(
            MemoryModuleType::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::at_vec(ram, self.speed, 0)),
        );
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        let bb = cx.mob.bounding_box();
        let hit = nearby_entities(cx, EntityClass::Living, &self.ram_targeting, &bb);
        if let Some(&target) = hit.first() {
            #[allow(clippy::cast_possible_truncation)]
            let attack = cx.mob.attack_damage() as f32;
            // `hurtServer` and, on true, `doPostAttackEffects`; both live
            // in the host, so the one call carries them.
            cx.mob.hurt_server(target, NO_AGGRO_MOB_ATTACK, attack);
            let speed_amp = cx.mob.effect_amplifier("speed").map_or(0, |a| a + 1);
            let slowness_amp = cx.mob.effect_amplifier("slowness").map_or(0, |a| a + 1);
            #[allow(clippy::cast_precision_loss)]
            let bonus = 0.25 * (speed_amp - slowness_amp) as f32;
            let force = mth::clamp_f32(cx.mob.speed() * RAM_SPEED_FORCE_FACTOR, 0.2, 3.0) + bonus;
            let blocked = cx.mob.apply_item_blocking(target, MOB_ATTACK, attack);
            let factor = if blocked > 0.0 { 0.5 } else { 1.0f32 };
            let strength = f64::from(factor * force) * self.knockback_force;
            let (x, z) = (self.ram_direction.x, self.ram_direction.z);
            cx.mob.knockback(target, strength, x, z, MOB_ATTACK, attack);
            self.finish_ram(brain, cx);
            let sound = self.impact_sound;
            cx.mob.play_sound(&sound, 1.0, 1.0);
        } else if RamTarget::has_rammed_horn_breaking_block(cx) {
            let sound = self.impact_sound;
            cx.mob.play_sound(&sound, 1.0, 1.0);
            if cx.mob.goat_drop_horn() {
                let horn = self.horn_break_sound;
                cx.mob.play_sound(&horn, 1.0, 1.0);
            }
            self.finish_ram(brain, cx);
        } else {
            let walk = brain
                .get_memory(MemoryModuleType::WalkTarget)
                .and_then(MemoryValue::as_walk_target)
                .map(|w| w.target.clone());
            let ram = match brain.get_memory(MemoryModuleType::RamTarget) {
                Some(MemoryValue::Vec3(v)) => Some(*v),
                _ => None,
            };
            let done = match (walk, ram) {
                (Some(w), Some(r)) => w
                    .current_position(&cx.mob)
                    .is_some_and(|p| p.closer_than(r, 0.25)),
                _ => true,
            };
            if done {
                self.finish_ram(brain, cx);
            }
        }
    }

    fn name(&self) -> &'static str {
        "RamTarget"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::EntityFacts;
    use crate::testing::{GridLevel, TestMob};

    const IMPACT: SoundSpec = SoundSpec("entity.goat.ram_impact");
    const HORN_BREAK: SoundSpec = SoundSpec("entity.goat.horn_break");

    fn behavior() -> Behavior<RamTarget<TestMob>> {
        RamTarget::new(
            UniformInt::of(600, 6000),
            // The goat's own conditions come from `GoatAi`; line of sight
            // is dropped here so the test drives the branch, not the double.
            TargetingConditions::for_combat().ignore_line_of_sight(),
            3.0,
            1.0,
            IMPACT,
            HORN_BREAK,
        )
    }

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let mut mob = TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 17);
        mob.on_ground = true;
        mob.attack_damage = 2.0;
        mob.speed = 1.0;
        let cx = MobAi::new(mob);
        let mut brain =
            Brain::<TestMob>::new(&[T::RamCooldownTicks, T::RamTarget, T::WalkTarget], vec![])
                .state;
        brain.set_memory_value(T::RamTarget, MemoryValue::Vec3(Vec3::new(10.5, 1.0, 0.0)));
        (cx, brain)
    }

    #[test]
    fn start_points_the_ram_direction_back_the_way_the_goat_came() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(b.end_timestamp(), 200);
        // Goat block position (0, 1, 0), ram target (10.5, 1.0, 0.0):
        // (0 - 10.5, 0, 0 - 0.0) normalised is exactly -X.
        assert_eq!(b.body.ram_direction, Vec3::new(-1.0, 0.0, 0.0));
        let MemoryValue::WalkTarget(w) = brain.get_memory(T::WalkTarget).unwrap() else {
            panic!("walk target")
        };
        assert_eq!(w.speed_modifier, 3.0);
        assert_eq!(w.close_enough_dist, 0);
    }

    #[test]
    fn arriving_with_nothing_hit_still_pays_the_cooldown() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        // The proximity disjunct cannot fire (see the dead-branch test
        // below), so the walk target going absent is what ends this.
        brain.erase_memory(T::WalkTarget);
        b.body.tick(&mut brain, &mut cx, 1);
        assert!(!brain.has_memory_value(T::RamTarget), "finishRam ran");
        let Some(MemoryValue::Int(n)) = brain.get_memory(T::RamCooldownTicks) else {
            panic!("cooldown")
        };
        assert!((600..=6000).contains(n), "UniformInt(600, 6000), got {n}");
    }

    #[test]
    fn the_cooldown_draw_is_from_the_level_random_not_the_mobs() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        let mob_random_before = cx.mob.random;
        brain.erase_memory(T::WalkTarget);
        b.body.tick(&mut brain, &mut cx, 1);
        assert_eq!(
            format!("{:?}", cx.mob.random),
            format!("{mob_random_before:?}"),
            "finishRam draws from level.getRandom(), not the goat's"
        );
    }

    #[test]
    fn a_hit_damages_with_no_aggro_and_knocks_back_with_mob_attack() {
        let (mut cx, mut brain) = setup();
        cx.mob
            .others
            .push((EntityClass::Living, EntityFacts::living(9, 0.6, 1.0, 0.5)));
        let mut b = behavior();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        cx.mob.calls.clear();
        b.body.tick(&mut brain, &mut cx, 1);
        let calls = cx.mob.calls.join(" | ");
        assert!(
            calls.contains("hurt_server 9 no_aggro_mob_attack 2"),
            "{calls}"
        );
        assert!(
            calls.contains("apply_item_blocking 9 mob_attack 2"),
            "{calls}"
        );
        // clamp(speed 1.0 * 1.65f, 0.2f, 3.0f) is the f32 1.65, widened to
        // f64 for `knockback` — which is why it is not the literal 1.65.
        assert!(
            calls.contains("knockback 9 1.649999976158142 -1 0 mob_attack 2"),
            "{calls}"
        );
        // The impact sound plays AFTER finishRam.
        let i_finish = cx
            .mob
            .calls
            .iter()
            .position(|c| c == "broadcastEntityEvent(59)")
            .unwrap();
        let i_sound = cx
            .mob
            .calls
            .iter()
            .position(|c| c.starts_with("playSound(entity.goat.ram_impact"))
            .unwrap();
        assert!(i_finish < i_sound, "finishRam then the sound");
    }

    #[test]
    fn blocking_halves_the_knockback_and_the_effects_cancel() {
        let (mut cx, mut brain) = setup();
        cx.mob
            .others
            .push((EntityClass::Living, EntityFacts::living(9, 0.6, 1.0, 0.5)));
        cx.mob.item_blocking.insert(9, 1.0);
        // amplifier 0 becomes 1 on both sides, so the bonus is zero.
        cx.mob.effect_amplifiers.insert("speed", 0);
        cx.mob.effect_amplifiers.insert("slowness", 0);
        let mut b = behavior();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        cx.mob.calls.clear();
        b.body.tick(&mut brain, &mut cx, 1);
        let calls = cx.mob.calls.join(" | ");
        assert!(
            calls.contains("knockback 9 0.824999988079071 -1 0 mob_attack 2"),
            "half of 1.65 in f32 then widened, got {calls}"
        );
    }

    #[test]
    fn the_horn_break_branch_needs_a_tagged_block_ahead_or_above_it() {
        for (tag_at_above, expect_break) in [(false, true), (true, true)] {
            let (mut cx, mut brain) = setup();
            cx.mob.delta_movement = Vec3::new(1.0, 0.0, 0.0);
            let ahead = BlockPos { x: 1, y: 1, z: 0 };
            let p = if tag_at_above {
                BlockPos {
                    x: ahead.x,
                    y: ahead.y + 1,
                    z: ahead.z,
                }
            } else {
                ahead
            };
            cx.mob.block_tags.push((p, SNAPS_GOAT_HORN));
            cx.mob.goat_horn_drops = true;
            let mut b = behavior();
            assert!(b.try_start(&mut brain, &mut cx, 0));
            cx.mob.calls.clear();
            b.body.tick(&mut brain, &mut cx, 1);
            let calls = cx.mob.calls.join(" | ");
            assert_eq!(calls.contains("goat_drop_horn"), expect_break, "{calls}");
            assert!(calls.contains("entity.goat.horn_break"), "{calls}");
            assert!(!brain.has_memory_value(T::RamTarget));
        }
    }

    #[test]
    fn a_second_independent_uniform_int_replay_agrees_on_the_cooldown() {
        // A cross-check on the draw, not a jar golden: the same
        // `UniformInt.sample` over the same level random.
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        let mut twin = TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 17);
        // `try_start` itself drew once from the level random (nextInt(1)).
        let _ = twin.level_random_next_int(1);
        let expect = UniformInt::of(600, 6000).sample_level(&mut twin);
        brain.erase_memory(T::WalkTarget);
        b.body.tick(&mut brain, &mut cx, 1);
        assert_eq!(
            brain.get_memory(T::RamCooldownTicks),
            Some(&MemoryValue::Int(expect))
        );
    }

    #[test]
    fn the_quarter_block_proximity_disjunct_can_never_fire() {
        // `RamTarget.start` sets `WALK_TARGET = new WalkTarget(ramTarget, ...)`,
        // and `WalkTarget(Vec3, float, int)` is
        // `BlockPosTracker(BlockPos.containing(v))`, whose `currentPosition()`
        // is `Vec3.atCenterOf(block)` — y = blockY + 0.5. Every `RAM_TARGET`
        // comes from `PrepareRamNearestTarget.getEdgeOfBlock`, which is
        // `Vec3.atBottomCenterOf(to).add(x, 0.0, z)` — y = blockY exactly.
        // So the two points differ by 0.5 on the y axis alone, and
        // `closerThan(ram, 0.25)` is false for every possible ram target.
        let ram = Vec3::new(6.5, 1.0, 0.5);
        let walk = WalkTarget::at_vec(ram, 3.0, 0);
        let mob = TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1);
        let current = walk.target.current_position(&mob).unwrap();
        assert_eq!(
            current.y - ram.y,
            0.5,
            "atCenterOf against atBottomCenterOf"
        );
        assert!(
            !current.closer_than(ram, 0.25),
            "the 0.25 test is unreachable, so the else branch is governed by \
             the two isEmpty() disjuncts"
        );
    }
}
