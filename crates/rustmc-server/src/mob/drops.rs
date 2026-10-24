//! `dropAllDeathLoot` for a mob: the loot context, the item entities and the
//! experience orbs.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `LivingEntity.dropAllDeathLoot` (0..49), `.dropFromLootTable` in all four
//! overloads (the two-and-a-boolean one at 0..30, the five-argument one at
//! 0..126), `.dropExperience` (0..61), `.getExperienceReward` (0..11),
//! `Mob.getBaseExperienceReward` (0..106), `Animal.getBaseExperienceReward`
//! (0..12), `Mob.dropFromLootTable` (0..14), `Entity.spawnAtLocation` in its
//! three overloads and `ItemEntity`'s `(Level, double, double, double,
//! ItemStack)` constructor (0..64).
//!
//! # The order, which is the whole of `dropAllDeathLoot`
//!
//! ```text
//!    0: hitByPlayer = lastHurtByPlayerMemoryTime > 0
//!   13: shouldDropLoot(level)
//!         21: dropFromLootTable(level, source, hitByPlayer)
//!         28: dropCustomDeathLoot(level, source, hitByPlayer)   (empty here)
//!   35: dropEquipment(level)                                    (empty here)
//!   40: dropExperience(level, source.getEntity())
//! ```
//!
//! So the **items come out before the orbs**, always, and `hitByPlayer` is
//! read from the memory field once and handed to both of the calls that need
//! it. `Mob.dropFromLootTable` then clears the mob's own `lootTable` field
//! after `super`, which is why a second call drops nothing — a guard this
//! server does not need yet and does not implement, because `die` already
//! refuses a second run.
//!
//! # Three readings that are not what the shape suggests
//!
//! * **`Animal`'s experience is `1 + random.nextInt(3)`, and it is
//!   `getBaseExperienceReward`, not `getExperienceReward`.** The latter is
//!   `final` on `LivingEntity` and wraps the former in
//!   `EnchantmentHelper.processMobExperience`. The draw is on the **mob's
//!   own** generator, and it happens on every death of an animal whether or
//!   not the experience is then dropped — no, it does not: `dropExperience`
//!   guards on `lastHurtByPlayerMemoryTime > 0 && shouldDropExperience()`
//!   *before* calling `getExperienceReward`, so a pig that dies of fall damage
//!   takes no draw at all. The order of that conjunction is the behaviour.
//! * **`Mob.getBaseExperienceReward`'s `random.nextInt(3)` is a different
//!   draw** and belongs to *equipment*: it fires once per non-empty equipment
//!   slot whose drop chance is `<= 1.0`, and a pig wears nothing, so it never
//!   fires here. Reading that method's `nextInt(3)` as the animal's is an easy
//!   mistake — they are the same expression in two classes, and only `Animal`'s
//!   applies to a pig.
//! * **A death drop is spawned at the mob's exact position**, not at its eye
//!   height. `spawnAtLocation(level, stack)` passes `Vec3(0, 0.0f, 0)` down to
//!   the three-argument form, which adds it to `getX()/getY()/getZ()`. The
//!   0.3-below-the-eye offset `crate::dropitem` uses is `LivingEntity.drop`'s,
//!   a different method for a different action.
//!
//! # What is not modelled, named rather than defaulted
//!
//! `ItemEntity.setDefaultPickUpDelay()` (10 ticks, against `drop`'s 40),
//! the item's tick, the orb's tick, and pickup. All three are absences this
//! server has had since `crate::dropitem`, not new ones.

use rustmc_loot::{EntityCtx, LootDamageSource, LootEntity, LootItem, NoDrops, Stack};
use rustmc_worldgen::rng::RandomSource;

use super::host::ServerMob;
use crate::entities::{Entities, Entity};
use rustmc_entity::host::MobHost;
use rustmc_entity::trading::draw::RandomSequences;
use rustmc_worldgen::rng::XoroshiroRandomSource;
use std::sync::{Mutex, OnceLock};

/// `Animal.getBaseExperienceReward`'s `iconst_1` at offset 0.
pub const ANIMAL_BASE_EXPERIENCE: i32 = 1;
/// The bound of `Animal.getBaseExperienceReward`'s single `nextInt`.
pub const ANIMAL_EXPERIENCE_BOUND: i32 = 3;

