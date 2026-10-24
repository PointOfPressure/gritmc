//! Loot tables, interpreted from vanilla's own datapack JSON.
//!
//! # Why this is an interpreter and not 1,355 ported functions
//!
//! Vanilla ships 1,355 loot tables inside the pinned jar at
//! `data/minecraft/loot_table/**.json`. `ci/loot-census.py` measured that the
//! whole corpus is written in a vocabulary of **42 tokens** — six entry types,
//! twelve conditions, eighteen functions, plus number-provider kinds and
//! `apply_bonus` formulas — and that **829 of the 1,355 tables use exactly
//! three of them**. Twenty-three tokens cover every block table that can be
//! evaluated without a live server.
//!
//! So the cost of this surface is the size of the vocabulary, not the size of
//! the corpus, and porting it table by table would be paying 1,355 times for 42
//! pieces of work. Nothing in this file knows what `minecraft:blocks/stone`
//! drops. It knows what `set_count` means.
//!
//! # What the JSON does NOT give, which is the whole difficulty
//!
//! The structure is free. The **draw order** is not, and for a seeded stream the
//! draw order is the entire difference between right and wrong. Three facts
//! below are from `javap -c` on the pinned jar and none of them is visible in
//! any loot table's JSON:
//!
//! * `LootPool.addRandomItems` calls `bonusRolls.getFloat(ctx)` **before**
//!   multiplying by luck. With luck at zero the product is zero, but a
//!   non-constant provider has already drawn, and the stream is advanced.
//! * `LootPool.addRandomItem` draws `nextInt(totalWeight)` **only when the pool
//!   expanded to more than one live entry**. A single-entry pool consumes no
//!   randomness at all, which is why 1,043 of the 1,111 evaluable block tables
//!   give the same answer under every seed.
//! * A condition that short-circuits does not draw. `survives_explosion` with no
//!   `EXPLOSION_RADIUS` in context returns true without touching the generator,
//!   so an implementation that "helpfully" rolled anyway would desynchronise
//!   every later draw in the same pool.
//!
//! An implementation can get all 42 opcodes individually right and still be
//! wrong on every multi-entry table by drawing in the wrong order. That is why
//! this module is gated against vectors that vanilla produced
//! (`ci/loot-golden.sh`) rather than against a reading of the JSON.
//!
//! # What this module still cannot do, said plainly
//!
//! * **Block -> table is data, not this crate's guess.** [`Loot`] takes the
//!   `block_loot_map.json` `ci/loot-golden.sh` extracts from
//!   `BlockBehaviour.getLootTable()`; an entity's table is the caller's to
//!   name (`EntityType.getDefaultLootTable`).
//! * **One call site so far**: `rustmc-server`'s `crate::lootroll` rolls a
//!   container's table on first open. Block drops and death drops are not
//!   wired to it on this branch.
//! * **Entity predicates are a narrow subset.** `entity_properties` reads
//!   `minecraft:flags` (`is_on_fire`, `is_baby`) and an exact
//!   `minecraft:entity_type` of `this`; every other key refuses. The
//!   no-entity arm (the parameter absent) is exact, and is what the 109
//!   entity tables are gated on.
//! * **The enchanting functions need a caller-supplied [`Enchanting`]** —
//!   the registry and `selectEnchantment` live in `rustmc-server`, which
//!   implements it and gates the 99 chest cases and 121 fills that reach one
//!   (`crate::lootroll::tests`). Without one they refuse. `exploration_map`
//!   refuses always: what it produces is a fact about the world's structures.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use rustmc_worldgen::rng::legacy::LegacyRandomSource;
use rustmc_worldgen::rng::RandomSource;

pub mod fill;
pub mod sequence;
use serde_json::Value;

/// A `'static` empty list, so a `.unwrap_or(...)` default outlives the loop that
/// borrows it. `&vec![]` here builds a temporary that is freed at the end of the
/// statement, which the borrow checker rejects in exactly the four places a
/// missing JSON key is being defaulted.
fn none() -> &'static Vec<Value> {
    static EMPTY: std::sync::OnceLock<Vec<Value>> = std::sync::OnceLock::new();
    EMPTY.get_or_init(Vec::new)
}

/// Deliberate faults, injectable only so the gate can be shown failing.
///
/// `tests/falsify_loot.rs` turns each of these on in turn and requires the
/// golden comparison to go RED. A check that cannot be made to fail is this
/// repo's central defect class, and a 17,744-case gate that agrees with vanilla
/// is worth nothing until it has been shown to disagree with a wrong
/// implementation. Each field is a plausible mistake, not a random corruption:
/// every one of them is something a careful reader of the JSON would do.
#[derive(Debug, Clone, Copy, Default)]
pub struct Faults {
    /// Draw the weighted-choice `nextInt` even when the pool has one live entry.
    /// This is the single most likely error: it produces the RIGHT item for this
    /// roll and desynchronises every later draw in the same pool.
    pub always_draw_entry: bool,
    /// Skip `bonusRolls.getFloat` when luck is zero, on the reasonable-looking
    /// grounds that its result is multiplied by zero anyway.
    pub skip_bonus_rolls_draw: bool,
    /// Filter zero-count stacks instead of emitting `air/0`, as the non-raw
    /// `getRandomItems` does.
    pub filter_empty_stacks: bool,
    /// Roll a `survives_explosion` check even with no explosion in context.
    pub always_roll_survives_explosion: bool,
    /// Treat an unknown item tag as matching nothing rather than resolving it.
    pub ignore_item_tags: bool,
    /// Skip `random_chance_with_enchanted_bonus`'s `nextFloat` when the
    /// attacker has no level — reading the unenchanted arm as a constant.
    pub skip_unenchanted_chance_draw: bool,
    /// Answer `killed_by_player` true whatever the context — a server that
    /// wired the condition to "a player was involved" rather than to the
    /// parameter `dropFromLootTable` sets.
    pub killed_by_player_always: bool,
    /// Draw `enchant_randomly`'s level even for a single-level enchantment —
    /// `nextIntBetweenInclusive` in place of `Mth.nextInt`, whose `min >= max`
    /// guard does not draw.
    pub enchant_level_always_draws: bool,
}

/// One item stack a roll produced: an id, a count and the data components
/// the roll's functions set on it.
///
/// `components` is keyed by the bare component id (`stored_enchantments`,
/// `damage`, …) and holds the value as the function produced it, **in the
/// order the functions set them** — see [`Components`]. The golden vectors
/// compare id and count only; the components are graded on the wire, where
/// `crate`'s caller turns them into a `DataComponentPatch`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Stack {
    pub item: String,
    pub count: i32,
    pub components: Components,
}

/// A stack's components in first-set order.
///
/// `PatchedDataComponentMap.set` (offsets 0..71) puts into a
/// `Reference2ObjectArrayMap`: a key already present keeps its slot and takes
/// the new value, a new key goes on the end, and `DataComponentPatch`'s
/// encoder walks that array in order. So the order the loot functions set
/// components in **is** the order of the patch on the wire, and a sorted map
/// (this field was a `BTreeMap` until the patch was graded) writes
/// `damage` before `enchantments` for a table that sets them the other way
/// round.
///
/// What this does not model, stated: `set` *removes* the key when the value
/// equals the item's prototype value, and a later non-default `set` then
/// appends it at the end. This crate does not know prototypes, so the caller
/// drops prototype-equal values at conversion; that is exact unless one
/// entry sets the same component twice, which no vanilla table does
/// (`rustmc-server`'s `lootroll` asserts it over every table).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Components(Vec<(String, Value)>);

impl Components {
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// `Reference2ObjectArrayMap.put`: replace in place, or append.
    pub fn insert(&mut self, key: String, value: Value) {
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.0.push((key, value)),
        }
    }

    /// The value under `key`, appending `make()` first if there is none.
    pub fn get_or_insert_with(&mut self, key: &str, make: impl FnOnce() -> Value) -> &mut Value {
        let i = match self.0.iter().position(|(k, _)| k == key) {
            Some(i) => i,
            None => {
                self.0.push((key.to_string(), make()));
                self.0.len() - 1
            }
        };
        &mut self.0[i].1
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// In first-set order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }
}

impl Stack {
    #[must_use]
    pub fn new(item: &str, count: i32) -> Self {
        Self {
            item: item.to_string(),
            count,
            components: Components::default(),
        }
    }
}

/// The block state a block table is rolled against.
#[derive(Debug, Clone, Default)]
pub struct BlockRef {
    pub block: String,
    pub props: BTreeMap<String, String>,
}

