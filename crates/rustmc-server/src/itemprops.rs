//! Every item's default data components — the table `item.rs` said was missing.
//!
//! # The gap, in our own words
//!
//! [`crate::item::DEFAULT_MAX_STACK_SIZE`] carries a doc comment that has been
//! true and admitted since the day it was written: every item this server can
//! stack is assumed to stack to 64, because `data/vanilla/items.json` is
//! Mojang's registry *report* and the report has no such column. The comment
//! ends "when the column lands there is one body to change and no call
//! sites". This is the column.
//!
//! **It is wrong for 295 of 1,537 items** — 246 stack to 1 and 49 to 16
//! (`ci/item-props-golden.sh`, pinned 26.2 jar, 2026-09-05). That is 19.2% of
//! the registry, and it is not an even 19.2%: the items that stack to 1 are
//! every tool, every weapon, every piece of armour and every filled bucket,
//! which is most of what a survival player carries.
//!
//! # And the fallback constant is not vanilla's default either
//!
//! `ItemInstance.getMaxStackSize()` is
//! `getOrDefault(DataComponents.MAX_STACK_SIZE, 1)` — offsets 0..19,
//! `javap -p -c -constants net.minecraft.world.item.ItemInstance`. The default
//! there is **1**, not 64. 64 is what `Item.Properties`'s constructor *writes
//! into the map*, so every registered item has the component and the default
//! is unreachable for them; an item that is somehow not in this table is
//! nearer to `AIR` than to `STONE`, and vanilla would give it 1.
//!
//! So [`ItemProps::max_stack_size`] returns 1 for an unknown name, which is
//! the transcription, and [`crate::item::DEFAULT_MAX_STACK_SIZE`] stays 64
//! and stays named for what it actually is: the `Item.Properties` default,
//! used where no table is loaded at all.
//!
//! # What reads it today, and what does not
//!
//! Today: [`crate::session_tables::SessionTables`] loads it and the boot line
//! reports the histogram. The click and inventory paths — `crate::click`,
//! `crate::player::Inventory` — still call [`crate::item::ItemStack`]'s
//! constant-backed `max_stack_size`, and converting them is deliberately not
//! in this commit: those files are the ones `origin/stack-gameplay`'s 301
//! patches rewrite, and a conversion landed here would be resolved by hand
//! against every one of them. **Stated rather than left to be discovered: a
//! bucket still stacks to 64 in a container on this branch.** The table is
//! the half that can land without collision, and it is the half that had to
//! be extracted rather than written.

use std::collections::BTreeMap;
use std::io;
use std::sync::OnceLock;

/// Where the extract lives, relative to the server's working directory.
///
/// **Load-bearing outside Rust**: `ci/gen-gameplay-tables.sh` derives the set
/// of tables a tree needs by grepping the repo for
/// `"data/vanilla/<name>.json"`, so this literal is what registers
/// `ci/item-props-golden.sh` as this tree's generator.
pub const TABLE_PATH: &str = "data/vanilla/item_props.json";

/// The component keys this table parses today.
///
/// Named as constants rather than spelled inline because each one is a
/// `minecraft:data_component_type` registry id — the same string the wire
/// carries in a `DataComponentPatch` — and a typo in one would read as an
/// item that simply does not have that component.
const MAX_STACK_SIZE: &str = "minecraft:max_stack_size";
const MAX_DAMAGE: &str = "minecraft:max_damage";
const DAMAGE: &str = "minecraft:damage";
const UNBREAKABLE: &str = "minecraft:unbreakable";
const TOOL: &str = "minecraft:tool";
const CONSUMABLE: &str = "minecraft:consumable";
const EQUIPPABLE: &str = "minecraft:equippable";
const BLOCKS_ATTACKS: &str = "minecraft:blocks_attacks";
const KINETIC_WEAPON: &str = "minecraft:kinetic_weapon";

/// One item's defaults.
///
/// Deliberately not a map of every component: the extract carries 59 distinct
/// component types over 19,356 entries, and a `BTreeMap<String, Value>` per
/// item would put most of a megabyte of JSON in the server's heap to answer
/// questions nothing asks yet. Columns are added here as the code that reads
/// them is transcribed, and `ci/item-props-golden.sh` extracts all 59
/// regardless, so adding one is a parse change and never a re-extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    /// `DataComponents.MAX_STACK_SIZE`. Present on every registered item.
    pub max_stack_size: i32,
    /// `DataComponents.MAX_DAMAGE`, absent on anything that cannot break.
    pub max_damage: Option<i32>,
    /// `DataComponents.DAMAGE` — the *default* damage, 0 on everything that
    /// has it. Carried because `ItemStack.isDamageableItem` tests its
    /// presence and not its value.
    pub damage: Option<i32>,
    /// `DataComponents.UNBREAKABLE`. A unit component: presence is the value.
    pub unbreakable: bool,
}

/// One `Tool.Rule`.
///
/// `speed` and `correct_for_drops` are **independently optional** in the
/// record and that is the whole subtlety of [`ItemProps::mining_speed`] and
/// [`ItemProps::correct_tool_for_drops`]: they are two separate loops over
/// the same list, each skipping the rules that do not carry its own field.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// Index into the resolved block sets. Not the set itself, because 38
    /// items share 17 sets and one of them is 510 blocks.
    blocks: usize,
    /// `Optional<Float> speed`.
    speed: Option<f32>,
    /// `Optional<Boolean> correctForDrops`.
    correct_for_drops: Option<bool>,
}

