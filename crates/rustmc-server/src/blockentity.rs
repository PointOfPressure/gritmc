//! Block entities: the per-position state a block state has nowhere to keep.
//!
//! # What a block entity is, in the only terms that matter here
//!
//! A block state is an index into a global palette — `minecraft:chest[facing=
//! north,type=single,waterlogged=false]` is one of 32,366 values, and every
//! chest facing north in the world is the *same* value. There is no room in it
//! for twenty-seven item stacks. Vanilla's answer is a second map, held on the
//! chunk, from `BlockPos` to `BlockEntity`, and that is what this is.
//!
//! Until now this server had none, which is why
//! `560-storing-an-item-in-a-chest` reads "this server, which has no block
//! entities and therefore nowhere to keep an item, answers neither".
//!
//! # The lifecycle, from `LevelChunk` and `ServerPlayerGameMode`
//!
//! Four events, and all four have to be here or a chest is a leak:
//!
//! * **created on placement.** `Level.setBlock` -> `LevelChunk.setBlockState`
//!   sees the new block implements `EntityBlock`, calls `newBlockEntity(pos,
//!   state)` and adds it. It also *removes* the old one first when the block
//!   changed, which is the same call doing both halves.
//! * **loaded with the chunk.** `SerializableChunkData` reads the chunk's
//!   `block_entities` list and `BlockEntity.loadStatic` rebuilds each one.
//! * **ticked where the type requires it.** `EntityBlock.getTicker` returns
//!   null for most types. A chest's is `ChestBlockEntity::lidAnimateTick`,
//!   which is **client-side only**; the server-side repeat that makes an open
//!   chest signal every five ticks comes from `ContainerOpenersCounter`'s
//!   scheduled tick and not from a block-entity ticker. So a chest needs no
//!   entry here, and saying so is the point — a "tick every block entity"
//!   loop would be inventing work vanilla does not do.
//! * **removed on break.** `Level.removeBlock` -> the same
//!   `setBlockState` path, which calls `removeBlockEntity` when the new state
//!   has no block entity of its own.
//!
//! # What this deliberately does not take over
//!
//! [`crate::containers::Openers`] is keyed by position and says in its own
//! doc that it is there "because this server has no block entities yet". It
//! is **left exactly where it is.** Vanilla hangs the counter off
//! `ChestBlockEntity`, so folding it in here would be tidier and more
//! faithful — and it would also rewrite the mechanism behind four checks
//! (090, 470, 480, 490) that are currently at `match`, to buy nothing any
//! check can see. The honest note is that the two will differ the first time
//! a chest is broken while somebody has it open: the block entity goes and
//! the counter does not. That is a real defect, it is written down here, and
//! it is not this link's.
//!
//! # Why the store is not on the chunk
//!
//! Vanilla's is, and this one is a process-wide map keyed by absolute
//! position, for the same reason [`crate::edits`] keeps a separate overlay:
//! the generated canvas is the object every parity measurement is taken
//! against and nothing a player does may be written into it. A chunk here is
//! a *view* of canvas-plus-overlay rebuilt on demand, so there is no durable
//! per-chunk object to hang this off. The consequence that matters is
//! `save_to`, which asks "which chunks were edited" — so this map has to be
//! able to answer "which of your entries are in chunk (cx, cz)", and
//! [`BlockEntities::in_chunk`] is that question.

use crate::interact::BlockPos;
use crate::item::ItemStack;
use rustmc_world::nbt::Nbt;
use std::collections::BTreeMap;
use std::sync::{OnceLock, RwLock};

/// The block-entity types this server models.
///
/// **Not the whole registry, and the gap is a hazard rather than a feature
/// list.** `BlockEntityTypes` declares upwards of forty. Modelling a type
/// this server does not is harmless; *failing* to model one whose block the
/// corpus places is not, because the block then keeps no state and every
/// later assertion about it is measuring an empty position.
///
/// The first two are the two the container checks reach. `minecraft:furnace`
/// is present because `470-a-furnace-opens-a-screen-a-chest-does-not` opens
/// one and `menu.rs` already gives it a 3-slot menu; a furnace with no store
/// would answer a click into it with silence for the wrong reason.
///
/// `minecraft:sign` is the third and it is the one that breaks the shape the
/// other two suggested: it holds no items, it is the only one so far whose
/// `getUpdatePacket()` is not null, and it is the only one whose type maps to
/// more than one block.
///
/// # The eleven added on 2026-08-12, and why those eleven
///
/// They are the types the gameplay corpus can **see**. `block_entity_data` is
/// the only channel the corpus has into this registry — the other route is
/// the chunk payload, and `trace::FAMILIES` records that payload as one
/// opaque digest with no decoder arm, so no type is legible from it — and
/// `ChunkHolder.broadcastBlockEntity` sends nothing when `getUpdatePacket()`
/// is null:
///
/// ```text
///   14: invokevirtual BlockEntity.getUpdatePacket:()Lnet/minecraft/network/protocol/Packet;
///   17: astore        5
///   19: aload         5
///   21: ifnull        31          <-- almost every type leaves here
/// ```
///
/// `BlockEntity.getUpdatePacket()` is `aconst_null; areturn`, and **29 of the
/// 49 registered types inherit it unchanged** — a chest, a barrel, a hopper
/// and a lectern each have a block entity and vanilla never says so on the
/// wire. Twenty override it, all twenty unconditionally. Of those twenty,
/// `end_gateway` has no item to place it with, and `structure_block`,
/// `jigsaw`, `test_block` and `test_instance_block` are `GameMasterBlock`s
/// that refuse to place for a player without op — measured, not read:
/// `ci/block-entity-probe.sh` records `grass_block,air` for all four. Two more
/// are excluded for being unreproducible rather than unreachable — a
/// `mob_spawner`'s `Delay` and a `vault`'s `display_item` are fresh random
/// draws on every run, so a check on either could never come back `match` for
/// anybody. That leaves eleven, and this enum now holds all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// `BlockEntityTypes.CHEST` — `ChestBlockEntity`, `getContainerSize()`
    /// 27, read off `bipush 27` in the method body.
    Chest,
    /// `BlockEntityTypes.FURNACE` — `AbstractFurnaceBlockEntity`, three
    /// slots: ingredient, fuel, result.
    Furnace,
    /// `BlockEntityTypes.SIGN` — `SignBlockEntity`. **Not a container**: it
    /// has no `getContainerSize` at all, and it is the first type here whose
    /// state is not items. See [`crate::sign`].
    Sign,
    /// `BlockEntityTypes.HANGING_SIGN` — `HangingSignBlockEntity`, which
    /// **extends `SignBlockEntity`** and inherits its `getUpdateTag`. So its
    /// payload is byte-identical to a sign's: both come back
    /// `rest d010602cd96118eb` freshly placed, measured in the same probe run.
    /// It is a separate type only because it is a separate registry entry —
    /// id 8 rather than 7 — and putting 7 on the wire for one decodes cleanly
    /// and renders as the wrong thing.
    HangingSign,
    /// `BlockEntityTypes.BANNER` — `BannerBlockEntity`, whose `getUpdateTag`
    /// is `saveWithoutMetadata()`. A plain banner's `saveAdditional` writes
    /// nothing, so the whole payload is the `components` map the metadata-less
    /// save always emits: `{components: {}}`.
    Banner,
    /// `BlockEntityTypes.BEACON` — `BeaconBlockEntity`, `getUpdateTag` is
    /// `saveCustomOnly()`, which is `{Levels: <int>}` and nothing else for a
    /// beacon with no name and no effects. Zero until a pyramid is under it,
    /// and this server computes no pyramid — see [`BlockEntity::update_tag`].
    Beacon,
    /// `BlockEntityTypes.BRUSHABLE_BLOCK` — `BrushableBlockEntity`. Empty
    /// until something is brushed out of it.
    BrushableBlock,
    /// `BlockEntityTypes.CAMPFIRE` — `CampfireBlockEntity`, four item slots
    /// (`iconst_4` into `NonNullList.withSize`) and **not a `Container`**: it
    /// implements `Clearable` only, so [`Kind::container_size`] is 0 while
    /// [`Kind::item_slots`] is 4.
    Campfire,
    /// `BlockEntityTypes.CONDUIT` — `ConduitBlockEntity`. Its update tag is
    /// empty for a conduit with no target.
    Conduit,
    /// `BlockEntityTypes.COPPER_GOLEM_STATUE` — `CopperGolemStatueBlockEntity`.
    CopperGolemStatue,
    /// `BlockEntityTypes.CREAKING_HEART` — `CreakingHeartBlockEntity`. Empty
    /// while it has no creaking bound to it.
    CreakingHeart,
    /// `BlockEntityTypes.DECORATED_POT` — `DecoratedPotBlockEntity`. Empty
    /// until it has sherds or an item.
    DecoratedPot,
    /// `BlockEntityTypes.SHELF` — `ShelfBlockEntity`, three item slots
    /// (`iconst_3`), a `ListBackedContainer` and therefore a real container.
    /// The only new type whose update tag has **two** keys, which is why it is
    /// worth a check of its own: `getUpdateTag` writes `Items` first and
    /// `align_items_to_bottom` second, and vanilla puts them on the wire in
    /// `HashMap` bucket order rather than either that order or a sorted one.
    Shelf,
    /// `BlockEntityTypes.SKULL` — `SkullBlockEntity`. Empty for a skull with
    /// no profile and no note-block sound.
    Skull,

    // ------------------------------------------------------------------
    // The remaining thirty-five, added 2026-09-05, in `minecraft:
    // block_entity_type` **protocol-id order** rather than in the order the
    // fourteen above happen to sit in. The order of a Rust enum's variants is
    // not on the wire — `Kind::id()` is — so this is a readability choice, and
    // registry order is the one order a reader can check against a report.
    //
    // Every one of them is `getUpdatePacket() == null` unless its own doc says
    // otherwise, so adding it changes nothing a client sees: it gives the
    // position somewhere to keep state, which is the thing the census in
    // `docs/ASSESSMENT-2026-08-19.md` §1.2 counts and this server did not have.
    // ------------------------------------------------------------------
    /// `BlockEntityTypes.TRAPPED_CHEST` — `TrappedChestBlockEntity extends
    /// ChestBlockEntity`, so 27 slots inherited and no method of its own that
    /// touches storage. The redstone half lives on `TrappedChestBlock`.
    TrappedChest,
    /// `BlockEntityTypes.ENDER_CHEST` — `EnderChestBlockEntity`. **Not a
    /// container**: it declares no `getContainerSize` and holds no
    /// `NonNullList` at all. The twenty-seven slots a player sees belong to
    /// `Player.enderChestInventory`, which is why an ender chest is the same
    /// contents in every ender chest and why breaking one loses nothing.
    EnderChest,
    /// `BlockEntityTypes.JUKEBOX` — `JukeboxBlockEntity`, one stack, saved
    /// under `RecordItem` with a `ticks_since_song_started` long beside it.
    Jukebox,
    /// `BlockEntityTypes.DISPENSER` — `DispenserBlockEntity`,
    /// `getContainerSize()` `bipush 9`.
    Dispenser,
    /// `BlockEntityTypes.DROPPER` — `DropperBlockEntity extends
    /// DispenserBlockEntity`, which is where its nine slots come from; it
    /// overrides only `getDefaultName` and the dispense behaviour.
    Dropper,
    /// `BlockEntityTypes.MOB_SPAWNER` — `SpawnerBlockEntity`. **Its
    /// `getUpdatePacket` is non-null**, and it is one of the two types
    /// deliberately outside the gameplay corpus: `BaseSpawner.Delay` is a
    /// fresh `nextInt` on every run, so a check on its update tag could never
    /// come back `match` for anybody.
    MobSpawner,
    /// `BlockEntityTypes.PISTON` — `PistonMovingBlockEntity`, on
    /// `minecraft:moving_piston` only. See [`crate::piston`] for the movement
    /// itself; what is here is the position's right to hold the state vanilla
    /// hangs off it.
    Piston,
    /// `BlockEntityTypes.BREWING_STAND` — `BrewingStandBlockEntity`, five
    /// slots (`iconst_5` into `NonNullList.withSize`), with `BrewTime` and
    /// `Fuel` beside the items.
    BrewingStand,
    /// `BlockEntityTypes.ENCHANTING_TABLE` — `EnchantingTableBlockEntity`. No
    /// `saveAdditional` and no `loadAdditional`: every field on it is the
    /// client-side book animation, so its saved compound is the four metadata
    /// keys and nothing else.
    EnchantingTable,
    /// `BlockEntityTypes.END_PORTAL` — `TheEndPortalBlockEntity`. State-free
    /// for the same reason: the teleport is `EndPortalBlock.entityInside`.
    EndPortal,
    /// `BlockEntityTypes.DAYLIGHT_DETECTOR` — `DaylightDetectorBlockEntity`.
    /// State-free; the signal lives in the block state's `power` property and
    /// `DaylightDetectorBlock.updateSignalStrength` writes it. Check `710`
    /// covers that path and needs no block entity, which is exactly why this
    /// one is a position's right to exist and not a behaviour.
    DaylightDetector,
    /// `BlockEntityTypes.HOPPER` — `HopperBlockEntity`, five slots
    /// (`iconst_5`), plus the `TransferCooldown` int.
    Hopper,
    /// `BlockEntityTypes.COMPARATOR` — `ComparatorBlockEntity`, one int saved
    /// under `OutputSignal`. This is the value a comparator in `subtract`
    /// mode reads back, and it is the only reason a comparator needs a block
    /// entity at all.
    Comparator,
    /// `BlockEntityTypes.STRUCTURE_BLOCK` — `StructureBlockEntity`.
    /// Non-null `getUpdatePacket`, and unreachable from the corpus:
    /// `StructureBlock` is a `GameMasterBlock` and refuses to place for a
    /// player without op. `ci/block-entity-probe.sh` records `grass_block,air`
    /// for it.
    StructureBlock,
    /// `BlockEntityTypes.END_GATEWAY` — `TheEndGatewayBlockEntity`. Non-null
    /// `getUpdatePacket` and no item places it, so it is reachable only from
    /// generated terrain.
    EndGateway,
    /// `BlockEntityTypes.COMMAND_BLOCK` — `CommandBlockEntity`, on all three
    /// command blocks. A `GameMasterBlock`, so unreachable from the corpus for
    /// the same reason as the structure block.
    CommandBlock,
    /// `BlockEntityTypes.SHULKER_BOX` — `ShulkerBoxBlockEntity`, twenty-seven
    /// slots (`bipush 27` into `NonNullList.withSize`), on the seventeen
    /// shulker-box blocks.
    ShulkerBox,
    /// `BlockEntityTypes.BARREL` — `BarrelBlockEntity`,
    /// `getContainerSize()` `bipush 27`.
    Barrel,
    /// `BlockEntityTypes.SMOKER` — `SmokerBlockEntity extends
    /// AbstractFurnaceBlockEntity`: the same three slots as a furnace, a
    /// different recipe book and a halved cook time.
    Smoker,
    /// `BlockEntityTypes.BLAST_FURNACE` — `BlastFurnaceBlockEntity extends
    /// AbstractFurnaceBlockEntity`, three slots.
    BlastFurnace,
    /// `BlockEntityTypes.LECTERN` — `LecternBlockEntity`. One stack, and it
    /// is **not** in the `Items` list: `saveAdditional` writes the book under
    /// `Book` and the page under `Page`, and the inner `Container` it exposes
    /// to hoppers is an anonymous class over that single field.
    Lectern,
    /// `BlockEntityTypes.BELL` — `BellBlockEntity`. No `saveAdditional`: the
    /// ring is a transient `ticks`/`shaking` pair rebuilt from the
    /// `block_event` a ring broadcasts, so a bell saves the four metadata keys
    /// and nothing else.
    Bell,
    /// `BlockEntityTypes.JIGSAW` — `JigsawBlockEntity`. Non-null
    /// `getUpdatePacket`, `GameMasterBlock`, unreachable from the corpus.
    Jigsaw,
    /// `BlockEntityTypes.BEEHIVE` — `BeehiveBlockEntity`, on `bee_nest` and
    /// `beehive`. Holds no items: `bees` is a list of whole serialised
    /// entities and `flower_pos` is a block position, which is why this type
    /// is [`Kind::container_size`] 0 and [`Kind::item_slots`] 0 both.
    Beehive,
    /// `BlockEntityTypes.SCULK_SENSOR` — `SculkSensorBlockEntity`.
    SculkSensor,
    /// `BlockEntityTypes.CALIBRATED_SCULK_SENSOR` —
    /// `CalibratedSculkSensorBlockEntity extends SculkSensorBlockEntity`, a
    /// separate registry entry with its own `VibrationUser`.
    CalibratedSculkSensor,
    /// `BlockEntityTypes.SCULK_CATALYST` — `SculkCatalystBlockEntity`.
    SculkCatalyst,
    /// `BlockEntityTypes.SCULK_SHRIEKER` — `SculkShriekerBlockEntity`.
    SculkShrieker,
    /// `BlockEntityTypes.CHISELED_BOOKSHELF` —
    /// `ChiseledBookShelfBlockEntity`, six slots (`bipush 6`) and a
    /// `last_interacted_slot` int. A `ListBackedContainer`, so a real
    /// container, and the six `slot_N_occupied` block-state properties are
    /// written from it.
    ChiseledBookshelf,
    /// `BlockEntityTypes.CRAFTER` — `CrafterBlockEntity`,
    /// `getContainerSize()` `bipush 9`, plus `crafting_ticks_remaining`,
    /// `disabled_slots` and `triggered`.
    Crafter,
    /// `BlockEntityTypes.TRIAL_SPAWNER` — `TrialSpawnerBlockEntity`. Non-null
    /// `getUpdatePacket`, and the second of the two types outside the corpus
    /// for being unreproducible run to run.
    TrialSpawner,
    /// `BlockEntityTypes.VAULT` — `net.minecraft.world.level.block.entity.
    /// vault.VaultBlockEntity`. Its `display_item` is a fresh loot draw on
    /// every run, so it is outside the corpus for the same reason as the two
    /// spawners.
    Vault,
    /// `BlockEntityTypes.TEST_BLOCK` — `TestBlockEntity`. Non-null
    /// `getUpdatePacket`, `GameMasterBlock`, unreachable from the corpus.
    TestBlock,
    /// `BlockEntityTypes.TEST_INSTANCE_BLOCK` — `TestInstanceBlockEntity`.
    /// Same three properties as the test block.
    TestInstanceBlock,
    /// `BlockEntityTypes.POTENT_SULFUR` — `PotentSulfurBlockEntity`, the
    /// forty-ninth and last registry entry.
    PotentSulfur,
}