/// The loot context, holding only what this interpreter can honestly supply.
///
/// Every field that is `None` is a subsystem this server does not have yet, and
/// the reason a condition reading it must **refuse** rather than guess: an
/// `entity_properties` check that answered `false` because there is no entity
/// system would silently suppress drops and read as a passing port.
#[derive(Debug, Clone, Default)]
pub struct Ctx {
    pub block: Option<BlockRef>,
    /// The held item's id, `None` for an empty hand.
    pub tool: Option<String>,
    /// `EXPLOSION_RADIUS`; `None` means the drop was not caused by an explosion.
    pub explosion_radius: Option<f32>,
    pub luck: f32,
    /// Enchantment levels on the tool, by bare enchantment id.
    pub enchants: BTreeMap<String, i32>,
    /// Item tags, from the jar. `#minecraft:swords` -> the ids in it.
    ///
    /// Behind an `Arc` because a `Ctx` is built per roll and the gate does
    /// 18,015 of them: cloning a 224-entry tag map and a 1,355-entry table map
    /// per roll turned a seven-second test into one that did not finish.
    pub item_tags: Arc<BTreeMap<String, Vec<String>>>,
    /// Every loot table by id, so a `loot_table` entry can resolve its
    /// reference. 57 entries across the corpus point at another table, and
    /// without this they can only refuse.
    pub tables: Arc<BTreeMap<String, Value>>,
    /// Faults to inject; always default outside the falsifier.
    pub faults: Faults,
    /// Set when the roll needed something this context cannot supply. A roll
    /// that sets this has NOT produced a vanilla-faithful answer, and the
    /// caller must treat its output as absent rather than as empty.
    pub refused: Option<String>,
    /// The `ENTITY` parameter set (`LootContextParamSets.ENTITY`'s five
    /// entity slots, the damage source and the origin). Default — every slot
    /// `None` — is a roll with no entity parameters, which is not an error:
    /// each optional read answers as vanilla's does for an absent parameter
    /// (`EntityPredicate.matches` is false for a null entity at offset 1,
    /// `hasParameter(LAST_DAMAGE_PLAYER)` is false, a missing `DAMAGE_SOURCE`
    /// is false) and none of them draws.
    pub entity: EntityCtx,
    /// `minecraft:enchantment` tags, for an `EnchantmentPredicate` whose
    /// `enchantments` field is a `#tag` reference.
    ///
    /// **A separate map from [`Ctx::item_tags`], because they are separate
    /// registries.** `#minecraft:smelts_loot` — the tag the pig's
    /// `furnace_smelt` names — lives in `data/minecraft/tags/enchantment/`
    /// and is not in `item_tags.json` at all.
    pub enchantment_tags: Arc<BTreeMap<String, Vec<String>>>,
    /// `RecipeType.SMELTING`, as input item id -> result item id, for
    /// `minecraft:furnace_smelt`. `None` refuses the function; `Some` with no
    /// entry for the stack is vanilla's no-recipe path, which returns the
    /// stack unchanged.
    pub smelting: Option<Arc<BTreeMap<String, String>>>,
    /// The enchantment registry, for `enchant_randomly`, `enchant_with_levels`
    /// and `set_damage`. `None` refuses them.
    pub enchanting: Option<Arc<dyn Enchanting>>,
}

/// The enchantment registry, as the loot functions that read it need it.
///
/// A trait rather than a table because the registry, its tags and
/// `EnchantmentHelper.selectEnchantment` are already ported — and golden-gated
/// — in `rustmc-server` (`crate::enchantment`, `crate::enchanting`), which
/// depends on this crate; a second copy here would be two places to be wrong.
/// The server implements it. With none supplied, the enchanting functions
/// refuse.
pub trait Enchanting: Send + Sync + std::fmt::Debug {
    /// The members of a `HolderSet<Enchantment>` in iteration order: a
    /// `#tag` in `TagLoader` order, a single id, or a list. `None` for the
    /// absent option, which every function maps to the whole registry in
    /// registry order (`Registry.listElements`).
    fn holder_set(&self, spec: Option<&Value>) -> Option<Vec<String>>;
    /// `Enchantment.canEnchant(stack)`: `supportedItems` holds the item.
    fn can_enchant(&self, enchantment: &str, item: &str) -> bool;
    /// `Enchantment.getMinLevel()` (always 1) and `getMaxLevel()`.
    fn level_range(&self, enchantment: &str) -> Option<(i32, i32)>;
    /// `EnchantmentHelper.selectEnchantment(random, stack, level, stream)`.
    fn select(
        &self,
        rng: &mut dyn RandomSource,
        item: &str,
        level: i32,
        stream: &[String],
    ) -> Vec<(String, i32)>;
    /// `ItemStack.getMaxDamage()` for a damageable item, `None` otherwise.
    fn max_damage(&self, item: &str) -> Option<i32>;
}

/// One entity, as the `ENTITY` loot-parameter set's five entity slots need to
/// be seen.
///
/// Every field is one thing an `EntityPredicate` sub-predicate reads. It is a
/// deliberately small struct: a field added here is a sub-predicate
/// [`entity_predicate`] can then stop refusing, and there is no way to add a
/// field without also deciding what reads it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LootEntity {
    /// The entity type's registry id, for `minecraft:type`.
    pub kind: String,
    /// `Entity.isOnFire()` — `EntityFlagsPredicate.isOnFire`.
    pub on_fire: bool,
    /// `Entity.isBaby()` — `EntityFlagsPredicate.isBaby`.
    pub baby: bool,
    /// True for a `Player`. `killed_by_player` does not read this (it reads
    /// whether `LAST_DAMAGE_PLAYER` is present at all), but
    /// `minecraft:type` on `direct_attacker` does.
    pub player: bool,
    /// What is in each equipment slot, by the slot names
    /// `EntityEquipmentPredicate`'s codec uses (`mainhand`, `offhand`,
    /// `head`, `chest`, `legs`, `feet`, `body`, `saddle`).
    pub equipment: BTreeMap<String, LootItem>,
}

/// One item stack, as an `ItemPredicate` inside an equipment predicate reads
/// it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LootItem {
    pub item: String,
    /// Enchantment levels by **bare** id, matching [`Ctx::enchant`]'s key
    /// convention.
    pub enchants: BTreeMap<String, i32>,
}

/// The `DAMAGE_SOURCE` parameter, as far as a damage-source predicate reads
/// it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LootDamageSource {
    /// The damage type's registry id, e.g. `minecraft:player_attack`.
    pub kind: String,
    /// Every `#minecraft:`-prefixed damage-type tag this type is in.
    pub tags: BTreeSet<String>,
}

/// The five entity slots `LootContextParamSets.ENTITY` declares, plus the
/// damage source and the origin.
///
/// `None` throughout is the block-drop case, and it is what makes
/// [`entity_properties`](condition) refuse rather than answer: an absent slot
/// in vanilla is a *missing required parameter*, which throws, and answering
/// `false` for it here would suppress a drop and read as a correct empty
/// result — the same reasoning the `Ctx` header gives for every other `None`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EntityCtx {
    /// `LootContextParams.THIS_ENTITY` — the entity that died.
    pub this_entity: Option<LootEntity>,
    /// `ATTACKING_ENTITY` — `damageSource.getEntity()`, the owner of the
    /// blow. `None` is `withOptionalParameter`'s absent case and is legal.
    pub attacking_entity: Option<LootEntity>,
    /// `DIRECT_ATTACKING_ENTITY` — `damageSource.getDirectEntity()`. For a
    /// melee hit this is the same entity as `attacking_entity`; for an arrow
    /// it is the arrow.
    pub direct_attacking_entity: Option<LootEntity>,
    /// `LAST_DAMAGE_PLAYER`, set only when `dropFromLootTable`'s `hitByPlayer`
    /// flag is true **and** `getLastHurtByPlayer()` is non-null.
    pub last_damage_player: Option<LootEntity>,
    /// `DAMAGE_SOURCE`.
    pub damage_source: Option<LootDamageSource>,
    /// `ORIGIN` — `this.position()` at the moment of death.
    pub origin: Option<(f64, f64, f64)>,
}

impl Ctx {
    fn refuse(&mut self, why: &str) {
        if self.refused.is_none() {
            self.refused = Some(why.to_string());
        }
    }
    fn enchant(&self, id: &str) -> i32 {
        let bare = id.rsplit(':').next().unwrap_or(id);
        *self.enchants.get(bare).unwrap_or(&0)
    }
    /// `EnchantmentHelper.getEnchantmentLevel(holder, living)` on
    /// `ATTACKING_ENTITY`: the highest level of `id` across its equipment, 0
    /// with no attacker.
    fn attacker_level(&self, id: &str) -> i32 {
        let bare = id.rsplit(':').next().unwrap_or(id);
        self.entity.attacking_entity.as_ref().map_or(0, |a| {
            a.equipment
                .values()
                .filter_map(|i| i.enchants.get(bare).copied())
                .max()
                .unwrap_or(0)
        })
    }
}

fn bare(s: &str) -> &str {
    s.rsplit(':').next().unwrap_or(s)
}

/// Roll one table, exactly as `LootTable.getRandomItemsRaw` does.
///
/// `seed` seeds a `LegacyRandomSource`, matching `RandomSource.create(long)` —
/// which is what the golden dumper passes, and what a real server passes when a
/// loot table has no named random sequence.
///
/// ```
/// let table: serde_json::Value = serde_json::from_str(
///     r#"{"pools":[{"rolls":1,"entries":[
///          {"type":"minecraft:item","name":"minecraft:stone"}]}]}"#,
/// )
/// .unwrap();
/// let mut ctx = rustmc_loot::Ctx::default();
/// let got = rustmc_loot::roll(&table, &mut ctx, 42);
/// assert_eq!(got.len(), 1);
/// assert_eq!(got[0].item, "minecraft:stone");
/// assert_eq!(got[0].count, 1);
/// // Nothing here needs a subsystem this server does not have, so the roll
/// // did not refuse. A refusal is not an empty drop: see `Ctx::refused`.
/// assert!(ctx.refused.is_none());
/// ```
pub fn roll(table: &Value, ctx: &mut Ctx, seed: i64) -> Vec<Stack> {
    let mut rng = LegacyRandomSource::new(seed);
    roll_with(table, ctx, &mut rng)
}

