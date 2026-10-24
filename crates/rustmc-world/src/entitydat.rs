//! One entity's compound inside `entities/r.X.Z.mca`'s `Entities` list.
//!
//! Reference (26.2), read from the jar with `javap -p -c -constants`:
//! `Entity.saveWithoutId`, `LivingEntity.addAdditionalSaveData`,
//! `Mob.addAdditionalSaveData`.
//!
//! [`crate::entitystore`] is the container and keeps every entity as raw NBT,
//! deliberately: "what an entity's fields mean is the server's business". This
//! is that business, in the same crate for the reason `playerdat` is —
//! `players/data/<uuid>.dat` is an entity compound too, and the two would
//! otherwise be two transcriptions of one method.
//!
//! # The shape, and the one thing that decides whether a file is loadable
//!
//! `saveWithoutId` writes twelve keys unconditionally and seven only when
//! they are away from their default. **The conditional seven are the whole
//! difficulty**, because a writer that emits them always produces a file
//! vanilla reads perfectly and that is not byte-identical to vanilla's, and a
//! golden that only round-trips through our own reader would never notice.
//!
//! ```text
//! Entity.saveWithoutId(ValueOutput), offsets 0..494
//!
//!   ALWAYS
//!     Pos            List<Double>[3]   Vec3.CODEC. The RIDDEN entity's
//!                                      position when this entity has a
//!                                      vehicle (offsets 0..44), its own
//!                                      otherwise (48..63).
//!     Motion         List<Double>[3]   Vec3.CODEC
//!     Rotation       List<Float>[2]    Vec2.CODEC — yaw then pitch
//!     fall_distance  Double            putDouble. A Double since 26.x.
//!     Fire           Short             putShort of an int field
//!     Air            Short             putShort of getAirSupply()
//!     OnGround       Boolean
//!     Invulnerable   Boolean
//!     PortalCooldown Int
//!     UUID           IntArray[4]       UUIDUtil.CODEC
//!     CustomName     storeNullable     absent when there is no custom name
//!
//!   ONLY WHEN SET — each behind an `ifeq` that skips the write
//!     CustomNameVisible  Boolean   222: ifeq 238
//!     Silent             Boolean   242: ifeq 258
//!     NoGravity          Boolean   262: ifeq 278
//!     Glowing            Boolean   282: ifeq 295
//!     TicksFrozen        Int       (getTicksFrozen() > 0)
//!     HasVisualFire      Boolean   321: ifeq 337
//!     Tags               List      346: ifne 368 — written when NOT empty
//!     data               Compound  375: ifne 394 — CustomData, same
//!     Passengers         List      403: ifeq 494, and discarded again at 482
//!                                  if every child declined to save
//!
//! LivingEntity.addAdditionalSaveData
//!     Health                                     Float
//!     HurtTime, DeathTime                        Short
//!     AbsorptionAmount                           Float
//!     current_impulse_context_reset_grace_time   Int
//!     current_explosion_impact_pos               storeNullable
//!     attributes                                 List<Compound>
//!     active_effects                             List, only when non-empty
//!     FallFlying                                 Boolean
//!     Brain                                      Compound
//!     last_hurt_by_player, last_hurt_by_player_memory_time
//!     last_hurt_by_mob,    ticks_since_last_hurt_by_mob
//!     equipment, locator_bar_icon
//!
//! Mob.addAdditionalSaveData
//!     CanPickUpLoot, PersistenceRequired, LeftHanded, NoAI   Boolean
//!     drop_chances, home_pos                                 store
//!     home_radius                                            Int
//!     DeathLootTableSeed                                     Long
//! ```
//!
//! # What is modelled and what is carried
//!
//! [`EntityData`] models the keys a server has to *read or write* to put an
//! entity in a world and take it out again: the identity, where it is, which
//! way it is facing, and how much health it has. Everything else — attributes,
//! `Brain`, `drop_chances`, `equipment`, the per-species keys like a pig's
//! `variant` and `sound_variant` — is carried verbatim in [`EntityData::extra`].
//!
//! **That is not laziness, it is what makes byte-identity reachable at all.**
//! A pig vanilla wrote carries 28 keys; this server has no equipment, no drop
//! chances and no brain, and a model that invented values for them would write
//! a file that differs from vanilla's in exactly the fields nothing here can
//! compute. The passthrough is the same device `ChunkColumn::extra` and
//! `PlayerData::extra` already use, and it is the reason all three round-trip
//! byte-for-byte.
//!
//! # Brain memories are REFUSED by name — and the first claim about them here
//! was wrong, which is why the refusal is now a ledger rather than a sentence
//!
//! `Brain.serializeStart` writes `memories` as a map of memory-module id to a
//! `{value, ttl}` pair, and a memory's value is a different shape per module —
//! a position, an entity uuid, a list of them, a boolean, an int. Reproducing
//! that needs the memory-module registry and the per-module codecs, neither of
//! which this server has. **That refusal stands.**
//!
//! What this file said until the test below first ran was that every `Brain`
//! in every harvested world is `{memories: {}}`. It is not. Measured over
//! every entity in every harvested vanilla world on this box — 438 entities,
//! 11 species, 181 of them carrying a `Brain` — **two `minecraft:zombie_nautilus`
//! carry a live `minecraft:attack_target_cooldown` memory** and the other 179
//! brains are empty. A claim written from reading one pig, refuted by the
//! first sweep, exactly as `docs/AGENT-BRIEF.md` says to expect.
//!
//! The distinction that survives, and it is the one that matters:
//!
//! * **carrying** a memory is byte-exact and needs no understanding of it.
//!   `every_real_vanilla_entity_re_encodes_byte_for_byte` passes on those two
//!   zombie nautiluses like everything else, because `extra` holds the `Brain`
//!   compound verbatim and hands it back unchanged.
//! * **acting on** one — reading a memory's value, expiring its ttl, writing a
//!   memory this server decided to record — is what is refused, and nothing
//!   here does it.
//!
//! So the ledger is [`BRAIN_MEMORY_MODULES`], the set the corpus actually
//! contains, and `a_real_vanilla_entity_regions_brains_hold_only_known_memories`
//! refuses a module outside it. A new harvest that produces a villager or a
//! piglin makes that test name the module it found, which is a decision
//! somebody takes rather than a structure the passthrough absorbs silently.
//! [`EntityData::brain_memories_are_empty`] remains the cheap predicate for a
//! caller that wants the common case.