/// A key's starting value, in a form a `const` table can hold.
///
/// `Nbt` owns a `String` and a `BTreeMap`, so it cannot be built in a `const`;
/// this can, and [`ExtraValue::to_nbt`] is the one place the two meet. The
/// variants are exactly the tag types vanilla's block entities write into a
/// **freshly constructed** compound — no `Double`, no `ByteArray`, no
/// `LongArray`, because none of the forty-nine writes one.
#[derive(Debug, Clone, Copy)]
pub enum ExtraValue {
    /// `putByte`, and `putBoolean`, which is a byte on the wire.
    Byte(i8),
    /// `putShort`.
    Short(i16),
    /// `putInt`.
    Int(i32),
    /// `putLong`.
    Long(i64),
    /// `putFloat`.
    Float(f32),
    /// `putString`, or a codec that resolves to one — an enum name, a
    /// resource location.
    Str(&'static str),
    /// `putIntArray`, whose length is fixed by the constructor: a campfire's
    /// two cooking arrays are four long and a crafter's `disabled_slots` is
    /// empty.
    IntArray(&'static [i32]),
    /// An empty `ValueOutput.list` — `bees`, `cursors`, `SpawnPotentials`.
    EmptyList,
    /// An empty compound — `RecipesUsed`, and `components`' own shape.
    EmptyCompound,
    /// A compound this table cannot spell inline. The two are the sculk
    /// family's `listener` and the test instance block's `data`, and each is
    /// a named function beside its table rather than a literal here.
    Nested(fn() -> Nbt),
}

/// Two values are equal when the tags they produce are equal.
///
/// Hand-written because [`ExtraValue::Nested`] holds a function pointer, and
/// comparing those compares *addresses* — which Rust does not guarantee are
/// unique, so a derived `PartialEq` would answer a question about codegen
/// units rather than about the value.
impl PartialEq for ExtraValue {
    fn eq(&self, other: &Self) -> bool {
        self.to_nbt() == other.to_nbt()
    }
}

impl ExtraValue {
    /// The tag, allocated.
    #[must_use]
    pub fn to_nbt(self) -> Nbt {
        match self {
            Self::Byte(v) => Nbt::Byte(v),
            Self::Short(v) => Nbt::Short(v),
            Self::Int(v) => Nbt::Int(v),
            Self::Long(v) => Nbt::Long(v),
            Self::Float(v) => Nbt::Float(v),
            Self::Str(v) => Nbt::String(v.to_string()),
            Self::IntArray(v) => Nbt::IntArray(v.to_vec()),
            Self::EmptyList => Nbt::List(Vec::new()),
            Self::EmptyCompound => Nbt::Compound(BTreeMap::new()),
            Self::Nested(f) => f(),
        }
    }
}

/// One key a type's save writes beside its items, with the value the
/// constructor leaves the field at.
///
/// **The tag width is part of the transcription and is not free to choose.**
/// `BrewTime` is a `putShort` and `Fuel` a `putByte` six instructions apart in
/// one method; a crafter's `triggered` is an **int** where every other
/// boolean-looking field here is a byte. A compound that gets any of them
/// wrong round-trips through this server and is refused by vanilla's codec,
/// which is why `ci/block-entity-golden.sh` dumps `Tag.getId()` beside every
/// key and the test compares on the pair.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExtraKey {
    /// The NBT key, exactly as the `ldc` in `saveAdditional` spells it.
    pub name: &'static str,
    /// The constructor's value, which is what a freshly placed block saves.
    pub value: ExtraValue,
}

impl ExtraKey {
    const fn new(name: &'static str, value: ExtraValue) -> Self {
        Self { name, value }
    }
    const fn int(name: &'static str, v: i32) -> Self {
        Self::new(name, ExtraValue::Int(v))
    }
    const fn short(name: &'static str, v: i16) -> Self {
        Self::new(name, ExtraValue::Short(v))
    }
    const fn byte_(name: &'static str, v: i8) -> Self {
        Self::new(name, ExtraValue::Byte(v))
    }
    const fn long(name: &'static str, v: i64) -> Self {
        Self::new(name, ExtraValue::Long(v))
    }
    const fn float(name: &'static str, v: f32) -> Self {
        Self::new(name, ExtraValue::Float(v))
    }
    const fn str_(name: &'static str, v: &'static str) -> Self {
        Self::new(name, ExtraValue::Str(v))
    }
    const fn list(name: &'static str) -> Self {
        Self::new(name, ExtraValue::EmptyList)
    }
    const fn compound(name: &'static str) -> Self {
        Self::new(name, ExtraValue::EmptyCompound)
    }
    const fn ints(name: &'static str, v: &'static [i32]) -> Self {
        Self::new(name, ExtraValue::IntArray(v))
    }
}

// ---------------------------------------------------------------------------
// The per-type key tables.
//
// Transcribed from each class's `saveAdditional` and constructor, and checked
// key-for-key and **tag-for-tag** against `ci/block-entity-golden.sh`'s dump of
// a freshly constructed instance's `saveWithFullMetadata` by
// `a_fresh_block_entity_saves_vanillas_keys_at_vanillas_tag_widths`.
//
// Two of the forty-nine have no entry here because the extract could not
// construct them at all: `minecraft:piston` and `minecraft:vault` both throw
// in a bootstrapped-only JVM (`VaultConfig.DEFAULT`'s class initialiser is the
// vault's). They are declared empty and the test skips the rows it has no
// answer for, rather than reading an absent answer as "writes nothing".
// ---------------------------------------------------------------------------

/// `HopperBlockEntity.cooldownTime`, `iconst_m1` in the constructor.
const HOPPER_EXTRAS: [ExtraKey; 1] = [ExtraKey::int("TransferCooldown", -1)];
/// The four `putShort`s of `AbstractFurnaceBlockEntity.saveAdditional`, and
/// the `RecipesUsed` map it stores through `RECIPES_USED_CODEC`.
const FURNACE_EXTRAS: [ExtraKey; 5] = [
    ExtraKey::short("cooking_time_spent", 0),
    ExtraKey::short("cooking_total_time", 0),
    ExtraKey::short("lit_time_remaining", 0),
    ExtraKey::short("lit_total_time", 0),
    ExtraKey::compound("RecipesUsed"),
];
/// One `putShort` and one `putByte`, six instructions apart.
const BREWING_STAND_EXTRAS: [ExtraKey; 2] =
    [ExtraKey::short("BrewTime", 0), ExtraKey::byte_("Fuel", 0)];
/// `ComparatorBlockEntity.output`, the value a comparator reads back.
const COMPARATOR_EXTRAS: [ExtraKey; 1] = [ExtraKey::int("OutputSignal", 0)];
/// `ChiseledBookShelfBlockEntity.lastInteractedSlot`, `iconst_m1`.
const CHISELED_BOOKSHELF_EXTRAS: [ExtraKey; 1] = [ExtraKey::int("last_interacted_slot", -1)];
/// `CrafterBlockEntity`. **`triggered` is an `int`**, not the byte every other
/// boolean-shaped field in this file is — `addTriggered` writes it through a
/// codec that resolves to one, and the golden dump says `triggered:3:0`.
const CRAFTER_EXTRAS: [ExtraKey; 3] = [
    ExtraKey::int("crafting_ticks_remaining", 0),
    ExtraKey::ints("disabled_slots", &[]),
    ExtraKey::int("triggered", 0),
];
/// `BeaconBlockEntity.levels`. The same key its update tag carries, and the
/// saved compound is where it also belongs.
const BEACON_EXTRAS: [ExtraKey; 1] = [ExtraKey::int("Levels", 0)];
/// `LecternBlockEntity.page`, `putInt("Page", page)` — written **only** when
/// the lectern holds a book (`saveAdditional` offset 12 tests
/// `getBook().isEmpty()` first); see the guard in `to_nbt`.
const LECTERN_EXTRAS: [ExtraKey; 1] = [ExtraKey::int("Page", 0)];
/// `ShelfBlockEntity.alignItemsToBottom`, `putBoolean` — a byte.
const SHELF_EXTRAS: [ExtraKey; 1] = [ExtraKey::byte_("align_items_to_bottom", 0)];
/// `CampfireBlockEntity`'s two `putIntArray`s, **four long**, one cell per
/// item slot. A zero-length array here would be a campfire whose four cooking
/// timers vanished.
const CAMPFIRE_EXTRAS: [ExtraKey; 2] = [
    ExtraKey::ints("CookingTimes", &[0, 0, 0, 0]),
    ExtraKey::ints("CookingTotalTimes", &[0, 0, 0, 0]),
];
/// `BeehiveBlockEntity.stored`, a list of whole serialised bees.
const BEEHIVE_EXTRAS: [ExtraKey; 1] = [ExtraKey::list("bees")];
/// `SculkCatalystBlockEntity`'s spreader cursors.
const SCULK_CATALYST_EXTRAS: [ExtraKey; 1] = [ExtraKey::list("cursors")];
/// `SculkSensorBlockEntity`, and `CalibratedSculkSensorBlockEntity` inherits it.
const SCULK_SENSOR_EXTRAS: [ExtraKey; 2] = [
    ExtraKey::int("last_vibration_frequency", 0),
    ExtraKey::new("listener", ExtraValue::Nested(vibration_listener)),
];
/// `SculkShriekerBlockEntity` — the same listener and a warning level, and
/// **no** `last_vibration_frequency`.
const SCULK_SHRIEKER_EXTRAS: [ExtraKey; 2] = [
    ExtraKey::new("listener", ExtraValue::Nested(vibration_listener)),
    ExtraKey::int("warning_level", 0),
];
/// `PotentSulfurBlockEntity.countdown`, which starts at -1 like a hopper's
/// cooldown and for the same reason: -1 is "not running", 0 is "due now".
const POTENT_SULFUR_EXTRAS: [ExtraKey; 1] = [ExtraKey::int("countdown", -1)];
/// `TheEndGatewayBlockEntity.age`, a `putLong`.
const END_GATEWAY_EXTRAS: [ExtraKey; 1] = [ExtraKey::long("Age", 0)];
/// `BaseSpawner`'s eight, all `putShort` but the potentials list. **`Delay`
/// is 20 on a fresh spawner and a random draw on every later save**, which is
/// why `mob_spawner` is outside the gameplay corpus; 20 is the constructor's
/// value and therefore the right thing for a block just placed.
const MOB_SPAWNER_EXTRAS: [ExtraKey; 8] = [
    ExtraKey::short("Delay", 20),
    ExtraKey::short("MaxNearbyEntities", 6),
    ExtraKey::short("MaxSpawnDelay", 800),
    ExtraKey::short("MinSpawnDelay", 200),
    ExtraKey::short("RequiredPlayerRange", 16),
    ExtraKey::short("SpawnCount", 4),
    ExtraKey::list("SpawnPotentials"),
    ExtraKey::short("SpawnRange", 4),
];
/// `CommandBlockEntity`, through `BaseCommandBlock.save` plus its own three.
/// `TrackOutput` and `UpdateLastExecution` start **true**.
const COMMAND_BLOCK_EXTRAS: [ExtraKey; 7] = [
    ExtraKey::str_("Command", ""),
    ExtraKey::int("SuccessCount", 0),
    ExtraKey::byte_("TrackOutput", 1),
    ExtraKey::byte_("UpdateLastExecution", 1),
    ExtraKey::byte_("auto", 0),
    ExtraKey::byte_("conditionMet", 0),
    ExtraKey::byte_("powered", 0),
];
/// `JigsawBlockEntity`. Three of the seven are resource locations that default
/// to `minecraft:empty` and one to `minecraft:air`, so an empty string would
/// be wrong in four places.
const JIGSAW_EXTRAS: [ExtraKey; 7] = [
    ExtraKey::str_("final_state", "minecraft:air"),
    ExtraKey::str_("joint", "rollable"),
    ExtraKey::str_("name", "minecraft:empty"),
    ExtraKey::int("placement_priority", 0),
    ExtraKey::str_("pool", "minecraft:empty"),
    ExtraKey::int("selection_priority", 0),
    ExtraKey::str_("target", "minecraft:empty"),
];
/// `StructureBlockEntity` — nineteen keys, the widest in the registry.
/// **`posY` starts at 1 and every other coordinate at 0**, `integrity` is a
/// float, `seed` a long, and `ignoreEntities`/`showboundingbox` start true
/// while the other three booleans start false. Every one of those is a place
/// a plausible guess is wrong.
const STRUCTURE_BLOCK_EXTRAS: [ExtraKey; 19] = [
    ExtraKey::str_("author", ""),
    ExtraKey::byte_("ignoreEntities", 1),
    ExtraKey::float("integrity", 1.0),
    ExtraKey::str_("metadata", ""),
    ExtraKey::str_("mirror", "NONE"),
    ExtraKey::str_("mode", "LOAD"),
    ExtraKey::str_("name", ""),
    ExtraKey::int("posX", 0),
    ExtraKey::int("posY", 1),
    ExtraKey::int("posZ", 0),
    ExtraKey::byte_("powered", 0),
    ExtraKey::str_("rotation", "NONE"),
    ExtraKey::long("seed", 0),
    ExtraKey::byte_("showair", 0),
    ExtraKey::byte_("showboundingbox", 1),
    ExtraKey::int("sizeX", 0),
    ExtraKey::int("sizeY", 0),
    ExtraKey::int("sizeZ", 0),
    ExtraKey::byte_("strict", 0),
];
/// `TestBlockEntity`.
const TEST_BLOCK_EXTRAS: [ExtraKey; 3] = [
    ExtraKey::str_("message", ""),
    ExtraKey::str_("mode", "start"),
    ExtraKey::byte_("powered", 0),
];
/// `TestInstanceBlockEntity`, whose whole state is one nested compound.
const TEST_INSTANCE_BLOCK_EXTRAS: [ExtraKey; 1] = [ExtraKey::new(
    "data",
    ExtraValue::Nested(test_instance_data),
)];

/// `VibrationSystem.Data`'s saved shape — the `listener` compound the three
/// sculk types share.
///
/// `{event_delay: 0, selector: {tick: -1L}}`. The inner `tick` is a **long**
/// and starts at -1, which is `VibrationSelector`'s "nothing selected"; the
/// outer `event_delay` is an int at 0.
fn vibration_listener() -> Nbt {
    let mut selector = BTreeMap::new();
    selector.insert("tick".to_string(), Nbt::Long(-1));
    let mut listener = BTreeMap::new();
    listener.insert("event_delay".to_string(), Nbt::Int(0));
    listener.insert("selector".to_string(), Nbt::Compound(selector));
    Nbt::Compound(listener)
}

/// `TestInstanceBlockEntity`'s `data` compound:
/// `{ignore_entities: 0b, rotation: "none", size: [I; 0,0,0], status: "cleared"}`.
fn test_instance_data() -> Nbt {
    let mut c = BTreeMap::new();
    c.insert("ignore_entities".to_string(), Nbt::Byte(0));
    c.insert("rotation".to_string(), Nbt::String("none".to_string()));
    c.insert("size".to_string(), Nbt::IntArray(vec![0, 0, 0]));
    c.insert("status".to_string(), Nbt::String("cleared".to_string()));
    Nbt::Compound(c)
}

/// The key every block entity carries and none of them declares.
///
/// `BlockEntity.saveWithoutMetadata` ends with an unconditional
/// `ValueOutput.store("components", DataComponentMap.CODEC, this.components)`
/// — a `store`, not a `storeNullable` — so **all forty-nine write it**, as an
/// empty compound when nothing has set one. This server wrote it for none of
/// them, which is one key missing from every block entity in every saved
/// chunk.
const COMPONENTS_KEY: &str = "components";

impl Kind {
    /// Whether this kind's block entity extends `BaseContainerBlockEntity`,
    /// whose `applyImplicitComponents` takes `minecraft:custom_name` off the
    /// placed stack and whose `getDisplayName` is then the menu title: the
    /// eight randomizable kinds below, the three furnaces and the brewing
    /// stand. Enchanting tables, beacons, banners and skulls are `Nameable`
    /// through their own classes and are not here.
    #[must_use]
    pub const fn is_nameable_container(self) -> bool {
        matches!(
            self,
            Self::Chest
                | Self::TrappedChest
                | Self::Barrel
                | Self::Furnace
                | Self::BlastFurnace
                | Self::Smoker
                | Self::Dispenser
                | Self::Dropper
                | Self::Hopper
                | Self::ShulkerBox
                | Self::BrewingStand
                | Self::Crafter
        )
    }

    /// The key a kind's `saveAdditional` stores its custom name under, for
    /// every kind whose `applyImplicitComponents` reads `CUSTOM_NAME`: the
    /// twelve [`Kind::is_nameable_container`] kinds, the banner, the beacon
    /// and the enchanting table write `CustomName`; `SkullBlockEntity` writes
    /// `custom_name` (its `saveAdditional`, offsets 36-45). `None` for a kind
    /// that has no name to take.
    #[must_use]
    pub const fn custom_name_key(self) -> Option<&'static str> {
        if self.is_nameable_container() {
            return Some("CustomName");
        }
        match self {
            Self::Banner | Self::Beacon | Self::EnchantingTable => Some("CustomName"),
            Self::Skull => Some("custom_name"),
            _ => None,
        }
    }

    /// Whether this kind's block entity is a `RandomizableContainer` — the
    /// classes extending `RandomizableContainerBlockEntity` in the pinned jar
    /// (`Barrel`, `Chest`, `Crafter`, `Dispenser`, `Hopper`, `ShulkerBox`,
    /// with `TrappedChest` and `Dropper` inheriting). `DecoratedPot` also
    /// implements the interface but saves its own way and is not here.
    #[must_use]
    pub const fn is_randomizable(self) -> bool {
        matches!(
            self,
            Self::Chest
                | Self::TrappedChest
                | Self::Barrel
                | Self::Crafter
                | Self::Dispenser
                | Self::Dropper
                | Self::Hopper
                | Self::ShulkerBox
        )
    }

    /// The `minecraft:block_entity_type` registry name, which is what goes in
    /// the `id` key of the saved compound **and** what resolves to the
    /// numeric type id `block_entity_data` carries — see
    /// [`crate::protocol_ids`].
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Chest => "minecraft:chest",
            Self::Furnace => "minecraft:furnace",
            Self::Sign => crate::sign::TYPE_ID,
            Self::HangingSign => "minecraft:hanging_sign",
            Self::Banner => "minecraft:banner",
            Self::Beacon => "minecraft:beacon",
            Self::BrushableBlock => "minecraft:brushable_block",
            Self::Campfire => "minecraft:campfire",
            Self::Conduit => "minecraft:conduit",
            Self::CopperGolemStatue => "minecraft:copper_golem_statue",
            Self::CreakingHeart => "minecraft:creaking_heart",
            Self::DecoratedPot => "minecraft:decorated_pot",
            Self::Shelf => "minecraft:shelf",
            Self::Skull => "minecraft:skull",
            Self::TrappedChest => "minecraft:trapped_chest",
            Self::EnderChest => "minecraft:ender_chest",
            Self::Jukebox => "minecraft:jukebox",
            Self::Dispenser => "minecraft:dispenser",
            Self::Dropper => "minecraft:dropper",
            Self::MobSpawner => "minecraft:mob_spawner",
            Self::Piston => "minecraft:piston",
            Self::BrewingStand => "minecraft:brewing_stand",
            Self::EnchantingTable => "minecraft:enchanting_table",
            Self::EndPortal => "minecraft:end_portal",
            Self::DaylightDetector => "minecraft:daylight_detector",
            Self::Hopper => "minecraft:hopper",
            Self::Comparator => "minecraft:comparator",
            Self::StructureBlock => "minecraft:structure_block",
            Self::EndGateway => "minecraft:end_gateway",
            Self::CommandBlock => "minecraft:command_block",
            Self::ShulkerBox => "minecraft:shulker_box",
            Self::Barrel => "minecraft:barrel",
            Self::Smoker => "minecraft:smoker",
            Self::BlastFurnace => "minecraft:blast_furnace",
            Self::Lectern => "minecraft:lectern",
            Self::Bell => "minecraft:bell",
            Self::Jigsaw => "minecraft:jigsaw",
            Self::Beehive => "minecraft:beehive",
            Self::SculkSensor => "minecraft:sculk_sensor",
            Self::CalibratedSculkSensor => "minecraft:calibrated_sculk_sensor",
            Self::SculkCatalyst => "minecraft:sculk_catalyst",
            Self::SculkShrieker => "minecraft:sculk_shrieker",
            Self::ChiseledBookshelf => "minecraft:chiseled_bookshelf",
            Self::Crafter => "minecraft:crafter",
            Self::TrialSpawner => "minecraft:trial_spawner",
            Self::Vault => "minecraft:vault",
            Self::TestBlock => "minecraft:test_block",
            Self::TestInstanceBlock => "minecraft:test_instance_block",
            Self::PotentSulfur => "minecraft:potent_sulfur",
        }
    }

    /// Every variant, in the order [`Kind::id`] resolves the registry.
    ///
    /// Exists so that a test can assert this enum against
    /// `minecraft:block_entity_type` **without** a hand-maintained list beside
    /// it — a list a new variant could be added without touching, which is the
    /// shape of a check that stops covering the thing it names.
    pub const ALL: [Self; 49] = [
        Self::Chest,
        Self::Furnace,
        Self::Sign,
        Self::HangingSign,
        Self::Banner,
        Self::Beacon,
        Self::BrushableBlock,
        Self::Campfire,
        Self::Conduit,
        Self::CopperGolemStatue,
        Self::CreakingHeart,
        Self::DecoratedPot,
        Self::Shelf,
        Self::Skull,
        Self::TrappedChest,
        Self::EnderChest,
        Self::Jukebox,
        Self::Dispenser,
        Self::Dropper,
        Self::MobSpawner,
        Self::Piston,
        Self::BrewingStand,
        Self::EnchantingTable,
        Self::EndPortal,
        Self::DaylightDetector,
        Self::Hopper,
        Self::Comparator,
        Self::StructureBlock,
        Self::EndGateway,
        Self::CommandBlock,
        Self::ShulkerBox,
        Self::Barrel,
        Self::Smoker,
        Self::BlastFurnace,
        Self::Lectern,
        Self::Bell,
        Self::Jigsaw,
        Self::Beehive,
        Self::SculkSensor,
        Self::CalibratedSculkSensor,
        Self::SculkCatalyst,
        Self::SculkShrieker,
        Self::ChiseledBookshelf,
        Self::Crafter,
        Self::TrialSpawner,
        Self::Vault,
        Self::TestBlock,
        Self::TestInstanceBlock,
        Self::PotentSulfur,
    ];

    /// `Container.getContainerSize()`, and **0 for a type that is not a
    /// container at all**.
    ///
    /// Zero rather than an `Option` because every caller wants a slot count
    /// and the honest slot count of a sign is none: `may_place` refuses every
    /// index against it, `item` is empty everywhere, and a click into a sign
    /// does nothing. An `Option` would push the same answer to each call site
    /// and give each one a chance to get it wrong.
    #[must_use]
    pub const fn container_size(self) -> usize {
        match self {
            Self::Chest => 27,
            Self::Furnace => 3,
            // `ShelfBlockEntity implements ListBackedContainer`, whose default
            // `getContainerSize` is its own list's length — `iconst_3`.
            Self::Shelf => 3,
            // `TrappedChestBlockEntity extends ChestBlockEntity` and adds no
            // storage of its own; `BarrelBlockEntity` and
            // `ShulkerBoxBlockEntity` are `bipush 27` independently.
            Self::TrappedChest | Self::Barrel | Self::ShulkerBox => 27,
            // `DispenserBlockEntity` is `bipush 9`; `DropperBlockEntity`
            // extends it and `CrafterBlockEntity` is its own `bipush 9`.
            Self::Dispenser | Self::Dropper | Self::Crafter => 9,
            // `HopperBlockEntity` and `BrewingStandBlockEntity` both
            // `iconst_5` into `NonNullList.withSize`.
            Self::Hopper | Self::BrewingStand => 5,
            // Both extend `AbstractFurnaceBlockEntity` — the same three slots
            // as a furnace, a different recipe book and a different cook time.
            Self::Smoker | Self::BlastFurnace => 3,
            // `ChiseledBookShelfBlockEntity`: `bipush 6`, one per
            // `slot_N_occupied` property on the block.
            Self::ChiseledBookshelf => 6,
            // **One, and neither of them saves it under `Items`.** A jukebox
            // holds the disc in a bare `ItemStack` field and writes it under
            // `RecordItem`; a decorated pot's single stack goes under `item`.
            // The container size is still 1 because both implement
            // `Container` — a hopper can put a disc in a jukebox — which is
            // exactly why the slot count and the save key are two questions.
            Self::Jukebox | Self::DecoratedPot => 1,
            // Everything else is not a `Container` at all. A campfire holds
            // four stacks and still answers 0 here, which is the distinction
            // [`Kind::item_slots`] exists to keep: `implements Clearable`,
            // no `getContainerSize` anywhere in its hierarchy.
            _ => 0,
        }
    }

    /// The length of the block entity's own `NonNullList<ItemStack>`.
    ///
    /// **Not the same question as [`Kind::container_size`], and conflating the
    /// two would be wrong in both directions.** `getContainerSize()` is what a
    /// menu and a slot index are checked against; this is what
    /// `ContainerHelper.saveAllItems` iterates when it writes the `Items` key
    /// of an update tag. A campfire has four stacks and no container, so a
    /// click into "slot 2 of a campfire" must be refused while its `Items`
    /// list still has to be four long — a single number cannot say both.
    #[must_use]
    pub const fn item_slots(self) -> usize {
        match self {
            // `CampfireBlockEntity`: `iconst_4` into `NonNullList.withSize`.
            Self::Campfire => 4,
            _ => self.container_size(),
        }
    }

    /// How many stacks this server stores for the type: [`Kind::item_slots`],
    /// plus the one bare `ItemStack` field a **lectern** holds its book in.
    ///
    /// `LecternBlockEntity.book` is neither a `NonNullList` (so
    /// [`Kind::item_slots`], which counts those, is 0 — vanilla's own golden
    /// says so) nor a container slot (`bookAccess` is an inner class, so
    /// [`Kind::container_size`] is 0 too), and it still has to live somewhere
    /// for `Book` to be saved and a `LecternMenu` to show it. The jukebox and
    /// the decorated pot hold theirs the same way and already had a slot,
    /// through their container size.
    #[must_use]
    pub const fn storage_slots(self) -> usize {
        match self {
            Self::Lectern => 1,
            _ => self.item_slots(),
        }
    }

    /// The key this type's `saveAdditional` writes its stacks under, and
    /// whether the key is written when there is nothing in it.
    ///
    /// # Three different answers over one question, and a size cannot say which
    ///
    /// `ContainerHelper.saveAllItems(out, items, alwaysWrite)` ends
    ///
    /// ```text
    ///   66: TypedOutputList.isEmpty : ifeq 87
    ///   75: iload_2 (alwaysWrite)   : ifne 87
    ///   79: ValueOutput.discard("Items")
    /// ```
    ///
    /// so the flag decides whether an empty container writes `{Items: []}` or
    /// writes no key at all. Every container in the registry passes `true`,
    /// through either the two-argument overload or an explicit `iconst_1` —
    /// **except `ShulkerBoxBlockEntity`, which passes `iconst_0`.** An empty
    /// shulker box saves no `Items` key, and one written with an empty list is
    /// a compound vanilla never emits.
    ///
    /// And three types with a slot save it under a different name entirely:
    /// `RecordItem` for a jukebox, `Book` for a lectern, `item` for a
    /// decorated pot. A `to_nbt` keyed on `item_slots() > 0` wrote `Items` for
    /// all three, which round-trips here and is rejected by vanilla.
    #[must_use]
    pub const fn items_key(self) -> Option<(&'static str, bool)> {
        match self {
            // `saveAllItems(out, items)` — the two-argument overload, whose
            // whole body is `iconst_1; invokestatic saveAllItems(_,_,Z)`.
            Self::Chest
            | Self::TrappedChest
            | Self::Furnace
            | Self::Smoker
            | Self::BlastFurnace
            | Self::Barrel
            | Self::Hopper
            | Self::Dispenser
            | Self::Dropper
            | Self::BrewingStand
            | Self::Crafter => Some(("Items", true)),
            // The three-argument form with an explicit `iconst_1`.
            Self::Campfire | Self::Shelf | Self::ChiseledBookshelf => Some(("Items", true)),
            // The one `iconst_0` in the registry.
            Self::ShulkerBox => Some(("Items", false)),
            // A single stack under its own name. All three are
            // `ValueOutput.store(key, ItemStack.CODEC, stack)` guarded by an
            // `isEmpty()` test, so an empty one writes no key — which is what
            // the `false` says.
            Self::Jukebox => Some(("RecordItem", false)),
            Self::Lectern => Some(("Book", false)),
            Self::DecoratedPot => Some(("item", false)),
            _ => None,
        }
    }

    /// The scalar keys this type's `saveAdditional` writes beside its items.
    ///
    /// # Why a table and not a field per type
    ///
    /// Ten of the forty-nine write one to four plain numbers and nothing else
    /// distinguishes them: a hopper's cooldown, a comparator's held signal, a
    /// furnace's four cook counters. A struct per type would be ten structs
    /// whose only content is a name, an NBT tag type and a starting value —
    /// which is a table, written ten times.
    ///
    /// **The tag type is part of the transcription and is not free to choose.**
    /// `BrewTime` is `putShort` and `Fuel` is `putByte` one line apart in
    /// `BrewingStandBlockEntity.saveAdditional`; a compound that wrote both as
    /// ints round-trips through this server and is refused by vanilla's codec.
    /// The four furnace counters are `putShort` while every other number here
    /// is `putInt`.
    ///
    /// The starting value is the constructor's, not zero. A freshly placed
    /// hopper saves `TransferCooldown: -1` (`iconst_m1; putfield cooldownTime`)
    /// and a chiseled bookshelf saves `last_interacted_slot: -1`. Writing 0
    /// for either is a hopper that transfers on its first tick instead of its
    /// eighth, and it is invisible in a round-trip test that starts from the
    /// value it wrote.
    ///
    /// **`ticks_since_song_started` is deliberately absent**: it sits behind
    /// `if (getSong() != null)` at `JukeboxBlockEntity.saveAdditional` offset
    /// 37. `Page` is inside a guard too — `if (!getBook().isEmpty())` at
    /// `LecternBlockEntity.saveAdditional` offset 12 — and is declared here
    /// now that the book it is conditional on is stored; `to_nbt` applies
    /// the guard.
    #[must_use]
    pub const fn extra_keys(self) -> &'static [ExtraKey] {
        match self {
            Self::Hopper => &HOPPER_EXTRAS,
            Self::Furnace | Self::Smoker | Self::BlastFurnace => &FURNACE_EXTRAS,
            Self::BrewingStand => &BREWING_STAND_EXTRAS,
            Self::Comparator => &COMPARATOR_EXTRAS,
            Self::ChiseledBookshelf => &CHISELED_BOOKSHELF_EXTRAS,
            Self::Crafter => &CRAFTER_EXTRAS,
            Self::Beacon => &BEACON_EXTRAS,
            Self::Lectern => &LECTERN_EXTRAS,
            Self::Shelf => &SHELF_EXTRAS,
            Self::Campfire => &CAMPFIRE_EXTRAS,
            Self::Beehive => &BEEHIVE_EXTRAS,
            Self::SculkCatalyst => &SCULK_CATALYST_EXTRAS,
            // `CalibratedSculkSensorBlockEntity extends SculkSensorBlockEntity`
            // and adds no saved state of its own.
            Self::SculkSensor | Self::CalibratedSculkSensor => &SCULK_SENSOR_EXTRAS,
            Self::SculkShrieker => &SCULK_SHRIEKER_EXTRAS,
            Self::PotentSulfur => &POTENT_SULFUR_EXTRAS,
            Self::EndGateway => &END_GATEWAY_EXTRAS,
            Self::MobSpawner => &MOB_SPAWNER_EXTRAS,
            Self::CommandBlock => &COMMAND_BLOCK_EXTRAS,
            Self::Jigsaw => &JIGSAW_EXTRAS,
            Self::StructureBlock => &STRUCTURE_BLOCK_EXTRAS,
            Self::TestBlock => &TEST_BLOCK_EXTRAS,
            Self::TestInstanceBlock => &TEST_INSTANCE_BLOCK_EXTRAS,
            // The remaining twenty-five save nothing beyond `components`, the
            // four metadata keys and, where they have one, their item list.
            // **`minecraft:piston` and `minecraft:vault` are in here because
            // nothing checked them**, not because they were read: neither
            // constructs in the extract's JVM, so the golden carries
            // `"saved": null` for both and this arm is a declared unknown
            // rather than a transcription. `PistonMovingBlockEntity` in
            // particular does save state (`blockState`, `facing`, `progress`,
            // `source`, `extending`) and none of it is here.
            _ => &[],
        }
    }

    /// Whether `getUpdateTag()` writes an `Items` key.
    ///
    /// The three types whose `getUpdateTag` reaches
    /// `ContainerHelper.saveAllItems` — and *only* those. A chest is a
    /// container with items and is **not** here, because its
    /// `getUpdatePacket()` is null and it therefore has no update tag on the
    /// wire at all.
    #[must_use]
    pub const fn update_tag_has_items(self) -> bool {
        matches!(self, Self::Campfire | Self::Shelf)
    }

    /// Parse a saved `id`. Unknown types are `None` — a chunk saved by a
    /// later version, or by vanilla, legitimately carries types this server
    /// has never heard of, and dropping them is better than guessing.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "minecraft:chest" => Some(Self::Chest),
            "minecraft:furnace" => Some(Self::Furnace),
            crate::sign::TYPE_ID => Some(Self::Sign),
            "minecraft:hanging_sign" => Some(Self::HangingSign),
            "minecraft:banner" => Some(Self::Banner),
            "minecraft:beacon" => Some(Self::Beacon),
            "minecraft:brushable_block" => Some(Self::BrushableBlock),
            "minecraft:campfire" => Some(Self::Campfire),
            "minecraft:conduit" => Some(Self::Conduit),
            "minecraft:copper_golem_statue" => Some(Self::CopperGolemStatue),
            "minecraft:creaking_heart" => Some(Self::CreakingHeart),
            "minecraft:decorated_pot" => Some(Self::DecoratedPot),
            "minecraft:shelf" => Some(Self::Shelf),
            "minecraft:skull" => Some(Self::Skull),
            "minecraft:trapped_chest" => Some(Self::TrappedChest),
            "minecraft:ender_chest" => Some(Self::EnderChest),
            "minecraft:jukebox" => Some(Self::Jukebox),
            "minecraft:dispenser" => Some(Self::Dispenser),
            "minecraft:dropper" => Some(Self::Dropper),
            "minecraft:mob_spawner" => Some(Self::MobSpawner),
            "minecraft:piston" => Some(Self::Piston),
            "minecraft:brewing_stand" => Some(Self::BrewingStand),
            "minecraft:enchanting_table" => Some(Self::EnchantingTable),
            "minecraft:end_portal" => Some(Self::EndPortal),
            "minecraft:daylight_detector" => Some(Self::DaylightDetector),
            "minecraft:hopper" => Some(Self::Hopper),
            "minecraft:comparator" => Some(Self::Comparator),
            "minecraft:structure_block" => Some(Self::StructureBlock),
            "minecraft:end_gateway" => Some(Self::EndGateway),
            "minecraft:command_block" => Some(Self::CommandBlock),
            "minecraft:shulker_box" => Some(Self::ShulkerBox),
            "minecraft:barrel" => Some(Self::Barrel),
            "minecraft:smoker" => Some(Self::Smoker),
            "minecraft:blast_furnace" => Some(Self::BlastFurnace),
            "minecraft:lectern" => Some(Self::Lectern),
            "minecraft:bell" => Some(Self::Bell),
            "minecraft:jigsaw" => Some(Self::Jigsaw),
            "minecraft:beehive" => Some(Self::Beehive),
            "minecraft:sculk_sensor" => Some(Self::SculkSensor),
            "minecraft:calibrated_sculk_sensor" => Some(Self::CalibratedSculkSensor),
            "minecraft:sculk_catalyst" => Some(Self::SculkCatalyst),
            "minecraft:sculk_shrieker" => Some(Self::SculkShrieker),
            "minecraft:chiseled_bookshelf" => Some(Self::ChiseledBookshelf),
            "minecraft:crafter" => Some(Self::Crafter),
            "minecraft:trial_spawner" => Some(Self::TrialSpawner),
            "minecraft:vault" => Some(Self::Vault),
            "minecraft:test_block" => Some(Self::TestBlock),
            "minecraft:test_instance_block" => Some(Self::TestInstanceBlock),
            "minecraft:potent_sulfur" => Some(Self::PotentSulfur),
            _ => None,
        }
    }

    /// Whether this type keeps sign text — the two `SignBlockEntity` types.
    #[must_use]
    pub const fn is_sign(self) -> bool {
        matches!(self, Self::Sign | Self::HangingSign)
    }
}

