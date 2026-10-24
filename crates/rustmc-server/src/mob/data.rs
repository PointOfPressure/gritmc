//! `SynchedEntityData` as a mob carries it: every accessor its class chain
//! defines, the value each was defined with, and the dirty bits
//! `ServerEntity.sendDirtyEntityData` drains.
//!
//! Reference (26.2): `Entity.<init>` (the eight `Entity` accessors),
//! `LivingEntity.defineSynchedData`, `Mob.defineSynchedData`,
//! `AgeableMob.defineSynchedData`, `Pig.defineSynchedData`,
//! `Bat.defineSynchedData`; `SynchedEntityData.set`, `packDirty`,
//! `getNonDefaultValues`; `SynchedEntityData$DataItem.isSetToDefault`;
//! `EntityDataSerializers.<clinit>`.
//!
//! # Ids
//!
//! `defineId(Class, serializer)` numbers accessors per class in `<clinit>`
//! order, continuing from the superclass's count. The chains here:
//!
//! ```text
//!   Entity        0 shared flags   1 air supply   2 custom name
//!                 3 custom name visible           4 silent
//!                 5 no gravity     6 pose         7 ticks frozen
//!   LivingEntity  8 living flags   9 health      10 effect particles
//!                11 effect ambience              12 arrow count
//!                13 stinger count 14 sleeping pos
//!   Mob          15 mob flags
//!   AgeableMob   16 baby          17 age locked
//!   Pig          18 boost time    19 variant     20 sound variant
//!   Cow          18 variant       19 sound variant
//!   MushroomCow  18 type (INT)
//!   Sheep        18 wool (BYTE: colour in the low nibble, 0x10 sheared)
//!   Chicken      18 variant       19 sound variant
//!   Bat          16 flags
//! ```
//!
//! `Animal` and `AbstractCow` define nothing of their own (`javap`: neither
//! has a `defineSynchedData` that calls `define`), so a kind's own accessors
//! start at 18 straight after `AgeableMob`'s two.
//!
//! `crate::synched` holds the player's chain and this module the mobs':
//! the two share the first fifteen ids and nothing about the rest.
//!
//! # Serializer ids
//!
//! `EntityDataSerializers.<clinit>` registers, in order: BYTE 0, INT 1,
//! LONG 2, FLOAT 3, STRING 4, COMPONENT 5, OPTIONAL_COMPONENT 6, ITEM_STACK
//! 7, BOOLEAN 8, ROTATIONS 9, BLOCK_POS 10, OPTIONAL_BLOCK_POS 11, DIRECTION
//! 12, OPTIONAL_LIVING_ENTITY_REFERENCE 13, BLOCK_STATE 14,
//! OPTIONAL_BLOCK_STATE 15, PARTICLE 16, PARTICLES 17, VILLAGER_DATA 18,
//! OPTIONAL_UNSIGNED_INT 19, POSE 20, CAT_VARIANT 21, CAT_SOUND_VARIANT 22,
//! COW_VARIANT 23, COW_SOUND_VARIANT 24, WOLF_VARIANT 25,
//! WOLF_SOUND_VARIANT 26, FROG_VARIANT 27, PIG_VARIANT 28,
//! PIG_SOUND_VARIANT 29, CHICKEN_VARIANT 30, CHICKEN_SOUND_VARIANT 31, …
//! (the `registerSerializer` call order in `<clinit>`, which is **not** the
//! order of the `putstatic`s: `CHICKEN_VARIANT` is declared before
//! `COW_VARIANT` and registered after `PIG_SOUND_VARIANT`).
//!
//! # Values are compared as the bytes they encode to
//!
//! `DataItem.isSetToDefault` is `initialValue.equals(value)` and `set` skips
//! an equal value (`ObjectUtils.notEqual`). For every type here — boxed
//! primitives and registry holders — equality is value equality, and the
//! encoding is injective, so equal bytes ⇔ equal values.
use crate::synched::DataValue;

