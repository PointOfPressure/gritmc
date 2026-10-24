//! `net.minecraft.world.entity.ai.sensing.{Sensor, SensorType}` from the
//! 26.2 jar.
//!
//! # `Sensor<E>`, offset by offset
//!
//! ```text
//! Sensor(int scanRate)         scanRate = n;  Sensor() is Sensor(20)   (DEFAULT_SCAN_RATE)
//! randomlyDelayStart(random)   timeToTick = random.nextInt(scanRate)   <- the mob's random, once, in Brain.<init>
//! tick(level, mob)  (0..36)    if (--timeToTick <= 0) { timeToTick = scanRate;
//!                                  updateTargetingConditionRanges(mob); doTick(level, mob) }
//! ```
//!
//! So a sensor with `scanRate` 20 and a delay draw of `d` first ticks on
//! the `d`th brain tick (the decrement precedes the test; a draw of 0 fires
//! on the first tick) and every 20 after.
//!
//! # The six static `TargetingConditions`, and the range they share
//!
//! `Sensor.<clinit>` builds six statics — `TARGET_CONDITIONS`
//! (`forNonCombat().range(16)`), the same `.ignoreInvisibilityTesting()`,
//! `ATTACK_TARGET_CONDITIONS` (`forCombat().range(16)`), the same with
//! `.ignoreInvisibilityTesting()`, `.ignoreLineOfSight()`, and both — and
//! `updateTargetingConditionRanges(mob)` **mutates all six** to the ticking
//! mob's `FOLLOW_RANGE` attribute before every `doTick`. They are read by
//! `isEntityTargetable` / `isEntityAttackable` /
//! `isEntityAttackableIgnoringLineOfSight`, which behaviours call outside
//! any sensor tick (`NearestVisibleLivingEntities.contains`,
//! `StopAttackingIfTargetInvalid`, …). **At that moment the range is
//! whichever mob's sensor ticked last on the server, not this mob's.** This
//! port keeps the same process-wide cell, [`SENSOR_RANGE`], so the quirk is
//! reproduced rather than tidied; it starts at `16.0`.
//!
//! `isEntityTargetable(level, attacker, target)` (0..33):
//! `attacker.getBrain().isMemoryValue(ATTACK_TARGET, target)` picks the
//! `ignoreInvisibilityTesting` variant, else the plain one; then
//! `.test(level, attacker, target)`. `isEntityAttackable` is the same on
//! the combat pair; `isEntityAttackableIgnoringLineOfSight` on the
//! line-of-sight pair.

use std::sync::atomic::{AtomicU64, Ordering};

use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};

use crate::ai::mob_ai::MobAi;
use crate::ai::targeting::TargetingConditions;
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::{EntityFacts, MobHost};

/// `Sensor.DEFAULT_SCAN_RATE`.
pub const DEFAULT_SCAN_RATE: i32 = 20;
/// `Sensor.DEFAULT_TARGETING_RANGE`.
pub const DEFAULT_TARGETING_RANGE: f64 = 16.0;

/// The range field shared by the six static conditions, as `f64` bits.
static SENSOR_RANGE: AtomicU64 = AtomicU64::new(0x4030_0000_0000_0000); // 16.0

/// The current shared range.
#[must_use]
pub fn sensor_range() -> f64 {
    f64::from_bits(SENSOR_RANGE.load(Ordering::Relaxed))
}

/// `updateTargetingConditionRanges(mob)`: all six statics take
/// `mob.getAttributeValue(FOLLOW_RANGE)`.
pub fn update_targeting_condition_ranges(range: f64) {
    SENSOR_RANGE.store(range.to_bits(), Ordering::Relaxed);
}

/// `Sensor.TARGET_CONDITIONS` at the current shared range.
#[must_use]
pub fn target_conditions<M: MobHost>() -> TargetingConditions<M> {
    TargetingConditions::for_non_combat().range(sensor_range())
}

