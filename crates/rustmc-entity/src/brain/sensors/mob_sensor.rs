//! `net.minecraft.world.entity.ai.sensing.MobSensor<T>`, from the 26.2
//! jar: a parameterised sensor that sets a boolean memory with an expiry
//! when any nearby entity passes a test. One `SensorType` creates one —
//! `ARMADILLO_SCARE_DETECTED` is
//! `new MobSensor<>(5, Armadillo::isScaredBy, Armadillo::canStayRolledUp,
//! DANGER_DETECTED_RECENTLY, 80)` (`SensorType.lambda$static$0` 0..23,
//! with the two method handles read out of the `BootstrapMethods`
//! table). `requires()` = `{NEAREST_LIVING_ENTITIES}`.
//!
//! ```text
//! MobSensor(scanRate, mobTest, readyTest, toSet, memoryTimeToLive)  -> super(scanRate)
//! doTick (0..26):             if !readyTest.test(mob): clearMemory(mob) else: checkForMobsNearby(mob)
//! checkForMobsNearby (0..53): o = brain.getMemory(NEAREST_LIVING_ENTITIES)
//!                             if o.isEmpty(): return                      <- NOT a clear
//!                             if o.get().stream().anyMatch(e -> mobTest.test(mob, e)): mobDetected(mob)
//! mobDetected (0..20):        brain.setMemoryWithExpiry(toSet, true, memoryTimeToLive)
//! clearMemory (0..11):        brain.eraseMemory(toSet)
//! ```
//!
//! **The scan rate is 5, not the default 20** (`iconst_5` at 4 of the
//! supplier), so this is the only sensor on the armadillo that runs four
//! times as often as the rest.
//!
//! Three separate outcomes, and the difference between the second and the
//! third is the whole shape of the class: `readyTest` false **erases**;
//! `readyTest` true with an absent `NEAREST_LIVING_ENTITIES` **returns
//! without touching the memory** (`checkForMobsNearby` 11..18); and
//! `readyTest` true with nobody matching also leaves it alone, because
//! `mobDetected` is only called on a match. So the memory decays by its
//! own 80-tick expiry rather than by any negative path except the
//! not-ready one.

use crate::ai::mob_ai::MobAi;
use crate::brain::memory::{MemoryModuleType, MemoryValue};
use crate::brain::sensor::{Sensor, SensorBody, SensorType};
use crate::brain::BrainState;
use crate::host::MobHost;

/// Which pair of predicates a [`MobSensor`] carries. Both are entity
/// methods outside this crate, so each is a host call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobSensorKind {
    /// `Armadillo::isScaredBy` and `Armadillo::canStayRolledUp`.
    ArmadilloScare,
}

/// `MobSensor<T>`.
#[derive(Debug, Clone, Copy)]
pub struct MobSensor {
    /// The `mobTest` / `readyTest` pair.
    pub kind: MobSensorKind,
    /// `toSet`.
    pub to_set: MemoryModuleType,
    /// `memoryTimeToLive`.
    pub memory_time_to_live: i64,
    ty: SensorType,
}

impl MobSensor {
    /// `SensorType.ARMADILLO_SCARE_DETECTED.create()`.
    #[must_use]
    pub fn armadillo_scare() -> Sensor<MobSensor> {
        Sensor::with_scan_rate(
            MobSensor {
                kind: MobSensorKind::ArmadilloScare,
                to_set: MemoryModuleType::DangerDetectedRecently,
                memory_time_to_live: 80,
                ty: SensorType::ArmadilloScareDetected,
            },
            5,
        )
    }

    /// `readyTest.test(mob)`.
    fn ready<M: MobHost>(&self, cx: &MobAi<M>) -> bool {
        match self.kind {
            MobSensorKind::ArmadilloScare => cx.mob.can_stay_rolled_up(),
        }
    }

    /// `mobTest.test(mob, e)`.
    fn mob_test<M: MobHost>(&self, cx: &MobAi<M>, id: i32) -> bool {
        match self.kind {
            MobSensorKind::ArmadilloScare => cx.mob.is_scared_by(id),
        }
    }
}

impl<M: MobHost> SensorBody<M> for MobSensor {
    fn do_tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        if !self.ready(cx) {
            brain.erase_memory(self.to_set);
            return;
        }
        let Some(MemoryValue::Entities(list)) =
            brain.get_memory(MemoryModuleType::NearestLivingEntities)
        else {
            // `Optional.isEmpty()` -> plain `return`: not a clear.
            return;
        };
        let list = list.clone();
        if list.iter().any(|id| self.mob_test(cx, *id)) {
            brain.set_memory_with_expiry(
                self.to_set,
                MemoryValue::Bool(true),
                self.memory_time_to_live,
            );
        }
    }

    fn requires(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::NearestLivingEntities]
    }

    fn sensor_type(&self) -> SensorType {
        self.ty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::sensor::SensorControl;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn not_ready_erases_but_an_absent_list_and_no_match_both_leave_the_memory_alone() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.can_stay_rolled_up = true;
        cx.mob.scary = vec![7];
        let mut brain = Brain::<TestMob>::new(
            &[T::NearestLivingEntities, T::DangerDetectedRecently],
            vec![],
        )
        .state;
        let mut s = MobSensor {
            kind: MobSensorKind::ArmadilloScare,
            to_set: T::DangerDetectedRecently,
            memory_time_to_live: 80,
            ty: SensorType::ArmadilloScareDetected,
        };
        // Ready, but NEAREST_LIVING_ENTITIES absent: plain return.
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(!brain.has_memory_value(T::DangerDetectedRecently));
        // Ready, list present, nothing scary: still no write.
        brain.set_memory_value(T::NearestLivingEntities, MemoryValue::Entities(vec![2, 3]));
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(!brain.has_memory_value(T::DangerDetectedRecently));
        // One scary entity anywhere in the list sets it with the TTL.
        brain.set_memory_value(
            T::NearestLivingEntities,
            MemoryValue::Entities(vec![2, 3, 7]),
        );
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::DangerDetectedRecently),
            Some(&MemoryValue::Bool(true))
        );
        // Not ready: erase, even with the scary entity still there.
        cx.mob.can_stay_rolled_up = false;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(!brain.has_memory_value(T::DangerDetectedRecently));
    }

    #[test]
    fn the_armadillo_sensor_scans_every_five_ticks_not_twenty() {
        let mut s = MobSensor::armadillo_scare();
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(
            &[T::NearestLivingEntities, T::DangerDetectedRecently],
            vec![],
        )
        .state;
        cx.mob.can_stay_rolled_up = true;
        cx.mob.scary = vec![7];
        brain.set_memory_value(T::NearestLivingEntities, MemoryValue::Entities(vec![7]));
        // A fresh Sensor ticks on its first tick, then every scan_rate.
        SensorControl::<TestMob>::tick(&mut s, &mut brain, &mut cx);
        assert!(brain.has_memory_value(T::DangerDetectedRecently));
        brain.erase_memory(T::DangerDetectedRecently);
        for _ in 0..4 {
            SensorControl::<TestMob>::tick(&mut s, &mut brain, &mut cx);
            assert!(
                !brain.has_memory_value(T::DangerDetectedRecently),
                "scan rate 5: four ticks are skipped"
            );
        }
        SensorControl::<TestMob>::tick(&mut s, &mut brain, &mut cx);
        assert!(
            brain.has_memory_value(T::DangerDetectedRecently),
            "and the fifth fires"
        );
    }
}
