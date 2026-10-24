//! Loot on this server: the tables, the world's random sequences, and the two
//! places a container gets a table and then rolls it.
//!
//! The interpreter, the random sequences and `LootTable.fill` live in
//! `crates/rustmc-loot`, each gated there against vectors vanilla produced
//! (`ci/loot-golden.sh`). This module is only the wiring, and the wiring is
//! two transcriptions:
//!
//! * **`BlockItem.updateCustomBlockEntityTag`** — a placed block item whose
//!   stack carries `minecraft:block_entity_data` loads that tag into the new
//!   block entity, if the type matches (offset 52, `if_acmpeq`) and the type
//!   is not op-only (offset 59). For a container that is how a `LootTable`
//!   and `LootTableSeed` arrive: `tryLoadLootTable`.
//! * **`RandomizableContainer.unpackLootTable(Player)`** — called from
//!   `RandomizableContainerBlockEntity.createMenu` (offset 13) **before** the
//!   menu is built, so before `startOpen` plays the lid and the sound:
//!
//!   ```text
//!    14: lootTable = getLootTable(); null -> return
//!    45: table = server.reloadableRegistries().getLootTable(lootTable)
//!    65: CriteriaTriggers.GENERATE_LOOT.trigger(player, lootTable)
//!    77: setLootTable(null)
//!    82: params = ORIGIN = Vec3.atCenterOf(pos)
//!   109:          withLuck(player.getLuck()), THIS_ENTITY = player
//!   126: table.fill(this, params.create(CHEST), getLootTableSeed())
//!   ```
//!
//! # Two unlanded branches carry a second copy of part of this, by name
//!
//! `entity-ports` added a `crate::loot` (`ServerLoot`, the death-drop half,
//! `mob::drops`) and a `crate::randomsequences` store with no world seed
//! wired to it. `train-g` folded that store into this one's — the two were
//! the same `RandomSequence.createSequence`, and this one's is gated against
//! vanilla's own vectors (`rustmc-loot`'s
//! `random_sequences_reproduce_vanillas_own_streams`) and seeded by `--seed`
//! — so a mob's death roll and a chest's draw from one store. `ranged`
//! carries the same commits and meets the same fold.
//!
//! # What is not here, said plainly
//!
//! * **Components on the rolled stacks are dropped.** This server's
//!   `ItemStack` carries one component (`block_entity_data`) and the loot
//!   functions that set others — enchantments, damage, potions — still make
//!   their draws in the interpreter, so the stream stays aligned and every
//!   *later* slot is right; the stack that carried the component arrives
//!   without it.
//! * **A roll the interpreter refuses fills nothing** and says so on stderr.
//!   A refusal means the generator stopped at an unknown point, and any slot
//!   written after it would be a guess.
//! * **`getItem` does not unpack.** Vanilla's `RandomizableContainerBlockEntity
//!   .getItem`/`removeItem`/`setItem` each call `unpackLootTable(null)`, so a
//!   hopper or comparator reading an unopened chest fills it too. This server
//!   unpacks on open only.
//! * **The `GENERATE_LOOT` advancement trigger** fires nothing here; no
//!   advancement in the shipped pack listens to it.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

use rustmc_loot::fill;
use rustmc_loot::sequence::RandomSequences;
use rustmc_loot::{context_random, roll_with, Ctx, Stack};
use rustmc_world::nbt::Nbt;
use serde_json::Value;

use crate::blockentity::{BlockEntity, LootRef};
use crate::item::ItemStack;
use crate::itemprops::ItemProps;
use crate::patch::Patch;

/// The loot tables, as `ci/loot-golden.sh` writes them out of the jar.
pub const TABLES_PATH: &str = "data/vanilla/loot_tables.json";
/// The item tags the tables' `match_tool` predicates resolve.
pub const ITEM_TAGS_PATH: &str = "data/vanilla/item_tags.json";

static WORLD_SEED: OnceLock<i64> = OnceLock::new();
static SEQUENCES: OnceLock<Mutex<RandomSequences>> = OnceLock::new();
static DATA: OnceLock<Option<Data>> = OnceLock::new();

struct Data {
    tables: Arc<BTreeMap<String, Value>>,
    item_tags: Arc<BTreeMap<String, Vec<String>>>,
    enchanting: Option<Arc<dyn rustmc_loot::Enchanting>>,
}

/// `rustmc_loot::Enchanting` over this server's enchantment registry
/// (`crate::enchantment`) and its `EnchantmentHelper.selectEnchantment`
/// (`crate::enchanting::select_enchantment`) — the port the enchanting table
/// already uses, so loot and the table cannot disagree about it.
#[derive(Debug)]
pub struct ServerEnchanting(pub crate::enchantment::Registry);

fn ns(id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("minecraft:{id}")
    }
}

impl rustmc_loot::Enchanting for ServerEnchanting {
    fn holder_set(&self, spec: Option<&Value>) -> Option<Vec<String>> {
        match spec {
            None => Some(self.0.order().to_vec()),
            Some(Value::String(s)) => match s.strip_prefix('#') {
                Some(tag) => self.0.tag(&ns(tag)).map(<[String]>::to_vec),
                None => Some(vec![ns(s)]),
            },
            Some(Value::Array(a)) => a.iter().map(|v| v.as_str().map(ns)).collect(),
            Some(_) => None,
        }
    }
    fn can_enchant(&self, enchantment: &str, item: &str) -> bool {
        self.0
            .get(&ns(enchantment))
            .is_some_and(|e| e.is_supported_item(&ns(item)))
    }
    fn level_range(&self, enchantment: &str) -> Option<(i32, i32)> {
        self.0
            .get(&ns(enchantment))
            .map(|e| (crate::enchantment::Enchantment::min_level(), e.max_level))
    }
    fn select(
        &self,
        mut rng: &mut dyn rustmc_worldgen::rng::RandomSource,
        item: &str,
        level: i32,
        stream: &[String],
    ) -> Vec<(String, i32)> {
        crate::enchanting::select_enchantment(&mut rng, &self.0, &ns(item), level, stream)
            .into_iter()
            .map(|i| (i.enchantment, i.level))
            .collect()
    }
    fn max_damage(&self, item: &str) -> Option<i32> {
        self.0.max_damage(&ns(item))
    }
}

/// Set the world seed, once, before any loot is rolled. A second call is
/// ignored: a world has one seed.
pub fn set_world_seed(seed: i64) {
    let _ = WORLD_SEED.set(seed);
}

/// The server-wide `RandomSequences`, created on first use from the world
/// seed (0 if none was set).
pub fn sequences() -> &'static Mutex<RandomSequences> {
    SEQUENCES.get_or_init(|| Mutex::new(RandomSequences::new(*WORLD_SEED.get().unwrap_or(&0))))
}

