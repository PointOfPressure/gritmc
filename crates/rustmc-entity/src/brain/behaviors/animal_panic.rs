//! `net.minecraft.world.entity.ai.behavior.AnimalPanic`, from the 26.2 jar.
//! `Behavior<PathfinderMob>` with entry `{IS_PANICKING: REGISTERED, HURT_BY:
//! REGISTERED}` and durations `(100, 120)` (`PANIC_MIN/MAX_DURATION`).
//!
//! ```text
//! AnimalPanic(speed)          damageTypes = mob -> DamageTypeTags.PANIC_CAUSES; position = mob -> LandRandomPos.getPos(mob, 5, 4)
//! AnimalPanic(speed, radius)  position = mob -> AirAndWaterRandomPos.getPos(mob, 5, 4, radius, view.x, view.z, PI/2)   (view = getViewVector(0))
//! AnimalPanic(speed, tags)    position as the first
//! checkExtraStartConditions (0..54):  HURT_BY.map(src -> src.is(damageTypes(mob))).orElse(false) || IS_PANICKING present
//! canStillUse:                        true   (the duration alone ends it)
//! start (0..31):                      IS_PANICKING = true; eraseMemory(WALK_TARGET); navigation.stop()
//! stop (0..14):                       eraseMemory(IS_PANICKING)
//! tick (0..47):                       if navigation.isDone(): v = getPanicPos(mob, level); if v != null: WALK_TARGET = WalkTarget(v, speed, 0)
//! getPanicPos (0..50):                if mob.isOnFire(): lookForWater(level, mob).map(Vec3::atBottomCenterOf) present -> it; else positionGetter(mob)
//! lookForWater (0..65):               if getBlockState(mob.blockPosition()).getCollisionShape(...) not empty: empty
//!                                     pred = Mth.ceil(mob.getBbWidth()) == 2 ? p -> squareOutSouthEast(p).allMatch(water) : p -> fluid(p).is(WATER)
//!                                     BlockPos.findClosestMatch(mob.blockPosition(), 5, 1, pred)
//! ```
//!
//! `PANIC_DISTANCE_HORIZONTAL = 5`, `PANIC_DISTANCE_VERTICAL = 4`. The
//! position getters walk the mob's random through `random_pos`; the base
//! class draws the duration from the level random on start.

use rustmc_data::BlockPos;

use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::{air_and_water_get_pos, land_get_pos};
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue, WalkTarget};
use crate::brain::BrainState;
use crate::host::{DamageTypeTag, MobHost, PathLevel};
use crate::phys::find_closest_match;
use crate::phys::Vec3;

/// `PANIC_MIN_DURATION`.
pub const PANIC_MIN_DURATION: i32 = 100;
/// `PANIC_MAX_DURATION`.
pub const PANIC_MAX_DURATION: i32 = 120;
/// `PANIC_DISTANCE_HORIZONTAL`.
pub const PANIC_DISTANCE_HORIZONTAL: i32 = 5;
/// `PANIC_DISTANCE_VERTICAL`.
pub const PANIC_DISTANCE_VERTICAL: i32 = 4;
const HALF_PI: f64 = 1.570_796_370_506_286_6;

/// Which `positionGetter` the constructor installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanicPosition {
    /// `LandRandomPos.getPos(mob, 5, 4)`.
    Land,
    /// `AirAndWaterRandomPos.getPos(mob, 5, 4, radius, view.x, view.z, PI/2)`.
    AirAndWater { radius: i32 },
}

/// `AnimalPanic`'s body; build one with the constructors below.
#[derive(Debug, Clone, Copy)]
pub struct AnimalPanic {
    pub speed_multiplier: f32,
    pub panic_causing_damage_types: DamageTypeTag,
    pub position: PanicPosition,
}

impl AnimalPanic {
    fn behavior(body: AnimalPanic) -> Behavior<AnimalPanic> {
        Behavior::with_durations(
            body,
            vec![
                (MemoryModuleType::IsPanicking, MemoryStatus::Registered),
                (MemoryModuleType::HurtBy, MemoryStatus::Registered),
            ],
            PANIC_MIN_DURATION,
            PANIC_MAX_DURATION,
        )
    }

    /// `AnimalPanic(float)`.
    #[must_use]
    pub fn new(speed_multiplier: f32) -> Behavior<AnimalPanic> {
        Self::behavior(AnimalPanic {
            speed_multiplier,
            panic_causing_damage_types: DamageTypeTag::PanicCauses,
            position: PanicPosition::Land,
        })
    }

    /// `AnimalPanic(float, int)`.
    #[must_use]
    pub fn with_radius(speed_multiplier: f32, radius: i32) -> Behavior<AnimalPanic> {
        Self::behavior(AnimalPanic {
            speed_multiplier,
            panic_causing_damage_types: DamageTypeTag::PanicCauses,
            position: PanicPosition::AirAndWater { radius },
        })
    }

    /// `AnimalPanic(float, Function<PathfinderMob, TagKey<DamageType>>)`
    /// with a constant tag.
    #[must_use]
    pub fn with_tag(speed_multiplier: f32, tag: DamageTypeTag) -> Behavior<AnimalPanic> {
        Self::behavior(AnimalPanic {
            speed_multiplier,
            panic_causing_damage_types: tag,
            position: PanicPosition::Land,
        })
    }