/// Serializer ids, from `EntityDataSerializers.<clinit>`.
pub mod serializer {
    pub const BYTE: i32 = 0;
    pub const INT: i32 = 1;
    pub const FLOAT: i32 = 3;
    pub const OPTIONAL_COMPONENT: i32 = 6;
    pub const BOOLEAN: i32 = 8;
    pub const OPTIONAL_BLOCK_POS: i32 = 11;
    pub const PARTICLES: i32 = 17;
    /// `EntityDataSerializers.ROTATIONS` — registered 9th: three floats.
    pub const ROTATIONS: i32 = 9;
    pub const POSE: i32 = 20;
    pub const COW_VARIANT: i32 = 23;
    pub const COW_SOUND_VARIANT: i32 = 24;
    pub const PIG_VARIANT: i32 = 28;
    pub const PIG_SOUND_VARIANT: i32 = 29;
    /// `EntityDataSerializers.COPPER_GOLEM_STATE` — registered 37th
    /// (`registerSerializer` call order in `<clinit>`), an `idMapper` varint.
    pub const COPPER_GOLEM_STATE: i32 = 37;
    /// `EntityDataSerializers.WEATHERING_COPPER_STATE` — 38th, an
    /// `idMapper` varint of the ordinal.
    pub const WEATHERING_COPPER_STATE: i32 = 38;
    pub const CHICKEN_VARIANT: i32 = 30;
    pub const CHICKEN_SOUND_VARIANT: i32 = 31;
}

/// Accessor ids shared by every mob.
pub mod id {
    pub const SHARED_FLAGS: u8 = 0;
    pub const AIR_SUPPLY: u8 = 1;
    pub const CUSTOM_NAME: u8 = 2;
    pub const CUSTOM_NAME_VISIBLE: u8 = 3;
    pub const SILENT: u8 = 4;
    pub const NO_GRAVITY: u8 = 5;
    pub const POSE: u8 = 6;
    pub const TICKS_FROZEN: u8 = 7;
    pub const LIVING_FLAGS: u8 = 8;
    pub const HEALTH: u8 = 9;
    pub const EFFECT_PARTICLES: u8 = 10;
    pub const EFFECT_AMBIENCE: u8 = 11;
    pub const ARROW_COUNT: u8 = 12;
    pub const STINGER_COUNT: u8 = 13;
    pub const SLEEPING_POS: u8 = 14;
    pub const MOB_FLAGS: u8 = 15;
    /// `AgeableMob.DATA_BABY_ID`.
    pub const BABY: u8 = 16;
    /// `AgeableMob.AGE_LOCKED`.
    pub const AGE_LOCKED: u8 = 17;
    /// `Pig.DATA_BOOST_TIME`.
    pub const PIG_BOOST_TIME: u8 = 18;
    /// `Pig.DATA_VARIANT_ID`.
    pub const PIG_VARIANT: u8 = 19;
    /// `Pig.DATA_SOUND_VARIANT_ID`.
    pub const PIG_SOUND_VARIANT: u8 = 20;
    /// `Bat.DATA_ID_FLAGS`.
    pub const BAT_FLAGS: u8 = 16;
    /// `IronGolem.DATA_FLAGS_ID`, a byte defaulting to 0; bit 1 is
    /// player-created.
    pub const IRON_GOLEM_FLAGS: u8 = 16;
    /// `SnowGolem.DATA_PUMPKIN_ID`, a byte defaulting to 16 (`bipush 16`).
    pub const SNOW_GOLEM_PUMPKIN: u8 = 16;
    /// `CopperGolem.DATA_WEATHER_STATE`, defaulting to `UNAFFECTED`.
    pub const COPPER_GOLEM_WEATHER: u8 = 16;
    /// `CopperGolem.COPPER_GOLEM_STATE`, defaulting to `IDLE`.
    pub const COPPER_GOLEM_STATE: u8 = 17;
    /// `ArmorStand.DATA_CLIENT_FLAGS` — id 15, because an armor stand is a
    /// `LivingEntity` and not a `Mob`: there is no `DATA_MOB_FLAGS_ID` in
    /// its chain.
    pub const ARMOR_STAND_FLAGS: u8 = 15;
    /// `DATA_HEAD_POSE` .. `DATA_RIGHT_LEG_POSE`, 16..21.
    pub const ARMOR_STAND_HEAD_POSE: u8 = 16;
    /// `Cow.DATA_VARIANT_ID`.
    pub const COW_VARIANT: u8 = 18;
    /// `Cow.DATA_SOUND_VARIANT_ID`.
    pub const COW_SOUND_VARIANT: u8 = 19;
    /// `MushroomCow.DATA_TYPE` — `Variant.id`, an `INT`.
    pub const MOOSHROOM_TYPE: u8 = 18;
    /// `Sheep.DATA_WOOL_ID`.
    pub const SHEEP_WOOL: u8 = 18;
    /// `Chicken.DATA_VARIANT_ID`.
    pub const CHICKEN_VARIANT: u8 = 18;
    /// `Chicken.DATA_SOUND_VARIANT_ID`.
    pub const CHICKEN_SOUND_VARIANT: u8 = 19;
}