/// `Sensor.TARGET_CONDITIONS_IGNORE_INVISIBILITY_TESTING`.
#[must_use]
pub fn target_conditions_ignore_invisibility_testing<M: MobHost>() -> TargetingConditions<M> {
    TargetingConditions::for_non_combat()
        .range(sensor_range())
        .ignore_invisibility_testing()
}

/// `Sensor.ATTACK_TARGET_CONDITIONS`.
#[must_use]
pub fn attack_target_conditions<M: MobHost>() -> TargetingConditions<M> {
    TargetingConditions::for_combat().range(sensor_range())
}

/// `Sensor.ATTACK_TARGET_CONDITIONS_IGNORE_INVISIBILITY_TESTING`.
#[must_use]
pub fn attack_target_conditions_ignore_invisibility_testing<M: MobHost>() -> TargetingConditions<M>
{
    TargetingConditions::for_combat()
        .range(sensor_range())
        .ignore_invisibility_testing()
}

/// `Sensor.ATTACK_TARGET_CONDITIONS_IGNORE_LINE_OF_SIGHT`.
#[must_use]
pub fn attack_target_conditions_ignore_line_of_sight<M: MobHost>() -> TargetingConditions<M> {
    TargetingConditions::for_combat()
        .range(sensor_range())
        .ignore_line_of_sight()
}

/// `Sensor.ATTACK_TARGET_CONDITIONS_IGNORE_INVISIBILITY_AND_LINE_OF_SIGHT`.
#[must_use]
pub fn attack_target_conditions_ignore_invisibility_and_line_of_sight<M: MobHost>(
) -> TargetingConditions<M> {
    TargetingConditions::for_combat()
        .range(sensor_range())
        .ignore_line_of_sight()
        .ignore_invisibility_testing()
}

/// `Sensor.isEntityTargetable(level, attacker, target)`.
#[must_use]
pub fn is_entity_targetable<M: MobHost>(
    brain: &BrainState,
    cx: &mut MobAi<M>,
    target: &EntityFacts,
) -> bool {
    if brain.is_memory_value(
        MemoryModuleType::AttackTarget,
        &MemoryValue::Entity(target.id),
    ) {
        target_conditions_ignore_invisibility_testing().test(cx, target)
    } else {
        target_conditions().test(cx, target)
    }
}

/// `Sensor.isEntityAttackable(level, attacker, target)`.
#[must_use]
pub fn is_entity_attackable<M: MobHost>(
    brain: &BrainState,
    cx: &mut MobAi<M>,
    target: &EntityFacts,
) -> bool {
    is_entity_attackable_with(attack_target_id(brain), cx, target)
}

/// The `ATTACK_TARGET` id as `Brain.isMemoryValue` sees it: an
/// **unregistered** memory answers `None` rather than throwing, because
/// `isMemoryValue` goes through `checkMemory`, which returns `false` for
/// one, and never reaches the `getMemory` that would throw.
#[must_use]
pub fn attack_target_id(brain: &BrainState) -> Option<i32> {
    if !brain.check_memory(MemoryModuleType::AttackTarget, MemoryStatus::ValuePresent) {
        return None;
    }
    brain
        .get_memory(MemoryModuleType::AttackTarget)
        .and_then(MemoryValue::as_entity)
}

/// `Sensor.isEntityAttackable` with the brain's `ATTACK_TARGET` id read
/// out in advance — the only thing the brain is consulted for. The
/// `NearestVisibleLivingEntitySensor` subclasses need this form, because
/// `findClosest` hands its predicate the mob and not the brain.
#[must_use]
pub fn is_entity_attackable_with<M: MobHost>(
    attack_target: Option<i32>,
    cx: &mut MobAi<M>,
    target: &EntityFacts,
) -> bool {
    if attack_target == Some(target.id) {
        attack_target_conditions_ignore_invisibility_testing().test(cx, target)
    } else {
        attack_target_conditions().test(cx, target)
    }
}