/// Which arm of `Item.use` an item takes.
///
/// `Item.use` is a four-test ladder over **components** — offsets 0-126,
/// `javap -p -c -constants net.minecraft.world.item.Item` — and every test is
/// answerable from the extract, so this is the whole dispatch even though
/// none of the arms' effects are ported:
///
/// ```text
///     7-34   c = stack.get(CONSUMABLE); if (c != null) return c.startConsuming(...)
///    35-69   e = stack.get(EQUIPPABLE); if (e != null && e.swappable())
///                return e.swapWithEquipmentSlot(...)
///    70-89   if (stack.has(BLOCKS_ATTACKS)) { startUsingItem(hand); return CONSUME }
///    90-122  k = stack.get(KINETIC_WEAPON); if (k != null)
///                { startUsingItem(hand); k.makeSound(player); return CONSUME }
///       123  return PASS
/// ```
///
/// Census over the 1,537-item registry, 2026-09-05: **1,411 `Pass`, 75
/// `Equippable`, 43 `Consumable`, 7 `KineticWeapon`, 1 `BlocksAttacks`.**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseArm {
    /// `Consumable.startConsuming` — eating and drinking. 43 items.
    Consumable,
    /// `Equippable.swapWithEquipmentSlot` — armour into its slot. 75 items,
    /// which is **not** the 84 that carry the component: `swappable()` is the
    /// second half of the test at offset 55.
    Equippable,
    /// `startUsingItem` then `CONSUME` — raising a shield. Exactly one item,
    /// and it reaches this arm only because `minecraft:shield` is one of the
    /// nine `equippable` items whose `swappable` is `false`.
    BlocksAttacks,
    /// `startUsingItem`, `KineticWeapon.makeSound`, then `CONSUME`. Seven
    /// items, all spears, and the component is new in 26.2.
    KineticWeapon,
    /// `getstatic PASS; areturn` at offset 123. 1,411 items — including every
    /// block item, so right-clicking air with a stone does nothing, which is
    /// this server's current behaviour for everything.
    Pass,
}

/// Every `Item.use` arm this process has reached and declined to run, by arm.
///
/// **Process-wide and monotonic, deliberately.** The alternative to counting
/// is silence, and a `use_item` this server answers with nothing looks
/// identical — from both ends and in every trace — to a client that never
/// right-clicked. That is the shape `crate::unhandled` was written for on
/// `origin/packets` and the same argument applies one layer in: the packet is
/// now decoded and dispatched, and each arm that is reached but not
/// implemented is a number a test can assert rather than a gap nobody counts.
///
/// Indexed by [`UseArm`]'s own discriminant order.
static USE_ARMS_REACHED: [std::sync::atomic::AtomicU64; 5] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];

/// Record that `handleUseItem` reached this arm.
///
/// `Pass` is counted too. It is 1,411 of the 1,537 items and vanilla does
/// nothing for it either, so counting it is what makes the other four
/// meaningful as a *ratio* rather than as a bare tally.
pub fn note_use_arm(arm: UseArm) {
    USE_ARMS_REACHED[arm as usize].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// How many times an arm has been reached in this process.
#[must_use]
pub fn use_arms_reached(arm: UseArm) -> u64 {
    USE_ARMS_REACHED[arm as usize].load(std::sync::atomic::Ordering::Relaxed)
}

/// One item's `minecraft:tool` component.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolComponent {
    rules: Vec<Rule>,
    /// `Tool.defaultMiningSpeed`, what `getMiningSpeed` falls through to.
    ///
    /// **Unexercised by the 26.2 extract**: no item writes the field, so
    /// every tool falls through to the codec's own default of 1.0 and the
    /// real table cannot tell a correct read of this field from a hard-coded
    /// 1.0. The unit tests carry a synthetic row that can.
    default_mining_speed: f32,
    /// `Tool.damagePerBlock`, read by `Item.mineBlock` (offsets 38-57) and by
    /// nothing here yet — this server has no durability.
    pub damage_per_block: i32,
    /// `Tool.canDestroyBlocksInCreative`, read by `Item.canDestroyBlock`
    /// (offsets 17-22). The codec omits it at its default, `true`, so the
    /// extract names it only on the items that set it false — every sword,
    /// the mace and the trident. See `crate::dig::can_destroy_block`.
    pub can_destroy_blocks_in_creative: bool,
}

/// The four component tests of `Item.use`, precomputed.
///
/// Booleans rather than the components themselves: none of the four arms'
/// *effects* is ported, so nothing needs `Consumable`'s consume seconds or
/// `Equippable`'s slot yet, and storing them would be a megabyte of JSON
/// answering questions nothing asks. `equippable_swappable` folds the second
/// half of the offset-55 test in, because the two are never asked apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct UseComponents {
    consumable: bool,
    equippable_swappable: bool,
    blocks_attacks: bool,
    kinetic_weapon: bool,
}

/// Item id to defaults.
#[derive(Debug, Clone)]
pub struct ItemProps {
    rows: BTreeMap<String, Row>,
    /// Every distinct resolved `HolderSet`, in the order the extract lists
    /// them. Membership is by block id, which is what
    /// `BlockState.is(HolderSet)` comes to for a set of whole blocks.
    sets: Vec<std::collections::BTreeSet<String>>,
    /// The `minecraft:tool` component per item that has one.
    tools: BTreeMap<String, ToolComponent>,
    /// The four components `Item.use` tests, per item that has any of them.
    use_components: BTreeMap<String, UseComponents>,
    /// `item.getClass().getName()` per item.
    ///
    /// The registry report names no class, and it is what vanilla dispatches
    /// `use`, `useOn` and `placeBlock` through. 70 distinct classes over
    /// 1,537 items; `BlockItem` is 938 of them and plain `Item` 236.
    classes: BTreeMap<String, String>,
    /// `LivingEntity.entityEventForEquipmentBreak(slot)`, per slot name.
    ///
    /// In the table because the method is a `tableswitch` over a `$SwitchMap`
    /// whose numbering is the compiler's: pairing the listing's arm order
    /// with `EquipmentSlot.values()` gets three of the eight wrong. The
    /// dumper invokes it per slot. See [`crate::durability::MAINHAND`].
    equipment_break_events: BTreeMap<String, i32>,
    /// The `minecraft:data_component_type` registry id per component name.
    /// See [`Self::component_type_id`].
    component_type_ids: BTreeMap<String, i32>,
    /// Prototype values of the components a loot function can set, per item
    /// that has any of them, in the extract's own JSON form. See
    /// [`Self::prototype`].
    prototypes: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
}