/// One `DataItem`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataItem {
    pub id: u8,
    pub serializer: i32,
    initial: Vec<u8>,
    value: Vec<u8>,
    dirty: bool,
}

/// The mob's `SynchedEntityData`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobData {
    items: Vec<DataItem>,
    is_dirty: bool,
}

/// A varint, as the wire writes it.
fn varint(v: i32) -> Vec<u8> {
    let mut b = Vec::with_capacity(5);
    rustmc_proto::varint::write_varint(&mut b, v).expect("a Vec write");
    b
}

impl MobData {
    /// `Entity.<init>`'s eight, in id order. `getMaxAirSupply()` is 300 for
    /// every entity here (`Entity.getMaxAirSupply`), `Pose.STANDING` is
    /// ordinal 0.
    fn entity_items() -> Vec<DataItem> {
        let d = |id: u8, serializer: i32, initial: Vec<u8>| DataItem {
            id,
            serializer,
            initial: initial.clone(),
            value: initial,
            dirty: false,
        };
        vec![
            d(id::SHARED_FLAGS, serializer::BYTE, vec![0]),
            d(id::AIR_SUPPLY, serializer::INT, varint(300)),
            d(id::CUSTOM_NAME, serializer::OPTIONAL_COMPONENT, vec![0]),
            d(id::CUSTOM_NAME_VISIBLE, serializer::BOOLEAN, vec![0]),
            d(id::SILENT, serializer::BOOLEAN, vec![0]),
            d(id::NO_GRAVITY, serializer::BOOLEAN, vec![0]),
            d(id::POSE, serializer::POSE, varint(0)),
            d(id::TICKS_FROZEN, serializer::INT, varint(0)),
            // LivingEntity.defineSynchedData
            d(id::LIVING_FLAGS, serializer::BYTE, vec![0]),
            d(
                id::HEALTH,
                serializer::FLOAT,
                1.0_f32.to_be_bytes().to_vec(),
            ),
            d(id::EFFECT_PARTICLES, serializer::PARTICLES, varint(0)),
            d(id::EFFECT_AMBIENCE, serializer::BOOLEAN, vec![0]),
            d(id::ARROW_COUNT, serializer::INT, varint(0)),
            d(id::STINGER_COUNT, serializer::INT, varint(0)),
            d(id::SLEEPING_POS, serializer::OPTIONAL_BLOCK_POS, vec![0]),
            // Mob.defineSynchedData
            d(id::MOB_FLAGS, serializer::BYTE, vec![0]),
        ]
    }

    /// A pig's data: the chain through `AgeableMob`, then `Pig`'s three.
    /// `variant` and `sound_variant` are the registry ids of
    /// `PigVariants.DEFAULT` (`temperate`) and `PigSoundVariants.CLASSIC`,
    /// which `Pig.defineSynchedData` defines as the initial values.
    #[must_use]
    pub fn pig(variant_default: i32, sound_variant_default: i32) -> Self {
        let mut items = Self::entity_items();
        let d = |id: u8, serializer: i32, initial: Vec<u8>| DataItem {
            id,
            serializer,
            initial: initial.clone(),
            value: initial,
            dirty: false,
        };
        items.push(d(id::BABY, serializer::BOOLEAN, vec![0]));
        items.push(d(id::AGE_LOCKED, serializer::BOOLEAN, vec![0]));
        items.push(d(id::PIG_BOOST_TIME, serializer::INT, varint(0)));
        items.push(d(
            id::PIG_VARIANT,
            serializer::PIG_VARIANT,
            varint(variant_default),
        ));
        items.push(d(
            id::PIG_SOUND_VARIANT,
            serializer::PIG_SOUND_VARIANT,
            varint(sound_variant_default),
        ));
        Self {
            items,
            is_dirty: false,
        }
    }