/// `random_sequences.dat`'s `sequences` compound: every sequence that has
/// been drawn from (or created), `{id: {source: [L; seedLo, seedHi]}}` —
/// `RandomSequence.CODEC` is a record of one `source` field, and
/// `Xoroshiro128PlusPlus.CODEC` a two-long stream. A sequence never created
/// is absent, which is also how vanilla writes a fresh world.
#[must_use]
pub fn sequences_nbt() -> Nbt {
    nbt_of_saved(
        &sequences()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .saved(),
    )
}

fn nbt_of_saved(saved: &rustmc_loot::sequence::SavedSequences) -> Nbt {
    let mut m = BTreeMap::new();
    for (id, lo, hi) in &saved.sequences {
        let mut entry = BTreeMap::new();
        entry.insert("source".to_string(), Nbt::LongArray(vec![*lo, *hi]));
        m.insert(id.clone(), Nbt::Compound(entry));
    }
    Nbt::Compound(m)
}

/// Reopen the sequences a previous session saved — the `data` compound of
/// `random_sequences.dat` — so a reloaded world's next chest or death rolls
/// what it would have rolled had the server never stopped.
///
/// Must run before anything rolls: the store is created once, and a store
/// already created (by a roll, or an earlier call) is left alone and `false`
/// returned. An entry whose `source` is not two longs is skipped, which
/// recreates that sequence fresh on first use — vanilla's codec would refuse
/// the file instead.
pub fn restore_sequences(data: &Nbt) -> bool {
    let world_seed = *WORLD_SEED.get().unwrap_or(&0);
    SEQUENCES
        .set(Mutex::new(RandomSequences::restore(
            world_seed,
            &saved_of_nbt(data),
        )))
        .is_ok()
}

fn saved_of_nbt(data: &Nbt) -> rustmc_loot::sequence::SavedSequences {
    let salt = match data.get("salt") {
        Some(Nbt::Int(v)) => *v,
        _ => 0,
    };
    let flag = |k: &str| !matches!(data.get(k), Some(Nbt::Byte(0)));
    let mut saved = rustmc_loot::sequence::SavedSequences {
        salt,
        include_world_seed: flag("include_world_seed"),
        include_sequence_id: flag("include_sequence_id"),
        sequences: Vec::new(),
    };
    if let Some(m) = data.get("sequences").and_then(Nbt::as_compound) {
        for (id, entry) in m {
            if let Some([lo, hi]) = entry.get("source").and_then(Nbt::as_long_array) {
                saved.sequences.push((id.clone(), *lo, *hi));
            }
        }
    }
    saved
}

fn data() -> Option<&'static Data> {
    DATA.get_or_init(|| match Data::load(std::path::Path::new(".")) {
        Ok(d) => Some(d),
        Err(e) => {
            eprintln!("loot: {e} — no loot will roll (ci/loot-golden.sh writes the tables)");
            None
        }
    })
    .as_ref()
}

impl Data {
    /// The three tables under `root` (the server's working directory, which
    /// is the repository root wherever it runs).
    fn load(root: &std::path::Path) -> Result<Self, String> {
        let load = |p: &str| -> Result<Value, String> {
            let path = root.join(p);
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{p}: {e}"))?;
            serde_json::from_str(&text).map_err(|e| format!("{p}: {e}"))
        };
        let tables = match load(TABLES_PATH)? {
            Value::Object(m) => m.into_iter().collect::<BTreeMap<_, _>>(),
            _ => return Err(format!("{TABLES_PATH} is not an object")),
        };
        let item_tags = serde_json::from_value(load(ITEM_TAGS_PATH)?)
            .map_err(|e| format!("{ITEM_TAGS_PATH}: {e}"))?;
        // Without the registry the enchanting functions refuse, which fills
        // nothing for a table that reaches one — said, not silent.
        let enchanting =
            match crate::enchantment::Registry::load(&root.join(crate::enchantment::TABLE_PATH)) {
                Ok(r) => Some(Arc::new(ServerEnchanting(r)) as Arc<dyn rustmc_loot::Enchanting>),
                Err(e) => {
                    eprintln!(
                        "loot: {}: {e} — enchanting loot functions will refuse",
                        crate::enchantment::TABLE_PATH
                    );
                    None
                }
            };
        Ok(Self {
            tables: Arc::new(tables),
            item_tags: Arc::new(item_tags),
            enchanting,
        })
    }
}

/// The `LootTable`/`LootTableSeed` a `block_entity_data` tag carries, read as
/// `RandomizableContainer.tryLoadLootTable` reads them: the table through
/// `LootTable.KEY_CODEC` (an identifier), the seed through
/// `getLongOr("LootTableSeed", 0L)` — so a seed of any other tag type is 0.
#[must_use]
pub fn loot_ref_of(tag: &Nbt) -> Option<LootRef> {
    let c = tag.as_compound()?;
    let table = c.get("LootTable")?.as_str()?;
    let table = if table.contains(':') {
        table.to_string()
    } else {
        format!("minecraft:{table}")
    };
    let seed = match c.get("LootTableSeed") {
        Some(Nbt::Long(v)) => *v,
        _ => 0,
    };
    Some(LootRef { table, seed })
}

/// `BlockItem.updateCustomBlockEntityTag` for the part of a tag this server
/// models: a randomizable container's loot table.
///
/// `be_type` is the stack's `TypedEntityData.type()` as a registry id and
/// `kind_type` the placed block entity's; a mismatch is offset 52's `return
/// false` and changes nothing. No kind this server models as randomizable is
/// `onlyOpCanSetNbt`, so offset 59's operator test cannot refuse here.
///
/// Returns whether the block entity changed.
pub fn apply_block_entity_data(
    be: &mut BlockEntity,
    be_type: i32,
    kind_type: Option<i32>,
    tag_bytes: &[u8],
) -> bool {
    if kind_type != Some(be_type) {
        return false;
    }
    let mut r = std::io::Cursor::new(tag_bytes);
    let Ok(Some(tag)) = rustmc_world::nbt::read_network_tag(&mut r) else {
        return false;
    };
    match loot_ref_of(&tag) {
        Some(l) if be.kind().is_randomizable() => {
            be.set_loot(Some(l));
            true
        }
        _ => false,
    }
}