/// The components a loot function sets on a rolled stack
/// (`crate::lootroll::patch_of`), whose prototype value decides whether
/// `PatchedDataComponentMap.set` keeps them in the patch at all.
pub const LOOT_SET_COMPONENTS: [&str; 7] = [
    "minecraft:damage",
    "minecraft:enchantments",
    "minecraft:stored_enchantments",
    "minecraft:potion_contents",
    "minecraft:suspicious_stew_effects",
    "minecraft:instrument",
    "minecraft:ominous_bottle_amplifier",
];

impl ItemProps {
    /// Parse the table at `path`.
    ///
    /// # Errors
    /// If the file is missing, is not JSON, or does not have a `table` array
    /// of `{item, components}` objects. A row whose `components` carries no
    /// `minecraft:max_stack_size` is an error and not a default: every item
    /// vanilla registers has one, so its absence means the extract was taken
    /// from a JVM whose data components were never bound, and defaulting it
    /// would turn that into 1,537 silently wrong stack sizes.
    pub fn load(path: &std::path::Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let doc: serde_json::Value = serde_json::from_str(&text).map_err(io::Error::other)?;
        Self::parse(&doc)
    }

    /// The same table from an already-parsed document.
    ///
    /// Split out from [`Self::load`] for the reason
    /// [`crate::player::ItemIds::parse`] is: tests that write a file and read
    /// it straight back race each other's truncations under `cargo test`'s
    /// thread pool.
    ///
    /// # Errors
    /// As [`Self::load`].
    pub fn parse(doc: &serde_json::Value) -> io::Result<Self> {
        let table = doc
            .get("table")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| io::Error::other("item props: no `table` array"))?;
        let mut rows = BTreeMap::new();
        for entry in table {
            let item = entry
                .get("item")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| io::Error::other("item props: a row has no `item`"))?;
            let comps = entry
                .get("components")
                .ok_or_else(|| io::Error::other("item props: a row has no `components`"))?;
            let max_stack_size = comps
                .get(MAX_STACK_SIZE)
                .and_then(serde_json::Value::as_i64)
                .ok_or_else(|| {
                    io::Error::other(format!(
                        "item props: {item} carries no {MAX_STACK_SIZE}, so this extract was \
                         taken before its data components were bound and every stack size in \
                         it is unreliable"
                    ))
                })?;
            rows.insert(
                item.to_owned(),
                Row {
                    max_stack_size: i32::try_from(max_stack_size).map_err(io::Error::other)?,
                    max_damage: comps
                        .get(MAX_DAMAGE)
                        .and_then(serde_json::Value::as_i64)
                        .and_then(|v| i32::try_from(v).ok()),
                    damage: comps
                        .get(DAMAGE)
                        .and_then(serde_json::Value::as_i64)
                        .and_then(|v| i32::try_from(v).ok()),
                    unbreakable: comps.get(UNBREAKABLE).is_some(),
                },
            );
        }
        if rows.is_empty() {
            return Err(io::Error::other("item props: the table is empty"));
        }

        // The resolved `HolderSet`s, keyed in the extract by the exact JSON
        // their rule's `blocks` field encoded to. `serde_json::to_string` of
        // that field reproduces the key, so the join needs no name parsing
        // and cannot mistake `"#minecraft:mineable/axe"` for a block id.
        let mut sets = Vec::new();
        let mut set_index: BTreeMap<String, usize> = BTreeMap::new();
        if let Some(obj) = doc
            .get("toolBlockSets")
            .and_then(serde_json::Value::as_object)
        {
            for (key, blocks) in obj {
                let members = blocks
                    .as_array()
                    .ok_or_else(|| {
                        io::Error::other(format!("item props: toolBlockSets[{key}] is not a list"))
                    })?
                    .iter()
                    .filter_map(|b| b.as_str().map(str::to_owned))
                    .collect();
                set_index.insert(key.clone(), sets.len());
                sets.push(members);
            }
        }

        let mut classes = BTreeMap::new();
        let mut use_components = BTreeMap::new();
        let mut tools = BTreeMap::new();
        for entry in table {
            if let (Some(item), Some(comps)) = (
                entry.get("item").and_then(serde_json::Value::as_str),
                entry.get("components"),
            ) {
                use_components.insert(
                    item.to_owned(),
                    UseComponents {
                        consumable: comps.get(CONSUMABLE).is_some(),
                        // `Equippable.swappable()` defaults to true in the
                        // codec, so an absent field is swappable and only an
                        // explicit `false` is not.
                        equippable_swappable: comps.get(EQUIPPABLE).is_some_and(|e| {
                            e.get("swappable")
                                .and_then(serde_json::Value::as_bool)
                                .unwrap_or(true)
                        }),
                        blocks_attacks: comps.get(BLOCKS_ATTACKS).is_some(),
                        kinetic_weapon: comps.get(KINETIC_WEAPON).is_some(),
                    },
                );
            }
            if let (Some(item), Some(class)) = (
                entry.get("item").and_then(serde_json::Value::as_str),
                entry.get("class").and_then(serde_json::Value::as_str),
            ) {
                classes.insert(item.to_owned(), class.to_owned());
            }
            let (Some(item), Some(tool)) = (
                entry.get("item").and_then(serde_json::Value::as_str),
                entry.get("components").and_then(|c| c.get(TOOL)),
            ) else {
                continue;
            };
            tools.insert(item.to_owned(), parse_tool(item, tool, &set_index)?);
        }

        let prototypes = table
            .iter()
            .filter_map(|entry| {
                let item = entry.get("item")?.as_str()?;
                let comps = entry.get("components")?.as_object()?;
                let kept: BTreeMap<String, serde_json::Value> = LOOT_SET_COMPONENTS
                    .iter()
                    .filter_map(|c| comps.get(*c).map(|v| ((*c).to_owned(), v.clone())))
                    .collect();
                (!kept.is_empty()).then(|| (item.to_owned(), kept))
            })
            .collect();
        Ok(Self {
            prototypes,
            rows,
            sets,
            tools,
            use_components,
            classes,
            component_type_ids: doc["componentTypeIds"]
                .as_object()
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| Some((k.clone(), i32::try_from(v.as_i64()?).ok()?)))
                        .collect()
                })
                .unwrap_or_default(),
            equipment_break_events: doc["equipmentBreakEvents"]
                .as_object()
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| Some((k.clone(), i32::try_from(v.as_i64()?).ok()?)))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }

    /// The process-wide copy, parsed once.
    ///
    /// # Errors
    /// As [`Self::load`]. The error is rebuilt rather than borrowed because
    /// `io::Error` is not `Clone`; same shape as
    /// [`crate::sound_types::SoundTypes::shared`].
    pub fn shared(path: &std::path::Path) -> io::Result<&'static Self> {
        static ONCE: OnceLock<io::Result<ItemProps>> = OnceLock::new();
        match ONCE.get_or_init(|| Self::load(path)) {
            Ok(t) => Ok(t),
            Err(e) => Err(io::Error::new(e.kind(), e.to_string())),
        }
    }

    /// `item`'s prototype value of `component` (one of
    /// [`LOOT_SET_COMPONENTS`]), as the extract's codec JSON wrote it; `None`
    /// when the prototype has no such component.
    #[must_use]
    pub fn prototype(&self, item: &str, component: &str) -> Option<&serde_json::Value> {
        self.prototypes.get(item)?.get(component)
    }

    /// The row for an item id, or `None` for a name not in the registry.
    #[must_use]
    pub fn row(&self, item: &str) -> Option<&Row> {
        self.rows.get(item)
    }

    /// `ItemInstance.getMaxStackSize()` — `getOrDefault(MAX_STACK_SIZE, 1)`.
    ///
    /// **The default is 1 and that is not a typo.** See the module note: 64 is
    /// `Item.Properties`'s constructor value, which every registered item
    /// carries explicitly, so the only way to reach this default is to ask
    /// about an item vanilla does not have.
    #[must_use]
    pub fn max_stack_size(&self, item: &str) -> i32 {
        self.rows.get(item).map_or(1, |r| r.max_stack_size)
    }

    /// `ItemStack.getMaxDamage()` — `getOrDefault(MAX_DAMAGE, 0)`.
    #[must_use]
    pub fn max_damage(&self, item: &str) -> i32 {
        self.rows
            .get(item)
            .and_then(|r| r.max_damage)
            .unwrap_or_default()
    }

    /// `ItemStack.isDamageableItem()` — offsets 0..35.
    ///
    /// `has(MAX_DAMAGE) && !has(UNBREAKABLE) && has(DAMAGE)`, all three read
    /// off the **stack's** components in vanilla. On this server no stack
    /// carries a component patch yet, so a stack's components are its item's
    /// defaults and this is the whole answer; the day a patch exists, this
    /// becomes the base it is applied to rather than a function to replace.
    #[must_use]
    pub fn is_damageable_item(&self, item: &str) -> bool {
        self.rows
            .get(item)
            .is_some_and(|r| r.max_damage.is_some() && !r.unbreakable && r.damage.is_some())
    }

    /// Whether `item`'s **prototype** carries `minecraft:damage` — which is
    /// what decides whether a stack's damage 0 is a patch entry or the
    /// default. See [`crate::item::prototype_value`].
    #[must_use]
    pub fn has_prototype_damage(&self, item: &str) -> bool {
        self.rows.get(item).is_some_and(|r| r.damage.is_some())
    }

    /// How many items the table holds, for the boot line.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the table holds nothing. `false` for anything [`Self::parse`]
    /// accepts — it refuses an empty table — and present because clippy asks
    /// for it beside `len`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// `Item.getDestroySpeed(stack, state)` — offsets 0-24.
    ///
    /// ```text
    ///    0-10   tool = stack.get(DataComponents.TOOL)
    ///   11-12   if (tool == null) goto 23
    ///   15-20   return tool.getMiningSpeed(state)
    ///      23   return 1.0F                          // fconst_1
    /// ```
    ///
    /// and `Tool.getMiningSpeed` is
    ///
    /// ```text
    ///   for (Rule rule : rules)
    ///       if (rule.speed.isPresent() && state.is(rule.blocks))
    ///           return rule.speed.get()
    ///   return defaultMiningSpeed
    /// ```
    ///
    /// **The `speed.isPresent()` test comes first, at offset 33, and a rule
    /// that fails it is skipped rather than ending the walk.** That is what
    /// separates this from [`Self::correct_tool_for_drops`], and the case
    /// that shows it is a wooden pickaxe on obsidian: rule 0 is
    /// `#incorrect_for_wooden_tool` with `correct_for_drops = false` and *no
    /// speed*, rule 1 is `#mineable/pickaxe` with `speed = 2.0`, and obsidian
    /// is in both. Vanilla mines it at **2.0** and drops **nothing**. A
    /// single fused loop gives one of those two answers and never both.
    #[must_use]
    pub fn mining_speed(&self, item: &str, block: &str) -> f32 {
        let Some(tool) = self.tools.get(item) else {
            return 1.0;
        };
        for rule in &tool.rules {
            if let Some(speed) = rule.speed {
                if self.set_contains(rule.blocks, block) {
                    return speed;
                }
            }
        }
        tool.default_mining_speed
    }

    /// `Item.isCorrectToolForDrops(stack, state)` — offsets 0-28 — over
    /// `Tool.isCorrectForDrops`, which is [`Self::mining_speed`]'s loop with
    /// `correctForDrops` in place of `speed` and `false` in place of
    /// `defaultMiningSpeed`.
    #[must_use]
    pub fn correct_tool_for_drops(&self, item: &str, block: &str) -> bool {
        let Some(tool) = self.tools.get(item) else {
            return false;
        };
        for rule in &tool.rules {
            if let Some(correct) = rule.correct_for_drops {
                if self.set_contains(rule.blocks, block) {
                    return correct;
                }
            }
        }
        false
    }

    /// `BlockState.is(HolderSet)`, for a set of whole blocks.
    fn set_contains(&self, set: usize, block: &str) -> bool {
        self.sets.get(set).is_some_and(|s| s.contains(block))
    }

    /// The `minecraft:tool` component, for a caller that wants
    /// `damage_per_block` or to count what is here.
    #[must_use]
    pub fn tool(&self, item: &str) -> Option<&ToolComponent> {
        self.tools.get(item)
    }

    /// The `minecraft:data_component_type` registry id for a component name.
    ///
    /// What `DataComponentPatch`'s stream codec writes for each key. All 111
    /// are in the table, not only the 59 some item's prototype carries: a
    /// patch names components no prototype has, and `minecraft:custom_name`
    /// — id 6 — is the obvious one.
    #[must_use]
    pub fn component_type_id(&self, name: &str) -> Option<i32> {
        self.component_type_ids.get(name).copied()
    }

    /// The inverse of [`Self::component_type_id`]: the name a patch's type id
    /// stands for.
    #[must_use]
    pub fn component_type_name(&self, id: i32) -> Option<&str> {
        self.component_type_ids
            .iter()
            .find(|(_, v)| **v == id)
            .map(|(k, _)| k.as_str())
    }

    /// How many component types the registry has — 111 in 26.2, against the
    /// 59 that appear in some item's defaults.
    #[must_use]
    pub fn component_type_count(&self) -> usize {
        self.component_type_ids.len()
    }

    /// `LivingEntity.entityEventForEquipmentBreak(slot)` for a slot name, or
    /// `None` for a table written before the column existed.
    ///
    /// `None` rather than a default, because the whole reason the column
    /// exists is that the value cannot be reconstructed: a default here would
    /// be the guess this column was added to remove.
    #[must_use]
    pub fn equipment_break_event(&self, slot: &str) -> Option<i32> {
        self.equipment_break_events.get(slot).copied()
    }

    /// The item's Java class name, or `None` for a name not in the registry.
    ///
    /// Used where vanilla dispatches on the class and no component says the
    /// same thing — `DoubleHighBlockItem.placeBlock` is the first such site.
    /// A class **name** rather than an enum because the extract carries the
    /// name and an enum here would be a hand-written list of 70 that goes
    /// stale at the next version, which is the defect `ci/item-props-golden.sh`
    /// exists to close rather than to move.
    #[must_use]
    pub fn class_of(&self, item: &str) -> Option<&str> {
        self.classes.get(item).map(String::as_str)
    }

    /// Is this item a `DoubleHighBlockItem`? 28 of them in 26.2.
    ///
    /// The class is spelled out here once. `is_a` rather than a public
    /// constant because the string is an implementation detail of the
    /// extract, and the question every caller actually has is this one.
    #[must_use]
    pub fn is_double_high(&self, item: &str) -> bool {
        self.class_of(item) == Some("net.minecraft.world.item.DoubleHighBlockItem")
    }

    /// `Item.use`'s arm for this item — see [`UseArm`].
    ///
    /// **Nothing calls this yet, and that is a stated gap rather than an
    /// oversight.** The packet that reaches `Item.use` is
    /// `minecraft:use_item`, [`rustmc_proto::ids::play::sb::USE_ITEM`], id
    /// 67, and no file under `crates/rustmc-server/` decodes it: right-click
    /// with nothing aimed at a block is unhandled on this server. Each arm's
    /// *effect* also needs a subsystem that does not exist — hunger for
    /// `Consumable`, an equipment model for `Equippable`, a use-duration
    /// clock for the other two. What is portable today is the **dispatch**,
    /// and it is here so the port that lands the packet fills bodies rather
    /// than re-deriving which component wins.
    ///
    /// An item not in the table is [`UseArm::Pass`], which is what
    /// `Item.use` does for a stack with none of the four components.
    #[must_use]
    pub fn use_arm(&self, item: &str) -> UseArm {
        let Some(comps) = self.use_components.get(item) else {
            return UseArm::Pass;
        };
        if comps.consumable {
            return UseArm::Consumable;
        }
        // **`swappable()` is the second half of the test at offset 55**, and
        // it is what sends `minecraft:shield` down the next arm instead of
        // this one. Nine items have `swappable: false`: the shield, the
        // carved pumpkin and seven heads.
        if comps.equippable_swappable {
            return UseArm::Equippable;
        }
        if comps.blocks_attacks {
            return UseArm::BlocksAttacks;
        }
        if comps.kinetic_weapon {
            return UseArm::KineticWeapon;
        }
        UseArm::Pass
    }

    /// How many items take each arm of `Item.use`, for a census that can be
    /// asserted rather than recomputed by hand.
    #[must_use]
    pub fn use_arm_count(&self, arm: UseArm) -> usize {
        self.classes
            .keys()
            .filter(|item| self.use_arm(item) == arm)
            .count()
    }

    /// How many items are of this Java class.
    ///
    /// The census in one call, so a test can assert vanilla's count rather
    /// than a list kept beside it: 938 `BlockItem`, 236 `Item`, 88
    /// `SpawnEggItem`, 28 `DoubleHighBlockItem` in 26.2.
    #[must_use]
    pub fn class_count(&self, class: &str) -> usize {
        self.classes.values().filter(|c| *c == class).count()
    }

    /// How many items carry a `minecraft:tool`, for the boot line.
    #[must_use]
    pub fn tools(&self) -> usize {
        self.tools.len()
    }

    /// How many items are damageable, for the boot line.
    ///
    /// **The `!has(UNBREAKABLE)` term of `isDamageableItem` has no witness in
    /// the extract** — no item in the 26.2 registry carries a default
    /// `minecraft:unbreakable`, measured 2026-09-05. So the real table cannot
    /// tell a correct transcription from one that dropped that term, and the
    /// synthetic row `b` in this module's tests is the only thing that can.
    /// Said here rather than left as a green count implying coverage it does
    /// not have.
    #[must_use]
    pub fn damageable(&self) -> usize {
        self.rows
            .iter()
            .filter(|(item, _)| self.is_damageable_item(item))
            .count()
    }

    /// How many items do **not** stack to 64, for the boot line.
    ///
    /// This is the number the whole table exists for: it is how wrong
    /// [`crate::item::DEFAULT_MAX_STACK_SIZE`] is, printed where an operator
    /// can see it rather than left in a doc comment.
    #[must_use]
    pub fn not_sixty_four(&self) -> usize {
        self.rows
            .values()
            .filter(|r| r.max_stack_size != 64)
            .count()
    }
}