/// Roll one table against a generator the caller owns.
///
/// **This, not [`roll`], is what a live server does for a table with a
/// `random_sequence`, and the difference is not the seed — it is the
/// algorithm.** `LootTable.getRandomItems(params, seed, consumer)` calls
/// `LootContext.Builder.withOptionalRandomSeed(seed)`, which installs a
/// `RandomSource.create(seed)` (a `LegacyRandomSource`) only when the seed is
/// **non-zero**. `Mob.getLootTableSeed()` is the `lootTableSeed` field and is
/// `0` for every mob that was not spawned from a structure or a spawner, so
/// `create(Optional<Identifier>)` falls through to
/// `level.getRandomSequence(name)` — and that is a
/// **`XoroshiroRandomSource`**, seeded
/// `upgradeSeedTo128bitUnmixed(worldSeed) XOR seedForKey(name)` then `.mixed()`
/// (`RandomSequence.createSequence`). Two different generators, drawing
/// different numbers from the same table.
///
/// So the golden vectors this crate is gated against — which pass an explicit
/// non-zero seed — exercise the legacy path, and **the sequence path had no
/// coverage at all before this function existed**. Naming it here rather than
/// leaving `roll` to be reached for is the point: a caller that wanted the
/// sequence and got `roll` would be wrong in a way no existing vector sees.
pub fn roll_with<R: RandomSource>(table: &Value, ctx: &mut Ctx, rng: &mut R) -> Vec<Stack> {
    let mut out = Vec::new();
    roll_into(table, ctx, rng, &mut out);
    out
}

/// The generator `LootContext.Builder.withOptionalRandomSeed(seed).create(seq)`
/// hands a roll, from `javap -c` on the pinned jar:
///
/// ```text
///   withOptionalRandomSeed(long)
///      0: lload_1 / lconst_0 / lcmp / ifeq 14     -- seed == 0: leave `random` null
///      8: RandomSource.create(J)                  -- LegacyRandomSource(seed)
///   create(Optional<Identifier>)
///     14: Optional.ofNullable(random)
///         .or(() -> sequence.map(server::getRandomSequence))
///         .orElseGet(level::getRandom)
/// ```
///
/// So a non-zero seed **always** wins, even for a table that names a
/// sequence, and a zero seed draws from the sequence the *table* names — not
/// from one derived from the seed, the position or the caller. `None` is the
/// third arm: no seed and no sequence, which is the level's own random and
/// is not something this crate can stand in for.
pub enum ContextRandom<'a> {
    Seeded(LegacyRandomSource),
    Sequence(&'a mut dyn RandomSource),
}

/// Pick the generator for a roll, exactly as `LootContext.Builder.create` does.
///
/// Returns `None` for the level-random arm (seed 0 and the table names no
/// `random_sequence`) — the caller must supply that generator itself.
pub fn context_random<'a>(
    table: &Value,
    seed: i64,
    sequences: &'a mut sequence::RandomSequences,
) -> Option<ContextRandom<'a>> {
    if seed != 0 {
        return Some(ContextRandom::Seeded(LegacyRandomSource::new(seed)));
    }
    let id = table.get("random_sequence").and_then(Value::as_str)?;
    Some(ContextRandom::Sequence(sequences.get(id)))
}

impl ContextRandom<'_> {
    pub fn as_dyn(&mut self) -> &mut dyn RandomSource {
        match self {
            Self::Seeded(r) => r,
            Self::Sequence(r) => &mut **r,
        }
    }
}

/// The body of a roll, against a generator the caller owns. Split out so a
/// nested `loot_table` entry can continue the SAME stream rather than starting
/// a new one.
fn roll_into<R: RandomSource>(table: &Value, ctx: &mut Ctx, rng: &mut R, out: &mut Vec<Stack>) {
    let table_fns = table.get("functions").and_then(Value::as_array);

    for pool in table
        .get("pools")
        .and_then(Value::as_array)
        .unwrap_or_else(|| none())
    {
        // Pool conditions first, and a failing pool draws nothing further.
        if !all_conditions(pool.get("conditions"), ctx, rng) {
            continue;
        }
        let rolls = number_int(pool.get("rolls"), ctx, rng);
        // `bonusRolls.getFloat(ctx)` is evaluated whether or not luck is zero.
        let bonus_f = match pool.get("bonus_rolls") {
            Some(_) if ctx.faults.skip_bonus_rolls_draw && ctx.luck == 0.0 => 0.0,
            Some(v) => number_float(Some(v), ctx, rng),
            None => 0.0,
        };
        let bonus = (bonus_f * ctx.luck).floor() as i32;
        let pool_fns = pool.get("functions").and_then(Value::as_array);

        for _ in 0..(rolls + bonus) {
            add_random_item(pool, ctx, rng, pool_fns, table_fns, out);
        }
    }
}

/// `LootPool.addRandomItem` — expansion, then a single weighted choice.
fn add_random_item<R: RandomSource>(
    pool: &Value,
    ctx: &mut Ctx,
    rng: &mut R,
    pool_fns: Option<&Vec<Value>>,
    table_fns: Option<&Vec<Value>>,
    out: &mut Vec<Stack>,
) {
    let mut live: Vec<(&Value, i32)> = Vec::new();
    let mut total = 0i32;
    for e in pool
        .get("entries")
        .and_then(Value::as_array)
        .unwrap_or_else(|| none())
    {
        expand(e, ctx, rng, &mut live, &mut total);
    }
    if live.is_empty() || total <= 0 {
        return;
    }
    // The single-entry case does NOT draw. Getting this wrong desynchronises
    // every subsequent roll in the same pool while still producing the right
    // item for this one, which is the hardest possible failure to notice.
    let chosen = if live.len() == 1 && !ctx.faults.always_draw_entry {
        live[0].0
    } else if live.len() == 1 {
        let _ = rng.next_i32_bound(total);
        live[0].0
    } else {
        let mut r = rng.next_i32_bound(total);
        let mut pick = live[live.len() - 1].0;
        for (e, w) in &live {
            r -= *w;
            if r < 0 {
                pick = e;
                break;
            }
        }
        pick
    };
    create_stack(chosen, ctx, rng, pool_fns, table_fns, out);
}

/// `LootPoolEntryContainer.expand` — collect the entries eligible this roll.
fn expand<'a, R: RandomSource>(
    e: &'a Value,
    ctx: &mut Ctx,
    rng: &mut R,
    live: &mut Vec<(&'a Value, i32)>,
    total: &mut i32,
) -> bool {
    if !all_conditions(e.get("conditions"), ctx, rng) {
        return false;
    }
    match bare(e.get("type").and_then(Value::as_str).unwrap_or("")) {
        // `AlternativesEntry`: the FIRST child that expands wins, and the rest
        // are never visited — so their conditions never draw either.
        "alternatives" => {
            for c in e
                .get("children")
                .and_then(Value::as_array)
                .unwrap_or_else(|| none())
            {
                if expand(c, ctx, rng, live, total) {
                    return true;
                }
            }
            false
        }
        "group" => {
            let mut any = false;
            for c in e
                .get("children")
                .and_then(Value::as_array)
                .unwrap_or_else(|| none())
            {
                any |= expand(c, ctx, rng, live, total);
            }
            any
        }
        "sequence" => {
            for c in e
                .get("children")
                .and_then(Value::as_array)
                .unwrap_or_else(|| none())
            {
                if !expand(c, ctx, rng, live, total) {
                    return false;
                }
            }
            true
        }
        "item" | "empty" | "dynamic" | "loot_table" | "tag" => {
            let weight = e.get("weight").and_then(Value::as_i64).unwrap_or(1) as i32;
            let quality = e.get("quality").and_then(Value::as_i64).unwrap_or(0) as i32;
            let w = weight + (quality as f32 * ctx.luck).floor() as i32;
            if w > 0 {
                live.push((e, w));
                *total += w;
            }
            true
        }
        other => {
            ctx.refuse(&format!("entry type {other}"));
            false
        }
    }
}

fn create_stack<R: RandomSource>(
    e: &Value,
    ctx: &mut Ctx,
    rng: &mut R,
    pool_fns: Option<&Vec<Value>>,
    table_fns: Option<&Vec<Value>>,
    out: &mut Vec<Stack>,
) {
    match bare(e.get("type").and_then(Value::as_str).unwrap_or("")) {
        "empty" => {}
        "item" => {
            let name = match e.get("name").and_then(Value::as_str) {
                Some(n) => n.to_string(),
                None => {
                    ctx.refuse("item entry without a name");
                    return;
                }
            };
            let mut stack = Stack::new(&name, 1);
            // Entry functions, then pool functions, then table functions — the
            // order `LootItemFunction.decorate` nests the consumers in.
            for f in e
                .get("functions")
                .and_then(Value::as_array)
                .unwrap_or_else(|| none())
            {
                apply_function(f, &mut stack, ctx, rng);
            }
            for f in pool_fns.unwrap_or_else(|| none()) {
                apply_function(f, &mut stack, ctx, rng);
            }
            for f in table_fns.unwrap_or_else(|| none()) {
                apply_function(f, &mut stack, ctx, rng);
            }
            // `getRandomItemsRaw` does NOT filter empty stacks — the filtering
            // lives in the private `getRandomItems(LootContext)` above it. A
            // stack whose count reached zero is `ItemStack.EMPTY`, which reads
            // back as `minecraft:air` at count 0, and vanilla's vectors contain
            // 69 of them. Dropping them silently is a real divergence: the
            // consumer downstream sees a different number of stacks.
            if stack.count <= 0 {
                if ctx.faults.filter_empty_stacks {
                    return;
                }
                stack = Stack::new("minecraft:air", 0);
            }
            out.push(stack);
        }
        "dynamic" => ctx.refuse("dynamic entry (needs block entity contents)"),
        // `NestedLootTable.createItemStack` resolves the reference and calls
        // `getRandomItemsRaw` on the SAME context — so the nested table draws
        // from the same stream, at the position this entry reached it, and the
        // functions of the entry, pool and table above it decorate whatever
        // comes out. Rolling it with a fresh generator would produce the right
        // items in the wrong order and desynchronise everything after.
        "loot_table" => {
            let Some(name) = e.get("value").and_then(Value::as_str) else {
                ctx.refuse("loot_table entry without a value");
                return;
            };
            let key = name.strip_prefix("minecraft:").unwrap_or(name).to_string();
            let Some(nested) = ctx.tables.get(&key).cloned() else {
                ctx.refuse(&format!("unresolved loot_table reference {name}"));
                return;
            };
            let mut inner = Vec::new();
            roll_into(&nested, ctx, rng, &mut inner);
            for mut stack in inner {
                for f in e
                    .get("functions")
                    .and_then(Value::as_array)
                    .unwrap_or_else(|| none())
                {
                    apply_function(f, &mut stack, ctx, rng);
                }
                for f in pool_fns.unwrap_or_else(|| none()) {
                    apply_function(f, &mut stack, ctx, rng);
                }
                for f in table_fns.unwrap_or_else(|| none()) {
                    apply_function(f, &mut stack, ctx, rng);
                }
                out.push(stack);
            }
        }
        "tag" => ctx.refuse("tag entry"),
        other => ctx.refuse(&format!("createItemStack for {other}")),
    }
}