/// The containers generation placed with a loot table, into the block-entity
/// store — so a chest that decoration or a structure generated opens stocked
/// instead of empty.
///
/// Generation already knows: `RandomizableContainer.setBlockEntityLootTable(
/// level, random, pos, table)` is `setLootTable(table, random.nextLong())`,
/// and the worldgen crate records that draw and the table at the position it
/// happened (`rustmc_worldgen::block_entity::Record` — the side channel
/// `ci/be-channel-identical.sh` proves leaves every block state unchanged and
/// `ci/be-payload-r16-12345.sh` compares against vanilla's saved payloads).
/// What was missing is the hand-off: the server's canvas kept the block and
/// dropped the record, so the chest had no block entity at all.
///
/// A record becomes a block entity only when it carries a `LootTable`, the
/// block standing there has a randomizable container kind, and the store has
/// nothing at that position yet — a chest already opened (and so already
/// holding its loot, with the table cleared) is never re-armed by a tile
/// being generated a second time.
///
/// Returns how many were adopted.
pub fn adopt_generated(
    canvas: &rustmc_worldgen::canvas::Canvas,
    chunks: impl Iterator<Item = (i32, i32)>,
    store: &crate::blockentity::BlockEntities,
) -> usize {
    let mut n = 0;
    for (cx, cz) in chunks {
        for ((x, y, z), record) in canvas.block_entity_records(cx, cz) {
            let Some(l) = loot_ref_of(&Nbt::Compound(record.nbt.clone())).map(|mut l| {
                // The draw is the record's own; a `LootTableSeed` in the
                // payload (a template's) is the fallback `tryLoadLootTable`
                // would read.
                if let Some(seed) = record.loot_seed {
                    l.seed = seed;
                }
                l
            }) else {
                continue;
            };
            let pos = (*x, *y, *z);
            let Some(kind) = crate::blockentity::for_state(canvas.block_at(*x, *y, *z)) else {
                continue;
            };
            if !kind.is_randomizable() || store.get(pos).is_some() {
                continue;
            }
            let mut be = BlockEntity::new(kind);
            be.set_loot(Some(l));
            store.insert(pos, be);
            n += 1;
        }
    }
    n
}

/// `RandomizableContainer.unpackLootTable(player)` then `LootTable.fill`.
///
/// `luck` is `player.getLuck()`; `max_stack` answers
/// `ItemStack.getMaxStackSize()` for the stack splitter.
///
/// Returns `Ok(true)` when a table was rolled into the container, `Ok(false)`
/// when there was none, and `Err` when one was taken off the container but
/// could not be rolled faithfully — the table is still cleared, as
/// `setLootTable(null)` at offset 77 precedes the roll.
pub fn unpack(
    be: &mut BlockEntity,
    luck: f32,
    max_stack: &dyn Fn(&str) -> i32,
    props: Option<&ItemProps>,
) -> Result<bool, String> {
    let Some(l) = be.take_loot() else {
        return Ok(false);
    };
    let d = data().ok_or("no loot tables loaded")?;
    let key = l.table.strip_prefix("minecraft:").unwrap_or(&l.table);
    // `ReloadableServerRegistries.Holder.getLootTable` answers
    // `LootTable.EMPTY` for an unknown key: nothing to roll and no sequence,
    // so with seed 0 the slot shuffle would draw on the level's own random —
    // which fills nothing either way, so there is nothing to place.
    let Some(table) = d.tables.get(key) else {
        return Ok(true);
    };
    let mut ctx = Ctx {
        luck,
        item_tags: Arc::clone(&d.item_tags),
        tables: Arc::clone(&d.tables),
        enchanting: d.enchanting.clone(),
        ..Default::default()
    };
    let mut seqs = sequences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(mut rng) = context_random(table, l.seed, &mut seqs) else {
        return Err(format!(
            "{} names no random_sequence and has seed 0: the level random is not modelled",
            l.table
        ));
    };
    let raw = roll_with(table, &mut ctx, &mut rng.as_dyn());
    if let Some(why) = ctx.refused {
        return Err(format!("{}: {why}", l.table));
    }
    // `fill` places into `getAvailableSlots` — the empty ones — and never
    // touches an occupied slot, so an occupied one is marked with a stand-in
    // and left exactly as it was, patch and all.
    let mut slots: Vec<Option<Stack>> = (0..be.size())
        .map(|i| {
            let s = be.item(i);
            (!s.is_empty()).then(|| Stack::new(s.item(), s.count()))
        })
        .collect();
    let was_empty: Vec<bool> = slots.iter().map(Option::is_none).collect();
    // Convert every rolled stack's components before anything is placed, so
    // a component this server cannot write refuses the whole fill rather
    // than leaving half a chest.
    let split = fill::split_stacks(raw, max_stack);
    for s in &split {
        patch_of(s, props)?;
    }
    fill::place(split, &mut slots, rng.as_dyn());
    for (i, s) in slots.into_iter().enumerate() {
        if !was_empty[i] {
            continue;
        }
        let stack = match s {
            None => ItemStack::empty(),
            Some(s) => ItemStack::new(&s.item, s.count).with_patch(patch_of(&s, props)?),
        };
        be.set_item(i, stack);
    }
    Ok(true)
}