    /// The chain through `AgeableMob` (`DATA_BABY_ID`, `AGE_LOCKED`), then
    /// `extra` — a kind's own accessors, in `<clinit>` order.
    fn ageable(extra: &[(u8, i32, Vec<u8>)]) -> Self {
        let mut items = Self::entity_items();
        let d = |id: u8, serializer: i32, initial: Vec<u8>| DataItem {
            id,
            serializer,
            initial: initial.clone(),
            value: initial,
            dirty: false,
        };
        items.push(d(id::BABY, serializer::BOOLEAN, vec![0]));
        items.push(d(id::AGE_LOCKED, serializer::BOOLEAN, vec![0]));
        for (id, ser, initial) in extra {
            items.push(d(*id, *ser, initial.clone()));
        }
        Self {
            items,
            is_dirty: false,
        }
    }

    /// A cow's data: `Cow.defineSynchedData` — the variant
    /// (`VariantUtils.getDefaultOrAny(TEMPERATE)`) and the sound variant
    /// (`CowSoundVariants.CLASSIC`), as registry ids.
    #[must_use]
    pub fn cow(variant_default: i32, sound_variant_default: i32) -> Self {
        Self::ageable(&[
            (
                id::COW_VARIANT,
                serializer::COW_VARIANT,
                varint(variant_default),
            ),
            (
                id::COW_SOUND_VARIANT,
                serializer::COW_SOUND_VARIANT,
                varint(sound_variant_default),
            ),
        ])
    }

    /// A mooshroom's data: `MushroomCow.defineSynchedData` —
    /// `DATA_TYPE` = `Variant.DEFAULT.id`, `RED`'s 0.
    #[must_use]
    pub fn mooshroom() -> Self {
        Self::ageable(&[(id::MOOSHROOM_TYPE, serializer::INT, varint(0))])
    }

    /// A sheep's data: `Sheep.defineSynchedData` — `DATA_WOOL_ID` = `(byte) 0`
    /// (white, not sheared).
    #[must_use]
    pub fn sheep() -> Self {
        Self::ageable(&[(id::SHEEP_WOOL, serializer::BYTE, vec![0])])
    }

    /// A chicken's data: `Chicken.defineSynchedData` — the variant
    /// (`ChickenVariants.TEMPERATE`) and the sound variant
    /// (`ChickenSoundVariants.CLASSIC`).
    #[must_use]
    pub fn chicken(variant_default: i32, sound_variant_default: i32) -> Self {
        Self::ageable(&[
            (
                id::CHICKEN_VARIANT,
                serializer::CHICKEN_VARIANT,
                varint(variant_default),
            ),
            (
                id::CHICKEN_SOUND_VARIANT,
                serializer::CHICKEN_SOUND_VARIANT,
                varint(sound_variant_default),
            ),
        ])
    }

    /// A bat's data: the chain through `Mob`, then `Bat.DATA_ID_FLAGS`.
    #[must_use]
    pub fn bat() -> Self {
        let mut items = Self::entity_items();
        items.push(DataItem {
            id: id::BAT_FLAGS,
            serializer: serializer::BYTE,
            initial: vec![0],
            value: vec![0],
            dirty: false,
        });
        Self {
            items,
            is_dirty: false,
        }
    }

    /// `ArmorStand.defineSynchedData`, over `LivingEntity`'s 0..14 — no
    /// `Mob` flags. The six poses are `ArmorStand.DEFAULT_*_POSE`:
    /// head and body `(0, 0, 0)`, left arm `(-10, 0, -10)`, right arm
    /// `(-15, 0, 10)`, left leg `(-1, 0, -1)`, right leg `(1, 0, 1)`.
    #[must_use]
    pub fn armor_stand() -> Self {
        let mut items = Self::entity_items();
        items.retain(|i| i.id != id::MOB_FLAGS);
        let rot = |x: f32, y: f32, z: f32| {
            let mut b = Vec::with_capacity(12);
            b.extend_from_slice(&x.to_be_bytes());
            b.extend_from_slice(&y.to_be_bytes());
            b.extend_from_slice(&z.to_be_bytes());
            b
        };
        let extra = [
            (id::ARMOR_STAND_FLAGS, serializer::BYTE, vec![0]),
            (
                id::ARMOR_STAND_HEAD_POSE,
                serializer::ROTATIONS,
                rot(0.0, 0.0, 0.0),
            ),
            (17, serializer::ROTATIONS, rot(0.0, 0.0, 0.0)),
            (18, serializer::ROTATIONS, rot(-10.0, 0.0, -10.0)),
            (19, serializer::ROTATIONS, rot(-15.0, 0.0, 10.0)),
            (20, serializer::ROTATIONS, rot(-1.0, 0.0, -1.0)),
            (21, serializer::ROTATIONS, rot(1.0, 0.0, 1.0)),
        ];
        for (id, serializer, initial) in extra {
            items.push(DataItem {
                id,
                serializer,
                initial: initial.clone(),
                value: initial,
                dirty: false,
            });
        }
        Self {
            items,
            is_dirty: false,
        }
    }