use crate::nbt::Nbt;
use std::collections::BTreeMap;
use std::io;

/// `Entity.getMaxAirSupply()` — `TOTAL_AIR_SUPPLY`, 300 ticks.
pub const MAX_AIR: i16 = 300;

/// `Health`, which is **two different fields of two different types sharing
/// one key name**, and the trap this module's golden caught on its first run.
///
/// ```text
/// LivingEntity.addAdditionalSaveData:  output.putFloat("Health", getHealth())
/// ItemEntity.addAdditionalSaveData:    output.putShort("Health", (short) this.health)
/// ```
///
/// `LivingEntity.health` is a float attribute; `ItemEntity.health` is an
/// `int` field initialised to `DEFAULT_HEALTH = 5` that counts how much
/// damage a dropped stack can take before it despawns. They are unrelated
/// quantities on disjoint class hierarchies, and vanilla gave them the same
/// tag name.
///
/// Measured on this box's harvested worlds: 109 `minecraft:item` entities
/// write it as a Short and 165 living entities across nine species write it
/// as a Float. A model with `health: Option<f32>` parses the mobs and refuses
/// every item — which is what happened, and is the entire argument for this
/// enum existing rather than a comment saying "careful".
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Health {
    /// `LivingEntity`'s, a Float.
    Living(f32),
    /// `ItemEntity`'s, a Short. `ItemEntity.DEFAULT_HEALTH` is 5.
    Item(i16),
}