// ------------------------------------------------------------------ conditions

fn all_conditions<R: RandomSource>(v: Option<&Value>, ctx: &mut Ctx, rng: &mut R) -> bool {
    let Some(arr) = v.and_then(Value::as_array) else {
        return true;
    };
    for c in arr {
        if !condition(c, ctx, rng) {
            return false;
        }
    }
    true
}

/// One loot condition, evaluated outside a table — what
/// `ConditionalEffect.matches(LootContext)` does with an enchantment effect's
/// `requirements`. `None` when the condition needed something `ctx` cannot
/// supply: the same refusal a roll makes, so a caller can tell "false" from
/// "unanswerable".
pub fn test_condition<R: RandomSource>(c: &Value, ctx: &mut Ctx, rng: &mut R) -> Option<bool> {
    let answer = condition(c, ctx, rng);
    ctx.refused.is_none().then_some(answer)
}

fn condition<R: RandomSource>(c: &Value, ctx: &mut Ctx, rng: &mut R) -> bool {
    match bare(c.get("condition").and_then(Value::as_str).unwrap_or("")) {
        // No explosion in context: true, and **no draw**.
        "survives_explosion" => match ctx.explosion_radius {
            None if ctx.faults.always_roll_survives_explosion => {
                let _ = rng.next_f32();
                true
            }
            None => true,
            Some(r) => rng.next_f32() <= 1.0 / r,
        },
        "random_chance" => {
            let p = number_float(c.get("chance"), ctx, rng);
            rng.next_f32() < p
        }
        "table_bonus" => {
            let level = ctx.enchant(c.get("enchantment").and_then(Value::as_str).unwrap_or(""));
            let chances = c
                .get("chances")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let idx = (level as usize).min(chances.len().saturating_sub(1));
            let p = chances.get(idx).and_then(Value::as_f64).unwrap_or(0.0) as f32;
            rng.next_f32() < p
        }
        "inverted" => !condition(c.get("term").unwrap_or(&Value::Null), ctx, rng),
        "any_of" => {
            for t in c
                .get("terms")
                .and_then(Value::as_array)
                .unwrap_or_else(|| none())
            {
                if condition(t, ctx, rng) {
                    return true;
                }
            }
            false
        }
        "all_of" => {
            for t in c
                .get("terms")
                .and_then(Value::as_array)
                .unwrap_or_else(|| none())
            {
                if !condition(t, ctx, rng) {
                    return false;
                }
            }
            true
        }
        "block_state_property" => block_state_property(c, ctx),
        // `LootItemKilledByPlayerCondition.test`: `hasParameter(LAST_DAMAGE_PLAYER)`.
        // It reads *presence*, not whether the killer was a player, so a mob
        // killed by a creeper after a player hit it still passes.
        "killed_by_player" => {
            ctx.entity.last_damage_player.is_some() || ctx.faults.killed_by_player_always
        }
        // `LootItemEntityPropertyCondition.test`: resolve the named slot, then
        // run the `EntityPredicate` against it. **No draws.** An unknown slot
        // NAME refuses; a known slot that is empty is vanilla's `false`.
        "entity_properties" => {
            let which = c.get("entity").and_then(Value::as_str).unwrap_or("this");
            match entity_slot(which, ctx) {
                SlotLookup::Unknown => {
                    ctx.refuse(&format!("entity_properties slot {which}"));
                    false
                }
                SlotLookup::Absent => false,
                SlotLookup::Present(e) => entity_predicate(c.get("predicate"), &e, ctx),
            }
        }
        // `LootItemRandomChanceWithEnchantedBonusCondition.test`, offsets
        // 0..81: the attacker's level (0 without a living attacker), then
        // `level > 0 ? enchantedChance.calculate(level) : unenchantedChance`,
        // then ONE `nextFloat() < chance` either way.
        "random_chance_with_enchanted_bonus" => {
            let id = c.get("enchantment").and_then(Value::as_str).unwrap_or("");
            let level = ctx.attacker_level(id);
            let chance = if level > 0 {
                match level_based(c.get("enchanted_chance"), level) {
                    Some(v) => v,
                    None => {
                        ctx.refuse("random_chance_with_enchanted_bonus level-based value");
                        return false;
                    }
                }
            } else {
                let p = c
                    .get("unenchanted_chance")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0) as f32;
                if ctx.faults.skip_unenchanted_chance_draw {
                    return p >= 1.0;
                }
                p
            };
            rng.next_f32() < chance
        }
        // `DamageSourceCondition.test`: no ORIGIN or no source is false, with
        // no draw (offsets 22..31). ORIGIN is always present where this
        // interpreter rolls.
        "damage_source_properties" => {
            let Some(src) = ctx.entity.damage_source.clone() else {
                return false;
            };
            damage_source_predicate(c.get("predicate"), &src, ctx)
        }
        "match_tool" => match_tool(c.get("predicate"), ctx),
        // Everything below needs a subsystem this server does not have. They
        // REFUSE rather than answering false: a false here would suppress a drop
        // and read as a correct empty result.
        other => {
            ctx.refuse(&format!("condition {other}"));
            false
        }
    }
}

fn block_state_property(c: &Value, ctx: &mut Ctx) -> bool {
    let Some(block) = ctx.block.clone() else {
        ctx.refuse("block_state_property without a block");
        return false;
    };
    if let Some(want) = c.get("block").and_then(Value::as_str) {
        if want != block.block {
            return false;
        }
    }
    let Some(props) = c.get("properties").and_then(Value::as_object) else {
        return true;
    };
    for (k, v) in props {
        let Some(actual) = block.props.get(k) else {
            return false;
        };
        match v {
            Value::String(s) => {
                if s != actual {
                    return false;
                }
            }
            // A range form, `{"min": "..", "max": ".."}` over an integer property.
            Value::Object(o) => {
                let Ok(n) = actual.parse::<i64>() else {
                    return false;
                };
                if let Some(min) = o.get("min").and_then(range_bound) {
                    if n < min {
                        return false;
                    }
                }
                if let Some(max) = o.get("max").and_then(range_bound) {
                    if n > max {
                        return false;
                    }
                }
            }
            _ => return false,
        }
    }
    true
}