/// `ClientboundBlockEntityDataPacket`'s body.
///
/// `STREAM_CODEC.composite`, in argument order, from the static initialiser:
///
/// ```text
///   long    BlockPos.STREAM_CODEC                        the packed position
///   varint  ByteBufCodecs.registry(BLOCK_ENTITY_TYPE)    the type id
///   nbt     ByteBufCodecs.TRUSTED_COMPOUND_TAG           getUpdateTag()
/// ```
///
/// The tag is **network** NBT: a nameless root, so a tag byte and a payload
/// with no length-prefixed name between them. `write_named_root` with an
/// empty name would put two zero bytes there and the client would read the
/// first key's type out of the middle of the name length.
///
/// # Errors
/// Only if the buffer write fails, which for a `Vec` cannot happen.
pub fn block_entity_data_body(pos: BlockPos, type_id: i32, tag: &Nbt) -> std::io::Result<Vec<u8>> {
    use std::io::Write as _;
    let mut out = Vec::new();
    rustmc_proto::wire::write_i64(&mut out, crate::interact::pack_pos(pos))?;
    rustmc_proto::varint::write_varint(&mut out, type_id)?;
    out.write_all(&nameless_nbt_bytes(tag)?)?;
    Ok(out)
}

/// A tag with no name and no length-prefixed name field: a type byte
/// followed directly by the payload. `write_named_root` with an empty name
/// puts two zero bytes there for the name length, which is not this shape —
/// see the doc on [`block_entity_data_body`] for why that distinction is
/// load-bearing.
///
/// # Errors
/// Only if the buffer write fails, which for a `Vec` cannot happen.
fn nameless_nbt_bytes(tag: &Nbt) -> std::io::Result<Vec<u8>> {
    let mut named = Vec::new();
    rustmc_world::nbt::write_named_root(&mut named, "", tag)?;
    if named.len() < 3 || named[1] != 0 || named[2] != 0 {
        return Err(std::io::Error::other(
            "named root was not the expected tag + empty-name shape",
        ));
    }
    let mut out = Vec::with_capacity(named.len() - 2);
    out.push(named[0]);
    out.extend_from_slice(&named[3..]);
    Ok(out)
}

/// The bytes `ClientboundLevelChunkPacketData$BlockEntityInfo`'s tag field
/// carries for one block entity: `RegistryFriendlyByteBuf.writeNbt(Tag)` on
/// `getUpdateTag(provider)`, read off `FriendlyByteBuf.writeNbt(ByteBuf,
/// Tag)` in the pinned jar — a `null` argument is rewritten to
/// `EndTag.INSTANCE` before `NbtIo.writeAnyTag` runs, and `writeAnyTag` on a
/// tag whose `getId()` is 0 (`TAG_End`) writes that one type byte and
/// returns, with no name and no payload. So `None` here is the single byte
/// `0x00`, confirmed on the wire: the one chest this branch recorded vanilla
/// sending had tag digest `af72e84c8601b7df`, which is `trace::digest` of
/// exactly that byte.
///
/// `BlockEntityInfo.create` builds this entry for **every** block entity in
/// the chunk, gated on `getUpdateTag().isEmpty()` alone — unlike
/// [`block_entity_data_body`]'s caller, this is not gated on
/// `getUpdatePacket()` being non-null, so a chest or a furnace gets an entry
/// here with a null tag even though neither ever reaches
/// `block_entity_data`.
///
/// # Errors
/// Only if the buffer write fails, which for a `Vec` cannot happen.
pub fn chunk_payload_tag_bytes(tag: Option<&Nbt>) -> std::io::Result<Vec<u8>> {
    match tag {
        Some(tag) => nameless_nbt_bytes(tag),
        None => Ok(vec![0]),
    }
}

/// One entry of `ClientboundLevelChunkPacketData$BlockEntityInfo` for a
/// block entity at `pos`.
///
/// `BlockEntityInfo.create`, offsets 17-33: `packedXZ` is
/// `SectionPos.sectionRelative(x) << 4 | SectionPos.sectionRelative(z)`, and
/// `sectionRelative` masks to the low four bits — `(x & 15) << 4 | (z & 15)`.
/// `type_id` is looked up by the caller
/// ([`crate::protocol_ids::ProtocolIds::block_entity_type`]) rather than
/// here, the same seam [`crate::world_send::StateId`] documents for block
/// states: this function has no opinion about where the registry id comes
/// from.
///
/// # Errors
/// If `pos.1` does not fit an `i16` (it always does, inside the build
/// height), or if the tag fails to encode, which for a `Vec` cannot happen.
pub fn chunk_payload_entry(
    pos: BlockPos,
    be: &BlockEntity,
    type_id: i32,
) -> std::io::Result<crate::chunk::BlockEntity> {
    let (x, y, z) = pos;
    Ok(crate::chunk::BlockEntity {
        packed_xz: (((x & 15) << 4) | (z & 15)) as u8,
        y: i16::try_from(y).map_err(std::io::Error::other)?,
        kind: type_id,
        nbt: chunk_payload_tag_bytes(be.update_tag().as_ref())?,
    })
}

/// `BlockState.getBlock()` — the state string with its property list cut off.
#[must_use]
pub fn block_of(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

/// `EntityBlock.newBlockEntity` — what block entity, if any, a block makes.
///
/// Takes a **state**, not a block name, and splits it here for the reason
/// [`crate::menu::menu_for_state`] gives: every caller has a state string in
/// hand, and a lookup keyed on the whole of
/// `minecraft:chest[facing=north,...]` matches nothing and reads as "chests
/// have no block entity" rather than as a mistake.
///
/// **`minecraft:crafting_table` is deliberately absent and that is a load-
/// bearing absence.** `CraftingTableBlock` is not an `EntityBlock`; its menu
/// comes from a `SimpleMenuProvider` built on the fly, and its grid lives on
/// the *menu* and is returned to the player when the menu closes. A crafting
/// table that grew a block entity here would keep its grid across a close,
/// which is exactly the bug `480-a-crafting-table-opens-a-menu-with-no-block-
/// entity` exists to notice.
#[must_use]
pub fn for_state(state: &str) -> Option<Kind> {
    let block = block_of(state);
    match block {
        "minecraft:chest" => Some(Kind::Chest),
        "minecraft:furnace" => Some(Kind::Furnace),
        "minecraft:beacon" => Some(Kind::Beacon),
        "minecraft:conduit" => Some(Kind::Conduit),
        "minecraft:creaking_heart" => Some(Kind::CreakingHeart),
        "minecraft:decorated_pot" => Some(Kind::DecoratedPot),
        // The thirty-five added 2026-09-05. Every one of them is a single
        // `getstatic Blocks.*` in `BlockEntityTypes`'s static initialiser
        // except the four broken out into tables below.
        //
        // **`minecraft:spawner`, not `minecraft:mob_spawner`.** The block and
        // the block-entity type disagree about the name, which is a one-word
        // trap the enum variant's own spelling hides: `Kind::MobSpawner.id()`
        // is `minecraft:mob_spawner` and the block that makes one is
        // `minecraft:spawner`. Same shape for `minecraft:moving_piston` ->
        // `minecraft:piston`.
        "minecraft:trapped_chest" => Some(Kind::TrappedChest),
        "minecraft:ender_chest" => Some(Kind::EnderChest),
        "minecraft:jukebox" => Some(Kind::Jukebox),
        "minecraft:dispenser" => Some(Kind::Dispenser),
        "minecraft:dropper" => Some(Kind::Dropper),
        "minecraft:spawner" => Some(Kind::MobSpawner),
        "minecraft:moving_piston" => Some(Kind::Piston),
        "minecraft:brewing_stand" => Some(Kind::BrewingStand),
        "minecraft:enchanting_table" => Some(Kind::EnchantingTable),
        "minecraft:end_portal" => Some(Kind::EndPortal),
        "minecraft:daylight_detector" => Some(Kind::DaylightDetector),
        "minecraft:hopper" => Some(Kind::Hopper),
        "minecraft:comparator" => Some(Kind::Comparator),
        "minecraft:structure_block" => Some(Kind::StructureBlock),
        "minecraft:end_gateway" => Some(Kind::EndGateway),
        "minecraft:barrel" => Some(Kind::Barrel),
        "minecraft:smoker" => Some(Kind::Smoker),
        "minecraft:blast_furnace" => Some(Kind::BlastFurnace),
        "minecraft:lectern" => Some(Kind::Lectern),
        "minecraft:bell" => Some(Kind::Bell),
        "minecraft:jigsaw" => Some(Kind::Jigsaw),
        "minecraft:sculk_sensor" => Some(Kind::SculkSensor),
        "minecraft:calibrated_sculk_sensor" => Some(Kind::CalibratedSculkSensor),
        "minecraft:sculk_catalyst" => Some(Kind::SculkCatalyst),
        "minecraft:sculk_shrieker" => Some(Kind::SculkShrieker),
        "minecraft:chiseled_bookshelf" => Some(Kind::ChiseledBookshelf),
        "minecraft:crafter" => Some(Kind::Crafter),
        "minecraft:trial_spawner" => Some(Kind::TrialSpawner),
        "minecraft:vault" => Some(Kind::Vault),
        "minecraft:test_block" => Some(Kind::TestBlock),
        "minecraft:test_instance_block" => Some(Kind::TestInstanceBlock),
        "minecraft:potent_sulfur" => Some(Kind::PotentSulfur),
        _ if COPPER_CHEST_BLOCKS.contains(&block) => Some(Kind::Chest),
        _ if COMMAND_BLOCKS.contains(&block) => Some(Kind::CommandBlock),
        _ if SHULKER_BOX_BLOCKS.contains(&block) => Some(Kind::ShulkerBox),
        _ if BEEHIVE_BLOCKS.contains(&block) => Some(Kind::Beehive),
        _ if SIGN_BLOCKS.contains(&block) => Some(Kind::Sign),
        _ if HANGING_SIGN_BLOCKS.contains(&block) => Some(Kind::HangingSign),
        _ if BANNER_BLOCKS.contains(&block) => Some(Kind::Banner),
        _ if BRUSHABLE_BLOCKS.contains(&block) => Some(Kind::BrushableBlock),
        _ if CAMPFIRE_BLOCKS.contains(&block) => Some(Kind::Campfire),
        _ if COPPER_GOLEM_STATUE_BLOCKS.contains(&block) => Some(Kind::CopperGolemStatue),
        _ if SHELF_BLOCKS.contains(&block) => Some(Kind::Shelf),
        _ if SKULL_BLOCKS.contains(&block) => Some(Kind::Skull),
        _ => None,
    }
}

/// The blocks `BlockEntityTypes.SIGN` declares valid, read off the
/// `getstatic Blocks.*` sequence its `register` call is handed — offsets
/// 229-350 of `BlockEntityTypes`'s static initialiser, twenty-four entries.
///
/// **The hanging signs are deliberately not here.** They are a separate
/// registration ending at offset 535 under `BlockEntityTypes.HANGING_SIGN`,
/// which is block-entity type 8 rather than 7, and its
/// `HangingSignBlockEntity` has its own text line width. Folding them in
/// would put the wrong type id on the wire for a block that otherwise behaves
/// identically — which decodes cleanly and renders as the wrong thing.
const SIGN_BLOCKS: [&str; 24] = [
    "minecraft:oak_sign",
    "minecraft:spruce_sign",
    "minecraft:birch_sign",
    "minecraft:acacia_sign",
    "minecraft:cherry_sign",
    "minecraft:jungle_sign",
    "minecraft:dark_oak_sign",
    "minecraft:pale_oak_sign",
    "minecraft:oak_wall_sign",
    "minecraft:spruce_wall_sign",
    "minecraft:birch_wall_sign",
    "minecraft:acacia_wall_sign",
    "minecraft:cherry_wall_sign",
    "minecraft:jungle_wall_sign",
    "minecraft:dark_oak_wall_sign",
    "minecraft:pale_oak_wall_sign",
    "minecraft:crimson_sign",
    "minecraft:crimson_wall_sign",
    "minecraft:warped_sign",
    "minecraft:warped_wall_sign",
    "minecraft:mangrove_sign",
    "minecraft:mangrove_wall_sign",
    "minecraft:bamboo_sign",
    "minecraft:bamboo_wall_sign",
];

/// The blocks `BlockEntityTypes.HANGING_SIGN` declares valid — the separate
/// registration that ends at offset 535 of the same static initialiser, and
/// the reason [`SIGN_BLOCKS`] does not include them.
const HANGING_SIGN_BLOCKS: [&str; 24] = [
    "minecraft:oak_hanging_sign",
    "minecraft:spruce_hanging_sign",
    "minecraft:birch_hanging_sign",
    "minecraft:acacia_hanging_sign",
    "minecraft:cherry_hanging_sign",
    "minecraft:jungle_hanging_sign",
    "minecraft:dark_oak_hanging_sign",
    "minecraft:pale_oak_hanging_sign",
    "minecraft:crimson_hanging_sign",
    "minecraft:warped_hanging_sign",
    "minecraft:mangrove_hanging_sign",
    "minecraft:bamboo_hanging_sign",
    "minecraft:oak_wall_hanging_sign",
    "minecraft:spruce_wall_hanging_sign",
    "minecraft:birch_wall_hanging_sign",
    "minecraft:acacia_wall_hanging_sign",
    "minecraft:cherry_wall_hanging_sign",
    "minecraft:jungle_wall_hanging_sign",
    "minecraft:dark_oak_wall_hanging_sign",
    "minecraft:pale_oak_wall_hanging_sign",
    "minecraft:crimson_wall_hanging_sign",
    "minecraft:warped_wall_hanging_sign",
    "minecraft:mangrove_wall_hanging_sign",
    "minecraft:bamboo_wall_hanging_sign",
];

/// `BlockEntityTypes.BANNER`'s two `getstatic`s are
/// `Blocks.BANNER` and `Blocks.WALL_BANNER`, each a `ColorCollection` whose
/// `asList()` is the sixteen dye colours in `DyeColor`'s own ordinal order.
/// Expanded here because this server has no `ColorCollection`.
const BANNER_BLOCKS: [&str; 32] = [
    "minecraft:white_banner",
    "minecraft:orange_banner",
    "minecraft:magenta_banner",
    "minecraft:light_blue_banner",
    "minecraft:yellow_banner",
    "minecraft:lime_banner",
    "minecraft:pink_banner",
    "minecraft:gray_banner",
    "minecraft:light_gray_banner",
    "minecraft:cyan_banner",
    "minecraft:purple_banner",
    "minecraft:blue_banner",
    "minecraft:brown_banner",
    "minecraft:green_banner",
    "minecraft:red_banner",
    "minecraft:black_banner",
    "minecraft:white_wall_banner",
    "minecraft:orange_wall_banner",
    "minecraft:magenta_wall_banner",
    "minecraft:light_blue_wall_banner",
    "minecraft:yellow_wall_banner",
    "minecraft:lime_wall_banner",
    "minecraft:pink_wall_banner",
    "minecraft:gray_wall_banner",
    "minecraft:light_gray_wall_banner",
    "minecraft:cyan_wall_banner",
    "minecraft:purple_wall_banner",
    "minecraft:blue_wall_banner",
    "minecraft:brown_wall_banner",
    "minecraft:green_wall_banner",
    "minecraft:red_wall_banner",
    "minecraft:black_wall_banner",
];

/// `BlockEntityTypes.SKULL` — fourteen blocks, seven heads in a floor and a
/// wall variant each.
const SKULL_BLOCKS: [&str; 14] = [
    "minecraft:skeleton_skull",
    "minecraft:skeleton_wall_skull",
    "minecraft:creeper_head",
    "minecraft:creeper_wall_head",
    "minecraft:dragon_head",
    "minecraft:dragon_wall_head",
    "minecraft:zombie_head",
    "minecraft:zombie_wall_head",
    "minecraft:wither_skeleton_skull",
    "minecraft:wither_skeleton_wall_skull",
    "minecraft:player_head",
    "minecraft:player_wall_head",
    "minecraft:piglin_head",
    "minecraft:piglin_wall_head",
];

/// `BlockEntityTypes.SHELF` — one per wood, in the alphabetical order the
/// registration itself uses.
const SHELF_BLOCKS: [&str; 12] = [
    "minecraft:acacia_shelf",
    "minecraft:bamboo_shelf",
    "minecraft:birch_shelf",
    "minecraft:cherry_shelf",
    "minecraft:crimson_shelf",
    "minecraft:dark_oak_shelf",
    "minecraft:jungle_shelf",
    "minecraft:mangrove_shelf",
    "minecraft:oak_shelf",
    "minecraft:pale_oak_shelf",
    "minecraft:spruce_shelf",
    "minecraft:warped_shelf",
];

/// `BlockEntityTypes.CAMPFIRE`.
const CAMPFIRE_BLOCKS: [&str; 2] = ["minecraft:campfire", "minecraft:soul_campfire"];

/// The eight blocks `BlockEntityTypes.CHEST` gains beyond `minecraft:chest`.
///
/// Its registration is not a block list at all:
///
/// ```text
///   getstatic     Blocks.COPPER_CHEST : WeatheringCopperCollection
///   invokevirtual WeatheringCopperCollection.asList : ()Ljava/util/List;
///   getstatic     Blocks.CHEST : Block
///   invokestatic  Util.copyAndAdd : (Ljava/util/List;Ljava/lang/Object;)
/// ```
///
/// so the eight names have to be expanded from a collection the bytecode
/// never enumerates. **This is the one place a block-entity table had a real
/// hole**: until 2026-09-05 `for_state` matched the literal
/// `"minecraft:chest"` and nothing else, so all eight copper chests were
/// blocks with a container, a menu and nowhere to keep an item — a chest that
/// silently ate what was put in it. The same four oxidation stages and their
/// four waxed twins as [`COPPER_GOLEM_STATUE_BLOCKS`], in
/// `WeatheringCopperCollection.asList`'s own order.
const COPPER_CHEST_BLOCKS: [&str; 8] = [
    "minecraft:copper_chest",
    "minecraft:exposed_copper_chest",
    "minecraft:weathered_copper_chest",
    "minecraft:oxidized_copper_chest",
    "minecraft:waxed_copper_chest",
    "minecraft:waxed_exposed_copper_chest",
    "minecraft:waxed_weathered_copper_chest",
    "minecraft:waxed_oxidized_copper_chest",
];

/// `BlockEntityTypes.COMMAND_BLOCK` — three `getstatic`s, one type.
const COMMAND_BLOCKS: [&str; 3] = [
    "minecraft:command_block",
    "minecraft:chain_command_block",
    "minecraft:repeating_command_block",
];

/// `BlockEntityTypes.SHULKER_BOX` — `Util.copyAndAdd` of
/// `Blocks.DYED_SHULKER_BOX.asList()` and `Blocks.SHULKER_BOX`, so sixteen
/// dyed boxes in `DyeColor` ordinal order plus the undyed one. The same
/// `ColorCollection` expansion [`BANNER_BLOCKS`] documents.
const SHULKER_BOX_BLOCKS: [&str; 17] = [
    "minecraft:white_shulker_box",
    "minecraft:orange_shulker_box",
    "minecraft:magenta_shulker_box",
    "minecraft:light_blue_shulker_box",
    "minecraft:yellow_shulker_box",
    "minecraft:lime_shulker_box",
    "minecraft:pink_shulker_box",
    "minecraft:gray_shulker_box",
    "minecraft:light_gray_shulker_box",
    "minecraft:cyan_shulker_box",
    "minecraft:purple_shulker_box",
    "minecraft:blue_shulker_box",
    "minecraft:brown_shulker_box",
    "minecraft:green_shulker_box",
    "minecraft:red_shulker_box",
    "minecraft:black_shulker_box",
    "minecraft:shulker_box",
];

/// `BlockEntityTypes.BEEHIVE` — `Blocks.BEE_NEST` first, then `Blocks.BEEHIVE`.
const BEEHIVE_BLOCKS: [&str; 2] = ["minecraft:bee_nest", "minecraft:beehive"];

/// `BlockEntityTypes.BRUSHABLE_BLOCK`.
const BRUSHABLE_BLOCKS: [&str; 2] = ["minecraft:suspicious_sand", "minecraft:suspicious_gravel"];

/// `BlockEntityTypes.COPPER_GOLEM_STATUE` — one `getstatic` of a
/// `WeatheringCopperCollection`, whose `asList()` is the four oxidation stages
/// and their four waxed twins.
const COPPER_GOLEM_STATUE_BLOCKS: [&str; 8] = [
    "minecraft:copper_golem_statue",
    "minecraft:exposed_copper_golem_statue",
    "minecraft:weathered_copper_golem_statue",
    "minecraft:oxidized_copper_golem_statue",
    "minecraft:waxed_copper_golem_statue",
    "minecraft:waxed_exposed_copper_golem_statue",
    "minecraft:waxed_weathered_copper_golem_statue",
    "minecraft:waxed_oxidized_copper_golem_statue",
];

/// One position's block entity.
///
/// `PartialEq` and not `Eq`: `extras` holds `Nbt`, which has a `Float` arm —
/// `StructureBlockEntity`'s `integrity` is one — and a float is not `Eq`.
/// Nothing in this crate keyed a block entity, and a total order over one
/// would be a comparison that has to decide what two NaNs mean.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockEntity {
    kind: Kind,
    /// `NonNullList<ItemStack>` of exactly `kind.container_size()` entries.
    /// Empty slots are [`ItemStack::empty`] rather than absent, which is what
    /// `NonNullList.withSize(n, ItemStack.EMPTY)` produces and what lets
    /// [`BlockEntity::item`] be total.
    ///
    /// Empty for a sign, because [`Kind::container_size`] is 0 there.
    items: Vec<ItemStack>,
    /// The scalar keys [`Kind::extra_keys`] declares, in that order.
    ///
    /// A `Vec` parallel to the table rather than a map: the table is `const`
    /// and per-kind, and twenty-five of the forty-nine kinds have none — an
    /// empty `Vec` allocates nothing, so a chest pays a pointer triple and no
    /// heap.
    extras: Vec<Nbt>,
    /// The sign's two texts, for the one kind that has them.
    ///
    /// **Boxed and optional rather than inlined**, because this struct is
    /// held once per block entity in the world and a chest must not pay for
    /// eight strings it can never use. `Option<Box<_>>` is one pointer.
    sign: Option<Box<crate::sign::Sign>>,
    /// The ticking state, for the twelve kinds this server ticks.
    ///
    /// Boxed and optional for the same reason `sign` is, and beside it rather
    /// than inside it because the two are unrelated: a sign has text and no
    /// ticker, a hopper a ticker and no text. `Some` exactly when
    /// `crate::behaviour::server_ticker` says `Ticker::Ported`, which is
    /// what the tick pass relies on to dispatch without a second table.
    behaviour: Option<Box<crate::behaviour::Behaviour>>,
    /// `RandomizableContainer`'s `lootTable` and `lootTableSeed`: a container
    /// that has not been opened yet and will be **filled** when it is.
    /// `Some` only on a [`Kind::is_randomizable`] kind. Boxed for the reason
    /// `sign` is: almost no block entity has one.
    loot: Option<Box<LootRef>>,
    /// `BaseContainerBlockEntity.name` — the `Component` a stack's
    /// `minecraft:custom_name` gave it at placement
    /// (`applyImplicitComponents`), kept as the NBT `ComponentSerialization
    /// .CODEC` writes, which is both what `CustomName` saves and what the
    /// menu title sends. `Some` only on a kind with a
    /// [`Kind::custom_name_key`].
    /// Boxed for `sign`'s reason.
    custom_name: Option<Box<Nbt>>,
    /// `DecoratedPotBlockEntity.decorations` in `PotDecorations.ordered()`
    /// form — back, left, right, front, `minecraft:brick` for a plain face.
    /// `None` is `PotDecorations.EMPTY`, which `saveAdditional` does not
    /// write (offsets 6-15). `Some` only on a [`Kind::DecoratedPot`].
    sherds: Option<Box<[String; 4]>>,
}