/// One item's `minecraft:tool`, joined to the resolved sets.
///
/// A rule whose `blocks` key is not in `toolBlockSets` is an **error**, not a
/// rule that matches nothing: the two are indistinguishable at every call
/// site afterwards, and a missing key means the extract's two halves came
/// from different runs.
fn parse_tool(
    item: &str,
    tool: &serde_json::Value,
    set_index: &BTreeMap<String, usize>,
) -> io::Result<ToolComponent> {
    let mut rules = Vec::new();
    for rule in tool
        .get("rules")
        .and_then(serde_json::Value::as_array)
        .unwrap_or(&Vec::new())
    {
        let blocks = rule.get("blocks").ok_or_else(|| {
            io::Error::other(format!("item props: {item} tool rule has no blocks"))
        })?;
        let key = serde_json::to_string(blocks).map_err(io::Error::other)?;
        let blocks = *set_index.get(&key).ok_or_else(|| {
            io::Error::other(format!(
                "item props: {item}'s tool rule names the block set {key}, which \
                 toolBlockSets does not carry — the two halves of this extract do not \
                 come from one run"
            ))
        })?;
        rules.push(Rule {
            blocks,
            #[expect(
                clippy::cast_possible_truncation,
                reason = "the component is a float and the extract is its JSON"
            )]
            speed: rule
                .get("speed")
                .and_then(serde_json::Value::as_f64)
                .map(|v| v as f32),
            correct_for_drops: rule
                .get("correct_for_drops")
                .and_then(serde_json::Value::as_bool),
        });
    }
    Ok(ToolComponent {
        rules,
        #[expect(
            clippy::cast_possible_truncation,
            reason = "as above; no 26.2 item writes this field at all"
        )]
        default_mining_speed: tool
            .get("default_mining_speed")
            .and_then(serde_json::Value::as_f64)
            .map_or(1.0, |v| v as f32),
        damage_per_block: tool
            .get("damage_per_block")
            .and_then(serde_json::Value::as_i64)
            .and_then(|v| i32::try_from(v).ok())
            .unwrap_or(1),
        can_destroy_blocks_in_creative: tool
            .get("can_destroy_blocks_in_creative")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(json: &str) -> serde_json::Value {
        serde_json::from_str(json).expect("test json")
    }

    #[test]
    fn a_row_without_max_stack_size_is_refused() {
        // The discriminating case: an extract taken from a JVM that never ran
        // `DataComponentInitializers` has rows with empty component maps, and
        // defaulting those to 64 is exactly the bug this table removes. It
        // must be an error, not a default.
        let e = ItemProps::parse(&doc(
            r#"{"table":[{"item":"minecraft:stone","components":{}}]}"#,
        ))
        .expect_err("a row with no max_stack_size must be refused");
        assert!(
            e.to_string().contains("minecraft:stone"),
            "the refusal must name the item: {e}"
        );
    }

    #[test]
    fn stack_sizes_come_from_the_components() {
        let t = ItemProps::parse(&doc(r#"{"table":[
                {"item":"minecraft:stone","components":{"minecraft:max_stack_size":64}},
                {"item":"minecraft:bucket","components":{"minecraft:max_stack_size":16}},
                {"item":"minecraft:water_bucket","components":{"minecraft:max_stack_size":1}}
            ]}"#))
        .expect("parse");
        assert_eq!(t.max_stack_size("minecraft:stone"), 64);
        assert_eq!(t.max_stack_size("minecraft:bucket"), 16);
        assert_eq!(t.max_stack_size("minecraft:water_bucket"), 1);
        assert_eq!(t.not_sixty_four(), 2);
    }

    #[test]
    fn an_unknown_item_is_one_and_not_sixty_four() {
        // `ItemInstance.getMaxStackSize()`'s own default. Asserted because the
        // obvious wrong answer here — reuse `DEFAULT_MAX_STACK_SIZE` — is the
        // one the rest of this server currently gives.
        let t = ItemProps::parse(&doc(
            r#"{"table":[{"item":"minecraft:stone","components":{"minecraft:max_stack_size":64}}]}"#,
        ))
        .expect("parse");
        assert_eq!(t.max_stack_size("minecraft:not_an_item"), 1);
    }

    #[test]
    fn damageable_needs_all_three_components() {
        // `has(MAX_DAMAGE) && !has(UNBREAKABLE) && has(DAMAGE)`. Four rows,
        // one per arm of the conjunction, so a transcription that dropped a
        // term fails rather than agreeing by luck on the common case.
        let t = ItemProps::parse(&doc(r#"{"table":[
                {"item":"a","components":{"minecraft:max_stack_size":1,
                    "minecraft:max_damage":1561,"minecraft:damage":0}},
                {"item":"b","components":{"minecraft:max_stack_size":1,
                    "minecraft:max_damage":1561,"minecraft:damage":0,
                    "minecraft:unbreakable":{}}},
                {"item":"c","components":{"minecraft:max_stack_size":1,
                    "minecraft:max_damage":1561}},
                {"item":"d","components":{"minecraft:max_stack_size":64}}
            ]}"#))
        .expect("parse");
        assert!(
            t.is_damageable_item("a"),
            "max_damage + damage, not unbreakable"
        );
        assert!(!t.is_damageable_item("b"), "unbreakable wins");
        assert!(!t.is_damageable_item("c"), "no damage component");
        assert!(!t.is_damageable_item("d"), "no max_damage");
        assert_eq!(t.max_damage("a"), 1561);
        assert_eq!(t.max_damage("d"), 0);
    }

    /// The real extract, and the numbers this branch published.
    ///
    /// Fixture-gated through `rustmc_data::fixture` so an absent table is a
    /// **failure** and not a skip — the whole point of that module. The three
    /// items are one per value in the histogram, and the histogram itself is
    /// asserted, because a table that had lost its stack-size column entirely
    /// would still answer 64 for stone.
    #[test]
    fn the_real_table_is_not_all_sixty_four() {
        let path = std::path::Path::new("../..").join(TABLE_PATH);
        if !rustmc_data::fixture::available(&path, "item props table", "ci/item-props-golden.sh") {
            return;
        }
        let t = ItemProps::load(&path).expect("load the real table");
        assert_eq!(t.len(), 1537, "the 26.2 item registry");
        assert_eq!(t.max_stack_size("minecraft:stone"), 64);
        assert_eq!(t.max_stack_size("minecraft:bucket"), 16);
        assert_eq!(t.max_stack_size("minecraft:water_bucket"), 1);
        assert_eq!(
            t.not_sixty_four(),
            295,
            "295 of 1537 items do not stack to 64 — the size of the error in \
             crate::item::DEFAULT_MAX_STACK_SIZE"
        );
        assert_eq!(
            t.damageable(),
            84,
            "items with max_damage and damage and no unbreakable"
        );
        assert!(t.is_damageable_item("minecraft:diamond_sword"));
        assert!(!t.is_damageable_item("minecraft:stone"));
        assert_eq!(t.max_damage("minecraft:diamond_sword"), 1561);
    }

    /// **The case that separates the two loops**, and the reason
    /// `getMiningSpeed` and `isCorrectForDrops` are not one walk.
    ///
    /// A wooden pickaxe's rules are `[#incorrect_for_wooden_tool
    /// {correct_for_drops: false}, #mineable/pickaxe {speed: 2.0,
    /// correct_for_drops: true}]`, and obsidian is in **both** sets. The
    /// speed loop skips rule 0 because it carries no `speed` and answers
    /// 2.0; the drops loop stops at rule 0 and answers false. A fused loop
    /// gives one of those two answers and never both.
    ///
    /// The golden pickaxe is the louder version of the same thing: it mines
    /// obsidian at **12.0**, faster than a diamond pickaxe mines stone, and
    /// still drops nothing.
    #[test]
    fn a_pickaxe_can_mine_fast_and_drop_nothing() {
        let path = std::path::Path::new("../..").join(TABLE_PATH);
        if !rustmc_data::fixture::available(&path, "item props table", "ci/item-props-golden.sh") {
            return;
        }
        let t = ItemProps::load(&path).expect("load");
        for (item, speed) in [
            ("minecraft:wooden_pickaxe", 2.0),
            ("minecraft:golden_pickaxe", 12.0),
        ] {
            assert!(
                (t.mining_speed(item, "minecraft:obsidian") - speed).abs() < f32::EPSILON,
                "{item} on obsidian: {} not {speed}",
                t.mining_speed(item, "minecraft:obsidian")
            );
            assert!(
                !t.correct_tool_for_drops(item, "minecraft:obsidian"),
                "{item} is in #incorrect_for_{{wooden,gold}}_tool for obsidian"
            );
        }
        // The positive control for that pair of assertions: the same item on
        // a block only the second rule matches answers true.
        assert!(t.correct_tool_for_drops("minecraft:wooden_pickaxe", "minecraft:stone"));
        assert!(
            (t.mining_speed("minecraft:wooden_pickaxe", "minecraft:stone") - 2.0).abs()
                < f32::EPSILON
        );
    }

    /// The rest of the table's tool answers, one per shape:
    /// a matched rule, the fall-through to `defaultMiningSpeed`, an item with
    /// no `minecraft:tool` at all, an inline block id rather than a tag, and
    /// a rule carrying a speed and **no** `correct_for_drops`.
    #[test]
    fn the_real_tool_component_answers_five_shapes() {
        let path = std::path::Path::new("../..").join(TABLE_PATH);
        if !rustmc_data::fixture::available(&path, "item props table", "ci/item-props-golden.sh") {
            return;
        }
        let t = ItemProps::load(&path).expect("load");
        assert_eq!(t.tools(), 38, "items carrying a minecraft:tool in 26.2");

        let cases: &[(&str, &str, f32, bool)] = &[
            // a matched rule
            ("minecraft:diamond_pickaxe", "minecraft:stone", 8.0, true),
            ("minecraft:diamond_axe", "minecraft:oak_log", 8.0, true),
            // the fall-through: a pickaxe on a log matches no rule of its own
            ("minecraft:diamond_pickaxe", "minecraft:oak_log", 1.0, false),
            // an inline block id, not a tag
            ("minecraft:shears", "minecraft:cobweb", 15.0, true),
            ("minecraft:diamond_sword", "minecraft:cobweb", 15.0, true),
            // #sword_instantly_mines carries a speed and NO correct_for_drops,
            // so bamboo is mined at Float.MAX_VALUE and drops nothing
            (
                "minecraft:diamond_sword",
                "minecraft:bamboo",
                f32::MAX,
                false,
            ),
            // no minecraft:tool at all: Item.getDestroySpeed's fconst_1
            ("minecraft:stone", "minecraft:stone", 1.0, false),
        ];
        for &(item, block, speed, correct) in cases {
            let got = t.mining_speed(item, block);
            assert!(
                (got - speed).abs() <= speed.abs() * f32::EPSILON,
                "{item} on {block}: speed {got} not {speed}"
            );
            assert_eq!(
                t.correct_tool_for_drops(item, block),
                correct,
                "{item} on {block}: drops"
            );
        }

        assert_eq!(
            t.tool("minecraft:diamond_sword")
                .expect("a sword is a tool")
                .damage_per_block,
            2,
            "Item.mineBlock reads this, offsets 38-57"
        );
    }

    /// A rule naming a block set the extract does not carry is an error.
    /// Without this the two halves of a stale extract read as "a rule that
    /// matches nothing", which is a plausible tool with a silently wrong
    /// answer for every block.
    #[test]
    fn a_tool_rule_with_no_resolved_block_set_is_refused() {
        let e = ItemProps::parse(&doc(
            r##"{"toolBlockSets":{},"table":[{"item":"minecraft:x","components":{
                "minecraft:max_stack_size":1,
                "minecraft:tool":{"rules":[{"blocks":"#minecraft:mineable/pickaxe","speed":8.0}]}
            }}]}"##,
        ))
        .expect_err("an unresolved block set must be refused");
        assert!(e.to_string().contains("mineable/pickaxe"), "{e}");
    }

    /// `Tool.defaultMiningSpeed` has **no witness in the 26.2 extract** — no
    /// item writes the field, so every real tool falls through to the
    /// codec's own 1.0 and the table above cannot separate a correct read of
    /// this field from a hard-coded 1.0. This synthetic row can.
    #[test]
    fn the_default_mining_speed_is_read_and_not_assumed() {
        // The key is the JSON the rule's own `blocks` field encoded to, so
        // it carries its own quotes: `"\"#minecraft:mineable/pickaxe\""`.
        let t = ItemProps::parse(&doc(
            r##"{"toolBlockSets":{"\"#minecraft:mineable/pickaxe\"":["minecraft:stone"]},
                "table":[{"item":"minecraft:x","components":{
                    "minecraft:max_stack_size":1,
                    "minecraft:tool":{"default_mining_speed":4.5,
                        "rules":[{"blocks":"#minecraft:mineable/pickaxe","speed":8.0}]}
                }}]}"##,
        ))
        .expect("parse");
        assert!((t.mining_speed("minecraft:x", "minecraft:stone") - 8.0).abs() < f32::EPSILON);
        assert!(
            (t.mining_speed("minecraft:x", "minecraft:dirt") - 4.5).abs() < f32::EPSILON,
            "the fall-through is defaultMiningSpeed, not 1.0"
        );
    }

    /// `Item.use`'s ladder over the whole registry, one assertion per arm
    /// plus the census.
    ///
    /// **The shield is the case that pins the order.** It carries both
    /// `minecraft:equippable` and `minecraft:blocks_attacks`, and it reaches
    /// the `BlocksAttacks` arm only because its `equippable.swappable` is
    /// `false` — the second half of the test at offset 55. An implementation
    /// that checked `has(EQUIPPABLE)` alone would swap a shield onto the
    /// player's head; one that checked `BLOCKS_ATTACKS` before `EQUIPPABLE`
    /// would happen to be right about the shield and wrong about every piece
    /// of armour. Only the census separates those two.
    #[test]
    fn item_use_dispatches_on_four_components_in_order() {
        let path = std::path::Path::new("../..").join(TABLE_PATH);
        if !rustmc_data::fixture::available(&path, "item props table", "ci/item-props-golden.sh") {
            return;
        }
        let t = ItemProps::load(&path).expect("load");
        for (item, arm) in [
            ("minecraft:apple", UseArm::Consumable),
            ("minecraft:potion", UseArm::Consumable),
            ("minecraft:diamond_helmet", UseArm::Equippable),
            ("minecraft:shield", UseArm::BlocksAttacks),
            ("minecraft:iron_spear", UseArm::KineticWeapon),
            ("minecraft:stone", UseArm::Pass),
            ("minecraft:diamond_sword", UseArm::Pass),
            // Not in the registry at all.
            ("minecraft:not_an_item", UseArm::Pass),
            // `swappable: false` and no `blocks_attacks`: falls all the way
            // through, which is why a carved pumpkin is not worn by
            // right-clicking.
            ("minecraft:carved_pumpkin", UseArm::Pass),
        ] {
            assert_eq!(t.use_arm(item), arm, "{item}");
        }

        assert_eq!(t.use_arm_count(UseArm::Pass), 1411);
        assert_eq!(t.use_arm_count(UseArm::Equippable), 75);
        assert_eq!(t.use_arm_count(UseArm::Consumable), 43);
        assert_eq!(t.use_arm_count(UseArm::KineticWeapon), 7);
        assert_eq!(t.use_arm_count(UseArm::BlocksAttacks), 1);
        assert_eq!(
            t.use_arm_count(UseArm::Pass)
                + t.use_arm_count(UseArm::Equippable)
                + t.use_arm_count(UseArm::Consumable)
                + t.use_arm_count(UseArm::KineticWeapon)
                + t.use_arm_count(UseArm::BlocksAttacks),
            1537,
            "the five arms partition the registry"
        );
        // 84 items carry `minecraft:equippable` and only 75 take that arm.
        // The nine-item gap IS `swappable: false`, and asserting it here is
        // what stops the 75 from being read as the component's own count.
        assert_eq!(
            t.class_count("net.minecraft.world.item.Item"),
            236,
            "unrelated census, kept beside it so a table swap fails loudly"
        );
    }

    /// `Equippable.swappable` defaults to **true** in the codec, so an
    /// `equippable` with no `swappable` field takes the `Equippable` arm.
    /// The real table has both shapes; this pins the default without
    /// depending on which items happen to omit it.
    #[test]
    fn an_equippable_with_no_swappable_field_is_swappable() {
        let t = ItemProps::parse(&doc(r#"{"table":[
                {"item":"a","components":{"minecraft:max_stack_size":1,
                    "minecraft:equippable":{"slot":"head"}}},
                {"item":"b","components":{"minecraft:max_stack_size":1,
                    "minecraft:equippable":{"slot":"head","swappable":false}}},
                {"item":"c","components":{"minecraft:max_stack_size":1,
                    "minecraft:equippable":{"slot":"head","swappable":false},
                    "minecraft:blocks_attacks":{}}}
            ]}"#))
        .expect("parse");
        assert_eq!(t.use_arm("a"), UseArm::Equippable);
        assert_eq!(t.use_arm("b"), UseArm::Pass, "not swappable, nothing after");
        assert_eq!(t.use_arm("c"), UseArm::BlocksAttacks, "the shield's shape");
    }
}