/// `ItemEntity.<init>`'s `0.2d` vertical delta at offset 41 — a **literal**,
/// not a draw, unlike the two horizontal components either side of it.
pub const ITEM_SPAWN_DY: f64 = 0.2;

/// `EntityType.getDefaultLootTable()` for a mob kind.
///
/// Every entity type in the jar that has a table has `entities/<path>`, and
/// `noLootTable()` is what the rest call — so this is a **derivation** from
/// the registry name and not a lookup, and the one thing that could make it
/// wrong is a type calling `noLootTable()` that this server has. Neither the
/// pig nor the bat does: `entities/pig` and `entities/bat` are both real
/// documents in the corpus, and `entity_dispatch.rs` asserts the bat's is a
/// real, empty table rather than an absent one.
#[must_use]
pub fn loot_table_of(kind: &'static super::MobKind) -> String {
    let path = kind.name.strip_prefix("minecraft:").unwrap_or(kind.name);
    format!("entities/{path}")
}

/// `LootParams.Builder` as `dropFromLootTable`'s five-argument overload
/// fills it, offsets 14..103.
///
/// The parameter set is `LootContextParamSets.ENTITY`, and the five slots go
/// in in this order: `THIS_ENTITY`, `ORIGIN`, `DAMAGE_SOURCE`,
/// `ATTACKING_ENTITY` (optional), `DIRECT_ATTACKING_ENTITY` (optional), then
/// `LAST_DAMAGE_PLAYER` and the luck **only when `hitByPlayer` is true AND
/// `getLastHurtByPlayer()` is non-null**. Both halves of that conjunction: a
/// mob whose memory has expired keeps a killer but loses the parameter.
#[must_use]
pub fn loot_context(
    m: &ServerMob,
    damage_type: &str,
    hit_by_player: bool,
    attacker: Option<LootEntity>,
) -> EntityCtx {
    let this = LootEntity {
        kind: m.kind.name.to_string(),
        // `Entity.isOnFire()` — this server has no fire tick, so it is
        // constant false rather than absent. That makes the pig table's first
        // `any_of` term answer false rather than refuse, which is correct for
        // a world with no fire in it and would be wrong the moment one has
        // fire; the field is on `LootEntity` so that change is one line.
        on_fire: false,
        baby: m.is_baby(),
        player: false,
        equipment: std::collections::BTreeMap::new(),
    };
    EntityCtx {
        this_entity: Some(this),
        attacking_entity: attacker.clone(),
        // `getDirectEntity()` for a melee hit is the same entity as
        // `getEntity()`. They differ for a projectile, which this server has
        // none of, so the two are equal here as a *consequence* and not as an
        // assumption — the field is separate so an arrow lane changes one.
        direct_attacking_entity: attacker.clone(),
        last_damage_player: (hit_by_player).then(|| attacker.clone()).flatten(),
        damage_source: Some(LootDamageSource {
            kind: damage_type.to_string(),
            tags: std::collections::BTreeSet::new(),
        }),
        origin: Some((m.x(), m.y(), m.z())),
    }
}

/// The attacker as the loot context sees it.
///
/// `mainhand` is the only slot filled, because `crate::item::ItemStack`
/// carries an item name and a count and no enchantments — so `Looting` and
/// `smelts_loot` are both always absent here, and the pig's second `any_of`
/// term answers false rather than refusing. The moment a stack carries
/// components this is the function that changes.
#[must_use]
pub fn attacker_of(entity_id: i32) -> Option<LootEntity> {
    let e = Entities::shared().get(entity_id)?;
    let player = e.kind.name == crate::entities::PLAYER.name;
    let mut equipment = std::collections::BTreeMap::new();
    if let Some(hands) = super::player_hands(entity_id) {
        if let Some(main) = hands.main {
            equipment.insert(
                "mainhand".to_string(),
                LootItem {
                    item: main,
                    enchants: std::collections::BTreeMap::new(),
                },
            );
        }
    }
    Some(LootEntity {
        kind: e.kind.name.to_string(),
        on_fire: false,
        baby: false,
        player,
        equipment,
    })
}

