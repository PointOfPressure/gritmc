//! The entities a structure adds while it is placed, as the compounds
//! `entities/r.X.Z.mca` saves.
//!
//! Reference: `WoodlandMansionPieces$WoodlandMansionPiece.handleDataMarker`,
//! `OceanMonumentPieces$OceanMonumentPiece.spawnElder`,
//! `EndCityPieces$EndCityPiece.handleDataMarker`, `Mob.finalizeSpawn`,
//! `Raider.finalizeSpawn`, `PatrollingMonster.finalizeSpawn`,
//! `Vindicator.finalizeSpawn` / `populateDefaultEquipmentSlots`,
//! `Mob.populateDefaultEquipmentEnchantments` / `enchantSpawnedEquipment`,
//! `RandomSource.triangle`, `Mth.createInsecureUUID`, and the save methods
//! (`Entity.saveWithoutId` down to each kind's `addAdditionalSaveData`) for
//! the key set, which is read off vanilla's own saved compounds in the twin
//! harvests' `entities/` files with its NBT types.
//!
//! # What is drawn, and from what
//!
//! `finalizeSpawn` takes `level.getRandom()`: during generation that is the
//! `WorldGenRegion`'s random, seeded per decorating chunk
//! (`Decoration::level_random`). It draws, in order:
//!
//! ```text
//!   Mob                 triangle(0, 0.11485) = 0.11485 * (nextDouble() - nextDouble())
//!                       -> follow_range's random_spawn_bonus modifier
//!                       nextFloat() < 0.05 -> LeftHanded
//!   PatrollingMonster   nothing for STRUCTURE (the 0.06 leader roll is skipped)
//!   Vindicator          then an iron axe, then enchantSpawnedEquipment's
//!                       nextFloat() for the main hand (armour slots are empty
//!                       and draw nothing)
//! ```
//!
//! and a mansion's "Group of Allays" draws `nextInt(3) + 1` before any of
//! them. A shulker and an item frame are added with no `finalizeSpawn` and
//! draw nothing.
//!
//! The vindicator's weapon roll only enchants when it is under `0.25 *
//! DifficultyInstance.getSpecialMultiplier()`, which is 0 below an effective
//! difficulty of 2 — every fresh chunk on easy or normal. The roll is spent
//! and never wins, which is what the harvests show: no enchanted axe.
//!
//! # What is not reproducible, and is not claimed
//!
//! `UUID` comes from each entity's own unseeded random, and so does a
//! shulker's yaw; two vanilla runs of one seed disagree on both and agree on
//! every other key. The UUID here is derived from the entity's block and
//! kind; the shulker's yaw is written as 0.

use rustmc_world::nbt::Nbt;
use std::collections::BTreeMap;

use crate::rng::RandomSource;

/// A structure mob kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mob {
    Evoker,
    Vindicator,
    Allay,
    ElderGuardian,
    Shulker,
    Witch,
    Cat,
    Drowned,
}

impl Mob {
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Evoker => "minecraft:evoker",
            Self::Vindicator => "minecraft:vindicator",
            Self::Allay => "minecraft:allay",
            Self::ElderGuardian => "minecraft:elder_guardian",
            Self::Shulker => "minecraft:shulker",
            Self::Witch => "minecraft:witch",
            Self::Cat => "minecraft:cat",
            Self::Drowned => "minecraft:drowned",
        }
    }

    /// `getMaxHealth()`, which a fresh mob's `Health` is.
    const fn max_health(self) -> f32 {
        match self {
            Self::Evoker | Self::Vindicator => 24.0,
            Self::Allay => 20.0,
            Self::ElderGuardian => 80.0,
            Self::Shulker => 30.0,
            Self::Witch => 26.0,
            Self::Cat => 10.0,
            Self::Drowned => 20.0,
        }
    }

    /// `FOLLOW_RANGE`'s base value for the kind.
    const fn follow_range(self) -> f64 {
        match self {
            Self::Evoker | Self::Vindicator => 12.0,
            Self::Allay | Self::ElderGuardian | Self::Witch | Self::Cat => 16.0,
            Self::Shulker => 16.0,
            Self::Drowned => 35.0,
        }
    }

    const fn is_raider(self) -> bool {
        matches!(self, Self::Evoker | Self::Vindicator | Self::Witch)
    }
}