    /// `lookForWater(level, mob)`.
    fn look_for_water<M: MobHost>(mob: &M) -> Option<BlockPos> {
        let pos = mob.block_position();
        let level = mob.level();
        if level.collision_shape_max_y(pos).is_some() {
            return None;
        }
        #[allow(clippy::cast_possible_truncation)]
        let wide = mob.bb_width().ceil() as i32 == 2;
        if wide {
            find_closest_match(pos, 5, 1, |p| {
                [(0, 0), (1, 0), (0, 1), (1, 1)].iter().all(|(dx, dz)| {
                    level
                        .block_facts(BlockPos {
                            x: p.x + dx,
                            y: p.y,
                            z: p.z + dz,
                        })
                        .fluid_is_water
                })
            })
        } else {
            find_closest_match(pos, 5, 1, |p| level.block_facts(p).fluid_is_water)
        }
    }

    /// `getPanicPos(mob, level)`.
    fn panic_pos<M: MobHost>(&self, cx: &mut MobAi<M>) -> Option<Vec3> {
        if cx.mob.is_on_fire() {
            if let Some(p) = Self::look_for_water(&cx.mob) {
                return Some(Vec3::at_bottom_center_of(p));
            }
        }
        match self.position {
            PanicPosition::Land => {
                land_get_pos(cx, PANIC_DISTANCE_HORIZONTAL, PANIC_DISTANCE_VERTICAL)
            }
            PanicPosition::AirAndWater { radius } => {
                let view = cx.mob.view_vector_at_zero();
                air_and_water_get_pos(
                    cx,
                    PANIC_DISTANCE_HORIZONTAL,
                    PANIC_DISTANCE_VERTICAL,
                    radius,
                    view.x,
                    view.z,
                    HALF_PI,
                )
            }
        }
    }
}

impl<M: MobHost> BehaviorBody<M> for AnimalPanic {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        let hurt = brain
            .get_memory(MemoryModuleType::HurtBy)
            .and_then(|v| match v {
                MemoryValue::DamageSource(d) => Some(d.kind),
                _ => None,
            })
            .is_some_and(|kind| cx.mob.damage_type_is(kind, self.panic_causing_damage_types));
        hurt || brain.has_memory_value(MemoryModuleType::IsPanicking)
    }

    fn can_still_use(&mut self, _brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) -> bool {
        true
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        brain.set_memory_value(MemoryModuleType::IsPanicking, MemoryValue::Bool(true));
        brain.erase_memory(MemoryModuleType::WalkTarget);
        cx.navigation.stop();
    }

    fn stop(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) {
        brain.erase_memory(MemoryModuleType::IsPanicking);
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if cx.navigation.is_done() {
            if let Some(v) = self.panic_pos(cx) {
                brain.set_memory_value(
                    MemoryModuleType::WalkTarget,
                    MemoryValue::WalkTarget(WalkTarget::at_vec(v, self.speed_multiplier, 0)),
                );
            }
        }
    }

    fn name(&self) -> &'static str {
        "AnimalPanic"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::{BehaviorControl, Status};
    use crate::brain::memory::{DamageSourceFacts, MemoryModuleType as T};
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let mut level = GridLevel::new();
        level.floor(-16, -16, 16, 16, 0);
        let mut cx = MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, 23));
        cx.mob.panic_damage_kinds = vec!["mob_attack"];
        let brain =
            Brain::<TestMob>::new(&[T::IsPanicking, T::HurtBy, T::WalkTarget], vec![]).state;
        (cx, brain)
    }

    #[test]
    fn panics_on_a_panic_causing_hit_and_picks_a_land_position_when_idle() {
        let (mut cx, mut brain) = setup();
        let mut b = AnimalPanic::new(1.5);
        assert!(!b.try_start(&mut brain, &mut cx, 0), "not hurt");
        brain.set_memory_value(
            T::HurtBy,
            MemoryValue::DamageSource(DamageSourceFacts {
                entity: None,
                direct_entity: None,
                kind: "fall",
            }),
        );
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "fall is not in PANIC_CAUSES here"
        );
        brain.set_memory_value(
            T::HurtBy,
            MemoryValue::DamageSource(DamageSourceFacts {
                entity: Some(9),
                direct_entity: Some(9),
                kind: "mob_attack",
            }),
        );
        let mut twin = MobAi::new(TestMob::new(cx.mob.level.clone(), 0.5, 1.0, 0.5, 23));
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain.get_memory(T::IsPanicking),
            Some(&MemoryValue::Bool(true))
        );
        assert!(b.end_timestamp() >= 100 && b.end_timestamp() <= 120);
        let expected = land_get_pos(&mut twin, 5, 4);
        b.tick_or_stop(&mut brain, &mut cx, 1);
        match expected {
            Some(p) => assert_eq!(
                brain.get_memory(T::WalkTarget).unwrap().as_walk_target(),
                Some(&WalkTarget::at_vec(p, 1.5, 0))
            ),
            None => assert!(!brain.has_memory_value(T::WalkTarget)),
        }
        // Once panicking, the memory alone keeps the behaviour startable.
        brain.erase_memory(T::HurtBy);
        b.do_stop(&mut brain, &mut cx, 2);
        assert!(!brain.has_memory_value(T::IsPanicking));
        brain.set_memory_value(T::IsPanicking, MemoryValue::Bool(true));
        assert!(b.try_start(&mut brain, &mut cx, 3));
        assert_eq!(BehaviorControl::<TestMob>::status(&b), Status::Running);
    }
}