/// Every `Brain.memories` module id present in the harvested vanilla worlds
/// on this box, measured 2026-09-05 over 438 entities in 27 worlds.
///
/// A ledger and not a capability list: this crate carries a memory verbatim
/// and interprets none of them. It exists so that a module NOBODY has seen
/// arriving in a later harvest is a red test naming that module, rather than
/// a compound the passthrough absorbs while the docs still say memories are
/// refused.
pub const BRAIN_MEMORY_MODULES: [&str; 1] = ["minecraft:attack_target_cooldown"];

/// One entity's save compound.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityData {
    /// `id` — the entity type's registry name. Written by
    /// `EntityType.create`'s caller rather than by `saveWithoutId`, which is
    /// why it is not in the table above; `save()` adds it and
    /// `saveWithoutId` deliberately does not.
    pub id: String,
    /// `UUID` — four ints, most significant first, `UUIDUtil.uuidToIntArray`.
    pub uuid: [i32; 4],
    /// `Pos` — three doubles.
    pub pos: [f64; 3],
    /// `Motion` — three doubles.
    pub motion: [f64; 3],
    /// `Rotation` — yaw then pitch.
    pub yaw: f32,
    pub pitch: f32,
    /// `OnGround`.
    pub on_ground: bool,
    /// `fall_distance`. A Double since 26.x, and it is the field most likely
    /// to be transcribed as a Float from memory of an older version — which
    /// produces a tag vanilla reads as the wrong type and silently defaults.
    pub fall_distance: f64,
    /// `Air`, in ticks. Vanilla writes an int field through `putShort`.
    pub air: i16,
    /// `Fire` — negative is the fire-immunity countdown, not burning.
    pub fire: i16,
    /// `Invulnerable`.
    pub invulnerable: bool,
    /// `PortalCooldown`.
    pub portal_cooldown: i32,
    /// `Health` — and **it is not one field, it is two with the same name.**
    ///
    /// See [`Health`]. `None` is an entity that writes no `Health` at all,
    /// which is every non-living entity that is not an item — a chest
    /// minecart, for instance.
    pub health: Option<Health>,
    /// Every key this does not model, verbatim, including `Brain`,
    /// `attributes`, `equipment` and everything species-specific. See the
    /// module doc.
    pub extra: BTreeMap<String, Nbt>,
}

/// Keys [`EntityData`] has a field for. Everything else goes to `extra`.
///
/// A list rather than a match arm per key so the two directions cannot
/// disagree: `from_nbt` removes exactly these and `to_nbt` puts exactly these
/// back. A key added to the struct and not to this list would be written
/// twice — once from the field and once out of `extra` — and the second write
/// would win.
/// `CustomName` is deliberately NOT in this list. It is `storeNullable`, so
/// its absence is the default, and a model that carried it as an
/// `Option<String>` would have to reproduce `ComponentSerialization.CODEC` to
/// write it back — a chat component, not a string. The passthrough carries it
/// unchanged instead, which is byte-exact and costs this crate no component
/// model.
const MODELLED: [&str; 12] = [
    "id",
    "UUID",
    "Pos",
    "Motion",
    "Rotation",
    "OnGround",
    "fall_distance",
    "Air",
    "Fire",
    "Invulnerable",
    "PortalCooldown",
    "Health",
];