fn range_bound(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn match_tool(pred: Option<&Value>, ctx: &mut Ctx) -> bool {
    let Some(pred) = pred else { return true };
    let tool = ctx.tool.clone();
    if let Some(items) = pred.get("items") {
        let ok = match (items, &tool) {
            (_, None) => false,
            (Value::String(s), Some(t)) => item_matches(s, t, ctx),
            (Value::Array(a), Some(t)) => a
                .iter()
                .filter_map(Value::as_str)
                .any(|s| item_matches(s, t, ctx)),
            _ => false,
        };
        if !ok {
            return false;
        }
    }
    for k in pred
        .as_object()
        .map(|o| o.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default()
    {
        match k.as_str() {
            "items" => {}
            // `{"predicates": {"minecraft:enchantments": [...]}}` — the only
            // sub-predicate the whole corpus uses, 156 times, and always a
            // minimum level on a named enchantment (silk touch, mostly).
            "predicates" => {
                let subs = pred.get("predicates").and_then(Value::as_object);
                let Some(subs) = subs else { continue };
                for (name, body) in subs {
                    if bare(name) != "enchantments" {
                        ctx.refuse(&format!("match_tool sub-predicate {name}"));
                        return false;
                    }
                    let list = body.as_array().cloned().unwrap_or_default();
                    for req in &list {
                        let ench = req
                            .get("enchantments")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let have = ctx.enchant(ench);
                        let levels = req.get("levels");
                        let min = levels
                            .and_then(|l| l.get("min"))
                            .and_then(range_bound)
                            .or_else(|| levels.and_then(range_bound))
                            .unwrap_or(1) as i32;
                        let max = levels
                            .and_then(|l| l.get("max"))
                            .and_then(range_bound)
                            .map(|v| v as i32);
                        if have < min || max.is_some_and(|m| have > m) {
                            return false;
                        }
                    }
                }
            }
            other => {
                ctx.refuse(&format!("match_tool predicate field {other}"));
                return false;
            }
        }
    }
    true
}

fn item_matches(spec: &str, tool: &str, ctx: &Ctx) -> bool {
    if let Some(tag) = spec.strip_prefix('#') {
        if ctx.faults.ignore_item_tags {
            return false;
        }
        let key = format!("#{}", normalise(tag));
        return ctx
            .item_tags
            .get(&key)
            .map(|ids| ids.iter().any(|i| normalise(i) == normalise(tool)))
            .unwrap_or(false);
    }
    normalise(spec) == normalise(tool)
}

/// A registry name with its namespace: `minecraft:` when it has none.
#[must_use]
pub fn normalise(id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("minecraft:{id}")
    }
}

// ------------------------------------------------------------------- functions

fn apply_function<R: RandomSource>(f: &Value, stack: &mut Stack, ctx: &mut Ctx, rng: &mut R) {
    if !all_conditions(f.get("conditions"), ctx, rng) {
        return;
    }
    match bare(f.get("function").and_then(Value::as_str).unwrap_or("")) {
        "set_count" => {
            let n = number_int(f.get("count"), ctx, rng);
            let add = f.get("add").and_then(Value::as_bool).unwrap_or(false);
            stack.count = if add { stack.count + n } else { n };
        }
        "limit_count" => {
            if let Some(l) = f.get("limit") {
                if let Some(min) = l.get("min").and_then(Value::as_i64) {
                    stack.count = stack.count.max(min as i32);
                }
                if let Some(max) = l.get("max").and_then(Value::as_i64) {
                    stack.count = stack.count.min(max as i32);
                }
            }
        }
        // No explosion in context: unchanged, and no draw.
        "explosion_decay" => {
            if let Some(r) = ctx.explosion_radius {
                let mut kept = 0;
                for _ in 0..stack.count {
                    if rng.next_f32() <= 1.0 / r {
                        kept += 1;
                    }
                }
                stack.count = kept;
            }
        }
        // Copy from a block entity or block state into components. Neither
        // changes the id or the count, which is all the vectors compare, so
        // this is a documented no-op HERE and not a claim that it is done.
        "copy_components" | "copy_state" | "copy_custom_data" | "copy_name" => {}
        "apply_bonus" => apply_bonus(f, stack, ctx, rng),
        "furnace_smelt" => {
            let Some(map) = ctx.smelting.clone() else {
                ctx.refuse("furnace_smelt without a smelting recipe map");
                return;
            };
            if let Some(result) = map.get(&normalise(&stack.item)) {
                stack.item = result.clone();
                let use_input = f
                    .get("use_input_count")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if !use_input {
                    stack.count = 1;
                }
            }
        }
        // `EnchantedCountIncreaseFunction.run`: the level comes off the
        // ATTACKING_ENTITY (offset 1), not the tool, and a missing attacker
        // or a level of 0 returns the stack before the count provider draws.
        "enchanted_count_increase" => {
            let id = f.get("enchantment").and_then(Value::as_str).unwrap_or("");
            let level = ctx.attacker_level(id);
            if level > 0 {
                let per = number_float(f.get("count"), ctx, rng);
                stack.count += (per * level as f32).round() as i32;
                if let Some(limit) = f.get("limit").and_then(Value::as_i64) {
                    if limit > 0 {
                        stack.count = stack.count.min(limit as i32);
                    }
                }
            }
        }
        // `SetPotionFunction` and `SetOminousBottleAmplifierFunction` set a
        // component. The first draws nothing; the second draws its number
        // provider (`getInt`, offset 5) and clamps it to 0..=4.
        "set_potion" => {
            if let Some(id) = f.get("id") {
                stack
                    .components
                    .insert("potion_contents".into(), id.clone());
            }
        }
        "set_ominous_bottle_amplifier" => {
            let a = number_int(f.get("amplifier"), ctx, rng).clamp(0, 4);
            stack
                .components
                .insert("ominous_bottle_amplifier".into(), Value::from(a));
        }
        "enchant_randomly" => enchant_randomly(f, stack, ctx, rng),
        "enchant_with_levels" => enchant_with_levels(f, stack, ctx, rng),
        "set_enchantments" => set_enchantments(f, stack, ctx, rng),
        // `SetItemDamageFunction.run`: a damageable item only — the draw at
        // offset 41 is inside the `isDamageableItem` branch.
        //   f = add ? 1 - damage/max : 0
        //   g = clamp(f + damage.getFloat(ctx), 0, 1)
        //   setDamageValue(floor(max * (1 - g)))
        "set_damage" => {
            let Some(e) = ctx.enchanting.clone() else {
                ctx.refuse("set_damage without an item registry");
                return;
            };
            if let Some(max) = e.max_damage(&stack.item) {
                let add = f.get("add").and_then(Value::as_bool).unwrap_or(false);
                let cur = stack
                    .components
                    .get("damage")
                    .and_then(Value::as_i64)
                    .unwrap_or(0) as f32;
                let base = if add { 1.0 - cur / max as f32 } else { 0.0 };
                let g = (base + number_float(f.get("damage"), ctx, rng)).clamp(0.0, 1.0);
                let dmg = (max as f32 * (1.0 - g)).floor() as i32;
                stack.components.insert("damage".into(), Value::from(dmg));
            }
        }
        // `SetInstrumentFunction.run`: `options.getRandomElement(random)` —
        // one `nextInt(size)` over the tag in `TagLoader` order, none when it
        // is empty.
        "set_instrument" => {
            let spec = f.get("options").and_then(Value::as_str).unwrap_or("");
            let key = format!("instrument/{spec}");
            // `item_tags.json` carries these under `instrument/#<id>`, in
            // `TagLoader` order (`ci/loot-golden.sh`).
            let Some(list) = ctx.item_tags.get(&key).cloned() else {
                ctx.refuse(&format!("set_instrument over {spec}"));
                return;
            };
            if !list.is_empty() {
                let i = rng.next_i32_bound(list.len() as i32) as usize;
                stack
                    .components
                    .insert("instrument".into(), Value::from(list[i].clone()));
            }
        }
        // `SetStewEffectFunction.run`: nothing unless the stack is suspicious
        // stew and the list is non-empty; then `Util.getRandom(effects)` and
        // the chosen entry's `duration.getInt` — two draws or more.
        "set_stew_effect" => {
            let effects = f
                .get("effects")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if normalise(&stack.item) == "minecraft:suspicious_stew" && !effects.is_empty() {
                let i = rng.next_i32_bound(effects.len() as i32) as usize;
                let mut d = number_int(effects[i].get("duration"), ctx, rng);
                // Offsets 59..77: seconds become ticks — `i *= 20` — unless
                // `MobEffect.isInstantaneous()`, which is true for exactly the
                // `InstantaneousMobEffect` subclasses: `HealOrHarmMobEffect`
                // (instant_health, instant_damage) and `SaturationMobEffect`.
                let effect = effects[i].get("type").and_then(Value::as_str).unwrap_or("");
                if !matches!(
                    normalise(effect).as_str(),
                    "minecraft:instant_health"
                        | "minecraft:instant_damage"
                        | "minecraft:saturation"
                ) {
                    d *= 20;
                }
                stack.components.insert(
                    "suspicious_stew_effects".into(),
                    serde_json::json!([effects[i].get("type"), d]),
                );
            }
        }
        // `SetNameFunction` draws nothing; the component is recorded.
        "set_name" => {
            if let Some(n) = f.get("name") {
                stack.components.insert("custom_name".into(), n.clone());
            }
        }
        // `ExplorationMapFunction` asks the level for the nearest structure
        // of a tag and, if there is one, replaces the map with a filled one.
        // It draws nothing, but what it produces is a fact about the world,
        // which this crate does not have.
        "exploration_map" => {
            if normalise(&stack.item) == "minecraft:map" {
                ctx.refuse("exploration_map (needs the level's structure search)");
            }
        }
        // `FurnaceSmeltFunction` needs the level's recipe manager. It is
        // refused only when reached: every table guards it behind an
        // `entity_properties` that a live, unburning mob fails.
        other => ctx.refuse(&format!("function {other}")),
    }
}

/// `ItemStack.enchant(holder, level)` — `EnchantmentHelper.updateEnchantments`
/// with `Mutable.upgrade`, which keeps the higher of an existing level and the
/// new one. An enchanted book keeps them under `stored_enchantments`.
fn enchant(stack: &mut Stack, id: &str, level: i32) {
    let key = if normalise(&stack.item) == "minecraft:enchanted_book" {
        "stored_enchantments"
    } else {
        "enchantments"
    };
    let entry = stack
        .components
        .get_or_insert_with(key, || Value::Object(serde_json::Map::new()));
    if let Some(m) = entry.as_object_mut() {
        let id = normalise(id);
        let cur = m.get(&id).and_then(Value::as_i64).unwrap_or(0) as i32;
        m.insert(id, Value::from(cur.max(level)));
    }
}

/// `new ItemStack(Items.ENCHANTED_BOOK)` in place of a book — a fresh stack,
/// so count 1 and no components carried (it is not `transmuteCopy`).
fn book_to_enchanted(stack: &mut Stack) {
    if normalise(&stack.item) == "minecraft:book" {
        *stack = Stack::new("minecraft:enchanted_book", 1);
    }
}

/// `EnchantRandomlyFunction.run`, offsets 0..124, and its `enchantItem`:
///
/// ```text
///   onlyCompatible' = !stack.is(BOOK) && onlyCompatible        (default true)
///   list = options.stream (or the whole registry) .filter(!onlyCompatible' || canEnchant)
///   Util.getRandomSafe(list, random)  -- nextInt(size), and NO draw when empty
///   level = Mth.nextInt(random, min, max)   -- no draw when min == max
///   stack.is(BOOK) ? new ItemStack(ENCHANTED_BOOK) : stack; enchant(holder, level)
/// ```
fn enchant_randomly<R: RandomSource>(f: &Value, stack: &mut Stack, ctx: &mut Ctx, rng: &mut R) {
    let Some(e) = ctx.enchanting.clone() else {
        ctx.refuse("enchant_randomly without an enchantment registry");
        return;
    };
    let is_book = normalise(&stack.item) == "minecraft:book";
    let only_compatible = !is_book
        && f.get("only_compatible")
            .and_then(Value::as_bool)
            .unwrap_or(true);
    let Some(options) = e.holder_set(f.get("options")) else {
        ctx.refuse("enchant_randomly over an unknown holder set");
        return;
    };
    let list: Vec<String> = options
        .into_iter()
        .filter(|id| !only_compatible || e.can_enchant(id, &stack.item))
        .collect();
    if list.is_empty() {
        return;
    }
    let pick = list[rng.next_i32_bound(list.len() as i32) as usize].clone();
    let Some((min, max)) = e.level_range(&pick) else {
        ctx.refuse(&format!("enchant_randomly picked unknown {pick}"));
        return;
    };
    let level = if ctx.faults.enchant_level_always_draws {
        rng.next_i32_bound(max - min + 1) + min
    } else {
        fill::mth_next_int(rng, min, max)
    };
    book_to_enchanted(stack);
    enchant(stack, &pick, level);
}

/// `EnchantWithLevelsFunction.run`: `levels.getInt(ctx)` (offset 19, a draw
/// for a non-constant provider) and then `EnchantmentHelper.enchantItem(random,
/// stack, levels, registryAccess, options)` — `selectEnchantment` over the
/// options (or the whole registry), the book swapped for an enchanted book
/// AFTER the selection, then each instance applied in order.
fn enchant_with_levels<R: RandomSource>(f: &Value, stack: &mut Stack, ctx: &mut Ctx, rng: &mut R) {
    let Some(e) = ctx.enchanting.clone() else {
        ctx.refuse("enchant_with_levels without an enchantment registry");
        return;
    };
    let levels = number_int(f.get("levels"), ctx, rng);
    let Some(stream) = e.holder_set(f.get("options")) else {
        ctx.refuse("enchant_with_levels over an unknown holder set");
        return;
    };
    let chosen = e.select(rng, &normalise(&stack.item), levels, &stream);
    book_to_enchanted(stack);
    for (id, level) in chosen {
        enchant(stack, &id, level);
    }
}

/// `SetEnchantmentsFunction.run`: a book becomes an enchanted book by
/// `transmuteCopy` (count and components kept), then each entry's level
/// provider is drawn in map order and applied — `add` sums with the existing
/// level, otherwise it sets; a level of 0 or less removes.
fn set_enchantments<R: RandomSource>(f: &Value, stack: &mut Stack, ctx: &mut Ctx, rng: &mut R) {
    if normalise(&stack.item) == "minecraft:book" {
        stack.item = "minecraft:enchanted_book".into();
    }
    let add = f.get("add").and_then(Value::as_bool).unwrap_or(false);
    let entries = f
        .get("enchantments")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if entries.len() > 1 {
        // The codec is an unbounded map and the draw order is its order; a
        // JSON object read here is key-sorted, so more than one entry is not
        // known to be in vanilla's order.
        ctx.refuse("set_enchantments with more than one entry");
        return;
    }
    let key = if normalise(&stack.item) == "minecraft:enchanted_book" {
        "stored_enchantments"
    } else {
        "enchantments"
    };
    for (id, provider) in entries {
        let n = number_int(Some(&provider), ctx, rng);
        let id = normalise(&id);
        let m = stack
            .components
            .get_or_insert_with(key, || Value::Object(serde_json::Map::new()));
        if let Some(m) = m.as_object_mut() {
            let cur = m.get(&id).and_then(Value::as_i64).unwrap_or(0) as i32;
            let level = if add { cur + n } else { n };
            if level <= 0 {
                m.remove(&id);
            } else {
                m.insert(id, Value::from(level.min(255)));
            }
        }
    }
}

/// `LevelBasedValue.calculate(level)` for the kinds the corpus uses:
/// `constant` and `linear` (`base + perLevelAboveFirst * (level - 1)`).
fn level_based(v: Option<&Value>, level: i32) -> Option<f32> {
    match v? {
        Value::Number(n) => Some(n.as_f64()? as f32),
        Value::Object(o) => match bare(o.get("type").and_then(Value::as_str).unwrap_or("")) {
            "constant" => Some(o.get("value")?.as_f64()? as f32),
            "linear" => {
                let base = o.get("base")?.as_f64()? as f32;
                let per = o.get("per_level_above_first")?.as_f64()? as f32;
                Some(base + per * (level - 1) as f32)
            }
            _ => None,
        },
        _ => None,
    }
}

/// What `entity_slot` found. An unknown slot name refuses; an empty known
/// slot is vanilla's `false` for a null entity.
enum SlotLookup {
    /// A slot name `LootContext.EntityTarget` does not have.
    Unknown,
    /// A known, optional slot that this death did not fill. Vanilla's
    /// `test` answers `false` for a null entity.
    Absent,
    Present(LootEntity),
}

/// `LootContext.EntityTarget`'s five names, from its enum constants'
/// `getSerializedName`.
fn entity_slot(which: &str, ctx: &Ctx) -> SlotLookup {
    let e = &ctx.entity;
    let slot = match which {
        "this" => &e.this_entity,
        "attacker" | "attacking_entity" => &e.attacking_entity,
        "direct_attacker" | "direct_attacking_entity" => &e.direct_attacking_entity,
        "attacking_player" | "last_damage_player" => &e.last_damage_player,
        _ => return SlotLookup::Unknown,
    };
    match slot {
        Some(v) => SlotLookup::Present(v.clone()),
        None => SlotLookup::Absent,
    }
}

/// `EntityPredicate.matches`, over the sub-predicate keys the jar's own
/// entity loot tables use.
///
/// Every other key **refuses**, and that is the whole design of this
/// function: an unrecognised sub-predicate answered `false` would silently
/// remove a drop, which is exactly the failure this crate's header names for
/// `entity_properties`. The refusal names the key, so the message says which
/// sub-predicate to write next.
fn entity_predicate(pred: Option<&Value>, e: &LootEntity, ctx: &mut Ctx) -> bool {
    let Some(obj) = pred.and_then(Value::as_object) else {
        // `EntityPredicate.ANY` — no sub-predicates, matches anything
        // non-null.
        return true;
    };
    for (key, v) in obj {
        let ok = match bare(key) {
            "flags" => entity_flags(v, e, ctx),
            "equipment" => entity_equipment(v, e, ctx),
            // `type` in the codec; `entity_type` in the shape the loot lane's
            // golden vectors were dumped with.
            "type" | "entity_type" => {
                let want = v.as_str().unwrap_or("");
                if want.starts_with('#') {
                    ctx.refuse("entity predicate type by tag");
                    false
                } else {
                    normalise(want) == normalise(&e.kind)
                }
            }
            other => {
                ctx.refuse(&format!("entity predicate {other}"));
                false
            }
        };
        if !ok {
            return false;
        }
    }
    true
}

/// `EntityFlagsPredicate.matches` — each field is an `Optional<Boolean>` and
/// an absent one is not a constraint.
fn entity_flags(v: &Value, e: &LootEntity, ctx: &mut Ctx) -> bool {
    let Some(obj) = v.as_object() else {
        return true;
    };
    for (key, want) in obj {
        let Some(want) = want.as_bool() else {
            ctx.refuse(&format!("entity flag {key} is not a boolean"));
            return false;
        };
        let have = match key.as_str() {
            "is_on_fire" => e.on_fire,
            "is_baby" => e.baby,
            other => {
                ctx.refuse(&format!("entity flag {other}"));
                return false;
            }
        };
        if have != want {
            return false;
        }
    }
    true
}

/// `EntityEquipmentPredicate.matches` — a map of slot name to `ItemPredicate`.
fn entity_equipment(v: &Value, e: &LootEntity, ctx: &mut Ctx) -> bool {
    let Some(obj) = v.as_object() else {
        return true;
    };
    for (slot, item_pred) in obj {
        let Some(held) = e.equipment.get(slot) else {
            // An empty slot is `ItemStack.EMPTY`, which no `ItemPredicate`
            // with constraints matches. This is vanilla's `false`, not a
            // refusal: the slot IS known and IS empty.
            return false;
        };
        if !item_predicate(item_pred, held, ctx) {
            return false;
        }
    }
    true
}

/// `ItemPredicate.test`, over the one sub-predicate an entity table uses.
///
/// The `predicates` map is `DataComponentPredicate`s keyed by component id,
/// and `minecraft:enchantments` is a **list** of
/// `EnchantmentPredicate{enchantments, levels}` — every element must match.
fn item_predicate(pred: &Value, item: &LootItem, ctx: &mut Ctx) -> bool {
    let Some(obj) = pred.as_object() else {
        return true;
    };
    for (key, v) in obj {
        let ok = match key.as_str() {
            "items" => {
                let want = v.as_str().unwrap_or("");
                if let Some(tag) = want.strip_prefix('#') {
                    ctx.item_tags.get(tag).is_some_and(|ids| {
                        ids.iter().any(|i| normalise(i) == normalise(&item.item))
                    })
                } else {
                    normalise(want) == normalise(&item.item)
                }
            }
            "predicates" => {
                let Some(map) = v.as_object() else {
                    ctx.refuse("item predicate `predicates` is not an object");
                    return false;
                };
                let mut all = true;
                for (comp, spec) in map {
                    match bare(comp) {
                        "enchantments" => {
                            for term in spec.as_array().unwrap_or_else(|| none()) {
                                if !enchantment_predicate(term, item, ctx) {
                                    all = false;
                                }
                            }
                        }
                        other => {
                            ctx.refuse(&format!("item component predicate {other}"));
                            return false;
                        }
                    }
                }
                all
            }
            other => {
                ctx.refuse(&format!("item predicate {other}"));
                return false;
            }
        };
        if !ok {
            return false;
        }
    }
    true
}

/// `EnchantmentPredicate.containedIn` — the enchantment reference is either
/// one id or a `#tag`, and `levels` is an optional `MinMaxBounds.Ints`.
///
/// An enchantment that is not present at all is level 0, and level 0 is
/// **not** "has the enchantment": `EnchantmentHelper.getItemEnchantmentLevel`
/// returns 0 for absent and the predicate's own iteration is over the stack's
/// `ItemEnchantments` map, which does not hold zeroes.
fn enchantment_predicate(term: &Value, item: &LootItem, ctx: &mut Ctx) -> bool {
    // `EnchantmentPredicate.containedIn` iterates the STACK's own
    // `ItemEnchantments`, so a stack with no enchantment component matches
    // nothing — without ever consulting the tag. That ordering is not
    // cosmetic here: `crate::item::ItemStack` carries no components at all, so
    // every stack this server can hold takes this arm, and a tag lookup
    // before it would refuse every roll whose killer is holding something.
    if item.enchants.values().all(|l| *l <= 0) {
        return false;
    }
    let want = term
        .get("enchantments")
        .and_then(Value::as_str)
        .unwrap_or("");
    let matching: Vec<i32> = if let Some(tag) = want.strip_prefix('#') {
        let Some(ids) = ctx.enchantment_tags.get(tag) else {
            ctx.refuse(&format!(
                "enchantment tag {tag} is not in the enchantment tag map"
            ));
            return false;
        };
        ids.iter()
            .filter_map(|id| item.enchants.get(bare(id)).copied())
            .filter(|l| *l > 0)
            .collect()
    } else {
        item.enchants
            .get(bare(want))
            .copied()
            .filter(|l| *l > 0)
            .into_iter()
            .collect()
    };
    if matching.is_empty() {
        return false;
    }
    let Some(levels) = term.get("levels") else {
        return true;
    };
    matching.iter().any(|l| {
        let min = range_bound(levels.get("min").unwrap_or(levels));
        let max = range_bound(levels.get("max").unwrap_or(levels));
        min.is_none_or(|m| i64::from(*l) >= m) && max.is_none_or(|m| i64::from(*l) <= m)
    })
}

/// `DamageSourcePredicate.matches`, over `tags` only.
fn damage_source_predicate(pred: Option<&Value>, src: &LootDamageSource, ctx: &mut Ctx) -> bool {
    let Some(obj) = pred.and_then(Value::as_object) else {
        return true;
    };
    for (key, v) in obj {
        match bare(key) {
            "tags" => {
                for term in v.as_array().unwrap_or_else(|| none()) {
                    let id = term.get("id").and_then(Value::as_str).unwrap_or("");
                    let expected = term
                        .get("expected")
                        .and_then(Value::as_bool)
                        .unwrap_or(true);
                    if src.tags.contains(&normalise(id)) != expected {
                        return false;
                    }
                }
            }
            other => {
                ctx.refuse(&format!("damage source predicate {other}"));
                return false;
            }
        }
    }
    true
}

fn apply_bonus<R: RandomSource>(f: &Value, stack: &mut Stack, ctx: &mut Ctx, rng: &mut R) {
    let level = ctx.enchant(f.get("enchantment").and_then(Value::as_str).unwrap_or(""));
    match bare(f.get("formula").and_then(Value::as_str).unwrap_or("")) {
        // `ApplyBonusCount.OreDrops`: draws whatever the level, including zero.
        "ore_drops" => {
            let mut mult = rng.next_i32_bound(level + 2) - 1;
            if mult < 0 {
                mult = 0;
            }
            stack.count *= mult + 1;
        }
        "uniform_bonus_count" => {
            let per = f
                .get("parameters")
                .and_then(|p| p.get("bonusMultiplier"))
                .and_then(Value::as_i64)
                .unwrap_or(1) as i32;
            stack.count += rng.next_i32_bound(per * level + 1);
        }
        "binomial_with_bonus_count" => {
            let p = f
                .get("parameters")
                .and_then(|p| p.get("probability"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0) as f32;
            let extra = f
                .get("parameters")
                .and_then(|p| p.get("extraRounds"))
                .and_then(Value::as_i64)
                .unwrap_or(0) as i32;
            for _ in 0..(level + extra) {
                if rng.next_f32() < p {
                    stack.count += 1;
                }
            }
        }
        other => ctx.refuse(&format!("apply_bonus formula {other}")),
    }
}

// ------------------------------------------------------------- number providers

fn number_int<R: RandomSource>(v: Option<&Value>, ctx: &mut Ctx, rng: &mut R) -> i32 {
    match v {
        None => 1,
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0).round() as i32,
        Some(o) => match bare(o.get("type").and_then(Value::as_str).unwrap_or("constant")) {
            "constant" => number_int(o.get("value"), ctx, rng),
            "uniform" => {
                let min = number_int(o.get("min"), ctx, rng);
                let max = number_int(o.get("max"), ctx, rng);
                if max <= min {
                    min
                } else {
                    min + rng.next_i32_bound(max - min + 1)
                }
            }
            "binomial" => {
                let n = number_int(o.get("n"), ctx, rng);
                let p = number_float(o.get("p"), ctx, rng);
                let mut hits = 0;
                for _ in 0..n {
                    if rng.next_f32() < p {
                        hits += 1;
                    }
                }
                hits
            }
            other => {
                ctx.refuse(&format!("number provider {other}"));
                0
            }
        },
    }
}

fn number_float<R: RandomSource>(v: Option<&Value>, ctx: &mut Ctx, rng: &mut R) -> f32 {
    match v {
        None => 0.0,
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0) as f32,
        Some(o) => match bare(o.get("type").and_then(Value::as_str).unwrap_or("constant")) {
            "constant" => number_float(o.get("value"), ctx, rng),
            "uniform" => {
                let min = number_float(o.get("min"), ctx, rng);
                let max = number_float(o.get("max"), ctx, rng);
                min + rng.next_f32() * (max - min)
            }
            _ => number_int(v, ctx, rng) as f32,
        },
    }
}