/// An unrolled loot table on a container — `RandomizableContainer.lootTable`
/// (a `ResourceKey<LootTable>`, written as its identifier) and
/// `lootTableSeed`, where 0 means "draw from the table's random sequence".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LootRef {
    pub table: String,
    pub seed: i64,
}

impl BlockEntity {
    /// A fresh block entity of `kind`, with every slot empty.
    #[must_use]
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            items: vec![ItemStack::empty(); kind.storage_slots()],
            extras: kind.extra_keys().iter().map(|k| k.value.to_nbt()).collect(),
            sign: kind
                .is_sign()
                .then(|| Box::new(crate::sign::Sign::default())),
            behaviour: crate::behaviour::Behaviour::for_kind(kind).map(Box::new),
            loot: None,
            custom_name: None,
            sherds: None,
        }
    }

    /// `DecoratedPotBlockEntity.decorations`, ordered; `None` for `EMPTY`.
    #[must_use]
    pub fn sherds(&self) -> Option<&[String; 4]> {
        self.sherds.as_deref()
    }

    /// `applyImplicitComponents`' `decorations = getOrDefault(
    /// POT_DECORATIONS, EMPTY)` (offsets 7-21), from the component's item
    /// list: `PotDecorations(List)` takes position `i` when the list is long
    /// enough and the item is not a brick (`getItem`, offsets 0-42), and
    /// `ordered()` writes an absent face back as `minecraft:brick`. A no-op on
    /// any kind but the pot.
    pub fn set_sherds(&mut self, items: &[String]) {
        if self.kind != Kind::DecoratedPot {
            return;
        }
        const BRICK: &str = "minecraft:brick";
        let face = |i: usize| items.get(i).map_or(BRICK, String::as_str).to_string();
        let faces = [face(0), face(1), face(2), face(3)];
        self.sherds = faces.iter().any(|f| f != BRICK).then(|| Box::new(faces));
    }

    /// `BaseContainerBlockEntity.name`: the custom name, if one was given.
    #[must_use]
    pub fn custom_name(&self) -> Option<&Nbt> {
        self.custom_name.as_deref()
    }

    /// `applyImplicitComponents`' `this.name = components.get(CUSTOM_NAME)`.
    /// A no-op on a kind whose `applyImplicitComponents` does not read the
    /// name — see [`Kind::custom_name_key`].
    pub fn set_custom_name(&mut self, name: Option<Nbt>) {
        if self.kind.custom_name_key().is_some() {
            self.custom_name = name.map(Box::new);
        }
    }

    /// `RandomizableContainer.getLootTable()` and its seed.
    #[must_use]
    pub fn loot(&self) -> Option<&LootRef> {
        self.loot.as_deref()
    }

    /// `RandomizableContainer.setLootTable(key, seed)`. Refused on a kind
    /// that is not a `RandomizableContainer` — a furnace has no such field
    /// and a key saved on one would be a tag vanilla never writes.
    pub fn set_loot(&mut self, loot: Option<LootRef>) {
        if self.kind.is_randomizable() {
            self.loot = loot.map(Box::new);
        }
    }

    /// `unpackLootTable`'s `setLootTable(null)` (offset 77), returning what
    /// was there.
    pub fn take_loot(&mut self) -> Option<LootRef> {
        self.loot.take().map(|b| *b)
    }

    /// The ticking state, for a kind this server ticks.
    ///
    /// `None` for the other thirty-seven, which is what makes a tick pass
    /// that reaches one of them a no-op rather than a panic — the same shape
    /// as [`BlockEntity::sign`] at a chest.
    #[must_use]
    pub fn behaviour(&self) -> Option<&crate::behaviour::Behaviour> {
        self.behaviour.as_deref()
    }

    /// The ticking state, mutably.
    pub fn behaviour_mut(&mut self) -> Option<&mut crate::behaviour::Behaviour> {
        self.behaviour.as_deref_mut()
    }

    /// The block entity's `ContainerData` — what a menu over it hands the
    /// client as `container_set_data`, in data-slot order.
    ///
    /// Read out of the two anonymous `ContainerData` classes' `get(int)`
    /// tableswitches in the pinned jar:
    ///
    /// * `AbstractFurnaceBlockEntity$1` — 0 `litTimeRemaining`, 1
    ///   `litTotalTime`, 2 `cookingTimer`, 3 `cookingTotalTime`;
    /// * `BrewingStandBlockEntity$1` — 0 `brewTime`, 1 `fuel`.
    ///
    /// Empty for every other kind, which is right for a chest (no data
    /// slots) and is also what a block entity whose behaviour is missing
    /// answers; a menu then has nothing to compare and sends nothing, rather
    /// than inventing zeros it cannot vouch for.
    #[must_use]
    pub fn container_data(&self) -> Vec<i32> {
        match self.behaviour() {
            Some(crate::behaviour::Behaviour::Furnace(f)) => vec![
                f.lit_time_remaining,
                f.lit_total_time,
                f.cooking_timer,
                f.cooking_total_time,
            ],
            Some(crate::behaviour::Behaviour::Brewing(b)) => vec![b.brew_time, b.fuel],
            _ => Vec::new(),
        }
    }

    /// The saved value of a key a **ticker** owns, read off the live
    /// behaviour rather than off the constructor's extras.
    ///
    /// `AbstractFurnaceBlockEntity.saveAdditional` writes its four counters
    /// with `putShort((short) field)` and `BrewingStandBlockEntity`'s writes
    /// `putShort("BrewTime", (short) brewTime)` and `putByte("Fuel", (byte)
    /// fuel)`. Those fields move every tick on the behaviour; the extras
    /// table only ever held the constructor's zeros, so until this existed a
    /// burning furnace saved as a cold one and a retired tile came back
    /// cold. The casts are the narrowing ones vanilla's are.
    #[allow(clippy::cast_possible_truncation)]
    fn live_extra(&self, name: &str) -> Option<Nbt> {
        match (self.behaviour(), name) {
            (Some(crate::behaviour::Behaviour::Furnace(f)), _) => Some(Nbt::Short(match name {
                "cooking_time_spent" => f.cooking_timer,
                "cooking_total_time" => f.cooking_total_time,
                "lit_time_remaining" => f.lit_time_remaining,
                "lit_total_time" => f.lit_total_time,
                _ => return None,
            }
                as i16)),
            (Some(crate::behaviour::Behaviour::Brewing(b)), "BrewTime") => {
                Some(Nbt::Short(b.brew_time as i16))
            }
            (Some(crate::behaviour::Behaviour::Brewing(b)), "Fuel") => {
                Some(Nbt::Byte(b.fuel as i8))
            }
            _ => None,
        }
    }

    /// `loadAdditional`'s half of [`Self::live_extra`]: the four
    /// `getShortOr(name, 0)` reads of the furnace and the stand's
    /// `getShortOr("BrewTime", 0)` / `getByteOr("Fuel", 0)`, put back on the
    /// behaviour the ticker reads.
    ///
    /// The stand's `ingredient` is not saved by vanilla and is not restored
    /// here: a brew in progress across a load meets `!ingredient.is(null)` on
    /// its first tick and resets, in both.
    fn seed_behaviour_from_extras(&mut self) {
        let get = |be: &Self, k: &str| be.extra_i32(k).unwrap_or(0);
        let furnace = crate::cooking::FurnaceState {
            lit_time_remaining: get(self, "lit_time_remaining"),
            lit_total_time: get(self, "lit_total_time"),
            cooking_timer: get(self, "cooking_time_spent"),
            cooking_total_time: get(self, "cooking_total_time"),
        };
        let (brew_time, fuel) = (get(self, "BrewTime"), get(self, "Fuel"));
        match self.behaviour_mut() {
            Some(crate::behaviour::Behaviour::Furnace(f)) => *f = furnace,
            Some(crate::behaviour::Behaviour::Brewing(b)) => {
                b.brew_time = brew_time;
                b.fuel = fuel;
            }
            _ => {}
        }
    }

    /// `Container.setItem` as a **menu** reaches it — which for the furnace
    /// family is `AbstractFurnaceBlockEntity.setItem`, not the base write.
    ///
    /// The override recomputes `cookingTotalTime` and zeroes `cookingTimer`
    /// when slot 0 receives a stack that is not the same item as what was
    /// there (see [`crate::cooking::set_item`]). A click that drops raw iron
    /// into an empty furnace is what gives the furnace its 200-tick total; a
    /// furnace written through the plain [`Self::set_item`] keeps a total of
    /// 0, and `serverTick`'s equality test then never fires — the furnace
    /// lights, burns its fuel and never smelts anything.
    ///
    /// Every other kind, and a furnace with no recipe table, takes the plain
    /// write. The ticker must **not** come through here: vanilla's
    /// `serverTick` writes the list directly, which is why
    /// [`Self::items_mut`] exists.
    pub fn set_item_from_menu(
        &mut self,
        slot: usize,
        stack: ItemStack,
        recipes: Option<&crate::recipes::Recipes>,
    ) {
        let cooking = match self.kind {
            Kind::Furnace => Some(crate::cooking::CookingKind::Smelting),
            Kind::BlastFurnace => Some(crate::cooking::CookingKind::Blasting),
            Kind::Smoker => Some(crate::cooking::CookingKind::Smoking),
            _ => None,
        };
        let (Some(cooking), Some(recipes)) = (cooking, recipes) else {
            self.set_item(slot, stack);
            return;
        };
        let Some(crate::behaviour::Behaviour::Furnace(mut state)) = self.behaviour().cloned()
        else {
            self.set_item(slot, stack);
            return;
        };
        crate::cooking::set_item(&mut state, &mut self.items, slot, stack, cooking, recipes);
        if let Some(crate::behaviour::Behaviour::Furnace(f)) = self.behaviour_mut() {
            *f = state;
        }
    }

    /// The sign state, for a sign.
    ///
    /// `None` for every other kind, which is what makes a `sign_update` at a
    /// chest do nothing — the same shape as vanilla's
    /// `instanceof SignBlockEntity` test at `updateSignText`'s offset 40.
    #[must_use]
    pub fn sign(&self) -> Option<&crate::sign::Sign> {
        self.sign.as_deref()
    }

    /// The sign state, mutably.
    pub fn sign_mut(&mut self) -> Option<&mut crate::sign::Sign> {
        self.sign.as_deref_mut()
    }

    /// Whether this type answers `getUpdatePacket()` with a packet.
    ///
    /// **`BlockEntity.getUpdatePacket()` returns null** and only some
    /// subclasses override it. Checked rather than assumed, by walking every
    /// registered type's class chain in the pinned jar: **29 of the 49 reach
    /// `BlockEntity.getUpdatePacket()` itself**, whose whole body is
    /// `aconst_null; areturn`. Chest and furnace are two of the 29. So a chest
    /// changing state broadcasts a `block_update` and nothing else, and a
    /// server that sent `block_entity_data` for every block entity would put a
    /// packet on the wire that vanilla never sends — measured from the other
    /// side too, by `ci/block-entity-probe.sh`, whose chest, barrel, hopper
    /// and lectern controls each placed their block and produced zero
    /// `block_entity_data` in the same run as eleven types that produced one.
    #[must_use]
    pub const fn has_update_packet(&self) -> bool {
        !matches!(self.kind, Kind::Chest | Kind::Furnace)
    }

    /// `getUpdateTag(provider)` — the payload of
    /// `ClientboundBlockEntityDataPacket`, for the kinds that have one.
    ///
    /// # Every arm is a shape, not a constant
    ///
    /// The temptation here is to write down the bytes the probe recorded and
    /// be done, which would pass every check in the corpus and be a lie the
    /// first time one of these block entities held anything. So each arm
    /// derives from what this server actually stores, and where this server
    /// stores nothing the arm says so rather than pretending:
    ///
    /// * **items** — `Campfire` and `Shelf` reach
    ///   `ContainerHelper.saveAllItems(out, items, true)`, which writes the
    ///   `Items` key even when the list is empty and skips empty stacks
    ///   inside it. That is [`BlockEntity::items_tag`], the same writer
    ///   [`BlockEntity::to_nbt`] uses, so an item put on a shelf appears here
    ///   without anything else changing;
    /// * **`align_items_to_bottom`** — `ShelfBlockEntity.alignItemsToBottom`,
    ///   a plain boolean field written after `Items`. It is set from the
    ///   shelf's own use path, which this server has no verb for, so it is
    ///   false for every state reachable here;
    /// * **`Levels`** — `BeaconBlockEntity.levels`, recomputed by its ticker
    ///   from the pyramid beneath it. **This server runs no beacon ticker**,
    ///   so this is 0 always, and that is correct for every beacon the corpus
    ///   can build and wrong for a beacon on a pyramid. Named here rather
    ///   than left to be discovered;
    /// * **`components`** — `BannerBlockEntity` alone among these goes through
    ///   `saveWithoutMetadata()` rather than `saveCustomOnly()`, and the
    ///   difference is exactly this key. The map is empty for a banner with
    ///   no custom name and no patterns, which is every banner this server
    ///   can make;
    /// * **empty** — six types whose custom save writes nothing at all in
    ///   their placed state.
    ///
    /// The key **order** inside a multi-key compound is not this function's
    /// to choose: [`rustmc_world::nbt::write_named_root`] emits `HashMap`
    /// bucket order, which is what vanilla's `CompoundTag.write` does and is
    /// neither insertion order nor sorted order. `Shelf` is the only new type
    /// with more than one key and is therefore the only one that exercises
    /// it — which is why it gets a check of its own.
    #[must_use]
    pub fn update_tag(&self) -> Option<Nbt> {
        if let Some(sign) = self.sign.as_deref() {
            return Some(sign.update_tag());
        }
        let mut c = BTreeMap::new();
        match self.kind {
            Kind::Chest | Kind::Furnace => return None,
            Kind::Beacon => {
                c.insert("Levels".to_string(), Nbt::Int(0));
            }
            Kind::Banner => {
                c.insert("components".to_string(), Nbt::Compound(BTreeMap::new()));
            }
            // `saveCustomOnly` is `saveAdditional` alone, and a skull's
            // writes only what it has: nothing for a plain one.
            Kind::Skull => {}
            Kind::Campfire => {
                c.insert("Items".to_string(), self.items_tag());
            }
            Kind::Shelf => {
                c.insert("Items".to_string(), self.items_tag());
                c.insert("align_items_to_bottom".to_string(), Nbt::Byte(0));
            }
            Kind::BrushableBlock
            | Kind::Conduit
            | Kind::CopperGolemStatue
            | Kind::CreakingHeart => {}
            // `saveCustomOnly`: `sherds` unless EMPTY, then `item` unless
            // empty (`saveAdditional` 6-66).
            Kind::DecoratedPot => {
                if let Some(sherds) = self.sherds.as_deref() {
                    c.insert("sherds".to_string(), Self::sherds_tag(sherds));
                }
                let stack = self.item(0);
                if !stack.is_empty() {
                    c.insert("item".to_string(), Self::stack_tag(&stack));
                }
            }
            // The two sign kinds returned above; this arm cannot be reached
            // with a well-formed value and is a panic-free fallthrough rather
            // than an `unreachable!` on a connection thread.
            Kind::Sign | Kind::HangingSign => return None,

            // The thirty-five added 2026-09-05. Twenty-seven of them inherit
            // `BlockEntity.getUpdatePacket()`, whose whole body is
            // `aconst_null; areturn`, so `ChunkHolder.broadcastBlockEntity`
            // returns at its `ifnull` and vanilla puts nothing on the wire for
            // them at all. `None` here is that, exactly: a chest, a barrel, a
            // hopper and a lectern each have a block entity and vanilla never
            // says so.
            Kind::TrappedChest
            | Kind::EnderChest
            | Kind::Jukebox
            | Kind::Dispenser
            | Kind::Dropper
            | Kind::Piston
            | Kind::BrewingStand
            | Kind::EnchantingTable
            | Kind::EndPortal
            | Kind::DaylightDetector
            | Kind::Hopper
            | Kind::Comparator
            | Kind::CommandBlock
            | Kind::ShulkerBox
            | Kind::Barrel
            | Kind::Smoker
            | Kind::BlastFurnace
            | Kind::Lectern
            | Kind::Bell
            | Kind::Beehive
            | Kind::SculkSensor
            | Kind::CalibratedSculkSensor
            | Kind::SculkCatalyst
            | Kind::SculkShrieker
            | Kind::ChiseledBookshelf
            | Kind::Crafter
            | Kind::PotentSulfur => return None,

            // **The eight that DO override it, and are still `None` here.**
            // This is a debt with a name, not an oversight, and the reason is
            // different for each group:
            //
            // * `structure_block`, `jigsaw`, `test_block` and
            //   `test_instance_block` are `GameMasterBlock`s that refuse to
            //   place for a player without op — measured, not read:
            //   `ci/block-entity-probe.sh` records `grass_block,air` for all
            //   four. `end_gateway` has no item that places it. So no
            //   scenario in this corpus can reach any of the five, and an
            //   update tag written for them would be a shape nothing could
            //   falsify;
            // * `mob_spawner`, `trial_spawner` and `vault` are excluded for
            //   being unreproducible rather than unreachable — a spawner's
            //   `Delay` and a vault's `display_item` are fresh random draws on
            //   every run, so a check on either could never come back `match`
            //   for anybody.
            //
            // Sending `None` where vanilla sends a packet is a divergence and
            // is recorded as one. Sending a *guessed* tag would be a
            // divergence too, and an invisible one.
            Kind::MobSpawner
            | Kind::StructureBlock
            | Kind::EndGateway
            | Kind::Jigsaw
            | Kind::TrialSpawner
            | Kind::Vault
            | Kind::TestBlock
            | Kind::TestInstanceBlock => return None,
        }
        // The banner's, beacon's and skull's update tags are built on
        // `saveAdditional` (`saveWithoutMetadata`, `saveCustomOnly`), and each
        // `saveAdditional` stores the name when there is one. The containers
        // returned `None` above and the enchanting table has no packet.
        if matches!(self.kind, Kind::Banner | Kind::Beacon | Kind::Skull) {
            if let (Some(n), Some(key)) = (self.custom_name.as_deref(), self.kind.custom_name_key())
            {
                c.insert(key.to_string(), n.clone());
            }
        }
        Some(Nbt::Compound(c))
    }

    /// One stack as `ItemStack.CODEC` writes it: the lowercase `id` and
    /// `count` that [`BlockEntity::items_tag`] puts inside a list entry,
    /// without the `Slot` byte `ItemStackWithSlot` adds around it.
    fn sherds_tag(sherds: &[String; 4]) -> Nbt {
        Nbt::List(sherds.iter().map(|s| Nbt::String(s.clone())).collect())
    }

    fn stack_tag(stack: &ItemStack) -> Nbt {
        let mut e = BTreeMap::new();
        e.insert("id".to_string(), Nbt::String(stack.item().to_string()));
        e.insert("count".to_string(), Nbt::Int(stack.count()));
        Nbt::Compound(e)
    }

    /// `ContainerHelper.saveAllItems(out, items, true)` — the `Items` list.
    ///
    /// The `true` is `alwaysWrite`: the key is emitted even when every slot is
    /// empty, which is why a freshly placed campfire's update tag is
    /// `{Items: []}` and not `{}`. Empty stacks inside it are skipped, from
    /// the `isEmpty` test at offset 37.
    fn items_tag(&self) -> Nbt {
        let mut items = Vec::new();
        for (i, stack) in self.items.iter().enumerate() {
            if stack.is_empty() {
                continue;
            }
            let mut e = BTreeMap::new();
            e.insert(
                "Slot".to_string(),
                Nbt::Byte(i8::try_from(i).unwrap_or(i8::MAX)),
            );
            e.insert("id".to_string(), Nbt::String(stack.item().to_string()));
            e.insert("count".to_string(), Nbt::Int(stack.count()));
            items.push(Nbt::Compound(e));
        }
        Nbt::List(items)
    }

    /// One of [`Kind::extra_keys`]' values, by name.
    ///
    /// A name this kind does not declare answers `None` rather than 0: a
    /// hopper's `TransferCooldown` starts at **-1** and a comparator's
    /// `OutputSignal` at 0, so "not declared" and "declared and zero" are
    /// different facts and a total accessor would hide the first.
    #[must_use]
    pub fn extra(&self, name: &str) -> Option<&Nbt> {
        let i = self.kind.extra_keys().iter().position(|k| k.name == name)?;
        self.extras.get(i)
    }

    /// The same, for the integral keys, as an `i32`.
    ///
    /// A key whose tag is not integral answers `None` rather than 0 — a
    /// caller asking a jigsaw's `pool` for a number has asked the wrong
    /// question and should be told so.
    #[must_use]
    pub fn extra_i32(&self, name: &str) -> Option<i32> {
        self.extra(name).and_then(nbt_as_i32_any)
    }

    /// Set one of them. A name this kind does not declare is ignored, which is
    /// the same refusal [`BlockEntity::set_item`] makes for a slot out of
    /// range and for the same reason: this runs on a connection thread.
    pub fn set_extra(&mut self, name: &str, value: Nbt) {
        let n = nbt_as_i32_any(&value);
        if let Some(i) = self.kind.extra_keys().iter().position(|k| k.name == name) {
            if let Some(slot) = self.extras.get_mut(i) {
                *slot = value;
            }
        }
        // A key a ticker owns is written through to the behaviour too, which
        // is where [`Self::to_nbt`] reads it from — see [`Self::live_extra`].
        if let Some(n) = n {
            match (self.behaviour_mut(), name) {
                (Some(crate::behaviour::Behaviour::Furnace(f)), "cooking_time_spent") => {
                    f.cooking_timer = n;
                }
                (Some(crate::behaviour::Behaviour::Furnace(f)), "cooking_total_time") => {
                    f.cooking_total_time = n;
                }
                (Some(crate::behaviour::Behaviour::Furnace(f)), "lit_time_remaining") => {
                    f.lit_time_remaining = n;
                }
                (Some(crate::behaviour::Behaviour::Furnace(f)), "lit_total_time") => {
                    f.lit_total_time = n;
                }
                (Some(crate::behaviour::Behaviour::Brewing(b)), "BrewTime") => b.brew_time = n,
                (Some(crate::behaviour::Behaviour::Brewing(b)), "Fuel") => b.fuel = n,
                _ => {}
            }
        }
    }

    #[must_use]
    pub const fn kind(&self) -> Kind {
        self.kind
    }

    /// `Container.getContainerSize()`.
    #[must_use]
    pub const fn size(&self) -> usize {
        self.items.len()
    }

    /// `Container.getItem(int)`. Out of range is empty, not a panic:
    /// `NonNullList` would throw, but every caller here has already been
    /// through `isValidSlotIndex`, and a panic on a connection thread takes
    /// the server down for a bad packet.
    #[must_use]
    pub fn item(&self, slot: usize) -> ItemStack {
        self.items
            .get(slot)
            .cloned()
            .unwrap_or_else(ItemStack::empty)
    }

    /// `Container.setItem(int, ItemStack)`.
    pub fn set_item(&mut self, slot: usize, stack: ItemStack) {
        if let Some(s) = self.items.get_mut(slot) {
            *s = stack;
        }
    }

    /// The whole `NonNullList`, mutably.
    ///
    /// The ticker path only. Vanilla's `AbstractFurnaceBlockEntity.serverTick`
    /// reaches the list directly rather than through `setItem` — which is not
    /// a shortcut but a behaviour: `setItem` recomputes `cookingTotalTime` and
    /// the ticker's own writes must not, or a furnace would restart its
    /// progress bar every time it consumed a fuel item. `crate::cooking
    /// ::set_item` is the other half and is what a *click* goes through.
    pub fn items_mut(&mut self) -> &mut [ItemStack] {
        &mut self.items
    }

    /// `Container.isEmpty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.iter().all(ItemStack::is_empty)
    }

    /// The saved compound: `id`, `x`, `y`, `z` and the type's own data.
    ///
    /// Key names are vanilla's and were read from bytecode rather than
    /// recalled: `BlockEntity.saveMetadata` writes `x`, `y` and `z` with
    /// `putInt`, `BlockEntity.addEntityType` writes the type under `id`, and
    /// `ContainerHelper.saveAllItems` writes the list under `Items` using
    /// `ItemStackWithSlot.CODEC`, whose slot key is the capitalised `Slot`
    /// while the stack's own keys are the lowercase `id` and `count`. Two
    /// different capitalisation conventions one level apart, which is the
    /// kind of thing that round-trips fine here and is rejected by vanilla.
    ///
    /// **Empty stacks are skipped**, which is `saveAllItems`' own `isEmpty()`
    /// test at offset 37 — a chest with one stone saves one entry, not
    /// twenty-seven. The `Items` key itself is still written when the list is
    /// empty, because `ChestBlockEntity.saveAdditional` calls the two-argument
    /// `saveAllItems`, which passes `true` for the flag that suppresses the
    /// `discard`.
    #[must_use]
    pub fn to_nbt(&self, pos: BlockPos) -> Nbt {
        // A sign's `saveAdditional` writes three keys and no `Items`, so it
        // gets its own writer rather than a container's with the item loop
        // skipped — an empty `Items` on a sign is a tag vanilla never emits.
        if let Some(sign) = self.sign.as_deref() {
            let mut c = match sign.to_nbt(pos) {
                Nbt::Compound(c) => c,
                other => return other,
            };
            // `Sign::to_nbt` stamps `crate::sign::TYPE_ID`, which is right for
            // the twenty-four sign blocks and wrong for the twenty-four
            // hanging ones: `saveMetadata` writes the **type's** registry
            // name, and a hanging sign saved as `minecraft:sign` reloads as a
            // block entity whose type does not match its block.
            c.insert("id".to_string(), Nbt::String(self.kind.id().to_string()));
            // `crate::sign` writes `saveAdditional`'s three keys and the four
            // metadata ones; `components` is `saveWithoutMetadata`'s and is
            // added here so the sign path and the general one agree.
            c.insert(COMPONENTS_KEY.to_string(), Nbt::Compound(BTreeMap::new()));
            return Nbt::Compound(c);
        }
        let mut c = BTreeMap::new();
        c.insert("id".to_string(), Nbt::String(self.kind.id().to_string()));
        c.insert("x".to_string(), Nbt::Int(pos.0));
        c.insert("y".to_string(), Nbt::Int(pos.1));
        c.insert("z".to_string(), Nbt::Int(pos.2));
        // `Slot` is a byte in `ItemStackWithSlot`'s codec, written inside
        // [`BlockEntity::items_tag`] with a saturating conversion rather than
        // an `as` — the day a 54-slot double chest lands, that is the line
        // that would otherwise silently wrap.
        //
        // **Only for the kinds whose `saveAdditional` writes items.** A
        // decorated pot with an empty `Items` key is a compound vanilla never
        // writes, and a chunk carrying one reads as a pot that has been
        // emptied rather than as one that never held anything.
        for (k, v) in self.kind.extra_keys().iter().zip(&self.extras) {
            // `LecternBlockEntity.saveAdditional` writes `Page` beside the
            // book and never without it.
            if self.kind == Kind::Lectern && self.item(0).is_empty() {
                continue;
            }
            let live = self.live_extra(k.name);
            c.insert(k.name.to_string(), live.unwrap_or_else(|| v.clone()));
        }
        // `BlockEntity.saveWithoutMetadata`'s last two instructions, and every
        // one of the forty-nine reaches them.
        c.insert(COMPONENTS_KEY.to_string(), Nbt::Compound(BTreeMap::new()));
        // `BaseContainerBlockEntity.saveAdditional`: `storeNullable(
        // "CustomName", ComponentSerialization.CODEC, name)` runs before the
        // subclass's loot-or-items choice, so a named loot chest keeps both.
        // The banner, beacon, enchanting table and skull store theirs the
        // same way under their own key.
        if let (Some(n), Some(key)) = (self.custom_name.as_deref(), self.kind.custom_name_key()) {
            c.insert(key.to_string(), n.clone());
        }
        // `DecoratedPotBlockEntity.saveAdditional` offsets 6-28: `sherds`,
        // `PotDecorations.CODEC` — the four ordered item names — unless EMPTY.
        if let Some(sherds) = self.sherds.as_deref() {
            c.insert("sherds".to_string(), Self::sherds_tag(sherds));
        }
        // `RandomizableContainer.trySaveLootTable`: with a table present the
        // container writes `LootTable` (and `LootTableSeed` unless it is 0)
        // and **instead of** its items — every `saveAdditional` in the family
        // is `if (!trySaveLootTable(output)) ContainerHelper.saveAllItems(..)`.
        if let Some(l) = self.loot.as_deref() {
            c.insert("LootTable".to_string(), Nbt::String(l.table.clone()));
            if l.seed != 0 {
                c.insert("LootTableSeed".to_string(), Nbt::Long(l.seed));
            }
            return Nbt::Compound(c);
        }
        match self.kind.items_key() {
            Some(("Items", always)) => {
                let list = self.items_tag();
                // `alwaysWrite` false and an empty list is
                // `ValueOutput.discard("Items")` — no key, not an empty one.
                // `ShulkerBoxBlockEntity` is the only type in the registry
                // that reaches it.
                if always || !matches!(&list, Nbt::List(v) if v.is_empty()) {
                    c.insert("Items".to_string(), list);
                }
            }
            // A single stack under its own name, and only when it is not
            // empty: all three call sites are guarded by an `ItemStack
            // .isEmpty()` test before the `store`.
            Some((key, _)) => {
                let stack = self.item(0);
                if !stack.is_empty() {
                    c.insert(key.to_string(), Self::stack_tag(&stack));
                }
            }
            None => {}
        }
        Nbt::Compound(c)
    }

    /// Rebuild from a saved compound, with its position.
    ///
    /// `None` when the compound names a type this server does not model or
    /// has no usable position — see [`Kind::from_id`]. A slot index outside
    /// the container is dropped rather than clamped: clamping would move
    /// somebody's items into slot 0 on load, which is worse than losing them
    /// and much harder to notice.
    #[must_use]
    pub fn from_nbt(nbt: &Nbt) -> Option<(BlockPos, Self)> {
        let c = nbt.as_compound()?;
        let kind = Kind::from_id(c.get("id")?.as_str()?)?;
        let pos = (
            c.get("x")?.as_i32()?,
            c.get("y")?.as_i32()?,
            c.get("z")?.as_i32()?,
        );
        let mut be = Self::new(kind);
        if kind.is_sign() {
            be.sign = Some(Box::new(crate::sign::Sign::from_nbt(nbt)));
            return Some((pos, be));
        }
        for (i, k) in kind.extra_keys().iter().enumerate() {
            // A missing key keeps the constructor's value, which is what
            // `ValueInput.getIntOr(name, default)` does — and the default in
            // every one of those calls is the field's own starting value.
            // The saved tag is taken only when its **type** matches what this
            // key is declared to be. A `BrewTime` that arrived as an int is
            // the shape vanilla's `getShortOr` refuses, and taking it would
            // write it back out at the wrong width on the next save — turning
            // a foreign compound into one of ours.
            if let Some(v) = c.get(k.name) {
                if let Some(slot) = be.extras.get_mut(i) {
                    if std::mem::discriminant(v) == std::mem::discriminant(slot) {
                        *slot = v.clone();
                    }
                }
            }
        }
        be.seed_behaviour_from_extras();
        // `BaseContainerBlockEntity.loadAdditional`: `parseCustomNameSafe(
        // input, "CustomName")` — a component, which as NBT is a string for
        // plain text and a compound or list otherwise.
        if let Some(n @ (Nbt::String(_) | Nbt::Compound(_) | Nbt::List(_))) =
            kind.custom_name_key().and_then(|k| c.get(k))
        {
            be.set_custom_name(Some(n.clone()));
        }
        // `DecoratedPotBlockEntity.loadAdditional` offsets 7-26.
        if let Some(Nbt::List(l)) = c.get("sherds") {
            let names: Vec<String> = l
                .iter()
                .filter_map(|n| n.as_str().map(str::to_string))
                .collect();
            be.set_sherds(&names);
        }
        // `tryLoadLootTable`: a `LootTable` key wins, `LootTableSeed` defaults
        // to 0, and the items are then **not** read.
        if kind.is_randomizable() {
            if let Some(table) = c.get("LootTable").and_then(Nbt::as_str) {
                let seed = match c.get("LootTableSeed") {
                    Some(Nbt::Long(v)) => *v,
                    _ => 0,
                };
                be.set_loot(Some(LootRef {
                    table: table.to_string(),
                    seed,
                }));
                return Some((pos, be));
            }
        }
        // The three single-stack types read their own key. `count` is the
        // codec's, whose `orElse` is 1.
        if let Some((key, _)) = kind.items_key() {
            if key != "Items" {
                if let Some(e) = c.get(key).and_then(Nbt::as_compound) {
                    if let Some(id) = e.get("id").and_then(Nbt::as_str) {
                        let count = e.get("count").and_then(Nbt::as_i32).unwrap_or(1);
                        be.set_item(0, ItemStack::new(id, count));
                    }
                }
                return Some((pos, be));
            }
        }
        if let Some(items) = c.get("Items").and_then(Nbt::as_list) {
            for entry in items {
                let Some(e) = entry.as_compound() else {
                    continue;
                };
                let Some(slot) = e.get("Slot").and_then(nbt_as_index) else {
                    continue;
                };
                let Some(id) = e.get("id").and_then(Nbt::as_str) else {
                    continue;
                };
                // `count` is written as an int here and vanilla's codec range
                // is 1..=99. A missing count is 1, which is the codec's own
                // `orElse`; a zero or negative one makes an empty stack and
                // therefore vanishes, which is the same as not saving it.
                let count = e.get("count").and_then(Nbt::as_i32).unwrap_or(1);
                if slot < be.size() {
                    be.set_item(slot, ItemStack::new(id, count));
                }
            }
        }
        Some((pos, be))
    }
}