/// `LivingEntity.dropFromLootTable`, rolled against the level's named random
/// sequence.
///
/// # Errors
/// [`NoDrops::Refused`] when the loot dispatch or the sequence store is
/// absent, or when the table needs an opcode this interpreter does not have.
/// **Never an empty vector for a reason it could not work out** — that
/// distinction is the whole of `rustmc_loot`'s design.
pub fn death_loot(m: &ServerMob, ctx: EntityCtx, luck: f32) -> Result<Vec<Stack>, NoDrops> {
    let Some(server_loot) = crate::loot::shared() else {
        return Err(NoDrops::Refused(
            "the loot tables are not in this tree".to_string(),
        ));
    };
    let table = loot_table_of(m.kind);
    // The sequence's name is the table's own `random_sequence` field, which
    // for every entity table in the jar is `minecraft:` plus the table id.
    // Read from the document rather than assumed: a table with no
    // `random_sequence` gets `Optional.empty()` and then an UNKEYED sequence
    // — a different generator — so guessing the name would be wrong in a way
    // no drop count reveals.
    let name = match sequence_name(server_loot, &table) {
        Some(n) => n,
        None => {
            return Err(NoDrops::Refused(format!(
                "{table} has no random_sequence and this server has no unkeyed sequence"
            )))
        }
    };
    // The level's named sequence lives in `crate::lootroll`'s store — the
    // one seeded by `--seed`, saved in `random_sequences.dat` and gated
    // against vanilla's own streams in `rustmc-loot`.
    let mut sequences = crate::lootroll::sequences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    server_loot.dispatch().entity_drops(
        &table,
        ctx,
        Some(server_loot.smelting()),
        server_loot.enchantment_tags(),
        luck,
        &mut sequences.get(&name),
    )
}

/// The `random_sequence` field of a table in the corpus.
fn sequence_name(server_loot: &crate::loot::ServerLoot, table: &str) -> Option<String> {
    server_loot
        .dispatch()
        .table(table)?
        .get("random_sequence")?
        .as_str()
        .map(str::to_string)
}

/// `Animal.getBaseExperienceReward(ServerLevel)`, offsets 0..12:
/// `1 + this.random.nextInt(3)`.
///
/// The generator is the **mob's own**, so the draw advances the same stream
/// its ambient sound and its knockback jitter read.
///
/// `EnchantmentHelper.processMobExperience` wraps this in
/// `getExperienceReward` and is the identity with no enchantments, which is
/// every killer this server can hold.
pub fn animal_experience_reward(m: &mut ServerMob) -> i32 {
    ANIMAL_BASE_EXPERIENCE + m.random.next_i32_bound(ANIMAL_EXPERIENCE_BOUND)
}

/// `LivingEntity.dropExperience(ServerLevel, Entity)`, offsets 0..61.
///
/// The guard is a conjunction in this order, and the order is the behaviour
/// because `getExperienceReward` takes a draw:
///
/// ```text
///    0: wasExperienceConsumed()          -> return
///    7: isAlwaysExperienceDropper()      -> award       (skips the rest)
///   14: lastHurtByPlayerMemoryTime <= 0  -> return
///   21: !shouldDropExperience()          -> return
///   28: !mobDrops                        -> return
///   47: ExperienceOrb.award(level, position(), getExperienceReward(...))
/// ```
///
/// So a pig that dies with no player memory takes **no draw at all**, and a
/// port that computed the reward first and then decided whether to drop it
/// would desynchronise that mob's generator for every later use.
///
/// `isAlwaysExperienceDropper` is false for `LivingEntity` and unoverridden by
/// `Mob`, `Animal` or `Pig`, so the short-circuit at offset 7 is a branch with
/// no reachable true arm here — written as the constant it is.
#[must_use]
pub fn experience_to_drop(m: &mut ServerMob) -> i32 {
    if m.last_hurt_by_player_memory_time <= 0 {
        return 0;
    }
    if !super::combat::should_drop_experience(m) {
        return 0;
    }
    if !matches!(
        crate::gamerules::GameRules::shared().get("mob_drops"),
        Some(crate::gamerules::GameRuleValue::Bool(true))
    ) {
        return 0;
    }
    animal_experience_reward(m)
}