/// What `finalizeSpawn` decided, drawn from the level random.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Finalized {
    pub follow_range_bonus: f64,
    pub left_handed: bool,
    /// A cat's `sound_variant`.
    pub cat_sound: Option<&'static str>,
    /// What a drowned drew: baby, main hand, the nautilus shell.
    pub drowned: Option<DrownedRoll>,
}

/// `Zombie.finalizeSpawn` and `Drowned`'s overrides, as drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrownedRoll {
    pub baby: bool,
    /// `minecraft:trident` or `minecraft:fishing_rod`.
    pub main_hand: Option<&'static str>,
    pub nautilus_shell: bool,
    /// A branch that spawns a second entity this port does not make — a
    /// chicken jockey or a zombie nautilus — was taken. Its draws are not
    /// made, so every later draw in the chunk's level random is off.
    pub refused: Option<&'static str>,
}

/// `finalizeSpawn(level, difficulty, STRUCTURE, null)` for `mob`, drawing
/// from the level random. `None` for a shulker, which is never finalized.
pub fn finalize_spawn<R: RandomSource + ?Sized>(mob: Mob, random: &mut R) -> Option<Finalized> {
    if mob == Mob::Shulker {
        return None;
    }
    // `Mob.finalizeSpawn`: `random.triangle(0.0, 0.11485000000000001)` is
    // `mode + deviation * (nextDouble() - nextDouble())`, the two doubles in
    // that order.
    let a = random.next_f64();
    let b = random.next_f64();
    let follow_range_bonus = 0.0 + 0.114_850_000_000_000_01 * (a - b);
    let left_handed = random.next_f32() < 0.05;
    if mob == Mob::Vindicator {
        // `enchantSpawnedEquipment(MAINHAND, 0.25f)`: the iron axe is there,
        // so its roll is spent; it wins only above an effective difficulty
        // of 2, which a fresh chunk never reaches.
        let _ = random.next_f32();
    }
    // `Cat.finalizeSpawn`, after `AgeableMob`'s (whose baby roll needs a
    // group size above 0 and so is never spent for a lone cat) and `Mob`'s:
    // `VariantUtils.selectVariantToSpawn` keeps the highest-priority
    // variants whose condition holds — inside a swamp hut that is
    // `all_black` alone, `#cats_spawn_as_black` at priority 1 — and
    // `Util.getRandomSafe` spends `nextInt(1)` on the one-element list;
    // then `CatSoundVariants.pickRandomSoundVariant` is `nextInt(2)` over
    // the registry, `classic` then `royal`.
    let cat_sound = (mob == Mob::Cat).then(|| {
        let _ = random.next_i32_bound(1);
        if random.next_i32_bound(2) == 0 {
            "minecraft:classic"
        } else {
            "minecraft:royal"
        }
    });
    let drowned = (mob == Mob::Drowned).then(|| drowned_roll(random));
    Some(Finalized {
        follow_range_bonus,
        left_handed,
        cat_sound,
        drowned,
    })
}