// ---------------------------------------------------------------- the dispatch

/// Everything needed to answer "what does breaking this block drop", loaded once.
///
/// # Why this type exists at all
///
/// The interpreter above is inert. It can roll any table it is handed and it has
/// no idea which table to hand it, which is the difference between 1,162 correct
/// tables and a server that drops cobblestone. `use_item_on.json` sat generated
/// and gated and *unread* for exactly this reason, and wiring one dispatch took
/// `acted` from 0 to 3,273 cells. This is that dispatch for loot.
///
/// # The mapping is data, and it is not the file names
///
/// A table called `blocks/stone.json` belongs to `minecraft:stone` 1,113 times
/// out of 1,196 — and inferring the other 83 from the convention would give 61
/// blocks another block's drops and 22 blocks drops they should not have, every
/// one of which would look like a correct port. `block_loot_map.json` is
/// extracted from `BlockBehaviour.getLootTable()` by `ci/loot-golden.sh` and is
/// the only mapping this type will use. **A block absent from it is an error,
/// never a default.**
#[derive(Debug)]
pub struct Loot {
    tables: Arc<BTreeMap<String, Value>>,
    item_tags: Arc<BTreeMap<String, Vec<String>>>,
    /// block id -> loot table id, or `None` where vanilla says the block has no
    /// table at all. The distinction matters: "no table" and "table not found"
    /// must never collapse into the same empty answer.
    block_tables: BTreeMap<String, Option<String>>,
}