    fn with(extra: &[(u8, i32, Vec<u8>)]) -> Self {
        let mut items = Self::entity_items();
        for (id, serializer, initial) in extra {
            items.push(DataItem {
                id: *id,
                serializer: *serializer,
                initial: initial.clone(),
                value: initial.clone(),
                dirty: false,
            });
        }
        Self {
            items,
            is_dirty: false,
        }
    }

    /// `IronGolem.defineSynchedData`: `DATA_FLAGS_ID` = `(byte) 0`.
    #[must_use]
    pub fn iron_golem() -> Self {
        Self::with(&[(id::IRON_GOLEM_FLAGS, serializer::BYTE, vec![0])])
    }

    /// `SnowGolem.defineSynchedData`: `DATA_PUMPKIN_ID` = `(byte) 16`.
    #[must_use]
    pub fn snow_golem() -> Self {
        Self::with(&[(id::SNOW_GOLEM_PUMPKIN, serializer::BYTE, vec![16])])
    }

    /// `CopperGolem.defineSynchedData`: `DATA_WEATHER_STATE` =
    /// `UNAFFECTED`, then `COPPER_GOLEM_STATE` = `IDLE`, both ordinal 0.
    #[must_use]
    pub fn copper_golem() -> Self {
        Self::with(&[
            (
                id::COPPER_GOLEM_WEATHER,
                serializer::WEATHERING_COPPER_STATE,
                varint(0),
            ),
            (
                id::COPPER_GOLEM_STATE,
                serializer::COPPER_GOLEM_STATE,
                varint(0),
            ),
        ])
    }

    fn item(&self, id: u8) -> &DataItem {
        self.items
            .iter()
            .find(|i| i.id == id)
            .unwrap_or_else(|| panic!("no accessor {id} on this entity"))
    }

    /// `get(accessor)`, as bytes.
    #[must_use]
    pub fn get(&self, id: u8) -> &[u8] {
        &self.item(id).value
    }

    /// `get` of a byte accessor.
    #[must_use]
    pub fn get_byte(&self, id: u8) -> u8 {
        self.get(id)[0]
    }

    /// `get` of a boolean accessor.
    #[must_use]
    pub fn get_bool(&self, id: u8) -> bool {
        self.get(id)[0] != 0
    }

    /// `get` of a float accessor.
    #[must_use]
    pub fn get_f32(&self, id: u8) -> f32 {
        let b = self.get(id);
        f32::from_be_bytes([b[0], b[1], b[2], b[3]])
    }

    /// `get` of a varint accessor.
    #[must_use]
    pub fn get_varint(&self, id: u8) -> i32 {
        let mut r = std::io::Cursor::new(self.get(id));
        rustmc_proto::varint::read_varint(&mut r).unwrap_or(0)
    }

    /// `set(accessor, value)` — the `force = false` form: a value equal to
    /// the current one changes nothing, one that differs is stored and marks
    /// both the item and the map dirty. `onSyncedDataUpdated` is the host's.
    /// Returns whether it changed.
    pub fn set(&mut self, id: u8, value: Vec<u8>) -> bool {
        let item = self
            .items
            .iter_mut()
            .find(|i| i.id == id)
            .unwrap_or_else(|| panic!("no accessor {id} on this entity"));
        if item.value == value {
            return false;
        }
        item.value = value;
        item.dirty = true;
        self.is_dirty = true;
        true
    }

    pub fn set_byte(&mut self, id: u8, v: u8) -> bool {
        self.set(id, vec![v])
    }

    pub fn set_bool(&mut self, id: u8, v: bool) -> bool {
        self.set(id, vec![u8::from(v)])
    }

    pub fn set_f32(&mut self, id: u8, v: f32) -> bool {
        self.set(id, v.to_be_bytes().to_vec())
    }