/// The rest of `Zombie.finalizeSpawn` and `Drowned.finalizeSpawn` for a
/// STRUCTURE spawn, after `Mob`'s three draws, on a fresh chunk (special
/// multiplier 0, so every `nextFloat() < k * f` loses but is spent):
///
/// ```text
///   canPickUpLoot           nextFloat
///   getSpawnAsBabyOdds      nextFloat < 0.05
///   (baby) jockey rolls     nextFloat < 0.05 -> chicken search, else
///                           nextFloat < 0.05 -> a new chicken (refused here)
///   canBreakDoors           nextFloat
///   Drowned equipment       nextFloat > 0.9 -> nextInt(16): < 10 trident,
///                           else fishing rod
///   weapon enchant          nextFloat, only with something in the main hand
///   (Halloween pumpkin      SpecialDates.isHalloween(): the real date; not
///                           drawn here, so a world generated on 31 October
///                           would differ)
///   Drowned nautilus shell  nextFloat < 0.03, off hand empty
///   Drowned trident jockey  nextFloat < 0.5 with a trident -> a zombie
///                           nautilus (refused here when not a baby)
/// ```
///
/// `handleAttributes` draws from the entity's own random, not this one.
fn drowned_roll<R: RandomSource + ?Sized>(random: &mut R) -> DrownedRoll {
    let mut refused = None;
    let _can_pick_up_loot = random.next_f32();
    let baby = random.next_f32() < 0.05;
    if baby {
        if f64::from(random.next_f32()) < 0.05 {
            // `getEntitiesOfClass(Chicken, ...)`: generation has made no
            // chicken yet in this port's world, so the search finds none.
        } else if f64::from(random.next_f32()) < 0.05 {
            refused = Some("a baby drowned's chicken jockey");
        }
    }
    let _can_break_doors = random.next_f32();
    let main_hand = if f64::from(random.next_f32()) > 0.9 {
        Some(if random.next_i32_bound(16) < 10 {
            "minecraft:trident"
        } else {
            "minecraft:fishing_rod"
        })
    } else {
        None
    };
    if main_hand.is_some() {
        let _ = random.next_f32();
    }
    let nautilus_shell = random.next_f32() < 0.03;
    if main_hand == Some("minecraft:trident") && random.next_f32() < 0.5 && !baby {
        refused = Some("a drowned's zombie nautilus");
    }
    DrownedRoll {
        baby,
        main_hand,
        nautilus_shell,
        refused,
    }
}

/// `Mth.createInsecureUUID` over a random seeded from the entity's block and
/// kind: deterministic, unique per block and kind, and not vanilla's (which
/// is unseeded).
#[must_use]
pub fn uuid(kind: &str, block: (i32, i32, i32), ordinal: u32) -> [i32; 4] {
    let mut h: i64 = 0x5DEE_CE66;
    for b in kind.bytes() {
        h = h.wrapping_mul(31).wrapping_add(i64::from(b));
    }
    h = h
        .wrapping_mul(0x9E37_79B9_7F4A_7C15_u64 as i64)
        .wrapping_add(i64::from(block.0))
        .wrapping_mul(31)
        .wrapping_add(i64::from(block.1))
        .wrapping_mul(31)
        .wrapping_add(i64::from(block.2))
        .wrapping_mul(31)
        .wrapping_add(i64::from(ordinal));
    let mut r = crate::rng::legacy::LegacyRandomSource::new(h);
    let most = (r.next_i64() & !0xF000) | 0x4000;
    let least = (r.next_i64() & 0x3FFF_FFFF_FFFF_FFFF) | i64::MIN;
    #[allow(clippy::cast_possible_truncation)]
    [
        (most >> 32) as i32,
        most as i32,
        (least >> 32) as i32,
        least as i32,
    ]
}

fn doubles(v: [f64; 3]) -> Nbt {
    Nbt::List(v.iter().map(|&d| Nbt::Double(d)).collect())
}

/// The keys every `Entity` save writes for an entity nothing has ticked.
fn entity_base(m: &mut BTreeMap<String, Nbt>, id: &str, pos: [f64; 3], yaw: f32, uuid: [i32; 4]) {
    m.insert("id".into(), Nbt::String(id.into()));
    m.insert("Pos".into(), doubles(pos));
    m.insert("Motion".into(), doubles([0.0; 3]));
    m.insert(
        "Rotation".into(),
        Nbt::List(vec![Nbt::Float(yaw), Nbt::Float(0.0)]),
    );
    m.insert("fall_distance".into(), Nbt::Double(0.0));
    m.insert("Fire".into(), Nbt::Short(0));
    m.insert("Air".into(), Nbt::Short(300));
    m.insert("OnGround".into(), Nbt::Byte(0));
    m.insert("Invulnerable".into(), Nbt::Byte(0));
    m.insert("PortalCooldown".into(), Nbt::Int(0));
    m.insert("UUID".into(), Nbt::IntArray(uuid.to_vec()));
}

