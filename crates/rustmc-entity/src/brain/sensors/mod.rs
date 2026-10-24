//! The `net.minecraft.world.entity.ai.sensing` sensor classes, one file
//! each, as [`crate::brain::sensor::SensorBody`] implementations.

pub mod adult_sensor;
pub mod axolotl_attackables_sensor;
pub mod dummy_sensor;
pub mod frog_attackables_sensor;
pub mod golem_sensor;
pub mod hoglin_specific_sensor;
pub mod hurt_by_sensor;
pub mod is_in_water_sensor;
pub mod mob_sensor;
pub mod nearest_bed_sensor;
pub mod nearest_item_sensor;
pub mod nearest_living_entity_sensor;
pub mod nearest_visible_living_entity_sensor;
pub mod piglin_brute_specific_sensor;
pub mod piglin_specific_sensor;
pub mod player_sensor;
pub mod secondary_poi_sensor;
pub mod tempting_sensor;
pub mod villager_babies_sensor;
pub mod villager_hostiles_sensor;
pub mod warden_entity_sensor;

pub use adult_sensor::AdultSensor;
pub use axolotl_attackables_sensor::AxolotlAttackablesSensor;
pub use dummy_sensor::DummySensor;
pub use frog_attackables_sensor::FrogAttackablesSensor;
pub use golem_sensor::GolemSensor;
pub use hoglin_specific_sensor::HoglinSpecificSensor;
pub use hurt_by_sensor::HurtBySensor;
pub use is_in_water_sensor::IsInWaterSensor;
pub use mob_sensor::MobSensor;
pub use nearest_bed_sensor::NearestBedSensor;
pub use nearest_item_sensor::NearestItemSensor;
pub use nearest_living_entity_sensor::NearestLivingEntitySensor;
pub use piglin_brute_specific_sensor::PiglinBruteSpecificSensor;
pub use piglin_specific_sensor::PiglinSpecificSensor;
pub use player_sensor::PlayerSensor;
pub use secondary_poi_sensor::SecondaryPoiSensor;
pub use tempting_sensor::TemptingSensor;
pub use villager_babies_sensor::VillagerBabiesSensor;
pub use villager_hostiles_sensor::VillagerHostilesSensor;
pub use warden_entity_sensor::WardenEntitySensor;

use crate::brain::sensor::{SensorControl, SensorType};
use crate::host::MobHost;

/// `SensorType.create()` for every sensor this crate transcribes.
///
/// The `Option` is kept from when it had gaps: every one of the 24
/// registered types is transcribed as of the fifth lane, and the `match`
/// below is **exhaustive with no wildcard arm**, so deleting a sensor is
/// now a compile error here rather than a `None` some caller has to
/// notice. `tests/brain_tables.rs` still asks this factory for every
/// sensor slot the twenty providers name.
#[must_use]
pub fn create<M: MobHost + 'static>(ty: SensorType) -> Option<Box<dyn SensorControl<M>>> {
    Some(match ty {
        SensorType::Dummy => Box::new(DummySensor::new()),
        SensorType::NearestLivingEntities => Box::new(NearestLivingEntitySensor::new()),
        SensorType::NearestPlayers => Box::new(PlayerSensor::new()),
        SensorType::NearestItems => Box::new(NearestItemSensor::new()),
        SensorType::NearestBed => Box::new(NearestBedSensor::new()),
        SensorType::HurtBy => Box::new(HurtBySensor::new()),
        SensorType::VillagerHostiles => Box::new(VillagerHostilesSensor::new()),
        SensorType::VillagerBabies => Box::new(VillagerBabiesSensor::new()),
        SensorType::SecondaryPois => Box::new(SecondaryPoiSensor::new()),
        SensorType::GolemDetected => Box::new(GolemSensor::new()),
        SensorType::PiglinSpecificSensor => Box::new(PiglinSpecificSensor::new()),
        SensorType::NearestAdult => Box::new(AdultSensor::new()),
        SensorType::NearestAdultAnyType => Box::new(AdultSensor::any_type()),
        SensorType::IsInWater => Box::new(IsInWaterSensor::new()),
        SensorType::AxolotlAttackables => Box::new(AxolotlAttackablesSensor::new()),
        SensorType::FrogAttackables => Box::new(FrogAttackablesSensor::new()),
        SensorType::HoglinSpecificSensor => Box::new(HoglinSpecificSensor::new()),
        SensorType::PiglinBruteSpecificSensor => Box::new(PiglinBruteSpecificSensor::new()),
        SensorType::ArmadilloScareDetected => Box::new(MobSensor::armadillo_scare()),
        SensorType::WardenEntitySensor => Box::new(WardenEntitySensor::new()),
        SensorType::BreezeAttackEntitySensor => Box::new(WardenEntitySensor::breeze()),
        SensorType::FoodTemptations => Box::new(TemptingSensor::for_animal()),
        SensorType::FrogTemptations => Box::new(TemptingSensor::frog()),
        SensorType::NautilusTemptations => Box::new(TemptingSensor::nautilus()),
    })
}