/// Why a drop could not be produced. Never merged with "dropped nothing".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoDrops {
    /// Vanilla says this block has no loot table. An empty drop is CORRECT.
    NoTableByDesign,
    /// The block is not in the extracted map at all — our data is stale or the
    /// caller invented an id. This is a bug, not a behaviour.
    UnknownBlock(String),
    /// The map names a table that is not in the corpus.
    MissingTable(String),
    /// The roll reached an opcode or a context this interpreter does not have.
    Refused(String),
}

impl Loot {
    pub fn new(
        tables: Arc<BTreeMap<String, Value>>,
        item_tags: Arc<BTreeMap<String, Vec<String>>>,
        block_tables: BTreeMap<String, Option<String>>,
    ) -> Self {
        Self {
            tables,
            item_tags,
            block_tables,
        }
    }

    /// What breaking `state.block` with `tool` drops, at `seed`.
    ///
    /// Returns `Err` rather than an empty vector whenever the answer is not
    /// known, so a caller cannot mistake "we could not work it out" for "it
    /// drops nothing" — which is the single most likely way a loot port reads as
    /// working while silently deleting every drop it does not understand.
    pub fn block_drops(
        &self,
        state: &BlockRef,
        tool: Option<&str>,
        seed: i64,
    ) -> Result<Vec<Stack>, NoDrops> {
        let table_id = match self.block_tables.get(&state.block) {
            None => return Err(NoDrops::UnknownBlock(state.block.clone())),
            Some(None) => return Err(NoDrops::NoTableByDesign),
            Some(Some(id)) => id,
        };
        let key = table_id.strip_prefix("minecraft:").unwrap_or(table_id);
        let Some(table) = self.tables.get(key) else {
            return Err(NoDrops::MissingTable(table_id.clone()));
        };
        let mut ctx = Ctx {
            block: Some(state.clone()),
            tool: tool.map(str::to_string),
            item_tags: Arc::clone(&self.item_tags),
            tables: Arc::clone(&self.tables),
            ..Default::default()
        };
        let out = roll(table, &mut ctx, seed);
        match ctx.refused {
            Some(why) => Err(NoDrops::Refused(why)),
            None => Ok(out),
        }
    }