/// A rolled stack's components as the `DataComponentPatch` vanilla's
/// `PatchedDataComponentMap` ends up holding.
///
/// Order is first-set order ([`rustmc_loot::Components`]). A value equal to
/// the item's prototype value is not in the patch at all —
/// `PatchedDataComponentMap.set` offsets 9..28 compare with `Objects.equals`
/// against the prototype and *remove* the key — which is how a goat horn
/// rolled `ponder_goat_horn`, an ominous bottle at amplifier 0 and a
/// `set_damage` that lands on 0 all reach the client as prototypes. Without
/// `props` nothing is known about prototypes and every value is kept.
///
/// # Errors
/// On a component this server has no stream codec for: writing it as
/// nothing would put a prototype on the client where vanilla puts something
/// else.
pub fn patch_of(stack: &Stack, props: Option<&ItemProps>) -> Result<Patch, String> {
    use crate::patch::Component as C;
    let item = rustmc_loot::normalise(&stack.item);
    let mut patch = Patch::default();
    for (key, v) in stack.components.iter() {
        let name = format!("minecraft:{key}");
        let proto = props.and_then(|p| p.prototype(&item, &name));
        let int = || {
            v.as_i64()
                .and_then(|i| i32::try_from(i).ok())
                .ok_or_else(|| format!("{item}: {name} is not an int: {v}"))
        };
        let pairs = |v: &Value| -> Result<Vec<(String, i32)>, String> {
            v.as_object()
                .ok_or_else(|| format!("{item}: {name} is not a map: {v}"))?
                .iter()
                .map(|(k, l)| {
                    let l = l
                        .as_i64()
                        .and_then(|l| i32::try_from(l).ok())
                        .ok_or_else(|| format!("{item}: {name} level {l}"))?;
                    Ok((rustmc_loot::normalise(k), l))
                })
                .collect()
        };
        let (component, is_proto) = match key {
            "damage" => {
                let d = int()?;
                (
                    C::Damage(d),
                    proto.and_then(Value::as_i64) == Some(i64::from(d)),
                )
            }
            "ominous_bottle_amplifier" => {
                let a = int()?;
                (
                    C::OminousBottleAmplifier(a),
                    proto.and_then(Value::as_i64) == Some(i64::from(a)),
                )
            }
            "enchantments" | "stored_enchantments" => {
                let p = pairs(v)?;
                // The prototype is `ItemEnchantments.EMPTY` on every item
                // that has the component; only an emptied map equals it.
                let is_proto = p.is_empty()
                    && proto
                        .and_then(Value::as_object)
                        .is_some_and(serde_json::Map::is_empty);
                let c = if key == "enchantments" {
                    C::Enchantments(p)
                } else {
                    C::StoredEnchantments(p)
                };
                (c, is_proto)
            }
            "potion_contents" => {
                let id = v
                    .as_str()
                    .ok_or_else(|| format!("{item}: potion_contents is not a potion id: {v}"))?;
                // `PotionContents.withPotion` on `EMPTY`: never the prototype,
                // which carries no potion.
                (C::PotionContents(Some(rustmc_loot::normalise(id))), false)
            }
            "suspicious_stew_effects" => {
                // `[effect, duration]`, one entry — `withEffectAdded` on the
                // empty list.
                let a = v.as_array().filter(|a| a.len() == 2);
                let (Some(e), Some(d)) = (
                    a.and_then(|a| a[0].as_str()),
                    a.and_then(|a| a[1].as_i64())
                        .and_then(|d| i32::try_from(d).ok()),
                ) else {
                    return Err(format!("{item}: suspicious_stew_effects {v}"));
                };
                (
                    C::SuspiciousStewEffects(vec![(rustmc_loot::normalise(e), d)]),
                    false,
                )
            }
            "instrument" => {
                let i = rustmc_loot::normalise(
                    v.as_str()
                        .ok_or_else(|| format!("{item}: instrument is not an id: {v}"))?,
                );
                let is_proto =
                    proto.and_then(Value::as_str).map(rustmc_loot::normalise) == Some(i.clone());
                (C::Instrument(i), is_proto)
            }
            other => return Err(format!("{item}: no stream codec for minecraft:{other}")),
        };
        if !is_proto {
            patch = patch.with(component);
        }
    }
    Ok(patch)
}

/// What `LivingEntity.dropFromLootTable(level, source, playerKilled, table)`
/// needs to know about a death, beyond the table itself:
///
/// ```text
///   14: LootParams.Builder(level)
///   22:   THIS_ENTITY      = this            -> `on_fire`, `is_baby`
///   29:   ORIGIN           = position()
///   39:   DAMAGE_SOURCE    = source          -> `damage_source`
///   46:   ATTACKING_ENTITY = source.getEntity()  (optional)  -> `attacker`
///   56:   DIRECT_ATTACKING_ENTITY = source.getDirectEntity() (optional)
///   74: if (playerKilled && lastHurtByPlayer != null)
///   83:   LAST_DAMAGE_PLAYER = it; withLuck(it.getLuck())  -> `killed_by_player`
///  113: table.getRandomItems(params, getLootTableSeed(), this::spawnAtLocation)
/// ```
#[derive(Debug, Clone, Default)]
pub struct Death {
    /// `minecraft:pig`.
    pub entity_type: String,
    pub on_fire: bool,
    pub is_baby: bool,
    /// `LAST_DAMAGE_PLAYER` was supplied.
    pub killed_by_player: bool,
    /// The living attacker's enchantment levels by bare id, if there is one.
    pub attacker: Option<BTreeMap<String, i32>>,
    /// `Mob.getLootTableSeed()` — the saved `DeathLootTableSeed`, 0 unless a
    /// command set one, and 0 draws from the table's random sequence.
    pub seed: i64,
}

impl Death {
    /// The `ENTITY` parameter set this death supplies. The attacker's levels
    /// sit on its main hand (only their maximum is ever read), the player
    /// slot is filled only when `LAST_DAMAGE_PLAYER` was, and the damage
    /// source is present with no type or tags.
    #[must_use]
    pub fn entity_ctx(&self) -> rustmc_loot::EntityCtx {
        use rustmc_loot::{LootDamageSource, LootEntity, LootItem};
        rustmc_loot::EntityCtx {
            this_entity: Some(LootEntity {
                kind: self.entity_type.clone(),
                on_fire: self.on_fire,
                baby: self.is_baby,
                ..Default::default()
            }),
            attacking_entity: self.attacker.as_ref().map(|levels| LootEntity {
                equipment: BTreeMap::from([(
                    "mainhand".to_string(),
                    LootItem {
                        item: String::new(),
                        enchants: levels.clone(),
                    },
                )]),
                ..Default::default()
            }),
            last_damage_player: self.killed_by_player.then(|| LootEntity {
                player: true,
                ..Default::default()
            }),
            damage_source: Some(LootDamageSource::default()),
            ..Default::default()
        }
    }
}

/// The stacks a death drops from its loot table, in drop order, after
/// `createStackSplitter` and with the empties `spawnAtLocation` ignores taken
/// out — each one is an `ItemEntity` the caller spawns at the mob.
///
/// `table` is the entity's `getLootTable()` (`entities/<id>` for most types).
/// The caller owns `shouldDropLoot` (`!isBaby && mob_drops`): this only rolls.
///
/// # Errors
/// When the tables are not loaded, the table is unknown, or the roll reached
/// something the interpreter refuses — never an empty vector in its place.
pub fn mob_drops(
    table: &str,
    death: &Death,
    max_stack: &dyn Fn(&str) -> i32,
) -> Result<Vec<ItemStack>, String> {
    let d = data().ok_or("no loot tables loaded")?;
    let mut seqs = sequences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    mob_drops_with(d, &mut seqs, table, death, max_stack)
}