    pub fn set_varint(&mut self, id: u8, v: i32) -> bool {
        self.set(id, varint(v))
    }

    /// `isDirty()`.
    #[must_use]
    pub const fn is_dirty(&self) -> bool {
        self.is_dirty
    }

    /// `packDirty()`: `null` unless the map is dirty; else every dirty
    /// item's value in id order, each cleared as it goes, and the map
    /// cleared.
    pub fn pack_dirty(&mut self) -> Option<Vec<DataValue>> {
        if !self.is_dirty {
            return None;
        }
        self.is_dirty = false;
        let mut out = Vec::new();
        for item in &mut self.items {
            if item.dirty {
                item.dirty = false;
                out.push(DataValue {
                    id: item.id,
                    serializer: item.serializer,
                    value: item.value.clone(),
                });
            }
        }
        Some(out)
    }

    /// `getNonDefaultValues()`: the items away from their definition, in id
    /// order, or `None` when there are none (vanilla allocates the list
    /// lazily and returns `null`).
    #[must_use]
    pub fn non_default_values(&self) -> Option<Vec<DataValue>> {
        let out: Vec<DataValue> = self
            .items
            .iter()
            .filter(|i| i.initial != i.value)
            .map(|i| DataValue {
                id: i.id,
                serializer: i.serializer,
                value: i.value.clone(),
            })
            .collect();
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_pig_has_twenty_one_accessors_all_at_their_definition() {
        let d = MobData::pig(0, 1);
        assert_eq!(d.items.len(), 21);
        assert_eq!(d.items.last().map(|i| i.id), Some(20));
        assert!(d.non_default_values().is_none());
        assert!(!d.is_dirty());
        assert_eq!(d.get_varint(id::PIG_SOUND_VARIANT), 1);
        assert_eq!(d.get_f32(id::HEALTH), 1.0);
        assert_eq!(d.get_varint(id::AIR_SUPPLY), 300);
    }

    #[test]
    fn setting_health_marks_dirty_once_and_pack_dirty_clears_it() {
        let mut d = MobData::pig(0, 1);
        assert!(d.set_f32(id::HEALTH, 10.0));
        assert!(
            !d.set_f32(id::HEALTH, 10.0),
            "an equal value is not a change"
        );
        assert!(d.is_dirty());
        let nd = d.non_default_values().expect("health is away from 1.0");
        assert_eq!(nd.len(), 1);
        assert_eq!(nd[0].id, id::HEALTH);
        assert_eq!(nd[0].serializer, serializer::FLOAT);
        let packed = d.pack_dirty().expect("dirty");
        assert_eq!(packed, nd);
        assert!(d.pack_dirty().is_none());
        // Still non-default after the flush: dirty and non-default are
        // different questions.
        assert!(d.non_default_values().is_some());
    }

    #[test]
    fn the_four_passive_kinds_end_at_their_own_accessors() {
        let cow = MobData::cow(1, 0);
        assert_eq!(cow.items.len(), 20);
        assert_eq!(cow.item(id::COW_SOUND_VARIANT).serializer, 24);
        let sheep = MobData::sheep();
        assert_eq!(sheep.items.len(), 19);
        assert_eq!(sheep.get_byte(id::SHEEP_WOOL), 0);
        let moo = MobData::mooshroom();
        assert_eq!(moo.items.len(), 19);
        assert_eq!(moo.item(id::MOOSHROOM_TYPE).serializer, serializer::INT);
        let chicken = MobData::chicken(2, 0);
        assert_eq!(chicken.items.len(), 20);
        assert_eq!(chicken.item(id::CHICKEN_VARIANT).serializer, 30);
        assert_eq!(chicken.get_varint(id::CHICKEN_VARIANT), 2);
        for d in [cow, sheep, moo, chicken] {
            assert!(d.non_default_values().is_none());
        }
    }

    #[test]
    fn a_value_set_back_to_its_definition_is_dirty_but_not_non_default() {
        let mut d = MobData::bat();
        assert_eq!(d.items.len(), 17);
        d.set_byte(id::BAT_FLAGS, 1);
        d.set_byte(id::BAT_FLAGS, 0);
        assert!(d.non_default_values().is_none());
        let packed = d.pack_dirty().expect("dirty");
        assert_eq!(packed.len(), 1);
        assert_eq!(packed[0].value, vec![0]);
    }
}