impl EntityData {
    /// The compound as it sits in the region file's `Entities` list.
    #[must_use]
    pub fn to_nbt(&self) -> Nbt {
        let mut m = self.extra.clone();
        let doubles = |v: &[f64]| Nbt::List(v.iter().map(|d| Nbt::Double(*d)).collect());
        m.insert("id".into(), Nbt::String(self.id.clone()));
        m.insert("UUID".into(), Nbt::IntArray(self.uuid.to_vec()));
        m.insert("Pos".into(), doubles(&self.pos));
        m.insert("Motion".into(), doubles(&self.motion));
        m.insert(
            "Rotation".into(),
            Nbt::List(vec![Nbt::Float(self.yaw), Nbt::Float(self.pitch)]),
        );
        m.insert("OnGround".into(), Nbt::Byte(i8::from(self.on_ground)));
        m.insert("fall_distance".into(), Nbt::Double(self.fall_distance));
        m.insert("Air".into(), Nbt::Short(self.air));
        m.insert("Fire".into(), Nbt::Short(self.fire));
        m.insert(
            "Invulnerable".into(),
            Nbt::Byte(i8::from(self.invulnerable)),
        );
        m.insert("PortalCooldown".into(), Nbt::Int(self.portal_cooldown));
        // Absent, not zero, when there is none — and the tag TYPE comes from
        // the variant, not from a guess. See [`Health`].
        match self.health {
            None => {}
            Some(Health::Living(v)) => {
                m.insert("Health".into(), Nbt::Float(v));
            }
            Some(Health::Item(v)) => {
                m.insert("Health".into(), Nbt::Short(v));
            }
        }
        Nbt::Compound(m)
    }

    /// Read one entity out of an `Entities` list.
    ///
    /// # Errors
    /// If the compound is not one, has no `id`, or a modelled key carries the
    /// wrong tag. A modelled key that is ABSENT is not an error — vanilla
    /// omits seven of them at their defaults — and the default is taken.
    pub fn from_nbt(root: &Nbt) -> io::Result<Self> {
        let map = root
            .as_compound()
            .ok_or_else(|| io::Error::other("entity is not a compound"))?;
        let wrong = |k: &str| io::Error::other(format!("entity: `{k}` has the wrong type"));
        let Some(Nbt::String(id)) = map.get("id") else {
            return Err(io::Error::other(
                "entity has no string `id` — `save()` writes it and \
                 `saveWithoutId` does not, so a compound without one came \
                 from the wrong side of that pair",
            ));
        };
        macro_rules! scalar {
            ($key:literal, $variant:path, $default:expr) => {
                match map.get($key) {
                    None => $default,
                    Some($variant(v)) => *v,
                    Some(_) => return Err(wrong($key)),
                }
            };
        }
        let triple = |key: &str| -> io::Result<[f64; 3]> {
            let Some(tag) = map.get(key) else {
                return Ok([0.0; 3]);
            };
            let Some(items) = tag.as_list() else {
                return Err(wrong(key));
            };
            if items.len() != 3 {
                return Err(wrong(key));
            }
            let mut out = [0.0; 3];
            for (slot, item) in out.iter_mut().zip(items) {
                let Nbt::Double(v) = item else {
                    return Err(wrong(key));
                };
                *slot = *v;
            }
            Ok(out)
        };
        let (yaw, pitch) = match map.get("Rotation") {
            None => (0.0, 0.0),
            Some(Nbt::List(items)) => match items.as_slice() {
                [Nbt::Float(y), Nbt::Float(p)] => (*y, *p),
                _ => return Err(wrong("Rotation")),
            },
            Some(_) => return Err(wrong("Rotation")),
        };
        let uuid = match map.get("UUID") {
            None => [0; 4],
            Some(Nbt::IntArray(v)) if v.len() == 4 => [v[0], v[1], v[2], v[3]],
            Some(_) => return Err(wrong("UUID")),
        };
        let health = match map.get("Health") {
            None => None,
            Some(Nbt::Float(v)) => Some(Health::Living(*v)),
            Some(Nbt::Short(v)) => Some(Health::Item(*v)),
            Some(_) => return Err(wrong("Health")),
        };
        let mut extra = map.clone();
        for key in MODELLED {
            extra.remove(key);
        }
        Ok(Self {
            id: id.clone(),
            uuid,
            pos: triple("Pos")?,
            motion: triple("Motion")?,
            yaw,
            pitch,
            on_ground: scalar!("OnGround", Nbt::Byte, 0) != 0,
            fall_distance: scalar!("fall_distance", Nbt::Double, 0.0),
            air: scalar!("Air", Nbt::Short, MAX_AIR),
            fire: scalar!("Fire", Nbt::Short, 0),
            invulnerable: scalar!("Invulnerable", Nbt::Byte, 0) != 0,
            portal_cooldown: scalar!("PortalCooldown", Nbt::Int, 0),
            health,
            extra,
        })
    }