/// `Sensor.isEntityAttackableIgnoringLineOfSight(level, attacker, target)`.
#[must_use]
pub fn is_entity_attackable_ignoring_line_of_sight<M: MobHost>(
    brain: &BrainState,
    cx: &mut MobAi<M>,
    target: &EntityFacts,
) -> bool {
    if brain.is_memory_value(
        MemoryModuleType::AttackTarget,
        &MemoryValue::Entity(target.id),
    ) {
        attack_target_conditions_ignore_invisibility_and_line_of_sight().test(cx, target)
    } else {
        attack_target_conditions_ignore_line_of_sight().test(cx, target)
    }
}

/// `Sensor.rememberPositives(int, BiPredicate)`: a predicate that stays
/// true for `n` further asks after a positive. The counter is an
/// `AtomicInteger` starting at 0: a positive sets it to `n`, a negative
/// decrements it and answers `decrementAndGet() >= 0`.
#[derive(Debug, Clone)]
pub struct RememberPositives {
    n: i32,
    counter: i32,
}

impl RememberPositives {
    #[must_use]
    pub fn new(n: i32) -> Self {
        RememberPositives { n, counter: 0 }
    }

    /// `test(t, u)` given the wrapped predicate's answer.
    pub fn test(&mut self, inner: bool) -> bool {
        if inner {
            self.counter = self.n;
            true
        } else {
            self.counter -= 1;
            self.counter >= 0
        }
    }
}

macro_rules! sensor_types {
    ($( $name:ident = $s:literal ),* $(,)?) => {
        /// `SensorType`, every registered instance in `<clinit>` order.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum SensorType {
            $( $name, )*
        }

        impl SensorType {
            /// Every type, in registration order.
            pub const ALL: &'static [SensorType] = &[ $( SensorType::$name, )* ];

            /// The registry name (`minecraft:` implied).
            #[must_use]
            pub fn name(self) -> &'static str {
                match self {
                    $( SensorType::$name => $s, )*
                }
            }
        }
    };
}

sensor_types! {
    Dummy = "dummy",
    NearestItems = "nearest_items",
    NearestLivingEntities = "nearest_living_entities",
    NearestPlayers = "nearest_players",
    NearestBed = "nearest_bed",
    HurtBy = "hurt_by",
    VillagerHostiles = "villager_hostiles",
    VillagerBabies = "villager_babies",
    SecondaryPois = "secondary_pois",
    GolemDetected = "golem_detected",
    ArmadilloScareDetected = "armadillo_scare_detected",
    PiglinSpecificSensor = "piglin_specific_sensor",
    PiglinBruteSpecificSensor = "piglin_brute_specific_sensor",
    HoglinSpecificSensor = "hoglin_specific_sensor",
    NearestAdult = "nearest_adult",
    NearestAdultAnyType = "nearest_adult_any_type",
    AxolotlAttackables = "axolotl_attackables",
    FoodTemptations = "food_temptations",
    FrogTemptations = "frog_temptations",
    NautilusTemptations = "nautilus_temptations",
    FrogAttackables = "frog_attackables",
    IsInWater = "is_in_water",
    WardenEntitySensor = "warden_entity_sensor",
    BreezeAttackEntitySensor = "breeze_attack_entity_sensor",
}

/// What `Brain` drives: the `Sensor` base class's public surface.
pub trait SensorControl<M: MobHost> {
    /// The registry type this sensor was created from.
    fn sensor_type(&self) -> SensorType;
    /// `randomlyDelayStart(random)`.
    fn randomly_delay_start(&mut self, random: &mut LegacyRandomSource);
    /// `tick(level, mob)`.
    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>);
    /// `requires()`.
    fn requires(&self) -> Vec<MemoryModuleType>;
}

/// The subclass half of `Sensor<E>`.
pub trait SensorBody<M: MobHost> {
    /// `doTick(level, mob)`.
    fn do_tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>);
    /// `requires()`.
    fn requires(&self) -> Vec<MemoryModuleType>;
    /// The registry type.
    fn sensor_type(&self) -> SensorType;
}

/// `Sensor<E>`: the base class around a [`SensorBody`].
pub struct Sensor<S> {
    /// The subclass.
    pub body: S,
    scan_rate: i32,
    time_to_tick: i64,
}