    /// What `entity_id` drops when it dies, rolled against `rng`.
    ///
    /// The table id is the caller's, not this dispatch's: `Loot` maps
    /// **blocks** to tables and there is no entity map here, because
    /// `EntityType.getDefaultLootTable` is `entities/<path>` for every type in
    /// the jar with a table and `noLootTable()` for the rest, and that is a
    /// fact about the entity type — which lives in `rustmc-server`'s
    /// `mob::MobKind` — rather than about loot. A wrong id here is a
    /// `MissingTable`, not a silent empty drop.
    ///
    /// `rng` is the caller's for the reason [`roll_with`] gives: an entity
    /// table with a `random_sequence` and a zero `lootTableSeed` draws from
    /// the level's named Xoroshiro sequence, not from a fresh legacy source.
    pub fn entity_drops<R: RandomSource>(
        &self,
        table_id: &str,
        entity: EntityCtx,
        smelting: Option<Arc<BTreeMap<String, String>>>,
        enchantment_tags: Arc<BTreeMap<String, Vec<String>>>,
        luck: f32,
        rng: &mut R,
    ) -> Result<Vec<Stack>, NoDrops> {
        let key = table_id.strip_prefix("minecraft:").unwrap_or(table_id);
        let Some(table) = self.tables.get(key) else {
            return Err(NoDrops::MissingTable(table_id.to_string()));
        };
        // `enchanted_count_increase` and `random_chance_with_enchanted_bonus`
        // read the ATTACKING entity's equipment (`Ctx::attacker_level`), not
        // a tool, so `enchants` stays empty here.
        let mut ctx = Ctx {
            item_tags: Arc::clone(&self.item_tags),
            tables: Arc::clone(&self.tables),
            entity,
            smelting,
            enchantment_tags,
            luck,
            ..Default::default()
        };
        let out = roll_with(table, &mut ctx, rng);
        match ctx.refused {
            Some(why) => Err(NoDrops::Refused(why)),
            None => Ok(out),
        }
    }

    /// [`Self::block_drops`] with the tool's enchantments, bare id to level —
    /// what `match_tool`'s `minecraft:enchantments` sub-predicate and
    /// `apply_bonus` read (Silk Touch, Fortune).
    pub fn block_drops_enchanted(
        &self,
        state: &BlockRef,
        tool: Option<&str>,
        enchants: BTreeMap<String, i32>,
        seed: i64,
    ) -> Result<Vec<Stack>, NoDrops> {
        let table_id = match self.block_tables.get(&state.block) {
            None => return Err(NoDrops::UnknownBlock(state.block.clone())),
            Some(None) => return Err(NoDrops::NoTableByDesign),
            Some(Some(id)) => id,
        };
        let key = table_id.strip_prefix("minecraft:").unwrap_or(table_id);
        let Some(table) = self.tables.get(key) else {
            return Err(NoDrops::MissingTable(table_id.clone()));
        };
        let mut ctx = Ctx {
            block: Some(state.clone()),
            tool: tool.map(str::to_string),
            enchants,
            item_tags: Arc::clone(&self.item_tags),
            tables: Arc::clone(&self.tables),
            ..Default::default()
        };
        let out = roll(table, &mut ctx, seed);
        match ctx.refused {
            Some(why) => Err(NoDrops::Refused(why)),
            None => Ok(out),
        }
    }

    /// One table's raw document, by id with or without the `minecraft:`
    /// prefix.
    ///
    /// Exposed because a caller has to read a table's own `random_sequence`
    /// field: `LootContext.Builder.create(Optional<Identifier>)` takes the
    /// sequence name from the **table**, and a caller that derived the name
    /// from the table id instead would be right for every entity table in
    /// this jar and wrong for the first one that is not — a difference no
    /// drop count reveals, because both names produce plausible numbers.
    #[must_use]
    pub fn table(&self, id: &str) -> Option<&Value> {
        let key = id.strip_prefix("minecraft:").unwrap_or(id);
        self.tables.get(key)
    }

    /// Every block this dispatch can answer for. Useful as a coverage figure and
    /// deliberately not called "supported": it counts blocks whose table rolls
    /// without refusing, not blocks whose drops have been verified.
    pub fn answerable_blocks(&self) -> usize {
        self.block_tables
            .iter()
            .filter(|(b, t)| match t {
                None => false,
                Some(id) => {
                    let key = id.strip_prefix("minecraft:").unwrap_or(id);
                    self.tables.contains_key(key) && !b.is_empty()
                }
            })
            .count()
    }
}

// ---------------------------------------------------------------------------

/// The draw-order facts this module's header states, asserted rather than
/// described.
///
/// **This module exists because the crate's lib target could not fail.** It
/// held no `#[test]` at all, and `ci/checks/test-targets.toml` runs the lib
/// target in T0 and T1, so a green gate on it measured the interpreter
/// *compiling* — `EXECUTES NO ASSERTION`, which
/// `ci/check-test-targets.py` refuses and which is this ledger's founding
/// defect. The three integration targets carry the parity claim; what was
/// missing is anything at all inside the unit target the gates also run.
///
/// The facts chosen are the three the header calls out as invisible in any
/// loot table's JSON and derived from `javap -c` on the pinned jar, because
/// those are the ones a reading of the datapack cannot check. Nothing here
/// reads a fixture, so it runs in a clean clone.
#[cfg(test)]
mod tests {
    use super::*;

    fn table(json: &str) -> Value {
        serde_json::from_str(json).expect("a table")
    }

    /// `LootPool.addRandomItem` draws `nextInt(totalWeight)` only when the
    /// pool expanded to MORE than one live entry. A single-entry pool consumes
    /// no randomness, which is why the header can say 1,043 of the 1,111
    /// evaluable block tables give the same answer under every seed.
    ///
    /// Measured as a *stream* property and not as an output one: the item is
    /// the same either way, so comparing items proves nothing. Two pools of
    /// one entry each, followed by a two-entry pool, and the choice the last
    /// pool makes is what moves if either earlier pool drew.
    #[test]
    fn a_single_entry_pool_consumes_no_randomness() {
        let one = table(
            r#"{"pools":[
                 {"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:a"}]},
                 {"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:b"}]},
                 {"rolls":1,"entries":[
                    {"type":"minecraft:item","name":"minecraft:x"},
                    {"type":"minecraft:item","name":"minecraft:y"}]}]}"#,
        );
        let two = table(
            r#"{"pools":[
                 {"rolls":1,"entries":[
                    {"type":"minecraft:item","name":"minecraft:x"},
                    {"type":"minecraft:item","name":"minecraft:y"}]}]}"#,
        );
        // Over a spread of seeds the third pool of `one` must pick exactly what
        // the only pool of `two` picks: the two single-entry pools before it
        // have advanced the stream by nothing.
        for i in 0..64i64 {
            let seed = i.wrapping_mul(0x9E37_79B9_7F4A_7C15u64 as i64);
            let a = roll(&one, &mut Ctx::default(), seed);
            let b = roll(&two, &mut Ctx::default(), seed);
            assert_eq!(a.len(), 3, "seed {seed}: three pools, three stacks");
            assert_eq!(
                a[2].item, b[0].item,
                "seed {seed}: a single-entry pool drew from the generator"
            );
        }
        // AND IT CAN GO RED. `Faults::always_draw_entry` is the crate's own
        // injection for exactly this mistake — draw `nextInt(totalWeight)`
        // even for one live entry — so the assertion above is shown biting
        // rather than asserted to. Without this the test would pass on an
        // implementation that never draws anywhere.
        let mut moved = 0;
        for i in 0..64i64 {
            let seed = i.wrapping_mul(0x9E37_79B9_7F4A_7C15u64 as i64);
            let mut bad = Ctx {
                faults: Faults {
                    always_draw_entry: true,
                    ..Faults::default()
                },
                ..Ctx::default()
            };
            let a = roll(&one, &mut bad, seed);
            let b = roll(&two, &mut Ctx::default(), seed);
            if a[2].item != b[0].item {
                moved += 1;
            }
        }
        assert!(
            moved > 0,
            "always_draw_entry moved nothing, so the assertion above is vacuous"
        );
    }

    /// And the control for the test above, without which it would pass on an
    /// implementation that never draws at all: the two-entry pool must be
    /// seed-dependent, or "the same under every seed" is vacuous.
    #[test]
    fn a_two_entry_pool_is_seed_dependent() {
        let t = table(
            r#"{"pools":[{"rolls":1,"entries":[
                 {"type":"minecraft:item","name":"minecraft:x"},
                 {"type":"minecraft:item","name":"minecraft:y"}]}]}"#,
        );
        // The seeds are SPREAD, not 0..64, and that is a finding rather than a
        // convenience. `LegacyRandomSource` is `java.util.Random`, whose
        // `nextInt(2)` is `(int)((2 * (long) next(31)) >> 31)` — the top bit of
        // a value that moves almost linearly in the seed for small consecutive
        // seeds. Over 0..64 it is **0 every time**, so this control read
        // "the pool never draws" on an implementation that draws correctly.
        let mut seen = std::collections::BTreeSet::new();
        for i in 0..64i64 {
            let seed = i.wrapping_mul(0x9E37_79B9_7F4A_7C15u64 as i64);
            seen.insert(roll(&t, &mut Ctx::default(), seed)[0].item.clone());
        }
        assert_eq!(
            seen.len(),
            2,
            "a two-entry pool produced one item over 64 seeds — it is not drawing"
        );
    }

    /// A roll that needed something this context cannot supply sets `refused`,
    /// and an empty result with `refused` set is NOT "no drops". The header
    /// says a condition reading an absent subsystem must refuse rather than
    /// guess; this is that, asserted.
    #[test]
    fn an_unsupplied_subsystem_refuses_rather_than_dropping_nothing() {
        let t = table(
            r#"{"pools":[{"rolls":1,"entries":[
                 {"type":"minecraft:item","name":"minecraft:stone",
                  "conditions":[{"condition":"minecraft:location_check",
                                 "predicate":{}}]}]}]}"#,
        );
        let mut ctx = Ctx::default();
        let got = roll(&t, &mut ctx, 1);
        // `location_check` reads the level at ORIGIN, which no `Ctx` here
        // carries. (`entity_properties` stood here until train-g: an absent
        // entity slot is vanilla's own `false`, graded against the jar's
        // vectors by `golden_loot`'s no-entity arm, so it no longer refuses.)
        assert!(
            ctx.refused.is_some(),
            "location_check was answered without a level: {got:?}"
        );
    }
}