/// The compound a structure mob saves with, standing at `block`'s centre
/// (`snapTo(x + 0.5, y, z + 0.5, 0, 0)`, or `setPos` for a shulker).
#[must_use]
pub fn mob_compound(
    mob: Mob,
    block: (i32, i32, i32),
    finalized: Option<Finalized>,
    uuid: [i32; 4],
) -> Nbt {
    let mut m = BTreeMap::new();
    let pos = [
        f64::from(block.0) + 0.5,
        f64::from(block.1),
        f64::from(block.2) + 0.5,
    ];
    entity_base(&mut m, mob.id(), pos, 0.0, uuid);
    // `LivingEntity.addAdditionalSaveData`.
    m.insert("Health".into(), Nbt::Float(mob.max_health()));
    m.insert("HurtTime".into(), Nbt::Short(0));
    m.insert("DeathTime".into(), Nbt::Short(0));
    m.insert("AbsorptionAmount".into(), Nbt::Float(0.0));
    m.insert("FallFlying".into(), Nbt::Byte(0));
    m.insert(
        "Brain".into(),
        Nbt::Compound(BTreeMap::from([(
            "memories".to_string(),
            Nbt::Compound(BTreeMap::new()),
        )])),
    );
    m.insert(
        "current_impulse_context_reset_grace_time".into(),
        Nbt::Int(0),
    );
    let attributes = match finalized {
        Some(f) => vec![Nbt::Compound(BTreeMap::from([
            ("base".to_string(), Nbt::Double(mob.follow_range())),
            (
                "id".to_string(),
                Nbt::String("minecraft:follow_range".into()),
            ),
            (
                "modifiers".to_string(),
                Nbt::List(vec![Nbt::Compound(BTreeMap::from([
                    ("amount".to_string(), Nbt::Double(f.follow_range_bonus)),
                    (
                        "id".to_string(),
                        Nbt::String("minecraft:random_spawn_bonus".into()),
                    ),
                    (
                        "operation".to_string(),
                        Nbt::String("add_multiplied_base".into()),
                    ),
                ]))]),
            ),
        ]))],
        None => Vec::new(),
    };
    m.insert("attributes".into(), Nbt::List(attributes));
    // `Mob.addAdditionalSaveData`.
    m.insert("CanPickUpLoot".into(), Nbt::Byte(0));
    m.insert(
        "PersistenceRequired".into(),
        Nbt::Byte(i8::from(mob != Mob::Shulker)),
    );
    m.insert(
        "LeftHanded".into(),
        Nbt::Byte(i8::from(finalized.is_some_and(|f| f.left_handed))),
    );
    if mob.is_raider() {
        // `PatrollingMonster` then `Raider`.
        m.insert("PatrolLeader".into(), Nbt::Byte(0));
        m.insert("Patrolling".into(), Nbt::Byte(0));
        m.insert("Wave".into(), Nbt::Int(0));
        m.insert("CanJoinRaid".into(), Nbt::Byte(1));
    }
    match mob {
        Mob::Vindicator => {
            m.insert(
                "equipment".into(),
                Nbt::Compound(BTreeMap::from([(
                    "mainhand".to_string(),
                    Nbt::Compound(BTreeMap::from([
                        ("count".to_string(), Nbt::Int(1)),
                        ("id".to_string(), Nbt::String("minecraft:iron_axe".into())),
                    ])),
                )])),
            );
        }
        Mob::Evoker => {
            m.insert("SpellTicks".into(), Nbt::Int(0));
        }
        Mob::Allay => {
            m.insert("DuplicationCooldown".into(), Nbt::Long(0));
            m.insert("Inventory".into(), Nbt::List(Vec::new()));
            m.insert(
                "listener".into(),
                Nbt::Compound(BTreeMap::from([
                    ("event_delay".to_string(), Nbt::Int(0)),
                    (
                        "selector".to_string(),
                        Nbt::Compound(BTreeMap::from([("tick".to_string(), Nbt::Long(-1))])),
                    ),
                ])),
            );
        }
        Mob::Shulker => {
            m.insert("AttachFace".into(), Nbt::Byte(0));
            m.insert("Peek".into(), Nbt::Byte(0));
            m.insert("Color".into(), Nbt::Byte(16));
        }
        Mob::Cat => {
            // `AgeableMob`, `Animal`, `TamableAnimal`, `Cat`.
            m.insert("Age".into(), Nbt::Int(0));
            m.insert("ForcedAge".into(), Nbt::Int(0));
            m.insert("AgeLocked".into(), Nbt::Byte(0));
            m.insert("InLove".into(), Nbt::Int(0));
            m.insert("Sitting".into(), Nbt::Byte(0));
            m.insert("CollarColor".into(), Nbt::Byte(14));
            m.insert("variant".into(), Nbt::String("minecraft:all_black".into()));
            if let Some(sound) = finalized.and_then(|f| f.cat_sound) {
                m.insert("sound_variant".into(), Nbt::String(sound.into()));
            }
        }
        Mob::Drowned => {
            let roll = finalized.and_then(|f| f.drowned);
            m.insert(
                "IsBaby".into(),
                Nbt::Byte(i8::from(roll.is_some_and(|r| r.baby))),
            );
            m.insert("CanBreakDoors".into(), Nbt::Byte(0));
            m.insert("DrownedConversionTime".into(), Nbt::Int(-1));
            m.insert("InWaterTime".into(), Nbt::Int(-1));
            let mut equipment = BTreeMap::new();
            let stack = |id: &str| {
                Nbt::Compound(BTreeMap::from([
                    ("count".to_string(), Nbt::Int(1)),
                    ("id".to_string(), Nbt::String(id.into())),
                ]))
            };
            if let Some(item) = roll.and_then(|r| r.main_hand) {
                equipment.insert("mainhand".to_string(), stack(item));
            }
            if roll.is_some_and(|r| r.nautilus_shell) {
                equipment.insert("offhand".to_string(), stack("minecraft:nautilus_shell"));
                m.insert(
                    "drop_chances".into(),
                    Nbt::Compound(BTreeMap::from([("offhand".to_string(), Nbt::Float(2.0))])),
                );
            }
            if !equipment.is_empty() {
                m.insert("equipment".into(), Nbt::Compound(equipment));
            }
            // `Zombie.handleAttributes` draws these from the entity's own
            // unseeded random; the two vanilla runs disagree on them. Written
            // at their base, with no bonus, and not claimed.
            if let Some(Nbt::List(attributes)) = m.get_mut("attributes") {
                attributes.push(Nbt::Compound(BTreeMap::from([
                    ("base".to_string(), Nbt::Double(0.230_000_004_172_325_13)),
                    (
                        "id".to_string(),
                        Nbt::String("minecraft:movement_speed".into()),
                    ),
                ])));
                attributes.push(Nbt::Compound(BTreeMap::from([
                    ("base".to_string(), Nbt::Double(0.0)),
                    (
                        "id".to_string(),
                        Nbt::String("minecraft:spawn_reinforcements".into()),
                    ),
                ])));
            }
        }
        Mob::ElderGuardian | Mob::Witch => {}
    }
    Nbt::Compound(m)
}