impl<S> Sensor<S> {
    /// `Sensor(int)`.
    #[must_use]
    pub fn with_scan_rate(body: S, scan_rate: i32) -> Self {
        Sensor {
            body,
            scan_rate,
            time_to_tick: 0,
        }
    }

    /// `Sensor()` — `Sensor(20)`.
    #[must_use]
    pub fn new(body: S) -> Self {
        Self::with_scan_rate(body, DEFAULT_SCAN_RATE)
    }

    /// `timeToTick`, for tests.
    #[must_use]
    pub fn time_to_tick(&self) -> i64 {
        self.time_to_tick
    }
}

impl<M: MobHost, S: SensorBody<M>> SensorControl<M> for Sensor<S> {
    fn sensor_type(&self) -> SensorType {
        self.body.sensor_type()
    }

    fn randomly_delay_start(&mut self, random: &mut LegacyRandomSource) {
        self.time_to_tick = i64::from(random.next_i32_bound(self.scan_rate));
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        self.time_to_tick -= 1;
        if self.time_to_tick <= 0 {
            self.time_to_tick = i64::from(self.scan_rate);
            update_targeting_condition_ranges(cx.mob.follow_range());
            self.body.do_tick(brain, cx);
        }
    }

    fn requires(&self) -> Vec<MemoryModuleType> {
        self.body.requires()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{GridLevel, TestMob};

    struct Counting(u32);

    impl SensorBody<TestMob> for Counting {
        fn do_tick(&mut self, _b: &mut BrainState, _cx: &mut MobAi<TestMob>) {
            self.0 += 1;
        }
        fn requires(&self) -> Vec<MemoryModuleType> {
            vec![MemoryModuleType::Dummy]
        }
        fn sensor_type(&self) -> SensorType {
            SensorType::Dummy
        }
    }

    #[test]
    fn a_sensor_first_ticks_delay_plus_one_ticks_in_and_then_every_scan_rate() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 3));
        let mut brain = BrainState::new();
        let mut s = Sensor::with_scan_rate(Counting(0), 5);
        let mut twin = LegacyRandomSource::new(3);
        let d = twin.next_i32_bound(5);
        s.randomly_delay_start(cx.mob.random());
        assert_eq!(s.time_to_tick(), i64::from(d));
        let mut first = None;
        for i in 1..=20 {
            s.tick(&mut brain, &mut cx);
            if s.body.0 == 1 && first.is_none() {
                first = Some(i);
            }
        }
        let expected_first = if d == 0 { 1 } else { d };
        assert_eq!(
            first,
            Some(expected_first),
            "--timeToTick <= 0 fires on tick d, or 1 for d = 0"
        );
        assert_eq!(s.body.0, 1 + (20 - expected_first) as u32 / 5);
    }

    #[test]
    fn twenty_four_sensor_types_in_registration_order() {
        assert_eq!(SensorType::ALL.len(), 24);
        assert_eq!(SensorType::ALL[23].name(), "breeze_attack_entity_sensor");
        assert_eq!(SensorType::GolemDetected.name(), "golem_detected");
    }

    #[test]
    fn remember_positives_counts_down_from_the_last_positive() {
        let mut r = RememberPositives::new(2);
        assert!(!r.test(false), "0 - 1 < 0");
        assert!(r.test(true));
        assert!(r.test(false), "2 - 1 >= 0");
        assert!(r.test(false), "1 - 1 >= 0");
        assert!(!r.test(false), "0 - 1 < 0");
    }

    #[test]
    fn a_tick_publishes_the_mobs_follow_range_to_the_shared_cell() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 3));
        cx.mob.follow_range = 23.0;
        let mut brain = BrainState::new();
        let mut s = Sensor::with_scan_rate(Counting(0), 1);
        s.tick(&mut brain, &mut cx);
        assert_eq!(sensor_range(), 23.0);
        update_targeting_condition_ranges(16.0);
    }
}