fn mob_drops_with(
    d: &Data,
    seqs: &mut RandomSequences,
    table: &str,
    death: &Death,
    max_stack: &dyn Fn(&str) -> i32,
) -> Result<Vec<ItemStack>, String> {
    let key = table.strip_prefix("minecraft:").unwrap_or(table);
    let t = d
        .tables
        .get(key)
        .ok_or_else(|| format!("no loot table {table}"))?;
    let mut ctx = Ctx {
        item_tags: Arc::clone(&d.item_tags),
        tables: Arc::clone(&d.tables),
        enchanting: d.enchanting.clone(),
        entity: death.entity_ctx(),
        ..Default::default()
    };
    let mut rng = context_random(t, death.seed, seqs)
        .ok_or_else(|| format!("{table} names no random_sequence and has seed 0"))?;
    let raw = roll_with(t, &mut ctx, &mut rng.as_dyn());
    if let Some(why) = ctx.refused {
        return Err(format!("{table}: {why}"));
    }
    Ok(fill::split_stacks(raw, max_stack)
        .into_iter()
        .filter(|s| s.count > 0)
        .map(|s| ItemStack::new(&s.item, s.count))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockentity::Kind;

    fn tag(table: &str, seed: Option<i64>) -> Nbt {
        let mut c = BTreeMap::new();
        c.insert("LootTable".to_string(), Nbt::String(table.to_string()));
        if let Some(s) = seed {
            c.insert("LootTableSeed".to_string(), Nbt::Long(s));
        }
        Nbt::Compound(c)
    }

    fn fixture(name: &str) -> Option<Value> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/vanilla")
            .join(name);
        if !rustmc_data::fixture::available(
            &path,
            name,
            "ci/loot-golden.sh / ci/gen-gameplay-tables.sh",
        ) {
            return None;
        }
        let text = std::fs::read_to_string(&path).expect("read fixture");
        Some(serde_json::from_str(&text).expect("parse fixture"))
    }

    /// The chest tables that roll an enchanting, damage, instrument or stew
    /// function, against vanilla's own vectors — through THIS server's
    /// enchantment registry and `select_enchantment`, which `rustmc-loot`
    /// reaches only through the [`rustmc_loot::Enchanting`] trait. Both the
    /// plain roll (`cases`) and the whole `LootTable.fill` (`fills`, both
    /// generator arms) are compared; components are produced but the vectors
    /// carry ids and counts, so ids and counts are what is compared.
    ///
    /// The point of the test is the DRAW ORDER: a function that is right about
    /// the item but takes one draw too few moves every later slot.
    #[test]
    fn enchanting_chest_tables_reproduce_vanillas_vectors() {
        let Some(t) = replay_enchanting(rustmc_loot::Faults::default()) else {
            return;
        };
        println!(
            "enchanting chest tables: cases {} match, {} mismatch, {} refused; fills {} match, {} mismatch",
            t.0, t.1, t.2, t.3, t.4
        );
        for e in t.5.iter().take(12) {
            println!("  {e}");
        }
        assert_eq!(
            t.1 + t.4,
            0,
            "an enchanting chest roll put a wrong item somewhere"
        );
        assert!(t.0 >= 60, "only {} enchanting chest cases reproduced", t.0);
        assert!(t.3 >= 50, "only {} enchanting chest fills reproduced", t.3);
    }

    /// The same vectors go RED when `enchant_randomly` draws a level for a
    /// single-level enchantment (mending, silk touch, …) — the guard in
    /// `Mth.nextInt` that the shared `next_i32_between_inclusive` lacks.
    #[test]
    fn the_enchanting_vectors_see_a_level_draw_too_many() {
        let f = rustmc_loot::Faults {
            enchant_level_always_draws: true,
            ..Default::default()
        };
        let Some(t) = replay_enchanting(f) else {
            return;
        };
        println!(
            "fault enchant_level_always_draws: {} cases and {} fills now differ",
            t.1, t.4
        );
        assert!(
            t.1 + t.4 > 0,
            "the enchanting vectors are blind to a spurious level draw"
        );
    }

    type Tally = (usize, usize, usize, usize, usize, Vec<String>);

    fn replay_enchanting(faults: rustmc_loot::Faults) -> Option<Tally> {
        let (Some(golden), Some(tables), Some(tags), Some(ench)) = (
            fixture("loot.json"),
            fixture("loot_tables.json"),
            fixture("item_tags.json"),
            fixture("enchantments.json"),
        ) else {
            return None;
        };
        let tables: Arc<BTreeMap<String, Value>> = Arc::new(
            tables
                .as_object()
                .expect("tables")
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        );
        let item_tags: Arc<BTreeMap<String, Vec<String>>> =
            Arc::new(serde_json::from_value(tags).expect("tags"));
        let registry = crate::enchantment::Registry::from_json(&ench).expect("registry");
        let enchanting: Arc<dyn rustmc_loot::Enchanting> = Arc::new(ServerEnchanting(registry));
        let uses = |t: &Value| {
            let s = t.to_string();
            [
                "enchant_randomly",
                "enchant_with_levels",
                "set_damage",
                "set_instrument",
                "set_stew_effect",
                "set_enchantments",
            ]
            .iter()
            .any(|f| s.contains(f))
        };
        let ctx = || Ctx {
            item_tags: Arc::clone(&item_tags),
            tables: Arc::clone(&tables),
            enchanting: Some(Arc::clone(&enchanting)),
            faults,
            ..Default::default()
        };
        let (mut matched, mut mismatched, mut refused) = (0, 0, 0);
        let mut examples = Vec::new();
        for case in golden["cases"].as_array().expect("cases") {
            let id = case["table"].as_str().unwrap_or("");
            if case.get("error").is_some() || !id.starts_with("chests/") {
                continue;
            }
            let Some(table) = tables.get(id) else {
                continue;
            };
            if !uses(table) {
                continue;
            }
            let mut c = ctx();
            let got: Vec<(String, i64)> =
                rustmc_loot::roll(table, &mut c, case["seed"].as_i64().unwrap_or(0))
                    .into_iter()
                    .map(|s| (s.item, i64::from(s.count)))
                    .collect();
            let want: Vec<(String, i64)> = case["stacks"]
                .as_array()
                .expect("stacks")
                .iter()
                .map(|s| {
                    (
                        s["item"].as_str().unwrap_or("").to_string(),
                        s["count"].as_i64().unwrap_or(0),
                    )
                })
                .collect();
            if let Some(why) = c.refused {
                refused += 1;
                examples.push(format!("REFUSED {id}: {why}"));
            } else if got == want {
                matched += 1;
            } else {
                mismatched += 1;
                examples.push(format!(
                    "{id} seed {}\n want {want:?}\n got  {got:?}",
                    case["seed"]
                ));
            }
        }
        let max_stack: BTreeMap<String, i32> = golden["max_stack"]
            .as_object()
            .expect("max_stack")
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    i32::try_from(v.as_i64().unwrap_or(64)).unwrap_or(64),
                )
            })
            .collect();
        let max = |id: &str| *max_stack.get(id).unwrap_or(&64);
        let mut stores: BTreeMap<String, RandomSequences> = BTreeMap::new();
        let (mut fills_ok, mut fills_bad) = (0, 0);
        for row in golden["fills"].as_array().expect("fills") {
            let id = row["table"].as_str().unwrap_or("");
            if row.get("error").is_some() {
                continue;
            }
            let Some(table) = tables.get(id) else {
                continue;
            };
            if !uses(table) {
                continue;
            }
            let seed = row["seed"].as_i64().unwrap_or(0);
            let mut legacy = rustmc_worldgen::rng::LegacyRandomSource::new(seed);
            let rng: &mut dyn rustmc_worldgen::rng::RandomSource = if row["arm"] == "seed" {
                &mut legacy
            } else {
                stores
                    .entry(id.to_string())
                    .or_insert_with(|| RandomSequences::new(seed))
                    .get(table["random_sequence"].as_str().unwrap_or(""))
            };
            let mut c = ctx();
            let raw = roll_with(table, &mut c, &mut &mut *rng);
            if c.refused.is_some() {
                stores.remove(id);
                continue;
            }
            let mut chest: Vec<Option<Stack>> = vec![None; 27];
            fill::place(fill::split_stacks(raw, &max), &mut chest, rng);
            let got: Vec<(i64, String, i64)> = chest
                .iter()
                .enumerate()
                .filter_map(|(i, s)| {
                    s.as_ref()
                        .map(|s| (i as i64, s.item.clone(), i64::from(s.count)))
                })
                .collect();
            let want: Vec<(i64, String, i64)> = row["slots"]
                .as_array()
                .expect("slots")
                .iter()
                .map(|t| {
                    (
                        t[0].as_i64().unwrap_or(-1),
                        t[1].as_str().unwrap_or("").to_string(),
                        t[2].as_i64().unwrap_or(0),
                    )
                })
                .collect();
            if got == want {
                fills_ok += 1;
            } else {
                fills_bad += 1;
                examples.push(format!(
                    "FILL {id} {} {seed}\n want {want:?}\n got  {got:?}",
                    row["arm"]
                ));
            }
        }
        Some((matched, mismatched, refused, fills_ok, fills_bad, examples))
    }

    fn props_fixture() -> Option<ItemProps> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(crate::itemprops::TABLE_PATH);
        if !rustmc_data::fixture::available(&path, "item props", "ci/item-props-golden.sh") {
            return None;
        }
        Some(ItemProps::load(&path).expect("item props"))
    }

    fn rolled(item: &str, comps: &[(&str, Value)]) -> Stack {
        let mut s = Stack::new(item, 1);
        for (k, v) in comps {
            s.components.insert((*k).to_string(), v.clone());
        }
        s
    }

    /// `PatchedDataComponentMap.set` drops a value equal to the prototype's,
    /// and keeps the rest in the order they were set.
    #[test]
    fn a_rolled_stacks_patch_is_its_set_order_minus_its_prototype() {
        let Some(p) = props_fixture() else { return };
        use crate::patch::Component as C;
        let entries = |s: &Stack| -> Vec<C> {
            patch_of(s, Some(&p))
                .expect("converts")
                .entries()
                .map(|(_, c)| c.clone())
                .collect()
        };
        // A goat horn's prototype IS ponder: rolling it changes nothing.
        let ponder = rolled(
            "minecraft:goat_horn",
            &[("instrument", Value::from("minecraft:ponder_goat_horn"))],
        );
        assert_eq!(entries(&ponder), vec![]);
        let seek = rolled(
            "minecraft:goat_horn",
            &[("instrument", Value::from("minecraft:seek_goat_horn"))],
        );
        assert_eq!(
            entries(&seek),
            vec![C::Instrument("minecraft:seek_goat_horn".into())]
        );
        // Amplifier 0 and damage 0 are both prototypes.
        let bottle = rolled(
            "minecraft:ominous_bottle",
            &[("ominous_bottle_amplifier", Value::from(0))],
        );
        assert_eq!(entries(&bottle), vec![]);
        // Set order, not name order: enchantments then damage stays so.
        let sword = rolled(
            "minecraft:golden_sword",
            &[
                (
                    "enchantments",
                    serde_json::json!({"minecraft:sharpness": 2}),
                ),
                ("damage", Value::from(11)),
            ],
        );
        assert_eq!(
            entries(&sword),
            vec![
                C::Enchantments(vec![("minecraft:sharpness".into(), 2)]),
                C::Damage(11)
            ]
        );
        let unworn = rolled(
            "minecraft:golden_sword",
            &[
                ("damage", Value::from(0)),
                (
                    "enchantments",
                    serde_json::json!({"minecraft:sharpness": 2}),
                ),
            ],
        );
        assert_eq!(
            entries(&unworn),
            vec![C::Enchantments(vec![("minecraft:sharpness".into(), 2)])]
        );
        // A component with no stream codec here refuses.
        let named = rolled("minecraft:map", &[("custom_name", Value::from("x"))]);
        assert!(patch_of(&named, Some(&p)).is_err());
    }

    /// The one case [`patch_of`]'s prototype filter is not exact for is a
    /// component set twice in one entry's function chain (prototype first,
    /// something else second, would move it to the end). No vanilla table
    /// does that — checked over every entry of every table, with the pool's
    /// and the table's own functions appended as `LootTable` applies them.
    #[test]
    fn no_loot_entry_sets_a_prototype_reachable_component_twice() {
        let Some(tables) = fixture("loot_tables.json") else {
            return;
        };
        fn sets(f: &Value) -> Option<&'static str> {
            match f
                .get("function")?
                .as_str()?
                .trim_start_matches("minecraft:")
            {
                "set_damage" => Some("damage"),
                "set_instrument" => Some("instrument"),
                "set_ominous_bottle_amplifier" => Some("ominous_bottle_amplifier"),
                "set_enchantments" => Some("enchantments"),
                _ => None,
            }
        }
        fn funcs(v: &Value) -> Vec<&Value> {
            v.get("functions")
                .and_then(Value::as_array)
                .map(|a| a.iter().collect())
                .unwrap_or_default()
        }
        fn entries<'a>(e: &'a Value, out: &mut Vec<&'a Value>) {
            out.push(e);
            if let Some(c) = e.get("children").and_then(Value::as_array) {
                for c in c {
                    entries(c, out);
                }
            }
        }
        let (mut chains, mut twice) = (0, Vec::new());
        for (name, t) in tables.as_object().expect("tables") {
            for pool in t
                .get("pools")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let mut es = Vec::new();
                for e in pool
                    .get("entries")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    entries(e, &mut es);
                }
                for e in es {
                    chains += 1;
                    let chain: Vec<&str> = funcs(e)
                        .into_iter()
                        .chain(funcs(pool))
                        .chain(funcs(t))
                        .filter_map(sets)
                        .collect();
                    let mut seen = std::collections::BTreeSet::new();
                    for c in chain {
                        if !seen.insert(c) {
                            twice.push(format!("{name}: {c}"));
                        }
                    }
                }
            }
        }
        println!(
            "{chains} entry chains, {} set a component twice",
            twice.len()
        );
        assert!(chains > 2500, "only {chains} chains walked");
        assert!(twice.is_empty(), "{twice:?}");
    }

    /// `chests/end_city_treasure` at `LootTableSeed` 3, slot for slot and
    /// patch for patch, against what vanilla sent for the same chest.
    ///
    /// The reference is the two vanilla recordings of the dropped scenario
    /// 886 (`~/rustmc-itemc`, 2026-10-01): the content packet's `rest` digest
    /// covers every slot after the boots' first component, and rebuilding
    /// that tail from exactly the stacks below reproduces both vanilla
    /// digests (`42a1ca57...`, `96271796...`) and ours (`01c07df9...`). The
    /// three differ only in the order of two two-entry `ItemEnchantments`
    /// maps, an identity-hash walk, so enchantments compare as sets here.
    #[test]
    fn end_city_seed_3_fills_the_chest_vanilla_sent() {
        type Slot = (usize, String, i32, Vec<(String, i32)>);
        let Some(p) = props_fixture() else {
            return;
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let d = Data::load(&root).expect("loot data");
        let t = d.tables.get("chests/end_city_treasure").expect("table");
        let mut rng = rustmc_worldgen::rng::LegacyRandomSource::new(3);
        let mut ctx = Ctx {
            item_tags: Arc::clone(&d.item_tags),
            tables: Arc::clone(&d.tables),
            enchanting: d.enchanting.clone(),
            ..Default::default()
        };
        let raw = roll_with(t, &mut ctx, &mut &mut rng);
        assert_eq!(ctx.refused, None);
        let max = |id: &str| p.max_stack_size(id);
        let mut chest: Vec<Option<Stack>> = vec![None; 27];
        fill::place(fill::split_stacks(raw, &max), &mut chest, &mut rng);
        let ench = |l: &[(&str, i32)]| {
            let mut v: Vec<(String, i32)> = l
                .iter()
                .map(|(e, n)| (format!("minecraft:{e}"), *n))
                .collect();
            v.sort();
            v
        };
        let want = [
            (
                1,
                "diamond_boots",
                1,
                ench(&[("mending", 1), ("protection", 4)]),
            ),
            (3, "gold_ingot", 1, vec![]),
            (4, "iron_ingot", 1, vec![]),
            (7, "diamond", 4, vec![]),
            (9, "iron_ingot", 1, vec![]),
            (10, "diamond_leggings", 1, ench(&[("vanishing_curse", 1)])),
            (12, "diamond", 3, vec![]),
            (14, "gold_ingot", 1, vec![]),
            (19, "iron_ingot", 4, vec![]),
            (
                20,
                "diamond_leggings",
                1,
                ench(&[("protection", 3), ("unbreaking", 3)]),
            ),
        ];
        let mut got: Vec<Slot> = Vec::new();
        for (i, s) in chest.iter().enumerate() {
            let Some(s) = s else { continue };
            let patch = patch_of(s, Some(&p)).expect("patch");
            let mut e = Vec::new();
            for (_, c) in patch.entries() {
                match c {
                    crate::patch::Component::Enchantments(l) => e.extend(l.iter().cloned()),
                    other => panic!("slot {i}: unexpected component {other:?}"),
                }
            }
            e.sort();
            got.push((i, rustmc_loot::normalise(&s.item), s.count, e));
        }
        let want: Vec<Slot> = want
            .into_iter()
            .map(|(i, item, n, e)| (i, format!("minecraft:{item}"), n, e))
            .collect();
        assert_eq!(got, want);
    }

    /// A pig's death drops, through [`mob_drops`] with a live, unburning,
    /// adult pig and no attacker — what `/kill` produces — against vanilla's
    /// own rolls of `entities/pig` on its random sequence at world seed
    /// 20260803, first and second death. Ids and counts in order; the empty
    /// stacks vanilla's raw roll carries are the ones `spawnAtLocation`
    /// drops, so they are dropped from the expectation too.
    #[test]
    fn a_pigs_death_drops_what_vanilla_rolls_on_the_pig_sequence() {
        let (Some(golden), Some(_), Some(_)) = (
            fixture("loot.json"),
            fixture("loot_tables.json"),
            fixture("item_tags.json"),
        ) else {
            return;
        };
        let rows: Vec<&Value> = golden["entity_cases"]
            .as_array()
            .expect("entity_cases")
            .iter()
            .filter(|c| c["table"] == "entities/pig" && c["arm"].as_str().is_some())
            .collect();
        assert_eq!(rows.len(), 2, "two sequence rows for the pig");
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let d = Data::load(&root).expect("loot data");
        let mut seqs = RandomSequences::new(20_260_803);
        let death = Death {
            entity_type: "minecraft:pig".into(),
            ..Default::default()
        };
        for row in rows {
            let want: Vec<(String, i32)> = row["stacks"]
                .as_array()
                .expect("stacks")
                .iter()
                .filter(|s| s["count"].as_i64().unwrap_or(0) > 0)
                .map(|s| {
                    (
                        s["item"].as_str().unwrap_or("").to_string(),
                        i32::try_from(s["count"].as_i64().unwrap_or(0)).unwrap_or(0),
                    )
                })
                .collect();
            let got: Vec<(String, i32)> =
                mob_drops_with(&d, &mut seqs, "minecraft:entities/pig", &death, &|_| 64)
                    .expect("rolled")
                    .into_iter()
                    .map(|s| (s.item().to_string(), s.count()))
                    .collect();
            assert_eq!(got, want, "{}", row["arm"]);
            assert!(!want.is_empty(), "a pig always drops a porkchop");
        }
    }

    /// A record with a table becomes a stocked-on-open chest; a record with
    /// no table, a block with no container, and a position that already has
    /// a block entity do not.
    #[test]
    fn generated_loot_records_become_unopened_chests() {
        use rustmc_worldgen::block_entity::Record;
        let mut canvas = rustmc_worldgen::canvas::Canvas::new(-64, 384);
        canvas.record_block_entities();
        canvas.ensure_chunk(0, 0, "minecraft:air");
        let rec = |table: Option<&str>, seed| Record {
            nbt: table
                .map(|t| BTreeMap::from([("LootTable".to_string(), Nbt::String(t.into()))]))
                .unwrap_or_default(),
            loot_seed: seed,
        };
        canvas.set_block(
            1,
            10,
            1,
            "minecraft:chest[facing=north,type=single,waterlogged=false]",
        );
        canvas.record_block_entity(
            1,
            10,
            1,
            rec(Some("minecraft:chests/simple_dungeon"), Some(-77)),
        );
        canvas.set_block(
            2,
            10,
            1,
            "minecraft:chest[facing=north,type=single,waterlogged=false]",
        );
        canvas.record_block_entity(2, 10, 1, rec(None, Some(5)));
        canvas.set_block(3, 10, 1, "minecraft:stone");
        canvas.record_block_entity(
            3,
            10,
            1,
            rec(Some("minecraft:chests/simple_dungeon"), Some(5)),
        );
        canvas.set_block(
            4,
            10,
            1,
            "minecraft:chest[facing=north,type=single,waterlogged=false]",
        );
        canvas.record_block_entity(4, 10, 1, rec(Some("minecraft:chests/igloo_chest"), Some(9)));
        let store = crate::blockentity::BlockEntities::new();
        store.insert((4, 10, 1), BlockEntity::new(Kind::Chest));
        let n = adopt_generated(&canvas, std::iter::once((0, 0)), &store);
        assert_eq!(n, 1);
        assert_eq!(
            store.get((1, 10, 1)).and_then(|b| b.loot().cloned()),
            Some(LootRef {
                table: "minecraft:chests/simple_dungeon".into(),
                seed: -77
            })
        );
        assert!(store.get((2, 10, 1)).is_none(), "no table, no block entity");
        assert!(store.get((3, 10, 1)).is_none(), "stone holds no container");
        assert!(
            store
                .get((4, 10, 1))
                .and_then(|b| b.loot().cloned())
                .is_none(),
            "an existing block entity is never re-armed"
        );
        // A second generation of the same tile adopts nothing new.
        assert_eq!(adopt_generated(&canvas, std::iter::once((0, 0)), &store), 0);
    }

    /// The bridge on a world the real generator made, against what vanilla
    /// saved for the same seed and square: every dungeon chest in the 9x9
    /// chunks around chunk (58, 78) of seed 12345, its position and its
    /// `LootTableSeed`.
    ///
    /// The vanilla side is pinned from the region files of five independent
    /// vanilla harvests of that seed (`oracle-work/gen-wide-12345-r16-ow-{a,b,
    /// aa,ab,c}`, chunks 54..62 x 74..82 at `minecraft:full`), all five
    /// agreeing to the last bit — so these four are not one lucky reference:
    ///
    /// ```text
    ///   (886, -27, 1200) chests/simple_dungeon  936796807845658639
    ///   (887, -27, 1201) chests/simple_dungeon  5337749413150248763
    ///   (989, -39, 1302) chests/simple_dungeon  -4831706545268117701
    ///   (993, -39, 1303) chests/simple_dungeon  6575673093383925621
    /// ```
    ///
    /// With the seed pinned, a generated chest's first-open contents are
    /// `LootTable.fill` on that seed, which `rustmc-loot`'s fill vectors gate.
    #[test]
    fn generated_dungeon_chests_carry_vanillas_own_loot_seeds() {
        if fixture("block_props.json").is_none() {
            return;
        }
        std::env::set_var("RUSTMC_RECORD_BLOCK_ENTITIES", "1");
        let area = rustmc_worldgen::pipeline::Area::square(58, 78, 4);
        let generated = worldgen_assembly::generate::generate(
            12_345,
            area,
            rustmc_worldgen::pipeline::Through::Decoration,
        )
        .expect("generate");
        let store = crate::blockentity::BlockEntities::new();
        let n = adopt_generated(&generated.canvas, area.chunks(), &store);
        let mut got: Vec<(crate::interact::BlockPos, String, i64)> = Vec::new();
        for (cx, cz) in area.chunks() {
            for (pos, be) in store.in_chunk(cx, cz) {
                if let Some(l) = be.loot() {
                    got.push((pos, l.table.clone(), l.seed));
                }
            }
        }
        got.sort();
        let d = "minecraft:chests/simple_dungeon".to_string();
        let want = vec![
            ((886, -27, 1200), d.clone(), 936_796_807_845_658_639),
            ((887, -27, 1201), d.clone(), 5_337_749_413_150_248_763),
            ((989, -39, 1302), d.clone(), -4_831_706_545_268_117_701),
            ((993, -39, 1303), d, 6_575_673_093_383_925_621),
        ];
        assert_eq!(n, want.len());
        assert_eq!(got, want);
    }

    /// `random_sequences.dat` round trip: what a session saves, a reopened
    /// session continues — the `data` compound built here wrapped as
    /// `saved::random_sequences` writes it, and read back the way `--world`
    /// does.
    #[test]
    fn saved_sequences_reopen_where_they_stopped() {
        let mut live = RandomSequences::new(20_260_803);
        live.get("minecraft:chests/simple_dungeon").next_i64();
        live.get("minecraft:entities/pig").next_i64();
        let data = rustmc_world::level::saved::random_sequences(0, nbt_of_saved(&live.saved()));
        let mut back = RandomSequences::restore(20_260_803, &saved_of_nbt(&data));
        for id in [
            "minecraft:chests/simple_dungeon",
            "minecraft:entities/pig",
            "minecraft:entities/cow",
        ] {
            assert_eq!(live.get(id).next_i64(), back.get(id).next_i64(), "{id}");
        }
        assert_eq!(saved_of_nbt(&data).sequences.len(), 2);
    }

    /// `getLongOr` reads a long and nothing else; an int seed is 0.
    #[test]
    fn the_seed_is_a_long_or_it_is_zero() {
        assert_eq!(
            loot_ref_of(&tag("chests/simple_dungeon", Some(42))),
            Some(LootRef {
                table: "minecraft:chests/simple_dungeon".into(),
                seed: 42
            })
        );
        let mut t = tag("minecraft:chests/igloo_chest", None);
        if let Nbt::Compound(c) = &mut t {
            c.insert("LootTableSeed".into(), Nbt::Int(42));
        }
        assert_eq!(loot_ref_of(&t).map(|l| l.seed), Some(0));
    }

    /// The type test at offset 52 refuses a tag for another block entity
    /// type, and a non-container never takes a table.
    #[test]
    fn a_mismatched_type_or_a_non_container_takes_nothing() {
        let mut bytes = Vec::new();
        crate::commands::write_network_nbt(&mut bytes, &tag("chests/igloo_chest", Some(1)))
            .expect("write");
        let mut chest = BlockEntity::new(Kind::Chest);
        assert!(!apply_block_entity_data(&mut chest, 1, Some(2), &bytes));
        assert!(chest.loot().is_none());
        assert!(apply_block_entity_data(&mut chest, 1, Some(1), &bytes));
        assert_eq!(chest.loot().map(|l| l.seed), Some(1));
        let mut furnace = BlockEntity::new(Kind::Furnace);
        assert!(!apply_block_entity_data(&mut furnace, 3, Some(3), &bytes));
        assert!(furnace.loot().is_none());
    }
}