/// Every key vanilla writes into a freshly placed block entity's saved
/// compound that this server does not, one line each, `"<type> <key> tag <id>"`.
///
/// **A ratchet in both directions.** A difference not on this list fails
/// `a_fresh_block_entity_saves_vanillas_keys_at_vanillas_tag_widths`, and an
/// entry that has stopped being a difference fails it too — so the list cannot
/// outlive what it excuses and cannot quietly absorb a new gap.
#[cfg(test)]
const OUTSTANDING_SAVE_KEYS: &[&str] = &[];

/// A tag's value as `Tag.toString()` renders it, for the arms whose rendering
/// is deterministic.
///
/// `CompoundTag.toString` and `ListTag.toString` walk a `HashMap` view, so
/// their text is not stable across JVM runs and comparing it would test the
/// iteration order rather than the value. Both collapse to [`OPAQUE`] on this
/// side and [`opaque_or`] does the same on vanilla's.
#[cfg(test)]
fn snbt(n: &Nbt) -> String {
    match n {
        Nbt::Byte(v) => format!("{v}b"),
        Nbt::Short(v) => format!("{v}s"),
        Nbt::Int(v) => format!("{v}"),
        Nbt::Long(v) => format!("{v}L"),
        // Java prints a float with a trailing `.0` where Rust's `Display`
        // does not, so `{:?}` is the formatter that matches: `1.0f`.
        Nbt::Float(v) => format!("{v:?}f"),
        // `StringTag.quoteAndEscape` prefers a double quote unless the string
        // holds one; none of the defaults in this registry does.
        Nbt::String(v) => format!("\"{v}\""),
        Nbt::IntArray(v) => format!(
            "[I;{}]",
            v.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Nbt::List(_) | Nbt::Compound(_) => OPAQUE.to_string(),
        other => format!("{other:?}"),
    }
}

/// What a compound's or a list's value compares as on both sides.
#[cfg(test)]
const OPAQUE: &str = "<opaque>";

/// Vanilla's rendered value, or [`OPAQUE`] for a list (9) or compound (10).
#[cfg(test)]
fn opaque_or(tag_id: i64, value: &str) -> String {
    if tag_id == 9 || tag_id == 10 {
        OPAQUE.to_string()
    } else {
        value.to_string()
    }
}

/// `Tag.getId()` — the NBT type byte, which is half of what a saved key is.
#[cfg(test)]
const fn nbt_tag_id(n: &Nbt) -> i64 {
    match n {
        Nbt::Byte(_) => 1,
        Nbt::Short(_) => 2,
        Nbt::Int(_) => 3,
        Nbt::Long(_) => 4,
        Nbt::Float(_) => 5,
        Nbt::Double(_) => 6,
        Nbt::ByteArray(_) => 7,
        Nbt::String(_) => 8,
        Nbt::List(_) => 9,
        Nbt::Compound(_) => 10,
        Nbt::IntArray(_) => 11,
        Nbt::LongArray(_) => 12,
    }
}

/// Any integral tag as an `i32`.
///
/// `ValueInput.getIntOr` and `getShortOr` are separate calls in vanilla and a
/// short read as an int is the same number, so widening here loses nothing;
/// what it buys is that a `BrewTime` written by a tool as an int still loads.
fn nbt_as_i32_any(n: &Nbt) -> Option<i32> {
    match n {
        Nbt::Byte(b) => Some(i32::from(*b)),
        Nbt::Short(s) => Some(i32::from(*s)),
        Nbt::Int(i) => Some(*i),
        _ => None,
    }
}

/// A `Slot` value, which is a byte in vanilla's codec but is accepted as any
/// integral tag here — a hand-written or third-party chunk may carry an int,
/// and refusing it would lose items for a difference the format does not care
/// about.
fn nbt_as_index(n: &Nbt) -> Option<usize> {
    let v = match n {
        Nbt::Byte(b) => i32::from(*b),
        Nbt::Short(s) => i32::from(*s),
        Nbt::Int(i) => *i,
        _ => return None,
    };
    usize::try_from(v).ok()
}

/// Every block entity in the world, keyed by absolute position.
///
/// A `BTreeMap` rather than a hash map for the reason [`crate::edits`] gives:
/// nothing here iterates into a packet *today*, and the moment something does
/// — the save path does, right now — a hash order would make the output
/// non-reproducible, which this project treats as a defect by definition.
#[derive(Debug, Default)]
pub struct BlockEntities {
    map: RwLock<BTreeMap<BlockPos, BlockEntity>>,
    /// `Level.blockEntityTickers` — the positions
    /// `crate::behaviour::tick_block_entities` walks, in the order they
    /// were added.
    ///
    /// A separate list rather than a filter over `map`, because the order is
    /// vanilla's and it is not the map's. `Level.tickBlockEntities` iterates
    /// a `List<TickingBlockEntity>` built by appending, so two hoppers in a
    /// chain tick in the order they were placed and not in the order their
    /// coordinates sort — and a hopper chain is the one place in this whole
    /// module where that difference is visible on the wire, because whether
    /// an item crosses one hopper or two in a tick depends on which moved
    /// first.
    tickers: RwLock<Vec<BlockPos>>,
    /// `Level.pendingBlockEntityTickers`.
    ///
    /// Vanilla keeps a second list because `addBlockEntityTicker` can be
    /// called *during* `tickBlockEntities` — `this.tickingBlockEntities` is
    /// the flag that decides which list it lands in — and appending to the
    /// list being iterated is a `ConcurrentModificationException`. Here the
    /// hazard is the same shape: a ticker that places a block entity would
    /// otherwise extend the vector a `for` loop is walking. Draining pending
    /// at the top of the pass makes a block entity placed this tick first
    /// tick on the **next** one, which is what vanilla does.
    pending: RwLock<Vec<BlockPos>>,
}

impl BlockEntities {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The process-wide store, alongside [`crate::containers::Openers`] and
    /// [`crate::edits`]'s overlay.
    #[must_use]
    pub fn shared() -> &'static Self {
        static B: OnceLock<BlockEntities> = OnceLock::new();
        B.get_or_init(BlockEntities::new)
    }

    /// `LevelChunk.setBlockState`'s block-entity half, for one position.
    ///
    /// Both directions in one call, because vanilla does both in one place
    /// and splitting them is how a stale block entity survives a block
    /// change: a chest replaced by stone must lose its items, and a chest
    /// replaced by a *different* chest state — say a facing change — must
    /// keep them. The test for "different" is the block, not the state, which
    /// is `newState.is(oldState.getBlock())` in
    /// `LevelChunk.setBlockState`'s own guard.
    ///
    /// Returns whether anything changed, for callers that want to mark a
    /// chunk dirty.
    ///
    /// # The test is on the **block**, and it stopped being the same
    /// question as the type when signs arrived
    ///
    /// This compared the block-entity *kind* for as long as there were two
    /// of them, and that was exactly `newState.is(oldState.getBlock())` while
    /// each kind had one block. `Kind::Sign` has **twenty-four**, so a state
    /// change from `oak_sign` to `birch_sign` is one block replacing another
    /// — vanilla removes the block entity and makes a new one, and a
    /// kind-comparison would have kept the old text under the new block.
    /// The old state is therefore passed in rather than inferred.
    pub fn on_block_changed(&self, pos: BlockPos, old_state: &str, new_state: &str) -> bool {
        let want = for_state(new_state);
        let same_block = block_of(old_state) == block_of(new_state);
        let Ok(mut map) = self.map.write() else {
            return false;
        };
        match (map.contains_key(&pos), want) {
            // Same block: the state changed under it and the contents stay.
            // `LevelChunk.setBlockState`'s own guard.
            (true, Some(_)) if same_block => false,
            // Gained one, or swapped for a different block.
            (_, Some(now)) => {
                map.insert(pos, BlockEntity::new(now));
                // `LevelChunk.addAndRegisterBlockEntity` -> `updateBlockEntityTicker`
                // -> `Level.addBlockEntityTicker`. A block replaced by a
                // different block with a block entity is a *new* entry on the
                // list even where a stale one for the old kind is still on it;
                // the stale one is dropped by the `isRemoved` sweep at the top
                // of the pass, exactly as vanilla's `Iterator.remove` drops it.
                self.arm_ticker(pos, now);
                true
            }
            // The block no longer has a block entity. `removeBlockEntity`.
            (true, None) => {
                map.remove(&pos);
                true
            }
            (false, None) => false,
        }
    }

    /// `Level.addBlockEntityTicker`, for a kind that has a server ticker.
    ///
    /// Silent for the other thirty-seven: `updateBlockEntityTicker` only
    /// calls it when `getTicker` came back non-null, so a chest is never on
    /// the list and a pass that walked every block entity would be running
    /// something vanilla does not.
    fn arm_ticker(&self, pos: BlockPos, kind: Kind) {
        if !crate::behaviour::server_ticker(kind).runs() {
            return;
        }
        if let Ok(mut pending) = self.pending.write() {
            pending.push(pos);
        }
    }

    /// `LevelChunk.getBlockEntity`.
    #[must_use]
    pub fn get(&self, pos: BlockPos) -> Option<BlockEntity> {
        self.map.read().ok()?.get(&pos).cloned()
    }

    /// Read-modify-write under one lock.
    ///
    /// The whole of a container click happens inside this, because a click is
    /// a read of the slot, a decision, and a write, and another connection
    /// clicking the same chest between the read and the write would duplicate
    /// the stack. Vanilla is single-threaded and gets this for free; here the
    /// lock is the thing that stands in for the game loop, exactly as
    /// [`crate::broadcast::tick_lock`] does for block edits.
    /// Whether the store's write lock could be taken right now — for a test
    /// that stands in for `World::set_block`, whose `on_block_changed` takes
    /// it, to show a caller is not holding it across a block write.
    #[cfg(test)]
    pub(crate) fn write_lock_is_free(&self) -> bool {
        self.map.try_write().is_ok()
    }

    pub fn with_mut<T>(&self, pos: BlockPos, f: impl FnOnce(&mut BlockEntity) -> T) -> Option<T> {
        let mut map = self.map.write().ok()?;
        map.get_mut(&pos).map(f)
    }

    /// Run `f` with whatever block entity is at `pos`, or with `None`.
    ///
    /// The whole of a container click runs inside this, so the read, the
    /// decision and the write happen under one lock. `None` is not an error:
    /// a crafting table has no block entity by design, and a chest broken
    /// while open has lost the one it had. Both cases have to reach the click
    /// so it can decide, which is why this is not `with_mut`.
    pub fn with_menu<T>(&self, pos: BlockPos, f: impl FnOnce(Option<&mut BlockEntity>) -> T) -> T {
        match self.map.write() {
            Ok(mut map) => f(map.get_mut(&pos)),
            // A poisoned lock means another thread panicked holding it. The
            // honest answer is "there is no block entity here" rather than a
            // second panic on a connection thread.
            Err(_) => f(None),
        }
    }

    /// [`Self::with_menu`] over a `CompoundContainer`: `f` gets the block
    /// entity at `first` and the one at `second`, under one write lock, so a
    /// double chest's click reads and writes both halves atomically.
    ///
    /// `second` of `None` (or equal to `first`) is exactly `with_menu`.
    pub fn with_menu_pair<T>(
        &self,
        first: BlockPos,
        second: Option<BlockPos>,
        f: impl FnOnce(Option<&mut BlockEntity>, Option<&mut BlockEntity>) -> T,
    ) -> T {
        let Ok(mut map) = self.map.write() else {
            return f(None, None);
        };
        let Some(second) = second.filter(|p| *p != first) else {
            return f(map.get_mut(&first), None);
        };
        // A `BTreeMap` hands out one `&mut` at a time through `get_mut`; two
        // disjoint ones come from a single `range_mut` walk over the span
        // between them, which is two positions apart for a chest.
        let (lo, hi) = if first < second {
            (first, second)
        } else {
            (second, first)
        };
        let (mut a, mut b) = (None, None);
        for (k, v) in map.range_mut(lo..=hi) {
            if *k == first {
                a = Some(v);
            } else if *k == second {
                b = Some(v);
            }
        }
        f(a, b)
    }

    /// Force a block entity in at a position, whatever was there.
    ///
    /// The load path, and tests. Not the placement path — that is
    /// [`Self::on_block_changed`], which has to decide whether to keep what
    /// is already there.
    pub fn insert(&self, pos: BlockPos, be: BlockEntity) {
        let kind = be.kind();
        if let Ok(mut map) = self.map.write() {
            map.insert(pos, be);
        }
        self.arm_ticker(pos, kind);
    }

    /// `Level.tickBlockEntities` offsets 5-40 and its `Iterator.remove` at
    /// 86, as one call: append `pendingBlockEntityTickers`, clear it, drop
    /// every entry whose block entity has gone, and hand back the survivors
    /// in list order.
    ///
    /// Vanilla removes a dead entry *during* the walk and this removes it
    /// before, which is the same set one tick earlier. The difference is not
    /// observable: an entry is dead because its block entity was removed, and
    /// a removed block entity's ticker does nothing on the tick vanilla still
    /// visits it — `BoundTickingBlockEntity.tick` opens with
    /// `blockEntity.isRemoved()` and returns.
    ///
    /// Returns a snapshot rather than holding the lock across the walk,
    /// because a ticker takes the map lock itself through
    /// [`Self::with_mut`]. Positions the walk turns out not to want are
    /// filtered by the caller's own `shouldTickBlocksAt` and `isValid` gates,
    /// which is where vanilla puts them too.
    #[must_use]
    pub fn ticker_list(&self) -> Vec<BlockPos> {
        if let (Ok(mut tickers), Ok(mut pending)) = (self.tickers.write(), self.pending.write()) {
            tickers.append(&mut pending);
        }
        let live: std::collections::BTreeSet<BlockPos> = match self.map.read() {
            Ok(map) => map.keys().copied().collect(),
            Err(_) => return Vec::new(),
        };
        let Ok(mut tickers) = self.tickers.write() else {
            return Vec::new();
        };
        tickers.retain(|p| live.contains(p));
        tickers.clone()
    }

    /// How many entries are on the ticker list. Tests and diagnostics.
    #[must_use]
    pub fn ticker_count(&self) -> usize {
        self.tickers.read().map(|t| t.len()).unwrap_or(0)
    }

    /// Every block entity in one chunk, in position order, with its position.
    ///
    /// What the save path walks. Position order rather than insertion order
    /// because the file has to be reproducible; vanilla's own order here is
    /// its chunk map's iteration order, which is not something this server
    /// can reproduce and not something any reader depends on.
    #[must_use]
    pub fn in_chunk(&self, cx: i32, cz: i32) -> Vec<(BlockPos, BlockEntity)> {
        let Ok(map) = self.map.read() else {
            return Vec::new();
        };
        map.iter()
            .filter(|(pos, _)| pos.0 >> 4 == cx && pos.2 >> 4 == cz)
            .map(|(pos, be)| (*pos, be.clone()))
            .collect()
    }

    /// Every chunk that holds at least one block entity.
    ///
    /// The save path needs this **in addition to** `edits.edited_chunks()`:
    /// a chest whose block was placed in this session is in both, but a chest
    /// that was *loaded* from disk and then filled has no block edit at all,
    /// and saving only edited chunks would drop it. That is the difference
    /// between a container that survives a restart and one that survives only
    /// the restart after the one that created it.
    #[must_use]
    pub fn chunks(&self) -> std::collections::BTreeSet<(i32, i32)> {
        let Ok(map) = self.map.read() else {
            return std::collections::BTreeSet::new();
        };
        map.keys().map(|p| (p.0 >> 4, p.2 >> 4)).collect()
    }

    /// Forget every block entity in these chunks, and say how many went.
    ///
    /// # This is a memory bound, and it is also the easiest place in this
    /// tree to lose somebody's chest
    ///
    /// The store is process-wide and had **no eviction at all**. A tile
    /// retires — `crate::tiles::Tiles::retain_around` — and its canvas goes,
    /// its overlay is kept on purpose, and its block entities stayed resident
    /// for the life of the process whether or not anybody would ever look at
    /// them again. Worse than a placement leak: [`crate::world::World::
    /// load_from`] inserts **every** block entity in every saved chunk of a
    /// tile it publishes, so walking across a large saved world accumulates
    /// its chests at the rate a player can walk and never gives one back.
    ///
    /// The trap is that eviction is trivially easy to get wrong in a way
    /// **no existing test can see**. Every test of this store asks about a
    /// chest while its tile is still resident, so a `retain_around` that
    /// dropped block entities without saving them first would be green
    /// everywhere and would empty a chest the moment its tile went out of
    /// view. The caller is therefore required to have written the tile to
    /// disk before calling this, and
    /// `tiles::tests::a_chest_survives_the_retirement_that_evicted_it` is the
    /// test that fails if it has not.
    ///
    /// Retirement with persistence **off** does not evict, and that is the
    /// same choice `crate::tiles`' overlay already makes: with nowhere to
    /// write it down, the only way to keep a build is to hold it.
    pub fn evict_chunks(&self, chunks: impl IntoIterator<Item = (i32, i32)>) -> usize {
        let wanted: std::collections::BTreeSet<(i32, i32)> = chunks.into_iter().collect();
        if wanted.is_empty() {
            return 0;
        }
        let Ok(mut map) = self.map.write() else {
            return 0;
        };
        let before = map.len();
        map.retain(|pos, _| !wanted.contains(&(pos.0 >> 4, pos.2 >> 4)));
        // `LevelChunk.clearAllBlockEntities` — an unloaded chunk's tickers
        // leave the list with its block entities. Not left to the `isRemoved`
        // sweep: an evicted chunk that is re-loaded re-arms its tickers, and
        // a stale entry surviving one tick would then be on the list twice.
        if let Ok(mut tickers) = self.tickers.write() {
            tickers.retain(|pos| !wanted.contains(&(pos.0 >> 4, pos.2 >> 4)));
        }
        if let Ok(mut pending) = self.pending.write() {
            pending.retain(|pos| !wanted.contains(&(pos.0 >> 4, pos.2 >> 4)));
        }
        before - map.len()
    }

    /// How many block entities exist. Tests and diagnostics.
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.read().map(|m| m.len()).unwrap_or(0)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drop everything. The per-scenario reset, and tests that share the
    /// process-wide store.
    pub fn clear(&self) {
        if let Ok(mut map) = self.map.write() {
            map.clear();
        }
        if let Ok(mut tickers) = self.tickers.write() {
            tickers.clear();
        }
        if let Ok(mut pending) = self.pending.write() {
            pending.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`Kind`] is the whole of `minecraft:block_entity_type` and nothing
    /// else, checked against the registry rather than against a list in this
    /// file.
    ///
    /// # Why this test and not a count
    ///
    /// The doc on [`Kind`] said "not the whole registry" for a month and the
    /// gap was carried as prose. Prose does not go red. What makes this able
    /// to fail in **both** directions is that it compares two sets:
    ///
    /// * a variant this server invents that vanilla has no type for fails on
    ///   the `ours - theirs` side, and that is not hypothetical — a name is
    ///   one underscore from a plausible wrong guess (`mob_spawner` is the
    ///   registry name and `SpawnerBlockEntity` is the class, so
    ///   `minecraft:spawner` is exactly the id somebody would write);
    /// * a type vanilla declares that this server has no variant for fails on
    ///   the `theirs - ours` side, which is the direction that was 35 wide
    ///   before 2026-09-05 and is the reason this file exists.
    ///
    /// A `ALL.len() == 49` assertion would catch neither: it is satisfied by
    /// any forty-nine strings.
    ///
    /// The table is `data/vanilla/protocol_ids.json`, written by
    /// `ci/dump-protocol-ids.sh` out of the generator's own registry report,
    /// so this compares us to vanilla and not to a second copy of ourselves.
    #[test]
    fn every_registered_block_entity_type_has_a_kind_and_no_kind_is_invented() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/vanilla/protocol_ids.json");
        if !rustmc_data::fixture::available(&path, "protocol id extract", "ci/dump-protocol-ids.sh")
        {
            return;
        }
        let text = std::fs::read_to_string(&path).expect("read protocol_ids.json");
        let doc: serde_json::Value = serde_json::from_str(&text).expect("parse");
        let theirs: std::collections::BTreeSet<String> = doc
            .get("block_entity_types")
            .and_then(serde_json::Value::as_object)
            .expect("block_entity_types map")
            .keys()
            .cloned()
            .collect();
        let ours: std::collections::BTreeSet<String> =
            Kind::ALL.iter().map(|k| k.id().to_string()).collect();
        assert_eq!(
            ours.len(),
            Kind::ALL.len(),
            "two variants share one registry name"
        );
        let missing: Vec<_> = theirs.difference(&ours).collect();
        let invented: Vec<_> = ours.difference(&theirs).collect();
        assert!(
            missing.is_empty() && invented.is_empty(),
            "block-entity registry drift: {} unmodelled {missing:?}, {} invented {invented:?}",
            missing.len(),
            invented.len()
        );
        // Every name resolves back to the variant it came from. `from_id` is
        // the load path, so a variant reachable from `id()` and not from
        // `from_id` is one whose saved chunks come back empty.
        for k in Kind::ALL {
            assert_eq!(
                Kind::from_id(k.id()),
                Some(k),
                "{} does not round-trip",
                k.id()
            );
        }
    }

    /// The `data/vanilla/block_entities.json` table, or `None` when the
    /// extract is absent and fixtures are optional.
    fn golden() -> Option<serde_json::Value> {
        // The path is spelled as one quoted `data/vanilla/...` literal and
        // joined onto the manifest root separately, and that is load-bearing
        // rather than style. `ci/gen-gameplay-tables.sh` derives the set of
        // tables a tree needs by grepping `crates`, `tools` and `plugins` for
        // exactly that literal (its own comment says a path that is not one
        // "is still invisible"). Written as `.join("../../data/vanilla/
        // block_entities.json")` this table was in `generator_for()` and *not*
        // in the derived list, so stage 3 of the gate reported "21 tables
        // current" and never ran `ci/block-entity-golden.sh` — the three tests
        // below then panicked on a missing fixture in a gate dir, which is
        // T0 exit 101 on `block-entities-2`.
        const REL: &str = "data/vanilla/block_entities.json";
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(REL);
        if !rustmc_data::fixture::available(
            &path,
            "block entity extract",
            "ci/block-entity-golden.sh",
        ) {
            return None;
        }
        let text = std::fs::read_to_string(&path).expect("read block_entities.json");
        Some(serde_json::from_str(&text).expect("parse block_entities.json"))
    }

    /// `for_state` is `BlockEntityType.isValid` over every block in the
    /// registry, in both directions.
    ///
    /// # What makes this able to go red where a hand table could not
    ///
    /// The block-to-type map exists in vanilla only as the varargs handed to
    /// `BlockEntityTypes.register`, and four of the forty-nine registrations
    /// hand it a *collection* — `CHEST` is `Util.copyAndAdd(Blocks
    /// .COPPER_CHEST.asList(), Blocks.CHEST)`. So transcribing it means
    /// expanding a list the bytecode never prints, and a transcription checked
    /// against a second copy of itself is this project's central defect.
    ///
    /// `ci/block-entity-golden.sh` bootstraps the real registry and asks
    /// `isValid` of every block, and this compares the two **as sets**:
    ///
    /// * a block vanilla attaches to a type and we do not is a position with a
    ///   menu and nowhere to keep an item — which is not hypothetical, it is
    ///   what the eight copper chests were until 2026-09-05;
    /// * a block we attach and vanilla does not is a block entity invented at
    ///   a position vanilla leaves bare, which changes what `save_to` writes
    ///   into a chunk;
    /// * a block mapped to the *wrong* type is caught by comparing the id and
    ///   not just the presence — `minecraft:trapped_chest` and
    ///   `minecraft:chest` are twenty-seven slots each and two different
    ///   registry numbers on the wire.
    #[test]
    fn for_state_is_block_entity_type_is_valid_over_the_whole_block_registry() {
        let Some(doc) = golden() else { return };
        let table = doc.get("table").and_then(|t| t.as_array()).expect("table");
        assert_eq!(table.len(), 49, "the extract is not the whole registry");
        let mut attached: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut wrong = Vec::new();
        for row in table {
            let ty = row.get("type").and_then(|v| v.as_str()).expect("type");
            for b in row
                .get("blocks")
                .and_then(|v| v.as_array())
                .expect("blocks")
            {
                let block = b.as_str().expect("block name");
                attached.insert(block.to_string());
                match for_state(block) {
                    Some(k) if k.id() == ty => {}
                    other => wrong.push(format!(
                        "{block}: vanilla says {ty}, we say {:?}",
                        other.map(Kind::id)
                    )),
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "{} blocks disagree: {wrong:#?}",
            wrong.len()
        );
        // 186 over 49 types on 26.2. Asserted so that an extract that lost a
        // registration — a `blocks` list that came back empty for want of a
        // bootstrap — cannot pass this test by having nothing to compare.
        assert_eq!(
            attached.len(),
            186,
            "the extract's block coverage moved; re-read it before adjusting this"
        );

        // The other direction, over every block there is. `block_props.json`
        // is keyed by *state*, so the block names are taken from it rather
        // than from a list here — a block this server invents a block entity
        // for would otherwise have to be guessed at to be tested for.
        let props = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/vanilla/block_props.json");
        if !rustmc_data::fixture::available(
            &props,
            "block props extract",
            "ci/block-props-golden.sh",
        ) {
            return;
        }
        let text = std::fs::read_to_string(&props).expect("read block_props.json");
        let props: serde_json::Value = serde_json::from_str(&text).expect("parse");
        let mut invented = Vec::new();
        let mut seen = 0usize;
        for row in props
            .get("table")
            .and_then(|t| t.as_array())
            .expect("table")
        {
            let Some(block) = row.get("block").and_then(|v| v.as_str()) else {
                continue;
            };
            if attached.contains(block) {
                continue;
            }
            seen += 1;
            if let Some(k) = for_state(block) {
                invented.push(format!("{block} -> {}", k.id()));
            }
        }
        // 27,799 of `block_props.json`'s 32,366 states belong to blocks with
        // no block entity (measured 2026-09-05). The floor is here so that an
        // extract that came back empty or truncated cannot pass this half by
        // having nothing to sweep — the assertion below it is `is_empty()`,
        // which an empty input satisfies for free.
        assert!(
            seen > 20_000,
            "block_props gave only {seen} unattached states — the sweep is not sweeping"
        );
        invented.sort();
        invented.dedup();
        assert!(
            invented.is_empty(),
            "{} blocks get a block entity vanilla does not give one: {invented:#?}",
            invented.len()
        );
    }

    /// `container_size` and `item_slots` against the instantiated instance.
    ///
    /// These are two different questions over one object and the extract keeps
    /// them apart: `containerSize` is -1 for a type that is not a `Container`
    /// at all and `itemSlots` is -1 for one holding no `NonNullList`. A
    /// campfire is (-1, 4) and a jukebox is (1, -1) — neither collapses to a
    /// single number, which is why [`Kind::container_size`] and
    /// [`Kind::item_slots`] are separate methods and why folding them would be
    /// wrong in both directions.
    ///
    /// This server folds -1 to 0 at its boundary, because every caller wants a
    /// slot count and the honest slot count of a sign is none. The fold is
    /// applied *here*, to vanilla's answer, so the comparison is against
    /// vanilla and the fold is the only thing this test grants.
    #[test]
    fn container_size_and_item_slots_match_the_instantiated_block_entity() {
        let Some(doc) = golden() else { return };
        let mut checked = 0usize;
        let mut wrong = Vec::new();
        for row in doc.get("table").and_then(|t| t.as_array()).expect("table") {
            let ty = row.get("type").and_then(|v| v.as_str()).expect("type");
            // A type the extract could not instantiate carries no answer, and
            // an absent answer must not be read as zero. `minecraft:vault` is
            // the one: `VaultConfig.DEFAULT`'s class initialiser wants a
            // server the dumper does not have.
            if !row
                .get("created")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            {
                continue;
            }
            let Some(kind) = Kind::from_id(ty) else {
                wrong.push(format!("{ty}: no Kind"));
                continue;
            };
            let cs = row
                .get("containerSize")
                .and_then(serde_json::Value::as_i64)
                .expect("cs");
            let is = row
                .get("itemSlots")
                .and_then(serde_json::Value::as_i64)
                .expect("is");
            let cs = usize::try_from(cs.max(0)).expect("size");
            let is = usize::try_from(is.max(0)).expect("size");
            // `item_slots` defaults to `container_size`, so vanilla's -1 for a
            // real container means "no NonNullList of its own" and the slot
            // count is the container's. A jukebox is that shape: one container
            // slot, one bare `ItemStack` field.
            let want_slots = if is == 0 { cs } else { is };
            if kind.container_size() != cs || kind.item_slots() != want_slots {
                wrong.push(format!(
                    "{ty}: vanilla ({cs}, {want_slots}), ours ({}, {})",
                    kind.container_size(),
                    kind.item_slots()
                ));
            }
            checked += 1;
        }
        assert_eq!(
            checked, 48,
            "48 of the 49 types instantiate; vault does not"
        );
        assert!(
            wrong.is_empty(),
            "{} sizes disagree: {wrong:#?}",
            wrong.len()
        );
    }

    /// `ShulkerBoxBlockEntity` is the registry's only `iconst_0`.
    ///
    /// Every other container reaches `saveAllItems` with `alwaysWrite` true —
    /// through the two-argument overload, whose body is `iconst_1;
    /// invokestatic`, or through an explicit one — so an empty chest writes
    /// `{Items: []}`. A shulker box reaches the `discard` at offset 79 and
    /// writes no key. The chest half is the control: without it this passes on
    /// a server that never writes `Items` for anything.
    #[test]
    fn an_empty_shulker_box_writes_no_items_key_and_an_empty_chest_does() {
        let shulker = BlockEntity::new(Kind::ShulkerBox).to_nbt((0, 0, 0));
        let c = shulker.as_compound().expect("compound");
        assert!(
            c.get("Items").is_none(),
            "an empty shulker box writes no Items key, it does not write an empty one"
        );
        let chest = BlockEntity::new(Kind::Chest).to_nbt((0, 0, 0));
        assert!(
            chest
                .as_compound()
                .expect("compound")
                .get("Items")
                .is_some(),
            "the control: every other container writes the key empty"
        );
        // And a shulker box with something in it writes the key as usual.
        let mut be = BlockEntity::new(Kind::ShulkerBox);
        be.set_item(3, ItemStack::new("minecraft:stone", 5));
        let nbt = be.to_nbt((0, 0, 0));
        let items = nbt
            .as_compound()
            .expect("compound")
            .get("Items")
            .and_then(Nbt::as_list)
            .expect("a non-empty shulker box writes Items");
        assert_eq!(items.len(), 1);
        let (pos, back) = BlockEntity::from_nbt(&nbt).expect("round trip");
        assert_eq!(pos, (0, 0, 0));
        assert_eq!(back.item(3), ItemStack::new("minecraft:stone", 5));
    }

    /// A jukebox, a lectern and a decorated pot each keep one stack and none
    /// of them keeps it under `Items`.
    ///
    /// The keys are `RecordItem`, `Book` and `item` — three different
    /// capitalisation conventions over the same thing, read off the three
    /// `ldc` constants rather than recalled. Each `store` is guarded by an
    /// `ItemStack.isEmpty()` test, so an empty one writes no key.
    ///
    /// **`item_slots()` is the wrong question here and that is the point.**
    /// Before `items_key` existed, `to_nbt` wrote `Items` for anything with a
    /// slot, so a jukebox with a disc in it saved a compound vanilla rejects —
    /// and it round-tripped perfectly through this server's own loader, which
    /// is the failure this asserts against.
    #[test]
    fn the_single_stack_types_save_under_their_own_key_and_not_under_items() {
        for (kind, key, item) in [
            (Kind::Jukebox, "RecordItem", "minecraft:music_disc_13"),
            (Kind::DecoratedPot, "item", "minecraft:brick"),
        ] {
            let mut be = BlockEntity::new(kind);
            be.set_item(0, ItemStack::new(item, 1));
            let nbt = be.to_nbt((1, 2, 3));
            let c = nbt.as_compound().expect("compound");
            assert!(
                c.get("Items").is_none(),
                "{} must not write an Items list",
                kind.id()
            );
            let stack = c
                .get(key)
                .and_then(Nbt::as_compound)
                .unwrap_or_else(|| panic!("{} writes its stack under {key}", kind.id()));
            assert_eq!(stack.get("id").and_then(Nbt::as_str), Some(item));
            assert_eq!(stack.get("count").and_then(Nbt::as_i32), Some(1));
            let (pos, back) = BlockEntity::from_nbt(&nbt).expect("round trip");
            assert_eq!(pos, (1, 2, 3));
            assert_eq!(back.item(0), ItemStack::new(item, 1));
            // Empty writes no key at all, which is the `isEmpty()` guard.
            let empty = BlockEntity::new(kind).to_nbt((1, 2, 3));
            assert!(empty.as_compound().expect("compound").get(key).is_none());
        }
        // A lectern has a `Book` key and *no* container slot: `getContainerSize`
        // is absent from `LecternBlockEntity` and the one-slot container a
        // hopper sees is an anonymous inner class over the book field. So the
        // book has one stack of storage and the container size stays 0.
        assert_eq!(Kind::Lectern.items_key(), Some(("Book", false)));
        assert_eq!(Kind::Lectern.container_size(), 0);
        assert_eq!(Kind::Lectern.item_slots(), 0);
        assert_eq!(Kind::Lectern.storage_slots(), 1);
        // `Page` rides with the book and only with it.
        let mut lectern = BlockEntity::new(Kind::Lectern);
        let bare = lectern.to_nbt((0, 0, 0));
        assert!(bare.as_compound().expect("compound").get("Page").is_none());
        lectern.set_item(0, ItemStack::one("minecraft:writable_book"));
        let full = lectern.to_nbt((0, 0, 0));
        let c = full.as_compound().expect("compound");
        assert!(c.get("Book").is_some() && c.get("Page").is_some());
    }

    /// The scalar keys ten types write beside their items: exact names, exact
    /// tag types, exact starting values.
    ///
    /// # Three ways this could be wrong that a round-trip alone cannot see
    ///
    /// * **the tag type.** `BrewTime` is `putShort` and `Fuel` is `putByte`
    ///   six instructions apart in one method; writing either as an int
    ///   round-trips perfectly here and is refused by vanilla's codec. So the
    ///   assertion is on the `Nbt` variant and not only on the number;
    /// * **the starting value.** A freshly placed hopper writes
    ///   `TransferCooldown: -1` and a fresh chiseled bookshelf writes
    ///   `last_interacted_slot: -1`; a round-trip that starts from whatever
    ///   the code wrote is satisfied by 0 for both;
    /// * **the key name.** `cooking_time_spent` and `cooking_total_time` are
    ///   one word apart and a furnace writes both.
    ///
    /// The bytecode is `putShort`/`putInt`/`putByte` in each class's
    /// `saveAdditional`, and the values are the `iconst_m1`/`iconst_0` the
    /// constructor leaves the field at.
    #[test]
    fn the_scalar_keys_carry_vanillas_names_tags_and_starting_values() {
        let want: &[(Kind, &[(&str, Nbt)])] = &[
            (Kind::Hopper, &[("TransferCooldown", Nbt::Int(-1))]),
            (
                Kind::Furnace,
                &[
                    ("cooking_time_spent", Nbt::Short(0)),
                    ("cooking_total_time", Nbt::Short(0)),
                    ("lit_time_remaining", Nbt::Short(0)),
                    ("lit_total_time", Nbt::Short(0)),
                ],
            ),
            (
                Kind::BrewingStand,
                &[("BrewTime", Nbt::Short(0)), ("Fuel", Nbt::Byte(0))],
            ),
            (Kind::Comparator, &[("OutputSignal", Nbt::Int(0))]),
            (
                Kind::ChiseledBookshelf,
                &[("last_interacted_slot", Nbt::Int(-1))],
            ),
            (Kind::Crafter, &[("crafting_ticks_remaining", Nbt::Int(0))]),
        ];
        for (kind, keys) in want {
            let nbt = BlockEntity::new(*kind).to_nbt((0, 0, 0));
            let c = nbt.as_compound().expect("compound");
            for (name, tag) in *keys {
                assert_eq!(
                    c.get(*name),
                    Some(tag),
                    "{} writes {name} as {tag:?}",
                    kind.id()
                );
            }
        }
        // A smoker and a blast furnace inherit `AbstractFurnaceBlockEntity`'s
        // method whole, so they carry the same four keys — asserted rather
        // than assumed, because a `match` arm is exactly where that would be
        // dropped.
        for kind in [Kind::Smoker, Kind::BlastFurnace] {
            assert_eq!(
                kind.extra_keys(),
                Kind::Furnace.extra_keys(),
                "{} inherits the furnace's four counters",
                kind.id()
            );
        }
        // A chest declares none, and the keys are absent rather than zero.
        let chest = BlockEntity::new(Kind::Chest).to_nbt((0, 0, 0));
        assert!(chest
            .as_compound()
            .expect("compound")
            .get("TransferCooldown")
            .is_none());

        // Round trip, from values that are not the defaults so that a loader
        // which ignored the keys and rebuilt the constructor's values would
        // fail rather than pass.
        let mut be = BlockEntity::new(Kind::BrewingStand);
        be.set_extra("BrewTime", Nbt::Short(271));
        be.set_extra("Fuel", Nbt::Byte(19));
        be.set_item(3, ItemStack::new("minecraft:blaze_powder", 7));
        let nbt = be.to_nbt((4, 5, 6));
        assert_eq!(
            nbt.as_compound().expect("c").get("BrewTime"),
            Some(&Nbt::Short(271))
        );
        let (pos, back) = BlockEntity::from_nbt(&nbt).expect("round trip");
        assert_eq!(pos, (4, 5, 6));
        assert_eq!(back.extra_i32("BrewTime"), Some(271));
        assert_eq!(back.extra_i32("Fuel"), Some(19));
        assert_eq!(back.item(3), ItemStack::new("minecraft:blaze_powder", 7));
        // And the ticker sees them: the load seeds the behaviour.
        assert_eq!(back.container_data(), vec![271, 19]);

        // A key this kind does not declare is `None`, not 0 — the distinction
        // a hopper's -1 makes load-bearing.
        assert_eq!(back.extra("TransferCooldown"), None);
        assert_eq!(
            BlockEntity::new(Kind::Hopper).extra_i32("TransferCooldown"),
            Some(-1)
        );
    }

    /// The saved compound of a freshly placed block entity, key for key and
    /// **tag width for tag width**, against vanilla's own `saveWithFullMetadata`.
    ///
    /// # Why the tag id and not only the name
    ///
    /// `BrewTime` is a short and `Fuel` a byte, written six instructions
    /// apart in one method; the four furnace counters are shorts where every
    /// other number in this file is an int. A comparison on names alone
    /// passes on a server that writes all of them as ints — which round-trips
    /// perfectly here and is refused by vanilla's codec. So the key of the
    /// comparison is `(name, Tag.getId())`.
    ///
    /// # The outstanding differences are a declared list, not a silence
    ///
    /// [`OUTSTANDING_SAVE_KEYS`] names every key vanilla writes that this
    /// server does not, one line each with the reason. It is a ratchet: a
    /// *new* difference fails, and a key that starts being written has to be
    /// deleted from the list or the test fails the other way. A bare
    /// `assert!(missing.is_empty())` was not an option — some of those keys
    /// need mechanisms this branch does not build — and an unbounded skip
    /// would be a test that stops measuring as the gap grows.
    #[test]
    fn a_fresh_block_entity_saves_vanillas_keys_at_vanillas_tag_widths() {
        let Some(doc) = golden() else { return };
        let mut checked = 0usize;
        let mut extra = Vec::new();
        let mut missing = Vec::new();
        let mut stale = Vec::new();
        for row in doc.get("table").and_then(|t| t.as_array()).expect("table") {
            let ty = row.get("type").and_then(|v| v.as_str()).expect("type");
            let Some(saved) = row.get("saved").and_then(|v| v.as_array()) else {
                continue;
            };
            let Some(kind) = Kind::from_id(ty) else {
                continue;
            };
            // `BlockPos.ZERO` is what the extract instantiates at.
            let ours: std::collections::BTreeSet<(String, i64, String)> =
                match BlockEntity::new(kind).to_nbt((0, 0, 0)) {
                    Nbt::Compound(c) => c
                        .iter()
                        .map(|(k, v)| (k.clone(), nbt_tag_id(v), snbt(v)))
                        .collect(),
                    _ => panic!("{ty} does not save a compound"),
                };
            let theirs: std::collections::BTreeSet<(String, i64, String)> = saved
                .iter()
                .filter_map(|v| {
                    let s = v.as_str()?;
                    let (name, rest) = s.split_once(':')?;
                    let (id, value) = rest.split_once(':')?;
                    let id: i64 = id.parse().ok()?;
                    // A compound's and a list's rendering walks a `HashMap`
                    // view, which the JVM does not promise across runs, so
                    // their *value* is folded away on both sides and they are
                    // compared by name and tag id. Every scalar and every int
                    // array is compared in full — which is where the plausible
                    // wrong guesses live: `posY` starts at 1 where its
                    // siblings start at 0, `TrackOutput` at true, a fresh
                    // spawner's `Delay` at 20, a jigsaw's `pool` at
                    // `minecraft:empty` rather than at "".
                    Some((name.to_string(), id, opaque_or(id, value)))
                })
                .collect();
            checked += 1;
            for k in theirs.difference(&ours) {
                let line = format!("{ty} {} tag {} = {}", k.0, k.1, k.2);
                if OUTSTANDING_SAVE_KEYS.contains(&line.as_str()) {
                    continue;
                }
                missing.push(line);
            }
            for k in ours.difference(&theirs) {
                extra.push(format!("{ty} {} tag {} = {}", k.0, k.1, k.2));
            }
        }
        // A declared difference that is no longer a difference must be
        // deleted, or the list would silently outlive what it excuses.
        for line in OUTSTANDING_SAVE_KEYS {
            let ty = line.split_whitespace().next().expect("type");
            let name = line.split_whitespace().nth(1).expect("key");
            if let Some(kind) = Kind::from_id(ty) {
                if let Nbt::Compound(c) = BlockEntity::new(kind).to_nbt((0, 0, 0)) {
                    if c.contains_key(name) {
                        stale.push(line);
                    }
                }
            }
        }
        assert!(
            checked > 40,
            "only {checked} types carried a saved compound"
        );
        assert!(
            extra.is_empty(),
            "{} keys this server writes and vanilla does not: {extra:#?}",
            extra.len()
        );
        assert!(
            missing.is_empty(),
            "{} undeclared missing keys: {missing:#?}",
            missing.len()
        );
        assert!(
            stale.is_empty(),
            "{} entries of OUTSTANDING_SAVE_KEYS are no longer missing and must be deleted: {stale:#?}",
            stale.len()
        );
    }

    /// The same tag, carrying a different value.
    ///
    /// A round-trip test has to move each key **off its default at its own
    /// width** — writing an int over a short would be testing the loader's
    /// type check instead of its round trip, and leaving the default in place
    /// would pass on a loader that ignores the compound entirely.
    fn moved(v: &Nbt, n: i32) -> Nbt {
        match v {
            Nbt::Byte(_) => Nbt::Byte(i8::try_from(n % 100).expect("small")),
            Nbt::Short(_) => Nbt::Short(i16::try_from(n).expect("small")),
            Nbt::Int(_) => Nbt::Int(n),
            Nbt::Long(_) => Nbt::Long(i64::from(n)),
            Nbt::Float(_) => Nbt::Float(n as f32),
            Nbt::String(_) => Nbt::String(format!("moved-{n}")),
            Nbt::IntArray(a) => Nbt::IntArray(a.iter().map(|_| n).collect()),
            Nbt::List(_) => Nbt::List(vec![Nbt::Int(n)]),
            Nbt::Compound(_) => {
                let mut c = BTreeMap::new();
                c.insert("moved".to_string(), Nbt::Int(n));
                Nbt::Compound(c)
            }
            other => other.clone(),
        }
    }

    /// `PotDecorations`: a brick face is an absent one, a short list leaves
    /// the rest absent, all-absent is `EMPTY` and saves nothing; otherwise the
    /// four ordered names save as `sherds`, load back, and lead the update
    /// tag. Refused on anything but a pot.
    #[test]
    fn a_pot_keeps_its_sherds_in_order_with_bricks_for_plain_faces() {
        let n = |s: &str| s.to_string();
        let mut be = BlockEntity::new(Kind::DecoratedPot);
        be.set_sherds(&[n("minecraft:brick"), n("minecraft:brick")]);
        assert_eq!(be.sherds(), None, "all plain is EMPTY");
        assert!(
            be.update_tag()
                .and_then(|t| t.as_compound().map(|c| c.contains_key("sherds")))
                == Some(false)
        );
        be.set_sherds(&[
            n("minecraft:angler_pottery_sherd"),
            n("minecraft:brick"),
            n("minecraft:prize_pottery_sherd"),
        ]);
        let want = [
            n("minecraft:angler_pottery_sherd"),
            n("minecraft:brick"),
            n("minecraft:prize_pottery_sherd"),
            n("minecraft:brick"),
        ];
        assert_eq!(be.sherds(), Some(&want));
        let (_, back) = BlockEntity::from_nbt(&be.to_nbt((0, 0, 0))).expect("loads");
        assert_eq!(back.sherds(), Some(&want));
        let tag = back.update_tag().expect("a pot has an update packet");
        assert_eq!(
            tag.as_compound().and_then(|c| c.get("sherds")),
            Some(&Nbt::List(
                want.iter().map(|s| Nbt::String(s.clone())).collect()
            ))
        );
        let mut chest = BlockEntity::new(Kind::Chest);
        chest.set_sherds(&want);
        assert_eq!(chest.sherds(), None);
    }

    /// The custom name on every kind whose `applyImplicitComponents` reads
    /// one and on no other: set, saved (beside a loot table where there is
    /// one), loaded back; refused on a kind whose class has no name to take.
    #[test]
    fn a_container_keeps_its_custom_name_and_nothing_else_takes_one() {
        let name = Nbt::String("Loot".into());
        let mut named = 0;
        for kind in Kind::ALL {
            let mut be = BlockEntity::new(kind);
            be.set_custom_name(Some(name.clone()));
            let Some(key) = kind.custom_name_key() else {
                assert_eq!(be.custom_name(), None, "{}", kind.id());
                continue;
            };
            named += 1;
            be.set_loot(Some(LootRef {
                table: "minecraft:chests/simple_dungeon".into(),
                seed: 3,
            }));
            let nbt = be.to_nbt((0, 64, 0));
            assert_eq!(
                nbt.as_compound().and_then(|c| c.get(key)),
                Some(&name),
                "{}: saved before the loot-or-items choice",
                kind.id()
            );
            let (_, back) = BlockEntity::from_nbt(&nbt).expect("loads");
            assert_eq!(back.custom_name(), Some(&name), "{}", kind.id());
        }
        assert_eq!(
            named, 16,
            "the twelve concrete BaseContainerBlockEntity classes, the banner, \
             the beacon, the enchanting table and the skull"
        );
        // The three with an update packet carry it, under their own key.
        for (kind, key) in [
            (Kind::Banner, "CustomName"),
            (Kind::Beacon, "CustomName"),
            (Kind::Skull, "custom_name"),
        ] {
            let mut be = BlockEntity::new(kind);
            be.set_custom_name(Some(name.clone()));
            let tag = be.update_tag().expect("an update packet");
            assert_eq!(
                tag.as_compound().and_then(|c| c.get(key)),
                Some(&name),
                "{}",
                kind.id()
            );
        }
    }

    /// Every one of the forty-nine kinds round-trips its own state.
    ///
    /// Sweeping all of them rather than picking three, because the failure
    /// this catches is per-kind by construction: a `match` arm in `to_nbt`
    /// with no partner in `from_nbt` loses exactly the types that arm names
    /// and nothing else, and a hand-picked test covers whichever kinds its
    /// author was thinking about.
    ///
    /// Every slot is filled and every scalar set to a value that is **not**
    /// the default, so a loader that ignored the compound and rebuilt a fresh
    /// block entity would fail rather than pass. The stacks carry distinct
    /// counts for the same reason a slot index is not enough: a loader that
    /// put every stack in slot 0 would round-trip a one-slot container.
    #[test]
    fn every_kind_round_trips_its_items_and_its_scalars() {
        for kind in Kind::ALL {
            let mut be = BlockEntity::new(kind);
            for slot in 0..be.size().max(kind.item_slots()) {
                be.set_item(
                    slot,
                    ItemStack::new("minecraft:stone", i32::try_from(slot).expect("slot") + 1),
                );
            }
            // Move every key off its default, at its own tag width, so a
            // loader that ignored the compound and rebuilt a fresh block
            // entity fails rather than passes.
            for (i, k) in kind.extra_keys().iter().enumerate() {
                let n = 17 + i32::try_from(i).expect("i");
                be.set_extra(k.name, moved(&k.value.to_nbt(), n));
            }
            let nbt = be.to_nbt((7, -3, 11));
            let (pos, back) = BlockEntity::from_nbt(&nbt)
                .unwrap_or_else(|| panic!("{} does not load what it saved", kind.id()));
            assert_eq!(pos, (7, -3, 11), "{}", kind.id());
            assert_eq!(back.kind(), kind);
            for (i, k) in kind.extra_keys().iter().enumerate() {
                let n = 17 + i32::try_from(i).expect("i");
                assert_eq!(
                    back.extra(k.name),
                    Some(&moved(&k.value.to_nbt(), n)),
                    "{} loses {}",
                    kind.id(),
                    k.name
                );
            }
            // A sign's text lives in `crate::sign` and is round-tripped by
            // that module's own tests; its items list is empty by
            // construction, so the loop below is vacuous for it and that is
            // correct rather than skipped.
            for slot in 0..kind.item_slots() {
                // The single-stack types keep one stack under their own key,
                // so only slot 0 survives — which is what vanilla does too:
                // `RecordItem` is one `store`, not a list.
                if kind.items_key().is_some_and(|(k, _)| k != "Items") && slot > 0 {
                    continue;
                }
                assert_eq!(
                    back.item(slot),
                    ItemStack::new("minecraft:stone", i32::try_from(slot).expect("slot") + 1),
                    "{} loses slot {slot}",
                    kind.id()
                );
            }
        }
    }

    /// Placement creates one, breaking removes it — the two halves of
    /// `setBlockState`. Without the removal a chest's items would come back
    /// when a chest was placed at the same position again.
    #[test]
    fn a_block_entity_appears_on_placement_and_goes_on_break() {
        let store = BlockEntities::new();
        let pos = (4, 64, 4);
        assert!(store.get(pos).is_none(), "nothing there to start with");
        assert!(store.on_block_changed(pos, "minecraft:air", "minecraft:chest[facing=north]"));
        assert_eq!(store.get(pos).map(|b| b.kind()), Some(Kind::Chest));
        // Break it: `Level.removeBlock` writes air.
        assert!(store.on_block_changed(pos, "minecraft:chest[facing=north]", "minecraft:air"));
        assert!(store.get(pos).is_none(), "the chest took its items with it");
    }

    /// **The branch a tidier implementation gets wrong.** A chest whose
    /// *state* changes — the `type` property when a neighbour pairs with it,
    /// which `240-chest-pairs-with-its-neighbour` actually does — must keep
    /// its contents. An implementation that removed and recreated on every
    /// `setBlock` would empty a chest when somebody placed one next to it.
    #[test]
    fn a_state_change_that_keeps_the_block_keeps_the_contents() {
        let store = BlockEntities::new();
        let pos = (4, 64, 4);
        store.on_block_changed(pos, "minecraft:air", "minecraft:chest[type=single]");
        store.with_mut(pos, |be| {
            be.set_item(0, ItemStack::one("minecraft:stone"));
        });
        // The neighbour arrives and this chest becomes the left half.
        assert!(
            !store.on_block_changed(
                pos,
                "minecraft:chest[type=single]",
                "minecraft:chest[type=left]"
            ),
            "same block, so nothing was created or destroyed"
        );
        assert_eq!(
            store.get(pos).map(|b| b.item(0)),
            Some(ItemStack::one("minecraft:stone")),
            "the stone is still in it"
        );
    }

    /// Swapping one block-entity block for a *different* one replaces it
    /// rather than keeping the old contents under the new type.
    #[test]
    fn a_different_block_entity_block_replaces_rather_than_inherits() {
        let store = BlockEntities::new();
        let pos = (4, 64, 4);
        store.on_block_changed(pos, "minecraft:air", "minecraft:chest");
        store.with_mut(pos, |be| {
            be.set_item(0, ItemStack::one("minecraft:stone"));
        });
        assert!(store.on_block_changed(pos, "minecraft:chest", "minecraft:furnace"));
        let be = store.get(pos).expect("a furnace is there");
        assert_eq!(be.kind(), Kind::Furnace);
        assert_eq!(be.size(), 3, "and it is furnace-sized, not chest-sized");
        assert!(be.item(0).is_empty(), "it did not inherit the stone");
    }

    /// A block with no block entity never makes one, and the crafting table
    /// is named explicitly because it is the one that looks like it should.
    #[test]
    fn blocks_without_a_block_entity_make_none() {
        assert_eq!(
            for_state("minecraft:chest[facing=north]"),
            Some(Kind::Chest)
        );
        assert_eq!(
            for_state("minecraft:furnace[lit=false]"),
            Some(Kind::Furnace)
        );
        assert_eq!(for_state("minecraft:crafting_table"), None);
        assert_eq!(for_state("minecraft:stone"), None);
        let store = BlockEntities::new();
        assert!(!store.on_block_changed((0, 0, 0), "minecraft:air", "minecraft:crafting_table"));
        assert!(store.is_empty());
    }

    /// The container sizes, which are the field an implementation that
    /// hardcoded one container would get wrong — and the sign, which has
    /// none.
    #[test]
    fn the_container_kinds_have_the_sizes_their_classes_declare() {
        assert_eq!(Kind::Chest.container_size(), 27);
        assert_eq!(Kind::Furnace.container_size(), 3);
        assert_eq!(Kind::Sign.container_size(), 0);
        assert_eq!(BlockEntity::new(Kind::Chest).size(), 27);
        assert_eq!(BlockEntity::new(Kind::Furnace).size(), 3);
        assert_eq!(BlockEntity::new(Kind::Sign).size(), 0);
        // And a click into a sign has nowhere to land, which is what the
        // zero is *for*.
        assert!(BlockEntity::new(Kind::Sign).item(0).is_empty());
    }

    /// **Everything this server models except a chest and a furnace answers
    /// `getUpdatePacket()`.** Neither `ChestBlockEntity` nor
    /// `AbstractFurnaceBlockEntity` declares the method in the pinned jar, so
    /// the base `return null` stands and a chest changing state broadcasts a
    /// `block_update` and nothing more. A server that sent
    /// `block_entity_data` for every block entity would add a packet to four
    /// checks that are currently at `match`.
    ///
    /// *This was `only_the_sign_has_an_update_packet` until 2026-08-12, when
    /// eleven more overriding types landed and the name became a caveat that
    /// lagged its code.* The invariant it was really guarding is the negative
    /// one, and that is now stated as a partition over the whole enum rather
    /// than as a list of two — so the next type added has to choose a side.
    #[test]
    fn the_update_packet_partition_covers_every_kind() {
        let all = [
            Kind::Chest,
            Kind::Furnace,
            Kind::Sign,
            Kind::HangingSign,
            Kind::Banner,
            Kind::Beacon,
            Kind::BrushableBlock,
            Kind::Campfire,
            Kind::Conduit,
            Kind::CopperGolemStatue,
            Kind::CreakingHeart,
            Kind::DecoratedPot,
            Kind::Shelf,
            Kind::Skull,
        ];
        let mut quiet = 0;
        for kind in all {
            let be = BlockEntity::new(kind);
            assert_eq!(
                be.has_update_packet(),
                be.update_tag().is_some(),
                "{kind:?}: has_update_packet and update_tag disagree, and \
                 `broadcast_block_entity_if_needed` believes the second"
            );
            if !be.has_update_packet() {
                quiet += 1;
            }
        }
        assert_eq!(
            quiet, 2,
            "chest and furnace are the only modelled types whose \
             getUpdatePacket() is the inherited null"
        );
    }

    /// Every one of `BlockEntityTypes.SIGN`'s twenty-four valid blocks makes
    /// a sign, and the hanging signs make a **different kind**.
    ///
    /// *This test asserted `None` for the hanging signs until 2026-08-12, and
    /// it was right to.* Modelling neither is a smaller error than modelling
    /// one as the other: `HangingSignBlockEntity` inherits `getUpdateTag`
    /// whole, so the only thing separating the two on the wire is the type
    /// id, and a hanging sign resolved to `Kind::Sign` would send a packet
    /// whose every other byte is correct. The assertion moved from "not
    /// modelled" to "modelled as 8, never 7"; what it must never become is
    /// silence.
    #[test]
    fn the_sign_type_covers_its_twenty_four_blocks_and_the_hanging_ones_are_type_8() {
        assert_eq!(SIGN_BLOCKS.len(), 24);
        for block in SIGN_BLOCKS {
            assert_eq!(for_state(block), Some(Kind::Sign), "{block}");
        }
        // With properties on, which is the form every call site actually has.
        assert_eq!(
            for_state("minecraft:oak_sign[rotation=8,waterlogged=false]"),
            Some(Kind::Sign)
        );
        assert_eq!(
            for_state("minecraft:oak_wall_sign[facing=north,waterlogged=false]"),
            Some(Kind::Sign)
        );
        assert_eq!(HANGING_SIGN_BLOCKS.len(), 24);
        for hanging in HANGING_SIGN_BLOCKS {
            assert_eq!(
                for_state(hanging),
                Some(Kind::HangingSign),
                "{hanging} is BlockEntityTypes.HANGING_SIGN, type 8, not 7"
            );
        }
        assert_eq!(Kind::Sign.id(), "minecraft:sign");
        assert_eq!(Kind::HangingSign.id(), "minecraft:hanging_sign");
        // Both keep sign text, and that is the only thing they share here.
        assert!(Kind::Sign.is_sign() && Kind::HangingSign.is_sign());
    }

    /// **The branch the old kind-comparison got wrong the moment a type had
    /// more than one block.** `oak_sign` -> `birch_sign` is one block
    /// replacing another, so vanilla's `newState.is(oldState.getBlock())`
    /// fails and the block entity is rebuilt; a comparison on the *kind*
    /// would keep the oak sign's text under the birch one.
    #[test]
    fn a_different_sign_block_replaces_rather_than_inheriting_the_text() {
        let store = BlockEntities::new();
        let pos = (4, 64, 4);
        store.on_block_changed(pos, "minecraft:air", "minecraft:oak_sign[rotation=0]");
        store.with_mut(pos, |be| {
            let sign = be.sign_mut().expect("a sign");
            sign.set_allowed_player_editor(Some(7));
            sign.update_sign_text(7, true, &["oak".to_string()]);
        });
        assert_eq!(
            store
                .get(pos)
                .and_then(|b| b.sign().map(|s| s.text(true).messages()[0].clone())),
            Some("oak".to_string())
        );
        assert!(
            store.on_block_changed(
                pos,
                "minecraft:oak_sign[rotation=0]",
                "minecraft:birch_sign"
            ),
            "a different block is a different block entity"
        );
        assert_eq!(
            store
                .get(pos)
                .and_then(|b| b.sign().map(|s| s.text(true).messages()[0].clone())),
            Some(String::new()),
            "the birch sign inherited the oak sign's words"
        );
    }

    /// And the same block with a different *state* keeps its text —
    /// `rotation` changes under a sign whenever nothing at all happens to it,
    /// so this is the common case rather than the exotic one.
    #[test]
    fn a_sign_state_change_that_keeps_the_block_keeps_the_text() {
        let store = BlockEntities::new();
        let pos = (4, 64, 4);
        store.on_block_changed(pos, "minecraft:air", "minecraft:oak_sign[rotation=0]");
        store.with_mut(pos, |be| {
            let sign = be.sign_mut().expect("a sign");
            sign.set_allowed_player_editor(Some(7));
            sign.update_sign_text(7, true, &["keep".to_string()]);
        });
        assert!(!store.on_block_changed(
            pos,
            "minecraft:oak_sign[rotation=0]",
            "minecraft:oak_sign[rotation=8]"
        ));
        assert_eq!(
            store
                .get(pos)
                .and_then(|b| b.sign().map(|s| s.text(true).messages()[0].clone())),
            Some("keep".to_string())
        );
    }

    /// **The wire order of the update tag's keys, against Java's own answer.**
    ///
    /// This is the assertion the whole sign family's `match` rests on:
    /// `block_entity_data`'s projection decodes the position and the type id
    /// and hands **every remaining byte to a digest**, so the NBT has to be
    /// byte-identical to vanilla's and not merely equivalent. `CompoundTag`
    /// wraps a `java.util.HashMap` and `write` iterates `keySet()`, so the
    /// order is bucket order.
    ///
    /// The expected sequences are not reasoned about here. They were printed
    /// by a three-line Java program on the same key sets, run on the
    /// Threadripper on 2026-08-06:
    ///
    /// ```text
    ///   {front_text, back_text, is_waxed}          -> back_text, is_waxed, front_text
    ///   {messages, color, has_glowing_text}        -> has_glowing_text, color, messages
    /// ```
    ///
    /// Alphabetical order would give `back_text, front_text, is_waxed` and
    /// `color, has_glowing_text, messages`. Both are legal NBT for the same
    /// tag and both would fail the digest — which is why this is a test and
    /// not a comment.
    #[test]
    fn the_update_tag_writes_its_keys_in_javas_hash_map_order() {
        let mut be = BlockEntity::new(Kind::Sign);
        let sign = be.sign_mut().expect("a sign");
        sign.set_allowed_player_editor(Some(7));
        sign.update_sign_text(7, true, &["hello".to_string()]);
        let tag = be.update_tag().expect("a sign has an update tag");
        let body = block_entity_data_body((1, 0, 0), 7, &tag).expect("body");

        // The keys appear as length-prefixed names in the payload, so the
        // byte offset of each name is its position in the iteration.
        let offset = |needle: &str| {
            body.windows(needle.len())
                .position(|w| w == needle.as_bytes())
                .unwrap_or_else(|| panic!("`{needle}` is not in the packet body at all"))
        };
        let outer = ["back_text", "is_waxed", "front_text"];
        let mut last = 0;
        for key in outer {
            let at = offset(key);
            assert!(
                at > last,
                "outer key order is not Java's: `{key}` at {at} follows {last}, \
                 expected {outer:?}"
            );
            last = at;
        }
        // The inner order is asserted inside `back_text`, which is the first
        // compound in the payload — so every one of its three keys precedes
        // `front_text` and the offsets cannot be read out of the other face.
        let front_at = offset("front_text");
        let inner = ["has_glowing_text", "color", "messages"];
        let mut last = 0;
        for key in inner {
            let at = offset(key);
            assert!(at < front_at, "`{key}` was read out of the wrong face");
            assert!(
                at > last,
                "inner key order is not Java's: expected {inner:?}"
            );
            last = at;
        }
        // And the leading fields, which the projection decodes rather than
        // digests: the packed position and then the type id as a varint.
        assert_eq!(
            &body[..8],
            &crate::interact::pack_pos((1, 0, 0)).to_be_bytes()
        );
        assert_eq!(body[8], 7, "the block-entity type id, not the block id");
        assert_eq!(body[9], 10, "TAG_Compound, with no root name after it");
        assert_eq!(
            body[10], 10,
            "the first key's own tag byte — an empty root *name* here would \
             be two zero bytes and the client would read this out of the middle \
             of a length"
        );
    }

    /// **The eleven new types' update tags, against bytes vanilla put on a
    /// socket.**
    ///
    /// Not a reading of `getUpdateTag`'s bytecode and not a re-derivation of
    /// it: `ci/block-entity-probe.sh` boots a real 26.2 server, places one of
    /// each, and prints the payload of every `block_entity_data` it sends
    /// under `RUSTMC_TRACE_RAW`. These are those bytes, recorded on the
    /// Threadripper on 2026-08-12, superflat creative, seed 20260803.
    ///
    /// The projection digests every byte after the type id, so equivalent is
    /// not good enough and a golden vector is the only honest test. Three
    /// things it catches that a shape assertion would not: a sorted writer
    /// (`Items` would precede `align_items_to_bottom` on the shelf), a
    /// suppressed empty list (`{}` instead of `{Items: []}` on the campfire),
    /// and a missing `components` map on the banner — the one type whose
    /// `getUpdateTag` goes through `saveWithoutMetadata` rather than
    /// `saveCustomOnly`.
    #[test]
    fn every_new_update_tag_is_byte_identical_to_what_vanilla_sent() {
        const EMPTY: &str = "0a00";
        let cases: [(Kind, i32, &str); 11] = [
            (Kind::CreakingHeart, 10, EMPTY),
            (Kind::Skull, 16, EMPTY),
            (Kind::Conduit, 25, EMPTY),
            (Kind::BrushableBlock, 40, EMPTY),
            (Kind::DecoratedPot, 41, EMPTY),
            (Kind::CopperGolemStatue, 47, EMPTY),
            (Kind::Beacon, 15, "0a0300064c6576656c730000000000"),
            (Kind::Banner, 20, "0a0a000a636f6d706f6e656e74730000"),
            (Kind::Campfire, 32, "0a0900054974656d73000000000000"),
            (
                Kind::Shelf,
                39,
                "0a010015616c69676e5f6974656d735f746f5f626f74746f6d\
                 000900054974656d73000000000000",
            ),
            // A hanging sign inherits `SignBlockEntity.getUpdateTag`, so its
            // payload is a fresh sign's — the same bytes under a different
            // type id, which is the whole of what 771 asserts.
            (
                Kind::HangingSign,
                8,
                "0a0a00096261636b5f746578740100106861735f676c6f77696e675f74657874\
                 00080005636f6c6f720005626c61636b0900086d657373616765730800000004\
                 00000000000000000001000869735f7761786564000a000a66726f6e745f7465\
                 78740100106861735f676c6f77696e675f7465787400080005636f6c6f720005\
                 626c61636b0900086d65737361676573080000000400000000000000000000",
            ),
        ];
        for (kind, type_id, want) in cases {
            let be = BlockEntity::new(kind);
            assert!(
                be.has_update_packet(),
                "{kind:?} is in this table and so must have an update packet"
            );
            let tag = be.update_tag().unwrap_or_else(|| panic!("{kind:?} tag"));
            let body = block_entity_data_body((1, 0, 0), type_id, &tag).expect("body");
            // 8 bytes of packed position, then a one-byte varint type id —
            // every id here is under 128 — and the rest is the NBT the
            // probe printed.
            assert_eq!(body[8], u8::try_from(type_id).expect("a one-byte id"));
            let got: String = body[9..].iter().map(|b| format!("{b:02x}")).collect();
            assert_eq!(
                got,
                want.replace(char::is_whitespace, ""),
                "{kind:?}: this server's update tag is not the bytes vanilla sent"
            );
        }
    }

    /// The other half of the same measurement, and the reason the table above
    /// is evidence rather than a list: **29 of the 49 registered types send
    /// nothing at all**, because `BlockEntity.getUpdatePacket()` is
    /// `aconst_null; areturn` and they do not override it. The probe placed a
    /// chest, a barrel, a hopper and a lectern in the same run as the eleven
    /// and recorded zero `block_entity_data` for all four.
    ///
    /// This server models two of the 29, and a `None` here is what keeps
    /// `090`, `470`, `480` and `490` at `match`.
    #[test]
    fn the_two_null_update_packet_kinds_send_nothing() {
        for quiet in [Kind::Chest, Kind::Furnace] {
            let be = BlockEntity::new(quiet);
            assert!(!be.has_update_packet(), "{quiet:?}");
            assert!(
                be.update_tag().is_none(),
                "{quiet:?} must have no update tag at all — a server that sent one \
                 would put a packet on the wire vanilla never sends"
            );
        }
    }

    /// Every block in the eleven new registrations resolves to its own kind,
    /// and to exactly one.
    ///
    /// The lists are transcribed from `BlockEntityTypes`' static initialiser,
    /// which is where a transcription error would live — and a block that
    /// resolved to two kinds, or to the wrong one, would put a well-formed
    /// packet carrying the wrong type id on the wire.
    #[test]
    fn each_new_block_resolves_to_exactly_one_kind() {
        let groups: [(Kind, &[&str]); 11] = [
            (Kind::HangingSign, &HANGING_SIGN_BLOCKS),
            (Kind::Banner, &BANNER_BLOCKS),
            (Kind::Skull, &SKULL_BLOCKS),
            (Kind::Shelf, &SHELF_BLOCKS),
            (Kind::Campfire, &CAMPFIRE_BLOCKS),
            (Kind::BrushableBlock, &BRUSHABLE_BLOCKS),
            (Kind::CopperGolemStatue, &COPPER_GOLEM_STATUE_BLOCKS),
            (Kind::Beacon, &["minecraft:beacon"]),
            (Kind::Conduit, &["minecraft:conduit"]),
            (Kind::CreakingHeart, &["minecraft:creaking_heart"]),
            (Kind::DecoratedPot, &["minecraft:decorated_pot"]),
        ];
        let mut seen: Vec<&str> = Vec::new();
        for (kind, blocks) in groups {
            for b in blocks {
                assert_eq!(for_state(b), Some(kind), "{b}");
                // With a property list too — every caller has a state string.
                assert_eq!(for_state(&format!("{b}[facing=north]")), Some(kind), "{b}");
                assert!(!seen.contains(b), "{b} appears in two registrations");
                seen.push(b);
            }
        }
        assert_eq!(seen.len(), 24 + 32 + 14 + 12 + 2 + 2 + 8 + 4);
        // And the twenty-four plain signs are still their own kind: the
        // hanging list is a separate registration under a separate type id,
        // and folding the two together is the mistake this guards.
        for b in SIGN_BLOCKS {
            assert_eq!(for_state(b), Some(Kind::Sign), "{b}");
            assert!(!seen.contains(&b), "{b} is in both sign registrations");
        }
    }

    /// A hanging sign saves under its **own** type id.
    ///
    /// `Sign::to_nbt` stamps `crate::sign::TYPE_ID`, which is right for the
    /// twenty-four sign blocks and wrong for the twenty-four hanging ones —
    /// a chunk carrying `minecraft:sign` at a hanging sign's position reloads
    /// as a block entity whose type does not match its block.
    #[test]
    fn a_hanging_sign_saves_and_reloads_as_a_hanging_sign() {
        let mut be = BlockEntity::new(Kind::HangingSign);
        let sign = be.sign_mut().expect("a hanging sign keeps sign text");
        sign.set_allowed_player_editor(Some(3));
        sign.update_sign_text(3, true, &["swinging".to_string()]);
        let nbt = be.to_nbt((2, 65, 2));
        assert_eq!(
            nbt.as_compound()
                .and_then(|c| c.get("id"))
                .and_then(Nbt::as_str),
            Some("minecraft:hanging_sign")
        );
        let (pos, back) = BlockEntity::from_nbt(&nbt).expect("reload");
        assert_eq!(pos, (2, 65, 2));
        assert_eq!(back.kind(), Kind::HangingSign);
        assert_eq!(back.update_tag(), be.update_tag());
    }

    /// `getContainerSize()` and the length of the block entity's own item
    /// list are different questions, and a campfire is where they part.
    ///
    /// `CampfireBlockEntity implements Clearable` and has no
    /// `getContainerSize` anywhere in its hierarchy, so a click into "slot 2
    /// of a campfire" must be refused — while `getUpdateTag`'s
    /// `saveAllItems` still iterates a four-long list.
    #[test]
    fn a_campfire_holds_four_stacks_and_is_not_a_container() {
        assert_eq!(Kind::Campfire.container_size(), 0);
        assert_eq!(Kind::Campfire.item_slots(), 4);
        assert_eq!(BlockEntity::new(Kind::Campfire).size(), 4);
        // The shelf is the opposite case and the control for it: a real
        // `ListBackedContainer`, so both answers are 3.
        assert_eq!(Kind::Shelf.container_size(), 3);
        assert_eq!(Kind::Shelf.item_slots(), 3);
        // And a decorated pot has neither, so `to_nbt` must not write an
        // `Items` key at it — an empty `Items` on a pot reads as a pot that
        // has been emptied rather than one that never held anything.
        let nbt = BlockEntity::new(Kind::DecoratedPot).to_nbt((0, 0, 0));
        assert!(!nbt.as_compound().expect("compound").contains_key("Items"));
    }

    /// The chunk-payload channel's tag bytes, for a type with no update tag
    /// at all — chest, furnace, and every other type `getUpdateTag` is not
    /// overridden for.
    ///
    /// `FriendlyByteBuf.writeNbt(ByteBuf, Tag)` rewrites a `null` argument to
    /// `EndTag.INSTANCE` before `NbtIo.writeAnyTag` runs, and `writeAnyTag`
    /// on a tag whose `getId()` is 0 writes that one byte and returns — no
    /// name, no payload. Read off the pinned jar with `javap -p -c
    /// -constants` on `FriendlyByteBuf` and `NbtIo`, not from memory.
    #[test]
    fn a_null_update_tag_is_one_zero_byte_on_the_chunk_payload_channel() {
        assert_eq!(chunk_payload_tag_bytes(None).expect("body"), vec![0u8]);
    }

    /// The chunk-payload channel's tag bytes for a type that **does** have
    /// one must be the identical nameless encoding `block_entity_data`
    /// sends — same bytes, because it is the same field read the same way,
    /// just reached through a different packet.
    #[test]
    fn a_present_update_tag_matches_the_nameless_bytes_block_entity_data_sends() {
        let mut be = BlockEntity::new(Kind::Sign);
        let sign = be.sign_mut().expect("a sign");
        sign.set_allowed_player_editor(Some(7));
        sign.update_sign_text(7, true, &["hi".to_string()]);
        let tag = be.update_tag().expect("a sign has an update tag");

        let chunk_bytes = chunk_payload_tag_bytes(Some(&tag)).expect("chunk-channel bytes");
        // `block_entity_data_body`'s own leading fields are a fixed-width
        // i64 position plus a one-byte varint for type id 7 — asserted right
        // above this test — so its tail is exactly the tag bytes with no
        // guessing about where they start.
        let data_body = block_entity_data_body((1, 0, 0), 7, &tag).expect("body");
        assert_eq!(&data_body[9..], chunk_bytes.as_slice());
    }

    /// The whole entry: `BlockEntityInfo.create`, offsets 17-33 for the
    /// position and the tag rule from the two tests above, assembled the
    /// way [`World::chunk`](crate::world::World::chunk) now calls this.
    ///
    /// **This is the test that fails if `world.rs` goes back to
    /// `block_entities: Vec::new()`** — not because it calls that function
    /// (a unit test cannot: `crate::protocol_ids::shared()` reads
    /// `data/vanilla/protocol_ids.json` relative to the process's working
    /// directory, `cargo test` runs this crate's tests from
    /// `crates/rustmc-server`, and `data/vanilla` is at the repo root — but
    /// because it pins the exact entry `World::chunk` is now responsible for
    /// producing, so a regression there is a mechanical diff against this
    /// fixture rather than a fresh derivation.
    #[test]
    fn a_chest_becomes_one_chunk_payload_entry_with_a_null_tag() {
        let be = BlockEntity::new(Kind::Chest);
        // x = 17 -> section-relative 1; z = -3 -> section-relative 13 in
        // two's complement, matching `SectionPos.sectionRelative`'s `& 15`.
        let entry = chunk_payload_entry((17, -60, -3), &be, 1).expect("entry");
        assert_eq!(entry.packed_xz, (1 << 4) | 13);
        assert_eq!(entry.y, -60);
        assert_eq!(
            entry.kind, 1,
            "the block-entity registry id, passed through"
        );
        assert_eq!(entry.nbt, vec![0u8], "a chest's getUpdateTag() is empty");
    }

    /// A sign's own entry is not what this branch closes — `chunk_at`'s
    /// wiring only needed a chest — but the tag half of the entry has to
    /// hold for every kind this server models, not just the one with a null
    /// tag, or the branch would be proven on the one case that cannot show a
    /// wiring bug in the `Some` arm.
    #[test]
    fn a_sign_becomes_one_chunk_payload_entry_with_its_tag() {
        let mut be = BlockEntity::new(Kind::Sign);
        let sign = be.sign_mut().expect("a sign");
        sign.set_allowed_player_editor(Some(7));
        sign.update_sign_text(7, true, &["hi".to_string()]);
        let entry = chunk_payload_entry((1, 64, 0), &be, 7).expect("entry");
        assert_eq!(entry.packed_xz, 1 << 4);
        assert_eq!(entry.y, 64);
        assert_eq!(entry.kind, 7);
        assert!(entry.nbt.len() > 1, "a sign's tag is not the null byte");
        assert_eq!(entry.nbt[0], 10, "TAG_Compound, with no name after it");
    }

    /// A sign's saved compound is `saveAdditional`'s three keys plus the
    /// metadata, and **no `Items`** — the key every other type here writes.
    #[test]
    fn a_saved_sign_carries_its_texts_and_no_items_key() {
        let mut be = BlockEntity::new(Kind::Sign);
        let sign = be.sign_mut().expect("a sign");
        sign.set_allowed_player_editor(Some(7));
        sign.update_sign_text(7, true, &["written".to_string()]);
        let nbt = be.to_nbt((1, 64, 1));
        let c = nbt.as_compound().expect("compound");
        assert_eq!(c.get("id").and_then(Nbt::as_str), Some("minecraft:sign"));
        assert!(
            !c.contains_key("Items"),
            "a sign is not a container and vanilla writes it no Items key"
        );
        let (pos, back) = BlockEntity::from_nbt(&nbt).expect("round trip");
        assert_eq!(pos, (1, 64, 1));
        assert_eq!(
            back.sign().map(|s| s.text(true).messages()[0].clone()),
            Some("written".to_string())
        );
        assert_eq!(back, be, "and it comes back the same block entity");
    }

    /// The NBT round trip, in this crate — the format half is
    /// `rustmc-world`'s and is tested there. This pins the **key names**,
    /// which is what a reader outside this tree depends on.
    #[test]
    fn the_saved_compound_uses_vanillas_own_key_names() {
        let mut be = BlockEntity::new(Kind::Chest);
        be.set_item(3, ItemStack::new("minecraft:stone", 7));
        let nbt = be.to_nbt((10, 64, -3));
        let c = nbt.as_compound().expect("compound");
        assert_eq!(c.get("id").and_then(Nbt::as_str), Some("minecraft:chest"));
        assert_eq!(c.get("x").and_then(Nbt::as_i32), Some(10));
        assert_eq!(c.get("y").and_then(Nbt::as_i32), Some(64));
        assert_eq!(c.get("z").and_then(Nbt::as_i32), Some(-3));
        let items = c.get("Items").and_then(Nbt::as_list).expect("Items");
        assert_eq!(items.len(), 1, "only the non-empty slot is written");
        let e = items[0].as_compound().expect("entry");
        // Capital `Slot`, lowercase `id` and `count` — one level apart.
        assert!(matches!(e.get("Slot"), Some(Nbt::Byte(3))));
        assert_eq!(e.get("id").and_then(Nbt::as_str), Some("minecraft:stone"));
        assert_eq!(e.get("count").and_then(Nbt::as_i32), Some(7));

        let (pos, back) = BlockEntity::from_nbt(&nbt).expect("round trip");
        assert_eq!(pos, (10, 64, -3));
        assert_eq!(back, be, "and it comes back the same block entity");
    }

    /// **An empty chest still writes an `Items` key**, and the reason is one
    /// `iconst_1` deep in vanilla.
    ///
    /// `ContainerHelper.saveAllItems` takes a boolean that decides whether an
    /// empty list is `discard`ed, and the two-argument overload — which is
    /// what `ChestBlockEntity.saveAdditional` calls — passes **true**, so the
    /// `discard` at offset 79 is skipped. Dropping the key would look tidier,
    /// round-trip through this server perfectly, and produce a chunk that
    /// differs from vanilla's for every empty chest in the world.
    #[test]
    fn an_empty_container_still_writes_its_items_key() {
        let nbt = BlockEntity::new(Kind::Chest).to_nbt((0, 0, 0));
        let c = nbt.as_compound().expect("compound");
        let items = c
            .get("Items")
            .and_then(Nbt::as_list)
            .expect("the Items key is present even with nothing in it");
        assert!(items.is_empty());
    }

    /// An unknown type is dropped rather than guessed, and a slot outside the
    /// container is dropped rather than clamped into slot 0.
    #[test]
    fn unknown_types_and_out_of_range_slots_are_dropped_not_guessed() {
        // **`minecraft:beehive` until 2026-09-05, and that is the point.**
        // This test's stand-in for "a type this server does not model" was a
        // real registry entry, so the commit that modelled the beehive turned
        // the test red — correctly, and loudly. The replacement is a name no
        // version of this registry has ever held, because now that `Kind`
        // covers all forty-nine, the only unknown type left is one from a
        // chunk written by a *later* version, which is the case the doc on
        // `Kind::from_id` names and the case this test is for.
        let mut c = BTreeMap::new();
        c.insert(
            "id".to_string(),
            Nbt::String("minecraft:a_type_from_a_later_version".into()),
        );
        c.insert("x".to_string(), Nbt::Int(0));
        c.insert("y".to_string(), Nbt::Int(0));
        c.insert("z".to_string(), Nbt::Int(0));
        assert!(
            BlockEntity::from_nbt(&Nbt::Compound(c)).is_none(),
            "a type this server does not model is not invented"
        );

        let mut e = BTreeMap::new();
        e.insert("Slot".to_string(), Nbt::Byte(99));
        e.insert("id".to_string(), Nbt::String("minecraft:stone".into()));
        e.insert("count".to_string(), Nbt::Int(1));
        let mut c = BTreeMap::new();
        c.insert("id".to_string(), Nbt::String("minecraft:chest".into()));
        c.insert("x".to_string(), Nbt::Int(0));
        c.insert("y".to_string(), Nbt::Int(0));
        c.insert("z".to_string(), Nbt::Int(0));
        c.insert("Items".to_string(), Nbt::List(vec![Nbt::Compound(e)]));
        let (_, be) = BlockEntity::from_nbt(&Nbt::Compound(c)).expect("a chest");
        assert!(
            be.is_empty(),
            "slot 99 of a 27-slot chest is dropped, not folded into slot 0"
        );
    }

    /// Eviction takes exactly the named chunks and nothing next door.
    ///
    /// The three positions are one chunk apart on each axis and one of them
    /// is negative, because `-1 >> 4` is `-1`: an implementation that
    /// bucketed with `/ 16` would put `(-1, 64, -1)` in chunk `(0, 0)` and
    /// evict the wrong tile's chest while leaving this one's behind.
    #[test]
    fn eviction_takes_the_named_chunks_and_leaves_their_neighbours() {
        let store = BlockEntities::new();
        for pos in [(-1, 64, -1), (0, 64, 0), (17, 64, 33)] {
            store.on_block_changed(pos, "minecraft:air", "minecraft:chest");
        }
        assert_eq!(store.len(), 3);
        assert_eq!(store.evict_chunks([(0, 0)]), 1);
        assert_eq!(store.len(), 2);
        assert!(store.get((0, 64, 0)).is_none(), "chunk (0,0) was evicted");
        assert!(
            store.get((-1, 64, -1)).is_some(),
            "chunk (-1,-1) is a different chunk and `-1 >> 4` is -1"
        );
        // An empty request is a no-op rather than a wipe, which is the
        // difference between "this tile had no chunks" and "every chunk".
        assert_eq!(store.evict_chunks(std::iter::empty()), 0);
        assert_eq!(store.len(), 2);
        // And the rest go when their chunks are named.
        assert_eq!(store.evict_chunks([(-1, -1), (1, 2)]), 2);
        assert!(store.is_empty());
    }

    /// `in_chunk` and `chunks` have to agree, and both have to get negative
    /// coordinates right — `-1 >> 4` is `-1`, not `0`, and an arithmetic
    /// shift is the whole reason this is not a division.
    #[test]
    fn chunk_bucketing_is_an_arithmetic_shift_and_survives_negatives() {
        let store = BlockEntities::new();
        store.on_block_changed((-1, 64, -1), "minecraft:air", "minecraft:chest");
        store.on_block_changed((0, 64, 0), "minecraft:air", "minecraft:chest");
        store.on_block_changed((17, 64, 33), "minecraft:air", "minecraft:chest");
        assert_eq!(
            store.chunks(),
            [(-1, -1), (0, 0), (1, 2)].into_iter().collect()
        );
        assert_eq!(store.in_chunk(-1, -1).len(), 1);
        assert_eq!(store.in_chunk(-1, -1)[0].0, (-1, 64, -1));
        assert_eq!(store.in_chunk(0, 0).len(), 1);
        assert!(store.in_chunk(5, 5).is_empty());
    }
}