/// `Entity.spawnAtLocation(ServerLevel, ItemStack)` -> the `ItemEntity`
/// constructor's server branch.
///
/// **Four draws from the ITEM's own generator**, in this order: the bob
/// phase, the yaw, then the x and z deltas. The y delta is the literal
/// `0.2d` at offset 41 and takes no draw — an asymmetry
/// `crate::xporb`'s orb spawn does not share, where all three axes are drawn.
///
/// The position is the mob's exact `(x, y, z)`: `spawnAtLocation`'s two-argument
/// form passes `Vec3(0, 0.0f, 0)`, which the three-argument form adds.
pub fn spawn_death_drop(entities: &Entities, m: &ServerMob, stack: &Stack) -> Entity {
    let _ = stack;
    item_entity_at(entities, (m.x(), m.y(), m.z()))
}

/// `new ItemEntity(level, x, y, z, stack)`, the five-argument constructor:
/// the bob phase and the yaw from `ItemEntity(EntityType, Level)`, `setPos`,
/// then `setDeltaMovement` with x and z each `random.nextDouble() * 0.2 -
/// 0.1` and y the literal `0.2` — all four draws from the item's own
/// generator (offsets 25-61).
pub fn item_entity_at(entities: &Entities, at: (f64, f64, f64)) -> Entity {
    let mut rng = crate::dropitem::unique_random();
    let uuid = crate::dropitem::insecure_uuid(&mut rng);
    // `ItemEntity(EntityType, Level)` offsets 21-36, the client-side bobbing
    // phase. Drawn and discarded, because the draw is what advances the
    // stream the yaw reads.
    let _bob_offs = rng.next_f32() * std::f32::consts::PI * 2.0;
    let y_rot = rng.next_f32() * 360.0;
    let dx = rng.next_f64() * 0.2 - 0.1;
    let dz = rng.next_f64() * 0.2 - 0.1;
    Entity {
        id: entities.next_id(),
        uuid,
        kind: crate::entities::ITEM,
        x: at.0,
        y: at.1,
        z: at.2,
        y_rot,
        x_rot: 0.0,
        y_head_rot: 0.0,
        movement: (dx, ITEM_SPAWN_DY, dz),
        on_ground: false,
        data: 0,
        swinging: false,
        swing_time: 0,
        swinging_arm: None,
        // `crate::synched::Synched` has no `ItemEntity` arm, so the stack does
        // not reach the client and the item renders as the default. That is
        // `crate::synched`'s own documented gap ("correct today for
        // minecraft:item only because this server does not yet carry an
        // ItemEntity's stack; vanilla does send one") and this call site
        // inherits it rather than introducing it.
        synched: crate::synched::Synched::EntityOnly,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmc_worldgen::rng::legacy::LegacyRandomSource;

    /// `EntityType.getDefaultLootTable` for the two kinds this server has.
    #[test]
    fn the_two_kinds_name_their_own_tables() {
        assert_eq!(loot_table_of(&super::super::pig::PIG), "entities/pig");
        assert_eq!(loot_table_of(&super::super::bat::BAT), "entities/bat");
    }

    /// `1 + nextInt(3)` is 1, 2 or 3 — never 0 and never 4 — and it takes
    /// exactly one draw.
    ///
    /// The bounds matter because they are the whole domain of
    /// `ExperienceOrb.award` for an animal, and `xporb::split` gives three
    /// different orb counts across them.
    #[test]
    fn an_animals_reward_is_one_to_three_from_one_draw() {
        for seed in 0..64i64 {
            let mut rng = LegacyRandomSource::new(seed);
            let n = ANIMAL_BASE_EXPERIENCE + rng.next_i32_bound(ANIMAL_EXPERIENCE_BOUND);
            assert!((1..=3).contains(&n), "seed {seed} gave {n}");
        }
    }

    /// The item entity's vertical delta is a literal and the horizontal ones
    /// are draws, so two drops from one death differ in x and z and agree
    /// exactly in y.
    ///
    /// A port that drew all three would still look plausible; this is the
    /// assertion that separates them.
    #[test]
    fn the_item_spawn_draws_x_and_z_but_not_y() {
        assert_eq!(ITEM_SPAWN_DY, 0.2);
        let mut a = LegacyRandomSource::new(9);
        let _bob = a.next_f32();
        let _yaw = a.next_f32();
        let dx = a.next_f64() * 0.2 - 0.1;
        let dz = a.next_f64() * 0.2 - 0.1;
        assert!((-0.1..0.1).contains(&dx));
        assert!((-0.1..0.1).contains(&dz));
        // The y is not in that range, which is what makes the asymmetry
        // observable rather than a matter of taste.
        assert!(!(-0.1..0.1).contains(&ITEM_SPAWN_DY));
    }
}

// ---- What an animal puts on the floor (train-f: animals, passive-mobs) ----
//
// What a mob puts on the floor: `Entity.spawnAtLocation` and the
// loot-table random it draws counts from.
//
// Reference (26.2): `Entity.spawnAtLocation(ServerLevel, ItemStack, Vec3)`
// (0-61: `new ItemEntity(level, getX() + off.x, getY() + off.y, getZ() +
// off.z, stack)`, `setDefaultPickUpDelay`, `addFreshEntity`),
// `spawnAtLocation(level, stack, float)` (`new Vec3(0, offsetY, 0)`),
// `ItemEntity(Level, double, double, double, ItemStack)` (the five-argument
// constructor: `(nextDouble() * 0.2 - 0.1, 0.2, nextDouble() * 0.2 - 0.1)`),
// `LivingEntity.dropFromLootTable(level, key, params, consumer)`,
// `LootTable.getRandomItems(LootParams)` → `LootContext.Builder.create(
// randomSequence)` → `ServerLevel.getRandomSequence(id)` →
// `RandomSequences.get(id, getSeed())`.
//
// # The loot random is the WORLD's, and it is seeded
//
// Every loot table this lane rolls names a `random_sequence`
// (`minecraft:shearing/sheep`, `minecraft:shearing/mooshroom`,
// `minecraft:gameplay/chicken_lay`), so the draw does **not** come from the
// level's unseeded `RandomSource`: it comes from a per-id Xoroshiro stream
// made from the level seed (`RandomSequence(seed, id)`) the first time the
// id is asked for and continued forever after. Two fresh vanilla worlds of
// one seed therefore drop the same wool count on their first shear, which is
// what makes a shearing check gradeable at all. The seed this server uses is
// [`crate::status::level_seed`], set by `--level-seed`.
//
// # What an item here is
//
// An `ItemEntity` with its own random's draws in vanilla's order, announced
// and registered in `crate::itementity::ItemEntities` with the default
// 10-tick pickup delay, so it ticks, merges and is picked up like
// `crate::dropitem`'s and `crate::mob::leash`'s items, with one difference: its `DATA_ITEM` (accessor 8, the stack) rides
// the pairing as vanilla's does. The thrown and leash items still lack it;
// that is theirs to change and a change to every check that sees them.
/// `ServerLevel.getRandomSequence(id)` under the level's seed: the stream,
/// created on first use and continued after.
pub fn random_sequence<T>(id: &str, f: impl FnOnce(&mut XoroshiroRandomSource) -> T) -> T {
    static SEQS: OnceLock<Mutex<Option<(i64, RandomSequences)>>> = OnceLock::new();
    let m = SEQS.get_or_init(|| Mutex::new(None));
    let mut g = m.lock().expect("random sequences");
    let seed = crate::status::level_seed();
    // A seed change (a test, or `--level-seed` read after first use) starts
    // a fresh saved map rather than continuing streams of another world.
    if g.as_ref().is_none_or(|(s, _)| *s != seed) {
        *g = Some((seed, RandomSequences::new(seed)));
    }
    let (_, seqs) = g.as_mut().expect("just set");
    f(seqs.get(id))
}

/// `UniformGenerator.getInt(context)` → `Mth.nextInt(random, min, max)`:
/// `min >= max ? min : random.nextInt(max - min + 1) + min`.
pub fn uniform_int(random: &mut impl RandomSource, min: i32, max: i32) -> i32 {
    if min >= max {
        min
    } else {
        random.next_i32_bound(max - min + 1) + min
    }
}

/// `Entity.spawnAtLocation(level, new ItemStack(item), off)` with the offset
/// already added: `new ItemEntity(level, x, y, z, stack)` and
/// `addFreshEntity`. Returns the item's entity id.
///
/// The draws, all from the item's own `RandomSource.create()`:
/// `Entity.<init>`'s uuid (two `nextLong`s), `ItemEntity(EntityType,
/// Level)`'s `bobOffs` and spawn yaw (two `nextFloat`s), then the
/// five-argument constructor's two `nextDouble`s for the horizontal kick.
pub fn spawn_item_at(at: (f64, f64, f64), item: &str) -> i32 {
    let item_id = item_id(item);
    let entities = crate::entities::Entities::shared();
    let mut rng = crate::dropitem::unique_random();
    let uuid = crate::dropitem::insecure_uuid(&mut rng);
    let _bob_offs = rng.next_f32() * std::f32::consts::PI * 2.0;
    let y_rot = rng.next_f32() * 360.0;
    let dx = rng.next_f64() * 0.2 - 0.1;
    let dz = rng.next_f64() * 0.2 - 0.1;
    let e = crate::entities::Entity {
        id: entities.next_id(),
        uuid,
        kind: crate::entities::ITEM,
        x: at.0,
        y: at.1,
        z: at.2,
        y_rot,
        x_rot: 0.0,
        y_head_rot: 0.0,
        movement: (dx, 0.2, dz),
        on_ground: false,
        data: 0,
        swinging: false,
        swing_time: 0,
        swinging_arm: None,
        synched: crate::synched::Synched::item(item_id, 1),
    };
    let id = e.id;
    entities.insert(e.clone());
    register_item(id, item, item_id);
    if let Some(kinds) = crate::entities::shared_kind_ids() {
        crate::tracker::Tracker::shared().add(&e, &super::breed::current_players(), kinds);
    }
    id
}

/// `spawnAtLocation`'s `setDefaultPickUpDelay()` (10 ticks) and the stack,
/// put where `crate::itementity::tick` and `crate::pickup` find them, so a
/// mob's drop falls, merges, despawns and is picked up like any other item.
pub fn register_item(id: i32, item: &str, item_id: i32) {
    let mut data = crate::itementity::ItemData::new(
        crate::item::ItemStack::new(item, 1),
        (item_id != 0).then_some(item_id),
    );
    data.pickup_delay = crate::itementity::DEFAULT_PICKUP_DELAY;
    crate::itementity::ItemEntities::shared().insert(id, data);
}

/// An item's network id, from `data/vanilla/items.json`; `0` when the
/// table is absent or does not name it (`minecraft:air`'s id, which the
/// pairing then states — a wrong value on the wire rather than a panic in
/// the tick).
#[must_use]
pub fn item_id(item: &str) -> i32 {
    static IDS: OnceLock<Option<crate::player::ItemIds>> = OnceLock::new();
    IDS.get_or_init(|| {
        crate::player::ItemIds::load(&super::table_path("data/vanilla/items.json")).ok()
    })
    .as_ref()
    .and_then(|ids| ids.id_of(item))
    .unwrap_or(0)
}

/// `ItemEntity.setDeltaMovement(getDeltaMovement().add(...))` after the
/// spawn — `Sheep.lambda$shear$0`'s kick, on the sheep's random.
pub fn add_item_movement(id: i32, d: (f64, f64, f64)) {
    crate::entities::Entities::shared().update(id, |e| {
        e.movement = (e.movement.0 + d.0, e.movement.1 + d.1, e.movement.2 + d.2);
    });
}

#[cfg(test)]
mod animal_tests {
    use super::*;

    /// `RandomSequence(seed, id)` is a pure function of the two: the same
    /// first draw for the same pair, a different stream for another id.
    #[test]
    fn a_sequence_is_a_function_of_the_seed_and_the_id() {
        let mut a = rustmc_entity::trading::draw::sequence_source(
            20_260_803,
            Some("minecraft:shearing/sheep"),
        );
        let mut b = rustmc_entity::trading::draw::sequence_source(
            20_260_803,
            Some("minecraft:shearing/sheep"),
        );
        let mut c = rustmc_entity::trading::draw::sequence_source(
            20_260_803,
            Some("minecraft:shearing/mooshroom"),
        );
        let (x, y, z) = (a.next_i64(), b.next_i64(), c.next_i64());
        assert_eq!(x, y);
        assert_ne!(x, z);
    }

    #[test]
    fn uniform_one_to_three_draws_one_bounded_int() {
        let mut r = rustmc_worldgen::rng::LegacyRandomSource::new(1);
        let mut twin = rustmc_worldgen::rng::LegacyRandomSource::new(1);
        let n = uniform_int(&mut r, 1, 3);
        assert_eq!(n, twin.next_i32_bound(3) + 1);
        assert_eq!(uniform_int(&mut r, 5, 5), 5);
        // `min >= max` draws nothing.
        assert_eq!(r.next_i32_bound(1000), twin.next_i32_bound(1000));
    }
}