/// The end city ship's `new ItemFrame(level, pos, facing)` holding an
/// elytra. `facing` is `rotation.rotate(SOUTH)`'s 3D data value (down 0, up
/// 1, north 2, south 3, west 4, east 5); the frame hangs 1/32 off the block
/// face it is attached to.
#[must_use]
pub fn elytra_frame(block: (i32, i32, i32), facing: u8, uuid: [i32; 4]) -> Nbt {
    let (dx, dz, yaw) = match facing {
        2 => (0.0, -1.0, 180.0),
        4 => (-1.0, 0.0, 90.0),
        5 => (1.0, 0.0, 270.0),
        _ => (0.0, 1.0, 0.0),
    };
    // `HangingEntity.recalculateBoundingBox`: centre minus 0.46875 along the
    // facing.
    let pos = [
        f64::from(block.0) + 0.5 - dx * 0.468_75,
        f64::from(block.1) + 0.5,
        f64::from(block.2) + 0.5 - dz * 0.468_75,
    ];
    let mut m = BTreeMap::new();
    entity_base(&mut m, "minecraft:item_frame", pos, yaw, uuid);
    m.insert(
        "Facing".into(),
        Nbt::Byte(i8::try_from(facing).unwrap_or(3)),
    );
    m.insert(
        "block_pos".into(),
        Nbt::IntArray(vec![block.0, block.1, block.2]),
    );
    m.insert("Invisible".into(), Nbt::Byte(0));
    m.insert("Fixed".into(), Nbt::Byte(0));
    m.insert("ItemRotation".into(), Nbt::Byte(0));
    m.insert("ItemDropChance".into(), Nbt::Float(1.0));
    m.insert(
        "Item".into(),
        Nbt::Compound(BTreeMap::from([
            ("count".to_string(), Nbt::Int(1)),
            ("id".to_string(), Nbt::String("minecraft:elytra".into())),
        ])),
    );
    Nbt::Compound(m)
}