    /// Is this entity's `Brain` one this crate can carry without
    /// understanding it?
    ///
    /// `true` when there is no `Brain` at all, or when its `memories` map is
    /// empty — which is every entity in every harvested vanilla world on this
    /// box. `false` is the case the module doc refuses by name: a memory's
    /// value is a different shape per memory module, and reproducing it needs
    /// a registry this crate does not have.
    ///
    /// It exists so the refusal is **checkable** rather than a paragraph. A
    /// refusal nothing can test is indistinguishable from an oversight.
    #[must_use]
    pub fn brain_memories_are_empty(&self) -> bool {
        let Some(brain) = self.extra.get("Brain") else {
            return true;
        };
        match brain.get("memories") {
            None => true,
            Some(Nbt::Compound(m)) => m.is_empty(),
            Some(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pig() -> Nbt {
        let mut m = BTreeMap::new();
        m.insert("id".into(), Nbt::String("minecraft:pig".into()));
        m.insert("UUID".into(), Nbt::IntArray(vec![1, 2, 3, 4]));
        m.insert(
            "Pos".into(),
            Nbt::List(vec![
                Nbt::Double(888.0),
                Nbt::Double(63.0),
                Nbt::Double(1125.0),
            ]),
        );
        m.insert(
            "Motion".into(),
            Nbt::List(vec![Nbt::Double(0.0), Nbt::Double(0.0), Nbt::Double(0.0)]),
        );
        m.insert(
            "Rotation".into(),
            Nbt::List(vec![Nbt::Float(335.322_08), Nbt::Float(0.0)]),
        );
        m.insert("OnGround".into(), Nbt::Byte(0));
        m.insert("fall_distance".into(), Nbt::Double(0.0));
        m.insert("Air".into(), Nbt::Short(300));
        m.insert("Fire".into(), Nbt::Short(0));
        m.insert("Invulnerable".into(), Nbt::Byte(0));
        m.insert("PortalCooldown".into(), Nbt::Int(0));
        m.insert("Health".into(), Nbt::Float(10.0));
        m.insert("variant".into(), Nbt::String("minecraft:warm".into()));
        let mut brain = BTreeMap::new();
        brain.insert("memories".into(), Nbt::Compound(BTreeMap::new()));
        m.insert("Brain".into(), Nbt::Compound(brain));
        Nbt::Compound(m)
    }

    /// The round trip, and the two species-specific keys surviving it.
    ///
    /// The `variant` assertion is the one that matters: it is a key this
    /// crate has never heard of, and a model that dropped unknown keys would
    /// pass every other assertion here and write a pig vanilla loads as the
    /// wrong colour.
    #[test]
    fn a_pig_round_trips_and_keeps_the_keys_this_crate_does_not_model() {
        let original = pig();
        let parsed = EntityData::from_nbt(&original).expect("parses");
        assert_eq!(parsed.id, "minecraft:pig");
        assert_eq!(parsed.health, Some(Health::Living(10.0)));
        assert_eq!(parsed.air, 300);
        assert!(parsed.extra.contains_key("variant"));
        assert!(parsed.extra.contains_key("Brain"));
        assert!(
            !parsed.extra.contains_key("Pos"),
            "a modelled key stayed in extra"
        );
        assert_eq!(
            parsed.to_nbt(),
            original,
            "the round trip is not the identity"
        );
    }

    /// An entity that writes no `Health` must not gain one.
    ///
    /// The failure this pins is a plausible one: `health: f32` with a default
    /// of 0.0 round-trips every mob correctly and adds a key to every chest
    /// minecart in the world.
    #[test]
    fn an_entity_with_no_health_does_not_gain_one() {
        let mut m = BTreeMap::new();
        m.insert("id".into(), Nbt::String("minecraft:chest_minecart".into()));
        let original = Nbt::Compound(m);
        let parsed = EntityData::from_nbt(&original).expect("parses");
        assert_eq!(parsed.health, None);
        let back = parsed.to_nbt();
        assert!(
            back.get("Health").is_none(),
            "an item entity must not carry a Health tag"
        );
        // Everything else is a modelled key at its default, which vanilla
        // writes unconditionally — so the re-encode is LARGER than the input
        // here and that is correct. Asserted so the asymmetry is deliberate
        // rather than discovered.
        assert!(back.get("Pos").is_some(), "Pos is written unconditionally");
    }

    /// **`Health` keeps the tag type it arrived with**, which is the defect
    /// `every_real_vanilla_entity_re_encodes_byte_for_byte` found on its
    /// first run against 438 real entities.
    ///
    /// A dropped stack writes `Health` as a Short and a mob writes it as a
    /// Float, from two unrelated fields that happen to share a name. Reading
    /// both into an `f32` refuses the item; reading both into an `f32` and
    /// writing a Float back would corrupt it. Both directions are pinned
    /// here, on a compound of each shape, so a later simplification that
    /// collapses the enum fails immediately rather than at the next harvest.
    #[test]
    fn an_items_short_health_is_not_a_mobs_float_health() {
        let of = |id: &str, health: Nbt| {
            let mut m = BTreeMap::new();
            m.insert("id".into(), Nbt::String(id.into()));
            m.insert("Health".into(), health);
            Nbt::Compound(m)
        };
        let item = EntityData::from_nbt(&of("minecraft:item", Nbt::Short(5))).expect("item parses");
        assert_eq!(item.health, Some(Health::Item(5)));
        assert_eq!(
            item.to_nbt().get("Health"),
            Some(&Nbt::Short(5)),
            "an item's Health must go back out as a Short"
        );

        let pig = EntityData::from_nbt(&of("minecraft:pig", Nbt::Float(10.0))).expect("pig parses");
        assert_eq!(pig.health, Some(Health::Living(10.0)));
        assert_eq!(
            pig.to_nbt().get("Health"),
            Some(&Nbt::Float(10.0)),
            "a mob's Health must go back out as a Float"
        );

        // A third tag type is still an error: the two above are the only two
        // vanilla writes, and accepting an Int would be leniency this crate's
        // whole point is to refuse.
        assert!(EntityData::from_nbt(&of("minecraft:pig", Nbt::Int(10))).is_err());
    }

    /// `from_nbt` refuses a compound with no `id` rather than inventing one.
    ///
    /// `saveWithoutId` really does not write `id` — `save()` adds it — so a
    /// compound without one is a compound taken from the wrong side of that
    /// pair, and defaulting it to `minecraft:pig` or to an empty string would
    /// put an entity vanilla cannot construct into a region file.
    #[test]
    fn an_entity_with_no_id_is_refused() {
        let mut m = BTreeMap::new();
        m.insert("Health".into(), Nbt::Float(10.0));
        assert!(EntityData::from_nbt(&Nbt::Compound(m)).is_err());
    }

    /// The brain refusal is checkable in both directions.
    #[test]
    fn a_brain_with_a_memory_in_it_is_not_carryable() {
        let parsed = EntityData::from_nbt(&pig()).expect("parses");
        assert!(parsed.brain_memories_are_empty());

        let mut with_memory = parsed.clone();
        let mut memories = BTreeMap::new();
        memories.insert("minecraft:home".into(), Nbt::Compound(BTreeMap::new()));
        let mut brain = BTreeMap::new();
        brain.insert("memories".into(), Nbt::Compound(memories));
        with_memory
            .extra
            .insert("Brain".into(), Nbt::Compound(brain));
        assert!(
            !with_memory.brain_memories_are_empty(),
            "a non-empty memory map must be refused by name, not carried silently"
        );
    }

    /// Every real vanilla entity on this box, decoded and re-encoded
    /// **byte-identically**.
    ///
    /// This is the golden the module exists for, and it is a claim about the
    /// compound rather than about our reader: `encode_named_root` is run over
    /// vanilla's own parsed tag and over ours, and the two byte strings are
    /// compared. A lenient reader that dropped a key it did not understand,
    /// or a writer that added `Health` to an item entity, or one that wrote
    /// `fall_distance` as a Float, all fail here and all pass a round trip
    /// through our own reader.
    ///
    /// # Three population assertions, and why each one is inside the test
    ///
    /// A fixture-driven test that finds nothing passes. `docs/cold-start/`
    /// records that the player golden spent two hours comparing this server's
    /// output to itself and that its own `extra.len() >= 10` assertion is what
    /// caught it. So:
    ///
    /// * **at least one pig**, by name. The brief this module was written
    ///   under asks for a pig specifically, and "438 entities round-tripped"
    ///   is satisfied by 438 item entities, which carry none of the
    ///   `LivingEntity` or `Mob` keys the transcription above is about.
    /// * **at least five species**, so the coverage is not one shape repeated.
    /// * **an entity whose `extra` is large**, which is what says the
    ///   passthrough is carrying real unmodelled keys rather than being empty
    ///   because the fixture is trivial.
    #[test]
    fn every_real_vanilla_entity_re_encodes_byte_for_byte() {
        let probe = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/recipe-dump/world/dimensions/minecraft/overworld/entities"
        ));
        if !rustmc_data::fixture::available(
            probe,
            "a vanilla entity region",
            "ci/gen-gameplay-tables.sh",
        ) {
            return;
        }
        let mut seen = 0usize;
        let mut species = std::collections::BTreeSet::new();
        let mut widest_extra = 0usize;
        for path in vanilla_entity_regions() {
            for record in crate::anvil::read_region_records(&path)
                .expect("records")
                .values()
            {
                let root = crate::anvil::decode_region_record(record).expect("decode");
                let chunk = crate::entitystore::EntityChunk::from_nbt(&root).expect("parse");
                for entity in &chunk.entities {
                    let parsed = EntityData::from_nbt(entity).unwrap_or_else(|e| {
                        panic!("{}: {e}", path.display());
                    });
                    let want =
                        crate::nbt::encode_named_root("", entity).expect("re-encode vanilla's");
                    let ours = crate::nbt::encode_named_root("", &parsed.to_nbt()).expect("ours");
                    assert_eq!(
                        ours,
                        want,
                        "{} — {} did not re-encode byte-identically",
                        path.display(),
                        parsed.id
                    );
                    seen += 1;
                    species.insert(parsed.id.clone());
                    widest_extra = widest_extra.max(parsed.extra.len());
                }
            }
        }
        assert!(seen > 0, "no entities in any harvested vanilla world");
        assert!(
            species.contains("minecraft:pig"),
            "no pig among {seen} entities of {} species: {species:?}. \
             A golden over item entities alone exercises none of the \
             LivingEntity or Mob keys this module transcribes.",
            species.len()
        );
        assert!(
            species.len() >= 5,
            "only {} species among {seen} entities: {species:?}",
            species.len()
        );
        assert!(
            widest_extra >= 10,
            "the widest `extra` over {seen} entities holds {widest_extra} keys. \
             The passthrough is what makes byte-identity reachable, and a \
             fixture where nothing lands in it is a fixture that does not \
             test it."
        );
    }

    /// **Every `Brain.memories` module in the harvested worlds is one this
    /// file has already looked at**, which is what the module doc's refusal
    /// now rests on.
    ///
    /// This test replaced one that asserted every brain was EMPTY, and that
    /// assertion was false on its first run: two `minecraft:zombie_nautilus`
    /// carry `minecraft:attack_target_cooldown`. The claim had been written
    /// from reading a single pig.
    ///
    /// What is asserted now is the thing that can stay true: the set of
    /// modules present is [`BRAIN_MEMORY_MODULES`], and a module outside it
    /// is a red naming the species and the module. Carrying a memory is
    /// byte-exact and is proved by the golden above; ACTING on one is what is
    /// refused, and nothing in this crate does.
    #[test]
    fn a_real_vanilla_entity_regions_brains_hold_only_known_memories() {
        let probe = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/recipe-dump/world/dimensions/minecraft/overworld/entities"
        ));
        if !rustmc_data::fixture::available(
            probe,
            "a vanilla entity region",
            "ci/gen-gameplay-tables.sh",
        ) {
            return;
        }
        let mut with_brain = 0usize;
        let mut with_memories = 0usize;
        let mut unknown = Vec::new();
        let mut modules = std::collections::BTreeSet::new();
        for path in vanilla_entity_regions() {
            for record in crate::anvil::read_region_records(&path)
                .expect("records")
                .values()
            {
                let root = crate::anvil::decode_region_record(record).expect("decode");
                let chunk = crate::entitystore::EntityChunk::from_nbt(&root).expect("parse");
                for entity in &chunk.entities {
                    let parsed = EntityData::from_nbt(entity).expect("parse");
                    let Some(brain) = parsed.extra.get("Brain") else {
                        continue;
                    };
                    with_brain += 1;
                    let Some(Nbt::Compound(mem)) = brain.get("memories") else {
                        continue;
                    };
                    if mem.is_empty() {
                        continue;
                    }
                    with_memories += 1;
                    for module in mem.keys() {
                        modules.insert(module.clone());
                        if !BRAIN_MEMORY_MODULES.contains(&module.as_str()) {
                            unknown.push(format!("{} {module}", parsed.id));
                        }
                    }
                }
            }
        }
        // Population, in both directions. Without the first this passes on a
        // corpus with no `Brain` at all; without the second it passes on a
        // corpus where every brain is empty, which is what the previous
        // version of this test believed and which would make the ledger above
        // an assertion about nothing.
        assert!(
            with_brain > 0,
            "no entity in any harvested world carries a `Brain`"
        );
        assert!(
            with_memories > 0,
            "{with_brain} entities carry a `Brain` and not one holds a memory. \
             BRAIN_MEMORY_MODULES would then be a ledger of things this corpus \
             cannot show, which is the shape of a check that cannot fail."
        );
        assert!(
            unknown.is_empty(),
            "memory module(s) outside BRAIN_MEMORY_MODULES: {unknown:?}. \
             This crate carries a memory verbatim and interprets none, so \
             nothing is BROKEN — but the ledger in `crate::entitydat` is now \
             incomplete, and a module nobody has looked at is a decision \
             somebody takes rather than one the passthrough absorbs."
        );
        // The ledger must not be WIDER than the corpus either. A module listed
        // and never seen is a claim the fixtures cannot support, and it is how
        // this file's previous claim survived being wrong.
        for module in BRAIN_MEMORY_MODULES {
            assert!(
                modules.contains(module),
                "BRAIN_MEMORY_MODULES lists `{module}` and no entity in any \
                 harvested world carries it; the ledger records what was \
                 measured, not what might exist"
            );
        }
    }

    /// The fixture sweep, shared by the two goldens above.
    ///
    /// Deliberately a copy of `crate::entitystore::tests`' function rather
    /// than a shared one: a `#[cfg(test)]` helper cannot be imported across
    /// modules without making it non-test code, and the alternative — moving
    /// it out of `cfg(test)` — would put a filesystem walk into the shipped
    /// crate to save nine lines.
    fn vanilla_entity_regions() -> Vec<std::path::PathBuf> {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../oracle-work"));
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(root) else {
            return out;
        };
        for entry in entries.flatten() {
            for dim in ["overworld", "the_nether", "the_end"] {
                let dir = entry
                    .path()
                    .join(format!("world/dimensions/minecraft/{dim}/entities"));
                let Ok(files) = std::fs::read_dir(&dir) else {
                    continue;
                };
                for file in files.flatten() {
                    let path = file.path();
                    if path.extension().is_some_and(|e| e == "mca")
                        && std::fs::metadata(&path).is_ok_and(|m| m.len() > 8192)
                    {
                        out.push(path);
                    }
                }
            }
        }
        out.sort();
        out
    }
}