/// `EndSpikeFeature.placeSpike`'s crystal: `snapTo(cx + 0.5, height + 1,
/// cz + 0.5, yaw, 0)`, `setInvulnerable(false)` (the config's default) and no
/// beam target. `ShowBottom` is the entity's default, true. Key set and types
/// read off vanilla's saved compounds in `van-end-*-c0_0-ent-*/entities`.
#[must_use]
pub fn end_crystal(block: (i32, i32, i32), yaw: f32, uuid: [i32; 4]) -> Nbt {
    let pos = [
        f64::from(block.0) + 0.5,
        f64::from(block.1),
        f64::from(block.2) + 0.5,
    ];
    let mut m = BTreeMap::new();
    entity_base(&mut m, "minecraft:end_crystal", pos, yaw, uuid);
    m.insert("ShowBottom".into(), Nbt::Byte(1));
    Nbt::Compound(m)
}

/// `MineShaftCorridor.createChest`'s `MinecartChest`: `setInitialPos(x +
/// 0.5, y + 0.5, z + 0.5)` and `setLootTable(table, random.nextLong())`. The
/// remaining keys are constant over all 18 chest minecarts in three of this
/// lane's harvests, `HasTicked:1b` included.
#[must_use]
pub fn chest_minecart(block: (i32, i32, i32), loot_table: &str, seed: i64, uuid: [i32; 4]) -> Nbt {
    let mut m = BTreeMap::new();
    let pos = [
        f64::from(block.0) + 0.5,
        f64::from(block.1) + 0.5,
        f64::from(block.2) + 0.5,
    ];
    entity_base(&mut m, "minecraft:chest_minecart", pos, 0.0, uuid);
    m.insert("FlippedRotation".into(), Nbt::Byte(0));
    m.insert("HasTicked".into(), Nbt::Byte(1));
    m.insert("LootTable".into(), Nbt::String(loot_table.into()));
    m.insert("LootTableSeed".into(), Nbt::Long(seed));
    Nbt::Compound(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `triangle` is two `nextDouble`s, subtracted in order, and a
    /// vindicator spends one more float than an evoker.
    #[test]
    fn finalize_spawn_draws_in_vanillas_order() {
        let mut a = crate::rng::legacy::LegacyRandomSource::new(9);
        let mut b = crate::rng::legacy::LegacyRandomSource::new(9);
        let f = finalize_spawn(Mob::Vindicator, &mut a).expect("finalized");
        let (x, y) = (b.next_f64(), b.next_f64());
        assert_eq!(f.follow_range_bonus, 0.114_850_000_000_000_01 * (x - y));
        assert_eq!(f.left_handed, b.next_f32() < 0.05);
        let _ = b.next_f32();
        assert_eq!(
            a.next_i32(),
            b.next_i32(),
            "both streams are at the same place"
        );
        assert!(finalize_spawn(Mob::Shulker, &mut a).is_none());
    }

    /// A south-facing frame on (1306,119,-28) is where vanilla saved the
    /// ship's: `Pos` (1306.5, 119.5, -27.96875), `Facing` 3.
    #[test]
    fn the_ship_frame_hangs_where_vanillas_does() {
        let n = elytra_frame((1306, 119, -28), 3, [0; 4]);
        assert_eq!(
            n.get("Pos"),
            Some(&Nbt::List(vec![
                Nbt::Double(1306.5),
                Nbt::Double(119.5),
                Nbt::Double(-27.968_75)
            ]))
        );
        assert_eq!(n.get("Facing"), Some(&Nbt::Byte(3)));
    }
}
