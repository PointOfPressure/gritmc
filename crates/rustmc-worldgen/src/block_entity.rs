//! The block entities a freshly generated chunk carries.
//!
//! Reference (26.2, `javap -p -c -constants` on the pinned server jar — no
//! other implementation consulted, CLAUDE.md rule 4):
//! `net.minecraft.world.level.block.entity.BlockEntity`,
//! `PotentSulfurBlockEntity`, `PotentSulfurBlock`.
//!
//! # Why this is a post-pass over finished block states, and why that is
//! exact rather than an approximation
//!
//! In vanilla a block entity is not written by the feature that places the
//! block. `PotentSulfurBlock extends BaseEntityBlock`, and
//! `LevelChunk.setBlockState` creates the entity for **any** `EntityBlock`
//! state it stores and drops it again when that position is overwritten. The
//! net effect at save time is a statement about the *final* block states and
//! nothing else: **every position holding an entity-block has exactly one
//! block entity, and no other position has one.**
//!
//! So deriving them from the finished canvas is not "close enough" — it is
//! the same function vanilla computes, evaluated at the same point. It is
//! also the only construction that cannot perturb generation: this crate's
//! canvas is already final when this runs, so no draw is taken, no order is
//! observed and no block state can move. That property is worth more here
//! than elegance, because the standing requirement on this work is that every
//! published block-state figure stays byte-identical.
//!
//! **Measured rather than assumed.** Over six independently generated vanilla
//! references on the seed-12345 r=16 window, the count of `potent_sulfur`
//! blocks equals the count of `minecraft:potent_sulfur` block entities in
//! every one: 32/32, 32/32, 32/32, 32/32, 32/32, 30/30. If the entity were
//! conditional on anything but the block being there, those would come apart.
//! `docs/DETERMINISM.md` §40.
//!
//! # Which types are here, which are not, and what each absence is waiting on
//!
//! **Three of the ten classes are written as of 2026-08-17** — `potent_sulfur`
//! from the block state alone, and `mob_spawner`'s two sources through the side
//! channel: the dungeon's drawn mob and the mineshaft's constant cave spider.
//! With `potent_sulfur` that is 72 of the census window's 173 positions.
//!
//! **The other 101 are not waiting on a class, they are waiting on a draw.**
//! Every one of them is a container whose template supplies a `LootTable` and
//! whose placer never spends the `nextLong` the save writes beside it, because
//! `structure::place::place_piece` is called with `random: None` at four of its
//! five call sites — trial chambers (89), shipwreck (1), buried treasure (1) —
//! or because the draw is deliberately not modelled at all, as in
//! `structure::ocean_ruin`'s data-marker chest and its suspicious sand (8, and
//! `ruined_portal`'s single chest is the one that does thread its stream). A
//! `LootTable` with no `LootTableSeed` is refused by [`from_sections_recorded`]
//! and counted as [`EmitStats::incomplete`], because it would be a payload
//! wrong on one key where absence is merely a payload not yet reproduced.
//! Threading those streams is a change to the DRAW ORDER, and whether that is
//! also a change to BLOCK STATES turned out to be a question the pinned jar
//! answers rather than an assumption: in the jigsaw chain the only consumer of
//! that stream downstream of the containers is `FeaturePoolElement.place`, and
//! this port refuses feature pool elements out loud. `docs/DETERMINISM.md` §44
//! has the reading and the measurement; `RUSTMC_JIGSAW_STREAM` is the knob and
//! is unset by default, so the count above is still what an unflagged dump
//! reports.
//!
//! `docs/DETERMINISM.md` §40 is the census this was chosen from. On that
//! window vanilla writes 173 block entities of ten types, and they divide by
//! what it costs to reproduce them:
//!
//! * **A constant.** `potent_sulfur` — implemented here.
//!   `PotentSulfurBlockEntity.<init>` sets `waitingCountdown = -1` and
//!   `saveAdditional` writes exactly one key, `countdown`. All 32 in the
//!   census window are byte-identical modulo position.
//! * **An unresolved loot reference plus a seed.** `chest`, `dispenser`,
//!   `decorated_pot`, `brushable_block` save `LootTable` and
//!   `LootTableSeed` and **no item list**, so these need the table id the
//!   feature already knows and one `nextLong` at the right point in a draw
//!   sequence. **That `nextLong` is taken inside the placer** —
//!   `RandomizableContainer.setBlockEntityLootTable` calls
//!   `RandomSource.nextLong()` and hands it to `setLootTable(key, seed)` — so
//!   it is exactly as unreachable from `(name, x, y, z)` as a spawner's mob
//!   is, and no more tractable. This bullet used to end "tractable, and
//!   strictly a draw-order problem"; that is true of getting the value RIGHT
//!   and false of getting it at all from this signature.
//!   `docs/DETERMINISM.md` §42.2.
//! * **~~Resolved contents.~~ REFUTED 2026-08-17 — there are none.** This
//!   said `barrel` (5 of 10) and `hopper` (all 5) carry a real `Items` list
//!   and were "genuinely expensive". Read raw, the two barrel groups are
//!   **disjoint** — 5 are `{Items:[]}` with no loot table, 5 carry
//!   `LootTable`+`LootTableSeed` and no `Items` — and every hopper is
//!   `{Items:[],TransferCooldown:0}`. **Every `Items` list in the window is
//!   empty.** Nothing here is a resolved-contents port. §42.3.
//! * **A resolved choice this repo already makes — LANDED 2026-08-17.**
//!   `mob_spawner` stores `SpawnData:{entity:{id:...}}` and eight constants,
//!   and it has two sources on this window that no block state distinguishes:
//!   thirteen `<feature>` dungeons, whose mob is the `nextInt(4)`
//!   [`crate::feature::monster_room`] already drew, and six mineshaft
//!   corridors, whose mob is the constant `cave_spider`. Both now record
//!   through the side channel. Corroborated positionally: of the nineteen
//!   entries in the reference, exactly the six the census attributes to
//!   `minecraft:mineshaft` carry `cave_spider`, and the thirteen dungeon ones
//!   split 7 zombie / 5 skeleton / 1 spider — which is `{SKELETON, ZOMBIE,
//!   ZOMBIE, SPIDER}` and not a three-name list. §42.4.
//! * **A trap that data alone cannot see: `vault`.** All 13 in the window
//!   split 4/9 between `ominous=true` and `ominous=false` and their payloads
//!   split 4/9 the same way, which reads as a pure function of the block
//!   state. It is not. `VaultBlockEntity.<init>` sets `config` to
//!   `VaultConfig.DEFAULT` unconditionally and never reads
//!   `VaultBlock.OMINOUS`; the template places the ominous state and the
//!   ominous config together. Keying this file on `ominous=` would pass every
//!   test this window can run and be wrong the first time the pair is placed
//!   apart. §42.3.
//!
//! **Absence from this table still means "not yet reproduced", never "needs no
//! block entity".** A reader who takes it the other way would conclude the
//! chests in a generated world are correct, which they are not: they have no
//! contents at all. That is the whole subject of `docs/PARITY.md` §6 and it
//! remains open.

use rustmc_world::anvil::Section;
use rustmc_world::nbt::Nbt;
use std::collections::BTreeMap;

/// What a placer knew at a position that the finished block state does not say.
///
/// # Why this is one type rather than nine
///
/// `docs/DETERMINISM.md` §42 measured every block-entity payload on the
/// quotable window and found that **one of the ten classes is a function of
/// the finished block state and it is the one [`for_block`] already does**;
/// the other 141 of 173 positions are placer-written. That reads like nine
/// separate ports and is not, because of where the payloads come from:
/// vanilla's template format stores the block entity's own tag beside the
/// block, `StructureTemplate.placeInWorld` hands it to
/// `BlockEntity.loadWithComponents`, and the save writes it back. Read out of
/// the trial-chambers templates, the stored tags are exactly the payloads the
/// references save — `LootTable`, `sherds`, `config`, `normal_config` /
/// `ominous_config`, `Items`, `TransferCooldown` — so **carrying them needs no
/// per-class vocabulary at all.** What is per-class is only the completion the
/// save applies on top, which is [`complete`], and that is a small `match`
/// rather than nine ports.
///
/// Two things a template cannot supply, and they are the reason this is a
/// struct and not a bare map:
///
/// * **`loot_seed`** — `RandomizableContainer.setBlockEntityLootTable` spends
///   a `nextLong` at placement time, so the value belongs to the placer's
///   stream and not to the template. It is kept separate from `nbt` because it
///   is the one part of a payload this crate *draws* rather than copies, and a
///   payload whose provenance is mixed is a payload nobody can audit.
/// * **a code-side placer's own tag** — `MonsterRoomFeature` has no template.
///   It builds `SpawnData` from a draw and puts it straight in `nbt`.
// `Eq` is deliberately absent: `Nbt` holds floats, so it is `PartialEq` and
// cannot be `Eq`. Nothing here needs a total equality — the tests compare
// payloads and the emitter never keys a map on a record.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Record {
    /// The payload keys the placer had, in the shape the save writes them.
    ///
    /// Never the metadata keys — `id`, `keepPacked`, `x`, `y`, `z` are the
    /// emitter's, not a placer's, and a placer that set one would be able to
    /// move a position or rename a type from inside a feature. `components`
    /// is the exception: `loadWithComponents` reads it off a template's tag
    /// and the save writes it back, so a template-carried compound is used
    /// (an outpost's ominous banner) and anything else saves the empty map.
    pub nbt: BTreeMap<String, Nbt>,
    /// The `nextLong` `RandomizableContainer.setBlockEntityLootTable` spent
    /// here, when the placer spent one.
    ///
    /// Recorded even where it is not saved. `trySaveLootTable` writes
    /// `LootTableSeed` only `if (getLootTableSeed() != 0L)` and only inside
    /// the branch where a loot table is set — `lconst_0; lcmp; ifeq` in the
    /// 26.2 bytecode — and the five bare barrels on the census window are the
    /// case that proves it: `placeInWorld` spends the draw on them, because
    /// they are `RandomizableContainer`s carrying an `nbt`, and their saved
    /// tag is `{Items:[]}` with no seed in it. Recording the draw where it
    /// happens and deciding what to save at save time keeps that distinction
    /// where vanilla has it.
    pub loot_seed: Option<i64>,
}

/// Every [`Record`] a generation pass made, keyed by chunk.
///
/// The same shape as [`crate::canvas::PostProcessingMarks`] and for the same
/// reasons: keyed by chunk so the store is sparse and so a detached
/// decoration group can hand its own back, and a `Vec` in insertion order
/// rather than a map because **a position can be recorded twice and the last
/// one wins**. `LevelChunk.setBlockState` drops the block entity at a position
/// it overwrites and creates the new state's, so a chest placed over a spawner
/// keeps the chest's payload; folding in insertion order reproduces that
/// without the channel having to watch for overwrites.
pub type Records = BTreeMap<(i32, i32), Vec<((i32, i32, i32), Record)>>;

/// What every emit so far has done with the records it was given, summed.
///
/// **The counts in [`EmitStats`] are only a falsifier if somebody reads them,
/// and until this existed nobody did** — `column_from_canvas` took the `Vec`
/// and dropped the stats on the floor. A count that is computed and discarded
/// is exactly the shape of check this project keeps paying for: it looks like
/// evidence in the source and cannot go red anywhere.
///
/// Atomics rather than a thread-local because columns are built on the worker
/// pool: a per-thread counter would report whichever fraction of the world one
/// thread happened to write. Relaxed ordering is enough — these are counters
/// read once at the end, not a synchronisation mechanism.
mod totals {
    use std::sync::atomic::{AtomicU64, Ordering};

    pub static RECORDED: AtomicU64 = AtomicU64::new(0);
    pub static DERIVED: AtomicU64 = AtomicU64::new(0);
    pub static ORPHANED: AtomicU64 = AtomicU64::new(0);
    pub static INCOMPLETE: AtomicU64 = AtomicU64::new(0);

    pub fn add(stats: &super::EmitStats) {
        RECORDED.fetch_add(stats.recorded as u64, Ordering::Relaxed);
        DERIVED.fetch_add(stats.derived as u64, Ordering::Relaxed);
        ORPHANED.fetch_add(stats.orphaned as u64, Ordering::Relaxed);
        INCOMPLETE.fetch_add(stats.incomplete as u64, Ordering::Relaxed);
    }

    pub fn take() -> super::EmitStats {
        super::EmitStats {
            recorded: RECORDED.swap(0, Ordering::Relaxed) as usize,
            derived: DERIVED.swap(0, Ordering::Relaxed) as usize,
            orphaned: ORPHANED.swap(0, Ordering::Relaxed) as usize,
            incomplete: INCOMPLETE.swap(0, Ordering::Relaxed) as usize,
        }
    }
}

/// Every emit's stats since the last call, and reset.
///
/// Meant for a driver to print at the end of a run — see `worldgen-dump`. The
/// two drop counts are the falsifier `docs/DETERMINISM.md` §42.5 asks for:
/// `orphaned` is the channel working (a position whose block was replaced), and
/// `incomplete` is a payload this crate cannot finish because a draw was never
/// spent. A run that emits nothing and drops nothing has a channel that is off;
/// a run that drops everything has one that is wired to the wrong positions.
/// Neither is distinguishable from the other, or from a healthy run, without
/// these four numbers.
#[must_use]
pub fn take_emit_totals() -> EmitStats {
    totals::take()
}

/// The producer end of the side channel: a world a placer can tell what it
/// knew.
///
/// # Why a supertrait and not a default method
///
/// Every feature in this crate writes through a per-feature world trait, and
/// nearly all of them already share `PredicateWorld`. Hanging a defaulted
/// no-op there would have been one line and would have been the wrong shape:
/// a world type that then forgot to forward it would drop every payload its
/// features recorded, and **the loss moves no block state**, so nothing in
/// this repo could see it. That is this project's central defect class.
///
/// So it is a required method on a trait that only the traits whose features
/// actually record extend. A world that can place a container has to say what
/// it does with the payload, and a test double that says "nothing" says so at
/// its own site rather than by inheriting silence.
///
/// # Write-only, and that is the whole safety argument
///
/// There is no reader here on purpose. The channel carries data *out* of
/// generation; if a feature could read it back, generation would depend on
/// whether recording was on and the identity gate in
/// `ci/be-channel-identical.sh` would be the only thing standing between that
/// and a silently order-dependent world. One method, one direction.
pub trait BlockEntityRecorder {
    /// Record what this placer knew at a position.
    ///
    /// Called unconditionally by the placer, whether or not anything is
    /// recording — see [`crate::canvas::Canvas::record_block_entity`] for why
    /// the `if` belongs at the far end and not here.
    fn record_block_entity(&mut self, x: i32, y: i32, z: i32, record: Record);
}

/// The block-entity type id a block's entity saves itself under, when the
/// block has one this crate can write.
///
/// **The block name and the type id are not the same string, and two of the
/// four here differ.** Read out of `BlockEntityTypes`' static initialiser in
/// the pinned jar, where each `register(BlockEntityTypeIds.X, ..., Block...)`
/// call names its type key and its `validBlocks` array:
/// `MOB_SPAWNER` is registered with `Blocks.SPAWNER` alone, and
/// `BRUSHABLE_BLOCK` with `Blocks.SUSPICIOUS_SAND` and
/// `Blocks.SUSPICIOUS_GRAVEL` — so the block `minecraft:spawner` saves as
/// `minecraft:mob_spawner`, and both suspicious blocks save as
/// `minecraft:brushable_block`. Keying the emitter on the block name would
/// have written two type ids that do not exist and lost two classes to a
/// mismatch that reads as a missing position.
///
/// Only the blocks a placer in this crate can record are listed. A block
/// absent from here has its record **dropped**, which is what makes this
/// function the invalidation rule as well as the table: see
/// [`from_sections`].
#[must_use]
pub fn entity_id(block: &str) -> Option<&'static str> {
    Some(match block {
        "minecraft:potent_sulfur" => "minecraft:potent_sulfur",
        "minecraft:spawner" => "minecraft:mob_spawner",
        "minecraft:suspicious_sand" | "minecraft:suspicious_gravel" => "minecraft:brushable_block",
        "minecraft:chest" => "minecraft:chest",
        "minecraft:barrel" => "minecraft:barrel",
        "minecraft:dispenser" => "minecraft:dispenser",
        "minecraft:hopper" => "minecraft:hopper",
        "minecraft:decorated_pot" => "minecraft:decorated_pot",
        "minecraft:trial_spawner" => "minecraft:trial_spawner",
        "minecraft:vault" => "minecraft:vault",
        // `BlockEntityTypes.BANNER` is registered with all thirty-two banner
        // blocks — sixteen colours, standing and wall — and saves as
        // `minecraft:banner` whichever it is. End cities and woodland
        // mansions place them out of their templates.
        b if is_banner(b) => "minecraft:banner",
        // An end city's `third_floor_2` carries one.
        "minecraft:ender_chest" => "minecraft:ender_chest",
        // An end city ship carries one of each: two splash-free potions of
        // strong healing in the stand, and a dragon head on the bow.
        "minecraft:brewing_stand" => "minecraft:brewing_stand",
        b if is_skull(b) => "minecraft:skull",
        // Template-carried, each saving back what its tag loaded: igloos
        // (furnace, sign), trail ruins (furnace, blast furnace, campfire),
        // ancient cities (campfire, comparator, lectern).
        "minecraft:furnace" => "minecraft:furnace",
        "minecraft:blast_furnace" => "minecraft:blast_furnace",
        "minecraft:smoker" => "minecraft:smoker",
        "minecraft:campfire" | "minecraft:soul_campfire" => "minecraft:campfire",
        "minecraft:comparator" => "minecraft:comparator",
        "minecraft:lectern" => "minecraft:lectern",
        // Derived from the block alone; see `for_block`.
        "minecraft:sculk_sensor" => "minecraft:sculk_sensor",
        "minecraft:sculk_shrieker" => "minecraft:sculk_shrieker",
        "minecraft:sculk_catalyst" => "minecraft:sculk_catalyst",
        "minecraft:end_gateway" => "minecraft:end_gateway",
        b if b.ends_with("_sign") && !b.ends_with("_hanging_sign") => "minecraft:sign",
        _ => return None,
    })
}

/// One of the thirty-two blocks `BlockEntityTypes.BANNER` is registered with.
fn is_banner(block: &str) -> bool {
    const COLOURS: [&str; 16] = [
        "white",
        "orange",
        "magenta",
        "light_blue",
        "yellow",
        "lime",
        "pink",
        "gray",
        "light_gray",
        "cyan",
        "purple",
        "blue",
        "brown",
        "green",
        "red",
        "black",
    ];
    let Some(rest) = block.strip_prefix("minecraft:") else {
        return false;
    };
    let Some(colour) = rest
        .strip_suffix("_wall_banner")
        .or_else(|| rest.strip_suffix("_banner"))
    else {
        return false;
    };
    COLOURS.contains(&colour)
}

/// A `SignText` as `SignText.DIRECT_CODEC` writes it back.
///
/// A template stores each line as a component compound, `{text:"<----"}`;
/// `ComponentSerialization.CODEC` writes a plain literal with no style and no
/// siblings as the bare string, which is what the igloo's saved sign holds.
/// `filtered_messages` is written only when it differs from `messages`, and
/// the template's matches.
fn sign_text(text: &mut BTreeMap<String, Nbt>) {
    let plain = |line: &Nbt| match line {
        Nbt::Compound(c) if c.len() == 1 => match c.get("text") {
            Some(Nbt::String(s)) => Nbt::String(s.clone()),
            _ => line.clone(),
        },
        other => other.clone(),
    };
    for key in ["messages", "filtered_messages"] {
        if let Some(Nbt::List(lines)) = text.get_mut(key) {
            *lines = lines.iter().map(plain).collect();
        }
    }
    if text.get("filtered_messages").is_some()
        && text.get("filtered_messages") == text.get("messages")
    {
        text.remove("filtered_messages");
    }
}

/// One of the fourteen `SkullBlock`/`WallSkullBlock` blocks, all registered
/// with `BlockEntityTypes.SKULL`.
fn is_skull(block: &str) -> bool {
    matches!(
        block,
        "minecraft:skeleton_skull"
            | "minecraft:skeleton_wall_skull"
            | "minecraft:wither_skeleton_skull"
            | "minecraft:wither_skeleton_wall_skull"
            | "minecraft:zombie_head"
            | "minecraft:zombie_wall_head"
            | "minecraft:player_head"
            | "minecraft:player_wall_head"
            | "minecraft:creeper_head"
            | "minecraft:creeper_wall_head"
            | "minecraft:dragon_head"
            | "minecraft:dragon_wall_head"
            | "minecraft:piglin_head"
            | "minecraft:piglin_wall_head"
    )
}

/// The classes whose save is **only** metadata when nothing set a key, so a
/// record that carried nothing still has a payload to emit.
///
/// `BannerBlockEntity.saveAdditional` writes `patterns` only when they are not
/// `BannerPatternLayers.EMPTY` and `CustomName` only when one is set, so a
/// plain banner saves `{components, id, keepPacked, x, y, z}` and nothing else
/// — a woodland mansion's are exactly that. `EnderChestBlockEntity` has no
/// `saveAdditional` of its own at all. For every other class an empty record
/// is a placer that knew nothing, and [`for_recorded`] refuses it.
fn metadata_only_is_a_payload(id: &str) -> bool {
    // `SkullBlockEntity.saveAdditional` is three `storeNullable`s —
    // `profile`, `note_block_sound`, `custom_name` — none set on a ship's
    // dragon head.
    // `LecternBlockEntity.saveAdditional` writes `Book` and `Page` only when
    // it holds a book; an ancient city's lectern holds none.
    matches!(
        id,
        "minecraft:banner" | "minecraft:ender_chest" | "minecraft:skull" | "minecraft:lectern"
    )
}

/// The keys a class's save writes that came from **the placer**, and no others.
///
/// # Why an allowlist exists at all
///
/// A template stores a block entity's tag beside the block, and vanilla hands
/// the **whole** compound to `loadWithComponents`: the block entity's own
/// `loadAdditional` reads the keys it understands and silently drops the rest,
/// and then `saveAdditional` writes back only what it loaded. So the template
/// tag is not the saved tag, and a port that carries the tag through
/// unfiltered emits keys vanilla does not write — `id`, a jigsaw block's
/// `pool`/`target`/`final_state`, a `metadata` marker.
///
/// That is a payload **wrong on one key**, which this file's own rule ranks
/// below an absence, and it is the exact failure `docs/DETERMINISM.md` §44.8
/// measured from the other direction when eight decorated pots were right on
/// the loot keys and missing `sherds`.
///
/// So the parser carries the compound whole (`TemplateBlock::payload`, and
/// the reasoning is on that field) and **this is the one place that decides
/// what survives**. A class with no entry here keeps nothing, which makes the
/// default an absence rather than a guess.
fn saved_template_keys(id: &str) -> &'static [&'static str] {
    match id {
        // `trySaveLootTable` writes `LootTable`, and `saveAllItems` writes
        // `Items`; a template supplies either, never both, and §42.3 measured
        // the two barrel groups as disjoint.
        // `Items` is allowed through for the two classes that are measured to
        // have bare instances and withheld from the two that are not, on the
        // same scope limit as the synthesis below: all thirty chests and all
        // nineteen dispensers on the census window save a `LootTable`, and a
        // chest whose tag names none is one whose table arrives from a data
        // marker AFTER the template is placed. Letting its template `Items`
        // through scored it as a payload mismatch against vanilla's
        // `LootTable`; withholding it leaves an absence, which is the better
        // of the two answers and the honest one.
        "minecraft:barrel" | "minecraft:hopper" => &["LootTable", "Items"],
        // `Items` is let through for these two as well, and [`for_recorded`]
        // then drops it again when the list is EMPTY. A template chest whose
        // `Items` is empty is the ambiguous one — the marker chest of an end
        // city or a ship whose table arrives after the template is placed —
        // while a template chest that names items is data vanilla keeps:
        // `loadWithComponents` reads it, nothing sets a table on it, and
        // `saveAllItems` writes it back. Woodland mansions have them (a chest
        // of alliums, a chest of dark oak saplings).
        "minecraft:chest" | "minecraft:dispenser" => &["LootTable", "Items"],
        // `BannerBlockEntity.saveAdditional`: `patterns` when not
        // `BannerPatternLayers.EMPTY`, then `storeNullable("CustomName")`.
        "minecraft:banner" => &["patterns", "CustomName"],
        // `BrewingStandBlockEntity.saveAdditional`: `super` (`lock`,
        // `CustomName`), `putShort("BrewTime")`, `saveAllItems` (the
        // two-argument form, so `Items` always), `putByte("Fuel")`. The
        // template tag carries all three with the save's own types.
        "minecraft:brewing_stand" => &["Items", "BrewTime", "Fuel", "CustomName", "lock"],
        "minecraft:skull" => &["profile", "note_block_sound", "custom_name"],
        // `AbstractFurnaceBlockEntity.saveAdditional`: `super` (`lock`,
        // `CustomName`), the four shorts, `saveAllItems`, `RecipesUsed`.
        "minecraft:furnace" | "minecraft:blast_furnace" | "minecraft:smoker" => &[
            "lit_time_remaining",
            "cooking_time_spent",
            "cooking_total_time",
            "lit_total_time",
            "Items",
            "RecipesUsed",
            "CustomName",
            "lock",
        ],
        // `CampfireBlockEntity.saveAdditional`: `Items` (always), then the two
        // int arrays.
        "minecraft:campfire" => &["Items", "CookingTimes", "CookingTotalTimes"],
        "minecraft:comparator" => &["OutputSignal"],
        "minecraft:lectern" => &["Book", "Page"],
        // `SignBlockEntity.saveAdditional`: `front_text`, `back_text`,
        // `is_waxed`; the two texts are normalised in `complete`.
        "minecraft:sign" => &["front_text", "back_text", "is_waxed"],
        // `DecoratedPotBlockEntity.saveAdditional`: `sherds` when the
        // decorations are not `PotDecorations.EMPTY`, then `item` when the
        // stack is not empty, beside the loot keys.
        "minecraft:decorated_pot" => &["LootTable", "sherds", "item"],
        // `BrushableBlockEntity.saveAdditional`: `super`, then
        // `if (!trySaveLootTable(out) && !item.isEmpty()) store("item", ...)`.
        //
        // **`LootTableSeed` is in this list and in no other, and that is the
        // measurement rather than a guess.** A `BrushableBlockEntity` is NOT a
        // `RandomizableContainer` (§44.7), so `placeInWorld` spends no
        // `nextLong` on it and `Record::loot_seed` is `None` for one — yet all
        // seven on the census window save a `LootTableSeed:long`. The seed
        // therefore arrives the only other way a value can: as a key of the
        // template's own tag, loaded by `loadWithComponents`. Every other
        // class here takes its seed from the draw, which is why letting the
        // key through for them would be reading a stale template value over a
        // drawn one.
        //
        // `hit_direction` is written only after something has brushed the
        // block, which nothing has during generation.
        "minecraft:brushable_block" => &["LootTable", "LootTableSeed", "item"],
        // `VaultBlockEntity` loads its `config` from the tag; `server_data`
        // and `shared_data` are the save's own and are added by `complete`.
        // **NOT keyed on `ominous=`** — §42.3 killed that and it stays killed:
        // `VaultBlockEntity.<init>` sets `VaultConfig.DEFAULT` unconditionally
        // and never reads `VaultBlock.OMINOUS`, so the 13-of-13 correlation is
        // placement and not a rule.
        "minecraft:vault" => &["config"],
        // `TrialSpawner.store` writes the packed state data and then
        // `TrialSpawner$FullConfig`, which is these two keys. A spawner
        // nothing has triggered packs no state.
        "minecraft:trial_spawner" => &["normal_config", "ominous_config"],
        // The mob is a draw, not a template key, but it arrives through the
        // same record — `feature::monster_room` and the mineshaft both write
        // it. The seven constant shorts are the save's own, in `complete`.
        "minecraft:mob_spawner" => &["SpawnData"],
        // `EndGatewayFeature` sets the exit through `setExitPosition(pos,
        // exact)`; both arrive in the record. `Age` is the save's own.
        "minecraft:end_gateway" => &["exit_portal", "ExactTeleport"],
        _ => &[],
    }
}

/// The keys a type's own `saveAdditional` contributes beyond what a placer
/// recorded, applied to a payload on its way out.
///
/// This is where the nine classes differ and it is the whole of where they
/// differ. Everything else about a recorded payload is copied.
fn complete(id: &str, m: &mut BTreeMap<String, Nbt>) {
    match id {
        // `TheEndGatewayBlockEntity.saveAdditional`: `Age` as a long. A
        // gateway placed during generation has never ticked.
        "minecraft:end_gateway" => {
            m.entry("Age".to_string()).or_insert(Nbt::Long(0));
        }
        "minecraft:mob_spawner" => {
            // `BaseSpawner`'s field initialisers, which a spawner placed a
            // moment ago has never moved: `spawnDelay = 20`,
            // `minSpawnDelay = 200`, `maxSpawnDelay = 800`, `spawnCount = 4`,
            // `maxNearbyEntities = 6`, `requiredPlayerRange = 16`,
            // `spawnRange = 4`. Read off all nineteen `mob_spawner` entries in
            // `gen-wide-12345-r16-ow-c`, which agree on every one of them.
            //
            // **SHORTS, and this said `Int` for one commit.** The fields are
            // `int` in memory and `BaseSpawner.save` writes every one of the
            // seven with `ValueOutput.putShort` — seven `invokeinterface
            // putShort:(Ljava/lang/String;S)V` in a row in the pinned jar,
            // against `storeNullable` for `SpawnData` and `store` for
            // `SpawnPotentials`. The wrong type was caught by `region-differ
            // bediff`, which compares typed NBT: all nineteen positions came
            // back as a payload mismatch on exactly these seven keys and on no
            // other, with `SpawnData` matching.
            //
            // Worth the paragraph for HOW the wrong type got here: it was read
            // off a JSON rendering of the reference's NBT, and a JSON rendering
            // prints a short and an int identically. The bytecode was the only
            // source that could tell them apart, so this repo's rule that
            // every line comes from the bytecode is not a formality — a value
            // read off a rendering is a value read through an instrument that
            // has already thrown the answer away.
            m.entry("Delay".into()).or_insert(Nbt::Short(20));
            m.entry("MinSpawnDelay".into()).or_insert(Nbt::Short(200));
            m.entry("MaxSpawnDelay".into()).or_insert(Nbt::Short(800));
            m.entry("SpawnCount".into()).or_insert(Nbt::Short(4));
            m.entry("MaxNearbyEntities".into()).or_insert(Nbt::Short(6));
            m.entry("RequiredPlayerRange".into())
                .or_insert(Nbt::Short(16));
            m.entry("SpawnRange".into()).or_insert(Nbt::Short(4));
            // `spawnPotentials` is a `WeightedList` that saves as an empty
            // list when nothing built it, which is every spawner worldgen
            // places. Absent from the record and present in the save.
            m.entry("SpawnPotentials".into())
                .or_insert(Nbt::List(Vec::new()));
        }
        // The four `RandomizableContainer`s share ONE save, and that is why
        // the nine classes are one item rather than nine ports.
        // `Chest`/`Barrel`/`Dispenser`/`HopperBlockEntity.saveAdditional` are
        // the same three instructions in the 26.2 jar:
        //
        //     super.saveAdditional(out)
        //     if (!trySaveLootTable(out)) ContainerHelper.saveAllItems(out, items)
        //
        // **`Items:[]` is written rather than discarded, and that is the half
        // that had to come from the bytecode.** `saveAllItems(out, items)`
        // delegates to the three-argument form with `iconst_1`, and that form
        // ends `if (list.isEmpty() && !alwaysWrite) out.discard("Items")` —
        // `ifne` over the discard when the flag is set. So an all-empty
        // container saves the empty list, which is what all five bare barrels
        // and all five hoppers on the census window carry.
        //
        // This is the arm §42.1's "do not emit a constant payload for a
        // `hopper` or a bare `barrel`" null was waiting for. That null is not
        // overturned: emitting `{Items:[]}` for a container that *might* hold
        // loot is still wrong. What makes it right here is that the record
        // exists — the channel saw the placement and the template's tag named
        // no `LootTable` — so this is no longer a guess from uniformity.
        // **And a vault with no `config` in its tag saves an EMPTY one, not
        // no key.** Measured 2026-08-18: emitting the two empty maps without a
        // `config` scored nine of thirteen as a payload mismatch on exactly
        // that key, which is what sent this back to the jar.
        //
        // `VaultBlockEntity.<init>` sets `config = VaultConfig.DEFAULT`
        // unconditionally (§42.3), so every vault has one to save. What the
        // save writes for it is decided by the codec: all five of
        // `VaultConfig`'s fields — `loot_table`, `activation_range`,
        // `deactivation_range`, `key_item`, `override_loot_table_to_display`
        // — are built with `lenientOptionalFieldOf`, which OMITS a field on
        // encode when it equals the default it was given. A `DEFAULT` config
        // therefore encodes to `{}`: the key is present and the map inside it
        // is empty.
        //
        // That is exactly what the census shows and what a count alone could
        // not have explained — `config:compound=13` on all thirteen, with
        // `key_item`/`loot_table` inside it on only four (§42.1). The nine are
        // not vaults missing a config; they are vaults whose config is all
        // defaults.
        "minecraft:sign" => {
            for side in ["front_text", "back_text"] {
                if let Some(Nbt::Compound(text)) = m.get_mut(side) {
                    sign_text(text);
                }
            }
            m.entry("is_waxed".into()).or_insert(Nbt::Byte(0));
        }
        // The three keys `BrewingStandBlockEntity.saveAdditional` writes on
        // every save, at the values a stand nothing has ticked holds.
        "minecraft:brewing_stand" => {
            m.entry("BrewTime".into()).or_insert(Nbt::Short(0));
            m.entry("Fuel".into()).or_insert(Nbt::Byte(0));
            m.entry("Items".into()).or_insert(Nbt::List(Vec::new()));
        }
        "minecraft:vault" => {
            // **The template's `config` is not the saved `config`, and which
            // fields survive is per field rather than per class.**
            //
            // `VaultConfig`'s codec, read instruction by instruction in the
            // 26.2 jar:
            //
            //   loot_table                      lenientOptionalFieldOf(name, DEFAULT.lootTable)
            //   activation_range                lenientOptionalFieldOf(name, DEFAULT.activationRange)
            //   deactivation_range              lenientOptionalFieldOf(name, DEFAULT.deactivationRange)
            //   key_item                        fieldOf  — no optional wrapper at all
            //   override_loot_table_to_display  lenientOptionalFieldOf(name)  (the Optional form)
            //
            // An `optionalFieldOf(name, default)` OMITS its field on encode
            // when the value equals that default; a plain `fieldOf` always
            // writes. So a vault whose template names the default loot table
            // saves `key_item` and no `loot_table`, and one carrying an
            // ominous table saves both.
            //
            // **That is the 9/4 split exactly**, and it is why copying the
            // template tag verbatim scored nine of thirteen as a payload
            // mismatch on `config` (2026-08-18). Read off both sides at
            // (1051,-39,1260): the template gives
            // `{key_item:{count:1,id:trial_key}, loot_table:chests/trial_chambers/reward}`
            // and vanilla saves `{key_item:{count:1,id:trial_key}}`.
            //
            // The defaults are `VaultConfig`'s private no-arg constructor:
            // `BuiltInLootTables.TRIAL_CHAMBERS_REWARD`, `4.0d`, `4.5d`.
            if let Some(Nbt::Compound(cfg)) = m.get_mut("config") {
                cfg.retain(|k, v| match k.as_str() {
                    "loot_table" => {
                        v != &Nbt::String("minecraft:chests/trial_chambers/reward".into())
                    }
                    "activation_range" => v != &Nbt::Double(4.0),
                    "deactivation_range" => v != &Nbt::Double(4.5),
                    // `key_item` is a plain `fieldOf` and is always written;
                    // everything else the template happens to carry is not a
                    // `VaultConfig` field and the codec would not read it.
                    "key_item" | "override_loot_table_to_display" => true,
                    _ => false,
                });
            }
            m.entry("config".into())
                .or_insert(Nbt::Compound(BTreeMap::new()));
            // `VaultBlockEntity.saveAdditional` writes `server_data` and
            // `shared_data` unconditionally, and both are empty for a vault
            // nobody has opened. All thirteen on the census window carry
            // `server_data:{}` and `shared_data:{}` beside a `config` that
            // came from the template.
            m.entry("server_data".into())
                .or_insert(Nbt::Compound(BTreeMap::new()));
            m.entry("shared_data".into())
                .or_insert(Nbt::Compound(BTreeMap::new()));
        }
        _ => {}
    }
}

/// The block entity a freshly generated block of `name` carries, if any.
///
/// `name` is a **block name without its state properties** —
/// `minecraft:potent_sulfur`, not
/// `minecraft:potent_sulfur[potent_sulfur_state=wet]`. That is deliberate and
/// it is read off the bytecode rather than off the one state the census
/// happened to contain: `PotentSulfurBlock.newBlockEntity` ignores its
/// `BlockState` argument entirely and returns a new `PotentSulfurBlockEntity`
/// unconditionally, so keying on the state would invent a condition vanilla
/// does not have. All 32 entries in the census window are
/// `potent_sulfur_state=wet`, so the *data* could not have told us this and
/// only the bytecode could.
///
/// Returns the full saved compound including `id`, `x`, `y` and `z`, in the
/// shape `SerializableChunkData` writes: `saveWithoutMetadata` contributes
/// `id`, the type's own `saveAdditional` keys and `components`, and
/// `saveMetadata` contributes `x`/`y`/`z`. `keepPacked` is `0` for an entity
/// that is a live instance rather than an unread packed tag, which is what a
/// just-generated chunk always holds.
/// # What this signature can and cannot express
///
/// The arguments are a block name and a position, so **this function can only
/// serve classes whose saved payload is a function of those two things.**
/// That covers every class landed so far and it is not an accident: the
/// emitter runs as a post-pass over the finished canvas precisely so that it
/// cannot perturb generation (see `anvil_bridge.rs`), and a post-pass can only
/// read what the canvas holds.
///
/// `mob_spawner` is the first class that does not fit. Its mob is one
/// `nextInt(4)` into `{SKELETON, ZOMBIE, ZOMBIE, SPIDER}`, drawn inside
/// `feature::monster_room`, and nothing about `("minecraft:spawner", x, y, z)`
/// recovers it. **That channel now exists in this file** — [`Record`],
/// [`Records`] and [`for_recorded`] — so the classes it serves are arms of
/// [`complete`] rather than of the `match` below. This function keeps its
/// signature and its guarantee: what it answers, it answers off the finished
/// canvas, and it can be read with no channel at all.
///
/// **It is the first and it is not the only one — measured 2026-08-17.** Of
/// the 173 block entities on the census window, **141 are placer-written**:
/// every remaining class writes something this signature cannot see, whether
/// a draw (`LootTableSeed`, the spawner's mob), a template's stored NBT
/// (`trial_spawner`'s two configs, `decorated_pot`'s sherds, `vault`'s
/// config) or the mere fact that no loot table was set (a bare `barrel` and a
/// `hopper`, which are indistinguishable here from their loot-bearing
/// siblings — same block, same states). So the channel is not the price of
/// one exotic class; it is the price of the other nine, and after it each of
/// them is an arm of the `match`. The full per-class inventory of what it has
/// to carry, and what it costs measured off this tree, is
/// `docs/DETERMINISM.md` §42.
pub fn for_block(name: &str, x: i32, y: i32, z: i32) -> Option<Nbt> {
    let mut m: BTreeMap<String, Nbt> = BTreeMap::new();
    match name {
        "minecraft:potent_sulfur" => {
            // `PotentSulfurBlockEntity.saveAdditional` writes this one key,
            // and the constructor sets the field to -1 (`iconst_m1`). A
            // chunk that has never ticked has never moved it, and worldgen
            // never ticks.
            //
            // `eruptionTick` is a field on the same class and is deliberately
            // NOT here: `saveAdditional` does not write it.
            m.insert("countdown".into(), Nbt::Int(-1));
        }
        // A vibration listener nothing has ticked: `VibrationSystem.Data`
        // with no event, a selector at `tick:-1L` and `event_delay:0`. Read
        // off all 246 sensors and 31 shriekers in `sl2-city1-a` (seed 12345,
        // the ancient city at (32,37)), which agree, with their tag types.
        "minecraft:sculk_sensor" => {
            m.insert("last_vibration_frequency".into(), Nbt::Int(0));
            m.insert("listener".into(), untouched_listener());
        }
        "minecraft:sculk_shrieker" => {
            m.insert("warning_level".into(), Nbt::Int(0));
            m.insert("listener".into(), untouched_listener());
        }
        // `SculkCatalystBlockEntity`: its spreader's `cursors`, empty.
        "minecraft:sculk_catalyst" => {
            m.insert("cursors".into(), Nbt::List(Vec::new()));
        }
        _ => return None,
    }
    // `BlockEntity.saveWithoutMetadata` writes `components` for every type.
    // Empty for anything worldgen places — components arrive from item
    // stacks and commands, neither of which exists during generation.
    m.insert("components".into(), Nbt::Compound(BTreeMap::new()));
    m.insert("id".into(), Nbt::String(name.into()));
    m.insert("keepPacked".into(), Nbt::Byte(0));
    m.insert("x".into(), Nbt::Int(x));
    m.insert("y".into(), Nbt::Int(y));
    m.insert("z".into(), Nbt::Int(z));
    Some(Nbt::Compound(m))
}

/// Whether a block name carries a block entity we can currently reproduce.
///
/// Separate from [`for_block`] so a caller can skip the allocation while
/// scanning a palette, which is the hot path: a section's palette is tens of
/// entries and its cells are 4,096.
pub fn is_reproduced(name: &str) -> bool {
    matches!(
        name,
        "minecraft:potent_sulfur"
            | "minecraft:sculk_sensor"
            | "minecraft:sculk_shrieker"
            | "minecraft:sculk_catalyst"
    )
}

/// `VibrationSystem.Data` as a listener nothing has heard saves it.
fn untouched_listener() -> Nbt {
    Nbt::Compound(BTreeMap::from([
        (
            "selector".to_string(),
            Nbt::Compound(BTreeMap::from([("tick".to_string(), Nbt::Long(-1))])),
        ),
        ("event_delay".to_string(), Nbt::Int(0)),
    ]))
}

/// The block entity a block of `name` carries given what a placer recorded
/// there.
///
/// The counterpart of [`for_block`] for the 141 positions of 173 that a
/// finished block state cannot answer for. Same output shape, same metadata
/// keys, and the same rule about absence: a record this cannot turn into a
/// payload produces `None` rather than a guess.
///
/// # It is the invalidation rule too
///
/// `LevelChunk.setBlockState` drops the block entity at a position whose block
/// is replaced, so a payload is only vanilla's answer while the block it was
/// recorded against is still there. [`for_block`] gets that for free by
/// reading final states; a position-keyed channel has to reproduce it, and
/// this is where: the record is used **only** if the block now at that
/// position has a block entity at all (`entity_id`), and a record whose class
/// no longer matches is dropped. A chest recorded at a position a later
/// feature turned to stone emits nothing.
///
/// The record's own keys are copied, never merged with a guess, and the
/// metadata keys are written here rather than taken from the record — so a
/// producer cannot move a position or rename a type.
#[must_use]
pub fn for_recorded(name: &str, record: &Record, x: i32, y: i32, z: i32) -> Option<Nbt> {
    let id = entity_id(name)?;
    let mut m = record.nbt.clone();
    // Drop everything the class's save does not write. See
    // [`saved_template_keys`] for why this is the only place that decides it.
    let allowed = saved_template_keys(id);
    m.retain(|k, _| allowed.contains(&k.as_str()));
    let empty_list = |v: Option<&Nbt>| matches!(v, Some(Nbt::List(l)) if l.is_empty());
    // A chest's or dispenser's EMPTY template `Items` is withheld unless the
    // placer recorded `placeInWorld`'s draw: see the allowlist. A recorded
    // draw means the placer threaded the postProcess stream, and a placer
    // that does is one that handles its own markers — so a table that was
    // going to arrive has arrived as a later record at this position.
    if matches!(id, "minecraft:chest" | "minecraft:dispenser")
        && record.loot_seed.is_none()
        && empty_list(m.get("Items"))
    {
        m.remove("Items");
    }
    // `patterns` is saved only when it is not `BannerPatternLayers.EMPTY`;
    // a mansion's plain banners carry `patterns:[]` in their template tag.
    if id == "minecraft:banner" && empty_list(m.get("patterns")) {
        m.remove("patterns");
    }
    // `trySaveLootTable`: the seed is written inside the branch where a loot
    // table is set, and then only when it is non-zero (`lconst_0; lcmp;
    // ifeq`). A recorded draw with no table is a draw vanilla also spent and
    // also did not save — the five bare barrels on the census window.
    if m.contains_key("LootTable") {
        if let Some(seed) = record.loot_seed.filter(|&s| s != 0) {
            m.insert("LootTableSeed".into(), Nbt::Long(seed));
        }
    }
    // The rest of `trySaveLootTable`, for a seed that came from the template
    // rather than from a draw. It writes the key only INSIDE the branch where
    // a table is set (`lconst_0; lcmp; ifeq` guards the value itself), so a
    // seed with no table beside it, and a zero seed, are both not written.
    // Without this a `LootTableSeed` allowed through from a tag could outlive
    // the `LootTable` the allowlist did not carry.
    if !m.contains_key("LootTable") || m.get("LootTableSeed") == Some(&Nbt::Long(0)) {
        m.remove("LootTableSeed");
    }
    // `if (!trySaveLootTable(out)) saveAllItems(...)`: a container with a
    // table saves no `Items`, whatever its template carried.
    if m.contains_key("LootTable") {
        m.remove("Items");
    }
    // The four containers' shared save, here rather than in `complete` because
    // it needs the RECORD and not just the payload.
    //
    // `if (!trySaveLootTable(out)) ContainerHelper.saveAllItems(out, items)`,
    // and `saveAllItems`'s two-argument form passes `alwaysWrite = true`, so
    // an all-empty container writes `Items:[]` rather than discarding the key.
    //
    // **Gated on a recorded draw, and that gate is measured rather than
    // cautious.** `placeInWorld` spends a `nextLong` on every
    // `RandomizableContainer` carrying an `nbt`, so a recorded seed is proof
    // this position went through that branch with whatever table its tag
    // named — and no table named means an empty container. Without the gate,
    // one chest on the census window scored a payload mismatch on `Items`,
    // `LootTable` and `LootTableSeed` together: its table arrives from a data
    // marker after the template is placed, so the tag is silent and "no table
    // in the tag" is not "no table". A position with no recorded draw has not
    // been through that branch and nothing here can say its container is
    // empty.
    //
    // **`Items` is synthesised for a `barrel` and a `hopper` and NOT for a
    // `chest` or a `dispenser`, and that asymmetry is a scope limit rather
    // than a claim about the classes.** All thirty chests and all nineteen
    // dispensers on the census window carry a `LootTable`; the only ones that
    // do not are positions whose table is set AFTER the template is placed,
    // by a data marker this port does not yet handle. Measured 2026-08-18:
    // synthesising for them scored one chest as a payload mismatch on
    // `Items`, `LootTable` and `LootTableSeed` together — the worse answer.
    // Bare `barrel`s and `hopper`s are the case §42.1's null was drawn to
    // allow, and a recorded draw is what makes them readable.
    if matches!(id, "minecraft:barrel" | "minecraft:hopper")
        && record.loot_seed.is_some()
        && !m.contains_key("LootTable")
    {
        m.entry("Items".into()).or_insert(Nbt::List(Vec::new()));
    }
    // `putInt("TransferCooldown", cooldownTime)` sits AFTER the loot/items
    // if/else and OUTSIDE it, so it is written on every hopper that saves at
    // all — loot-bearing or not, draw or no draw. Gating it on the draw with
    // the `Items` rule above scored five hoppers as a payload mismatch on
    // exactly this key in the arm where no draw is spent. `cooldownTime` is 0
    // on an instance nothing has ticked, and worldgen never ticks.
    if id == "minecraft:hopper" && !m.is_empty() {
        m.entry("TransferCooldown".into()).or_insert(Nbt::Int(0));
    }
    complete(id, &mut m);
    // A record that carried nothing and completed to nothing would emit a
    // payload of pure metadata, which is a *different* wrong answer from
    // absence and would score as a payload mismatch. Refuse it.
    if m.is_empty() && !metadata_only_is_a_payload(id) {
        return None;
    }
    // `BlockEntity.loadWithComponents` reads `components` off the tag and
    // `saveWithoutMetadata` writes it back, so a template's components are the
    // saved ones. Sixteen templates carry any — the pillager outpost's
    // ominous banners, `{minecraft:item_name, minecraft:rarity}` — and every
    // other record has none, which saves as the empty map.
    let components = match record.nbt.get("components") {
        Some(c @ Nbt::Compound(_)) => c.clone(),
        _ => Nbt::Compound(BTreeMap::new()),
    };
    m.insert("components".into(), components);
    m.insert("id".into(), Nbt::String(id.into()));
    m.insert("keepPacked".into(), Nbt::Byte(0));
    m.insert("x".into(), Nbt::Int(x));
    m.insert("y".into(), Nbt::Int(y));
    m.insert("z".into(), Nbt::Int(z));
    Some(Nbt::Compound(m))
}

/// Every block entity a finished column carries, derived from its sections.
///
/// `chunk_x`/`chunk_z` are chunk coordinates; the returned compounds carry
/// world block coordinates, which is what the file format stores.
///
/// # The order is ours, not vanilla's, and that is stated rather than hidden
///
/// Vanilla saves this list in the iteration order of the chunk's
/// block-entity **map**, which is a hash order: read out of
/// `gen-wide-12345-r16-ow-c` chunk (66,79), the nineteen entries come in an
/// order that is neither by y, nor by position, nor by type. Reproducing it
/// means reproducing that map's bucket layout, which nothing here does.
///
/// So this emits in a deterministic scan order — ascending section, then the
/// section's own `y·256 + z·16 + x` cell order — and **does not claim to
/// match vanilla's**. That is sound for every use there is today, because
/// `region-differ bediff` keys block entities by position and never reads
/// the list order. It is **not** sound for a byte-identical region file, and
/// anyone who needs one has this paragraph rather than a surprise.
pub fn from_sections(sections: &[Section], chunk_x: i32, chunk_z: i32) -> Vec<Nbt> {
    from_sections_recorded(sections, chunk_x, chunk_z, &[]).0
}

/// What a chunk's emit did with the records it was given.
///
/// `orphaned` is the falsifier `docs/DETERMINISM.md` §42.5 asks for: a record
/// whose position no longer holds a block of its class is dropped, and a
/// channel that is quietly dropping everything is indistinguishable from a
/// channel nothing writes to unless the drops are counted. Absence already
/// means "not yet reproduced" in this project; an *unexplained* absence must
/// not be able to hide behind it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmitStats {
    /// Payloads emitted from a record.
    pub recorded: usize,
    /// Payloads emitted from the block state alone — [`for_block`].
    pub derived: usize,
    /// Records dropped because the position no longer holds their class.
    pub orphaned: usize,
    /// Records the emitter refused because the placer could not supply
    /// everything the save writes.
    ///
    /// One case today and it is not hypothetical: a container whose template
    /// gave it a `LootTable` but whose placer never drew a seed, because the
    /// piece's `RandomSource` is not threaded into
    /// `structure::place::place_piece` at four of its five call sites. Writing
    /// `LootTable` without `LootTableSeed` would be a payload that is *wrong
    /// on one key*, and this project reads absence as "not yet reproduced"
    /// while a wrong payload reads as done. So it emits nothing and shows up
    /// here.
    pub incomplete: usize,
}

/// Every block entity a finished column carries, given the records the
/// generation pass made for that chunk.
///
/// The channel's consumer. `records` is one chunk's slice in the order it was
/// recorded; a position recorded more than once keeps the **last** record,
/// which is what `LevelChunk.setBlockState` does when it replaces a block
/// entity.
#[must_use]
pub fn from_sections_recorded(
    sections: &[Section],
    chunk_x: i32,
    chunk_z: i32,
    records: &[((i32, i32, i32), Record)],
) -> (Vec<Nbt>, EmitStats) {
    let mut by_pos: BTreeMap<(i32, i32, i32), &Record> = BTreeMap::new();
    for (pos, record) in records {
        // Last wins, so a later placer's payload replaces an earlier one at
        // the same position exactly as its block replaced the earlier block.
        by_pos.insert(*pos, record);
    }
    let mut used = 0usize;
    let mut stats = EmitStats::default();
    let mut out = Vec::new();
    for section in sections {
        // A palette holding no entity block at all is the overwhelmingly
        // common case — most sections are stone and air — so decide it once
        // per section rather than once per cell.
        if !section
            .palette
            .iter()
            .any(|s| entity_id(s.split('[').next().unwrap_or(s)).is_some())
        {
            continue;
        }
        let base_y = i32::from(section.y) * 16;
        for i in 0..4096usize {
            let state = &section.palette[section.blocks.get(i).map_or(0, |&v| v as usize)];
            let name = state.split('[').next().unwrap_or(state);
            if entity_id(name).is_none() {
                continue;
            }
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_possible_wrap,
                reason = "an index bounded by 4096"
            )]
            let (x, y, z) = (
                chunk_x * 16 + (i % 16) as i32,
                base_y + (i / 256) as i32,
                chunk_z * 16 + ((i / 16) % 16) as i32,
            );
            // The state's own answer first: a class that is a function of the
            // finished block state needs no record and must not be able to be
            // overridden by one, or a producer could contradict the canvas.
            if let Some(tag) = for_block(name, x, y, z) {
                out.push(tag);
                stats.derived += 1;
                continue;
            }
            if let Some(record) = by_pos.get(&(x, y, z)) {
                used += 1;
                // A loot table with no seed behind it is a payload this crate
                // cannot finish. Refused here rather than inside
                // `for_recorded`, so that the count separates it from a class
                // nobody has ported: both emit nothing, and only one of them
                // is waiting on a draw somebody has to thread through.
                // The one class whose seed is a KEY of its tag rather than a
                // draw — a `BrushableBlockEntity` is not a
                // `RandomizableContainer`, so its `LootTableSeed` comes from
                // the template or from a rule's `append_loot` — is complete
                // with that key. Until 2026-10-02 it was counted here too, and
                // every trail ruin's 86-88 suspicious blocks were refused.
                let seed_in_tag = entity_id(name) == Some("minecraft:brushable_block")
                    && record.nbt.contains_key("LootTableSeed");
                if record.nbt.contains_key("LootTable")
                    && record.loot_seed.is_none()
                    && !seed_in_tag
                {
                    stats.incomplete += 1;
                    continue;
                }
                if let Some(tag) = for_recorded(name, record, x, y, z) {
                    out.push(tag);
                    stats.recorded += 1;
                }
            }
        }
    }
    // Everything the channel held for this chunk that no final block claimed.
    // Counted rather than ignored: see [`EmitStats`].
    stats.orphaned = by_pos.len() - used;
    totals::add(&stats);
    (out, stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A section whose palette holds nothing reproducible must be skipped
    /// wholesale, and one that does must yield exactly one entry per cell —
    /// at the right world coordinates. The coordinate arithmetic is the part
    /// worth a test: `y·256 + z·16 + x` puts z where an obvious reading puts
    /// y, and getting it wrong yields block entities that are *present* and
    /// in the wrong place, which scores as both a missing and a spurious
    /// position rather than as an obvious crash.
    #[test]
    fn cells_map_to_the_world_coordinates_the_format_stores() {
        let mut blocks = vec![0u16; 4096];
        // index = y*256 + z*16 + x, with y=3, z=5, x=7 inside the section.
        blocks[3 * 256 + 5 * 16 + 7] = 1;
        let section = Section {
            y: -2,
            palette: vec!["minecraft:stone".into(), "minecraft:potent_sulfur".into()],
            blocks,
            biome_palette: vec!["minecraft:plains".into()],
            biomes: Vec::new(),
            extra: std::collections::BTreeMap::new(),
        };
        let got = from_sections(&[section], 4, -3);
        assert_eq!(got.len(), 1);
        let Nbt::Compound(m) = &got[0] else { panic!() };
        assert_eq!(m.get("x"), Some(&Nbt::Int(4 * 16 + 7)));
        assert_eq!(m.get("y"), Some(&Nbt::Int(-2 * 16 + 3)));
        assert_eq!(m.get("z"), Some(&Nbt::Int(-3 * 16 + 5)));
    }

    /// An empty `blocks` means every cell is palette entry 0. A section whose
    /// entry 0 is the reproduced block is therefore 4,096 of them — the
    /// compressed single-state section the format allows. Skipping this case
    /// would lose every entry in a uniform section silently.
    #[test]
    fn an_empty_blocks_vector_means_every_cell_is_palette_entry_zero() {
        let full = Section {
            y: 0,
            palette: vec!["minecraft:potent_sulfur".into()],
            blocks: Vec::new(),
            biome_palette: vec!["minecraft:plains".into()],
            biomes: Vec::new(),
            extra: std::collections::BTreeMap::new(),
        };
        assert_eq!(from_sections(&[full], 0, 0).len(), 4096);
        let none = Section {
            y: 0,
            palette: vec!["minecraft:stone".into()],
            blocks: Vec::new(),
            biome_palette: vec!["minecraft:plains".into()],
            biomes: Vec::new(),
            extra: std::collections::BTreeMap::new(),
        };
        assert!(from_sections(&[none], 0, 0).is_empty());
    }

    /// The palette carries full state strings; the lookup key is the name.
    /// A section whose only sulfur is `potent_sulfur[potent_sulfur_state=wet]`
    /// — which is every one in the census window — must still produce an
    /// entry, or this ships as a no-op that passes its own unit tests.
    #[test]
    fn a_palette_entry_with_properties_still_matches() {
        let section = Section {
            y: 0,
            palette: vec!["minecraft:potent_sulfur[potent_sulfur_state=wet]".into()],
            blocks: Vec::new(),
            biome_palette: vec!["minecraft:plains".into()],
            biomes: Vec::new(),
            extra: std::collections::BTreeMap::new(),
        };
        assert_eq!(from_sections(&[section], 0, 0).len(), 4096);
    }

    #[test]
    fn potent_sulfur_matches_the_bytes_vanilla_writes() {
        let got = for_block("minecraft:potent_sulfur", 895, -23, 1114).expect("has one");
        let Nbt::Compound(m) = &got else {
            panic!("compound")
        };
        // The exact entry read out of gen-wide-12345-r16-ow-c at this
        // position — docs/DETERMINISM.md §40.3.
        assert_eq!(m.get("countdown"), Some(&Nbt::Int(-1)));
        assert_eq!(m.get("keepPacked"), Some(&Nbt::Byte(0)));
        assert_eq!(m.get("components"), Some(&Nbt::Compound(BTreeMap::new())));
        assert_eq!(
            m.get("id"),
            Some(&Nbt::String("minecraft:potent_sulfur".into()))
        );
        assert_eq!(m.get("x"), Some(&Nbt::Int(895)));
        assert_eq!(m.get("y"), Some(&Nbt::Int(-23)));
        assert_eq!(m.get("z"), Some(&Nbt::Int(1114)));
        // Nothing else. A surplus key is as wrong as a missing one and is
        // the more likely mistake, since it is what copying another type's
        // shape produces.
        assert_eq!(m.len(), 7, "exactly seven keys: {m:?}");
    }

    /// The block whose absence is the point: we place `minecraft:chest`
    /// today and must NOT invent a block entity for it, because the one
    /// vanilla writes carries a `LootTable` and a `LootTableSeed` we do not
    /// yet reproduce. A wrong payload is worse than none — it would score as
    /// a payload mismatch where absence scores as a missing position, and it
    /// would read as done.
    #[test]
    fn a_type_we_cannot_yet_reproduce_gets_nothing_rather_than_a_guess() {
        for name in [
            "minecraft:chest",
            "minecraft:mob_spawner",
            "minecraft:spawner",
            "minecraft:barrel",
            "minecraft:decorated_pot",
            "minecraft:stone",
        ] {
            assert!(for_block(name, 0, 0, 0).is_none(), "{name}");
            assert!(!is_reproduced(name), "{name}");
        }
    }

    fn compound(tag: &Nbt) -> &BTreeMap<String, Nbt> {
        match tag {
            Nbt::Compound(m) => m,
            other => panic!("compound: {other:?}"),
        }
    }

    fn loot(table: &str, seed: i64) -> Record {
        Record {
            nbt: BTreeMap::from([("LootTable".into(), Nbt::String(table.into()))]),
            loot_seed: Some(seed),
        }
    }

    /// The chest at (982,57,788) in `gen-wide-12345-r16-ow-c`, key for key.
    ///
    /// A golden vector rather than a shape assertion: the whole risk in this
    /// emitter is a surplus or missing key, and only a real saved entity can
    /// score that. Read raw off the reference — `{LootTable:
    /// "minecraft:chests/ruined_portal", LootTableSeed: 1406161612466394803,
    /// components:{}, keepPacked:0b, x:982, y:57, z:788,
    /// id:"minecraft:chest"}` — eight keys.
    #[test]
    fn a_recorded_chest_matches_the_bytes_vanilla_saved() {
        let record = loot("minecraft:chests/ruined_portal", 1_406_161_612_466_394_803);
        let got = for_recorded("minecraft:chest", &record, 982, 57, 788).expect("has one");
        let m = compound(&got);
        assert_eq!(
            m.get("LootTable"),
            Some(&Nbt::String("minecraft:chests/ruined_portal".into()))
        );
        assert_eq!(
            m.get("LootTableSeed"),
            Some(&Nbt::Long(1_406_161_612_466_394_803))
        );
        assert_eq!(m.get("id"), Some(&Nbt::String("minecraft:chest".into())));
        assert_eq!(m.get("components"), Some(&Nbt::Compound(BTreeMap::new())));
        assert_eq!(m.get("keepPacked"), Some(&Nbt::Byte(0)));
        assert_eq!(m.get("x"), Some(&Nbt::Int(982)));
        assert_eq!(m.get("y"), Some(&Nbt::Int(57)));
        assert_eq!(m.get("z"), Some(&Nbt::Int(788)));
        assert_eq!(m.len(), 8, "exactly eight keys: {m:?}");
    }

    /// The five bare barrels: a `RandomizableContainer` carrying an nbt, so
    /// `placeInWorld` spends the draw, and a saved tag with **no**
    /// `LootTableSeed` in it because `trySaveLootTable` writes the seed only
    /// inside the branch where a loot table is set.
    ///
    /// This is the case that separates recording a draw from saving it, and
    /// getting it wrong writes a key vanilla does not have onto ten of the
    /// window's 173 positions.
    #[test]
    fn a_recorded_draw_with_no_loot_table_saves_no_seed() {
        let record = Record {
            nbt: BTreeMap::from([("Items".into(), Nbt::List(Vec::new()))]),
            loot_seed: Some(-106_722_592_192_165_167),
        };
        let got = for_recorded("minecraft:barrel", &record, 1048, -2, 1272).expect("has one");
        let m = compound(&got);
        assert_eq!(m.get("Items"), Some(&Nbt::List(Vec::new())));
        assert!(!m.contains_key("LootTableSeed"), "{m:?}");
        assert!(!m.contains_key("LootTable"), "{m:?}");
        assert_eq!(m.len(), 7, "exactly seven keys: {m:?}");
    }

    /// `minecraft:spawner` saves as `minecraft:mob_spawner`, and the eight
    /// constants come from the type rather than from the placer.
    ///
    /// The entry at (982,-22,803) in the reference: `SpawnData:{entity:{id:
    /// "minecraft:zombie"}}`, `Delay:20`, `MinSpawnDelay:200`,
    /// `MaxSpawnDelay:800`, `SpawnCount:4`, `MaxNearbyEntities:6`,
    /// `RequiredPlayerRange:16`, `SpawnRange:4`, `SpawnPotentials:[]` — the
    /// seven counts as TAG_Short, which `BaseSpawner.save`'s seven `putShort`
    /// calls decide and which no JSON rendering of the reference can show.
    #[test]
    fn a_recorded_spawner_saves_under_the_type_id_with_its_constants() {
        let record = Record {
            nbt: BTreeMap::from([(
                "SpawnData".into(),
                Nbt::Compound(BTreeMap::from([(
                    "entity".into(),
                    Nbt::Compound(BTreeMap::from([(
                        "id".into(),
                        Nbt::String("minecraft:zombie".into()),
                    )])),
                )])),
            )]),
            loot_seed: None,
        };
        let got = for_recorded("minecraft:spawner", &record, 982, -22, 803).expect("has one");
        let m = compound(&got);
        assert_eq!(
            m.get("id"),
            Some(&Nbt::String("minecraft:mob_spawner".into())),
            "the block is `spawner` and the type is `mob_spawner`"
        );
        assert_eq!(m.get("Delay"), Some(&Nbt::Short(20)));
        assert_eq!(m.get("MinSpawnDelay"), Some(&Nbt::Short(200)));
        assert_eq!(m.get("MaxSpawnDelay"), Some(&Nbt::Short(800)));
        assert_eq!(m.get("SpawnCount"), Some(&Nbt::Short(4)));
        assert_eq!(m.get("MaxNearbyEntities"), Some(&Nbt::Short(6)));
        assert_eq!(m.get("RequiredPlayerRange"), Some(&Nbt::Short(16)));
        assert_eq!(m.get("SpawnRange"), Some(&Nbt::Short(4)));
        assert_eq!(m.get("SpawnPotentials"), Some(&Nbt::List(Vec::new())));
        assert_eq!(m.len(), 15, "exactly fifteen keys: {m:?}");
    }

    /// The saved `config` is the template's with every field the codec omits
    /// at its default removed — measured off both sides at (1051,-39,1260).
    #[test]
    fn a_vault_config_drops_the_fields_the_codec_omits_at_their_defaults() {
        let key_item = Nbt::Compound(BTreeMap::from([
            ("count".to_string(), Nbt::Int(1)),
            ("id".to_string(), Nbt::String("minecraft:trial_key".into())),
        ]));
        let plain = Record {
            nbt: BTreeMap::from([(
                "config".to_string(),
                Nbt::Compound(BTreeMap::from([
                    ("key_item".to_string(), key_item.clone()),
                    (
                        "loot_table".to_string(),
                        Nbt::String("minecraft:chests/trial_chambers/reward".into()),
                    ),
                ])),
            )]),
            loot_seed: None,
        };
        let tag = for_recorded("minecraft:vault", &plain, 0, 0, 0).expect("emits");
        assert_eq!(
            compound(&tag).get("config"),
            Some(&Nbt::Compound(BTreeMap::from([(
                "key_item".to_string(),
                key_item.clone()
            )]))),
            "the default loot table is omitted; key_item is a plain fieldOf and is not"
        );

        // An ominous vault names a non-default table, so the codec writes it.
        let ominous = Record {
            nbt: BTreeMap::from([(
                "config".to_string(),
                Nbt::Compound(BTreeMap::from([
                    ("key_item".to_string(), key_item.clone()),
                    (
                        "loot_table".to_string(),
                        Nbt::String("minecraft:chests/trial_chambers/ominous_reward".into()),
                    ),
                ])),
            )]),
            loot_seed: None,
        };
        let otag = for_recorded("minecraft:vault", &ominous, 0, 0, 0).expect("emits");
        let ocfg = compound(&otag).get("config").expect("a config");
        let Nbt::Compound(ocfg) = ocfg else {
            panic!("config is a compound")
        };
        assert_eq!(ocfg.len(), 2, "both keys survive: {ocfg:?}");
    }

    /// A seed that came from a template rather than from a draw is written
    /// under exactly `trySaveLootTable`'s two conditions, and a class that
    /// does not carry one from its tag cannot gain one.
    #[test]
    fn a_template_carried_loot_seed_follows_try_save_loot_table() {
        let tag_with = |seed: i64| Record {
            nbt: BTreeMap::from([
                (
                    "LootTable".to_string(),
                    Nbt::String("minecraft:chests/x".into()),
                ),
                ("LootTableSeed".to_string(), Nbt::Long(seed)),
            ]),
            loot_seed: None,
        };
        // Non-zero and beside a table: written. This is the seven
        // `brushable_block`s on the census window, which save a
        // `LootTableSeed` while spending no draw.
        let with_tag =
            for_recorded("minecraft:suspicious_gravel", &tag_with(99), 0, 0, 0).expect("emits");
        let m = compound(&with_tag);
        assert_eq!(m.get("LootTableSeed"), Some(&Nbt::Long(99)));
        // Zero: `lconst_0; lcmp; ifeq` skips the write.
        let zero_tag =
            for_recorded("minecraft:suspicious_gravel", &tag_with(0), 0, 0, 0).expect("emits");
        let z = compound(&zero_tag);
        assert!(
            !z.contains_key("LootTableSeed"),
            "a zero seed is not written"
        );
        // A seed with no table beside it: the key is written inside the
        // table's branch, so there is nothing to write it.
        let orphan = Record {
            nbt: BTreeMap::from([("LootTableSeed".to_string(), Nbt::Long(99))]),
            loot_seed: None,
        };
        assert!(
            for_recorded("minecraft:suspicious_gravel", &orphan, 0, 0, 0).is_none(),
            "a seed with no table completes to nothing"
        );
        // And a chest cannot pick one up from its tag: its seed is the draw,
        // so letting the key through would read a stale template value over
        // a drawn one.
        let chest_tag = for_recorded("minecraft:chest", &tag_with(99), 0, 0, 0).expect("emits");
        let c = compound(&chest_tag);
        assert!(
            !c.contains_key("LootTableSeed"),
            "no draw was recorded, so the chest has no seed to save"
        );
    }

    /// A bare barrel saves the EMPTY list rather than nothing, because
    /// `ContainerHelper.saveAllItems`'s two-argument form passes
    /// `alwaysWrite = true` and only the three-argument form's
    /// `if (list.isEmpty() && !alwaysWrite)` discards the key.
    #[test]
    fn a_recorded_bare_container_saves_the_empty_items_list() {
        let record = Record {
            nbt: BTreeMap::new(),
            // `placeInWorld` spends the draw on a bare barrel too — it is a
            // `RandomizableContainer` carrying an `nbt` — and the save drops
            // it because no table was set. The seed being present here and
            // absent from the payload is the point.
            loot_seed: Some(0x0BAD_F00D),
        };
        let tag = for_recorded("minecraft:barrel", &record, 1, 2, 3).expect("a barrel emits");
        let m = compound(&tag);
        assert_eq!(m.get("Items"), Some(&Nbt::List(Vec::new())));
        assert!(
            !m.contains_key("LootTableSeed"),
            "no table was set, so `trySaveLootTable` wrote neither key"
        );
        assert!(
            !m.contains_key("TransferCooldown"),
            "that key is the hopper's"
        );
    }

    /// The hopper writes `TransferCooldown` OUTSIDE the loot/items branch, so
    /// it is present on a loot-bearing hopper as well — and a loot-bearing
    /// container writes no `Items` at all, because `trySaveLootTable`
    /// returning true skips `saveAllItems` entirely.
    #[test]
    fn a_hopper_always_saves_its_cooldown_and_loot_suppresses_items() {
        let bare = for_recorded(
            "minecraft:hopper",
            // A recorded draw and no table: `placeInWorld` went through the
            // container branch and the tag named nothing, which is the only
            // state that proves the container is empty.
            &Record {
                nbt: BTreeMap::new(),
                loot_seed: Some(1),
            },
            1,
            2,
            3,
        )
        .expect("a hopper emits");
        let bm = compound(&bare);
        assert_eq!(bm.get("Items"), Some(&Nbt::List(Vec::new())));
        assert_eq!(bm.get("TransferCooldown"), Some(&Nbt::Int(0)));

        let with_loot = for_recorded(
            "minecraft:hopper",
            &loot("minecraft:chests/simple_dungeon", 7),
            1,
            2,
            3,
        )
        .expect("a loot hopper emits");
        // And a chest with no table and no marker handling emits nothing
        // rather than an empty container it cannot vouch for.
        assert!(
            for_recorded(
                "minecraft:chest",
                &Record {
                    nbt: BTreeMap::new(),
                    loot_seed: Some(1)
                },
                0,
                0,
                0
            )
            .is_none(),
            "a chest's table can arrive from a data marker after placement"
        );
        let lm = compound(&with_loot);
        assert!(
            !lm.contains_key("Items"),
            "trySaveLootTable returned true, so saveAllItems never ran"
        );
        assert_eq!(lm.get("TransferCooldown"), Some(&Nbt::Int(0)));
        assert_eq!(lm.get("LootTableSeed"), Some(&Nbt::Long(7)));
    }

    /// The vault at (1051,-39,1260): a `config` copied from the template and
    /// two empty maps the save adds.
    #[test]
    fn a_recorded_vault_gains_the_two_empty_maps_the_save_writes() {
        let record = Record {
            nbt: BTreeMap::from([(
                "config".into(),
                Nbt::Compound(BTreeMap::from([(
                    "key_item".into(),
                    Nbt::Compound(BTreeMap::from([
                        ("count".into(), Nbt::Int(1)),
                        ("id".into(), Nbt::String("minecraft:trial_key".into())),
                    ])),
                )])),
            )]),
            loot_seed: None,
        };
        let got = for_recorded("minecraft:vault", &record, 1051, -39, 1260).expect("has one");
        let m = compound(&got);
        assert_eq!(m.get("server_data"), Some(&Nbt::Compound(BTreeMap::new())));
        assert_eq!(m.get("shared_data"), Some(&Nbt::Compound(BTreeMap::new())));
        assert_eq!(m.len(), 9, "exactly nine keys: {m:?}");
    }

    /// A record at a position whose block is no longer that class is dropped,
    /// and dropped *loudly* — the count is what stops a channel that has
    /// stopped matching anything from looking like a channel nothing writes.
    #[test]
    fn a_record_whose_block_went_away_is_dropped_and_counted() {
        let section = Section {
            y: 0,
            palette: vec!["minecraft:stone".into(), "minecraft:chest".into()],
            blocks: {
                let mut b = vec![0u16; 4096];
                // y=1, z=2, x=3 holds the chest; the record below is for the
                // cell next to it, which is stone.
                b[256 + 2 * 16 + 3] = 1;
                b
            },
            biome_palette: vec!["minecraft:plains".into()],
            biomes: Vec::new(),
            extra: std::collections::BTreeMap::new(),
        };
        let here = loot("minecraft:chests/simple_dungeon", 7);
        let gone = loot("minecraft:chests/simple_dungeon", 9);
        let (out, stats) =
            from_sections_recorded(&[section], 0, 0, &[((3, 1, 2), here), ((4, 1, 2), gone)]);
        assert_eq!(out.len(), 1, "one chest, one orphan");
        assert_eq!(stats.recorded, 1);
        assert_eq!(stats.orphaned, 1);
        assert_eq!(compound(&out[0]).get("LootTableSeed"), Some(&Nbt::Long(7)));
    }

    /// The last record at a position wins, because the block state at that
    /// position is the last one written and `setBlockState` drops the entity
    /// it replaced.
    #[test]
    fn the_last_record_at_a_position_is_the_one_saved() {
        let section = Section {
            y: 0,
            palette: vec!["minecraft:chest".into()],
            blocks: Vec::new(),
            biome_palette: vec!["minecraft:plains".into()],
            biomes: Vec::new(),
            extra: std::collections::BTreeMap::new(),
        };
        let (out, stats) = from_sections_recorded(
            &[section],
            0,
            0,
            &[
                ((0, 0, 0), loot("minecraft:chests/first", 1)),
                ((0, 0, 0), loot("minecraft:chests/second", 2)),
            ],
        );
        assert_eq!(stats.orphaned, 0);
        let saved = out
            .iter()
            .map(compound)
            .find(|m| m.get("x") == Some(&Nbt::Int(0)) && m.get("y") == Some(&Nbt::Int(0)))
            .expect("the recorded cell");
        assert_eq!(
            saved.get("LootTable"),
            Some(&Nbt::String("minecraft:chests/second".into()))
        );
    }

    /// A record cannot rename a type, move a position, or invent components:
    /// the metadata keys are the emitter's.
    #[test]
    fn a_record_cannot_write_the_metadata_keys() {
        let record = Record {
            nbt: BTreeMap::from([
                ("LootTable".into(), Nbt::String("minecraft:chests/x".into())),
                ("id".into(), Nbt::String("minecraft:beacon".into())),
                ("x".into(), Nbt::Int(-1)),
                ("keepPacked".into(), Nbt::Byte(1)),
            ]),
            loot_seed: Some(3),
        };
        let got = for_recorded("minecraft:chest", &record, 10, 20, 30).expect("has one");
        let m = compound(&got);
        assert_eq!(m.get("id"), Some(&Nbt::String("minecraft:chest".into())));
        assert_eq!(m.get("x"), Some(&Nbt::Int(10)));
        assert_eq!(m.get("keepPacked"), Some(&Nbt::Byte(0)));
    }

    /// A loot table with no seed emits nothing and is counted as incomplete,
    /// not as an orphan. The two absences have different fixes — one needs a
    /// class ported, the other needs a draw threaded — and a single counter
    /// would hide the difference.
    #[test]
    fn a_loot_table_with_no_seed_is_refused_and_counted_separately() {
        let section = Section {
            y: 0,
            palette: vec!["minecraft:chest".into()],
            blocks: Vec::new(),
            biome_palette: vec!["minecraft:plains".into()],
            biomes: Vec::new(),
            extra: std::collections::BTreeMap::new(),
        };
        let seedless = Record {
            nbt: BTreeMap::from([(
                "LootTable".into(),
                Nbt::String("minecraft:chests/shipwreck_supply".into()),
            )]),
            loot_seed: None,
        };
        let (out, stats) = from_sections_recorded(&[section], 0, 0, &[((0, 0, 0), seedless)]);
        assert!(
            out.iter()
                .map(compound)
                .all(|m| m.get("y") != Some(&Nbt::Int(0))
                    || m.get("x") != Some(&Nbt::Int(0))
                    || m.get("z") != Some(&Nbt::Int(0))),
            "the seedless position emitted a payload"
        );
        assert_eq!(stats.incomplete, 1);
        assert_eq!(stats.orphaned, 0, "the block is there; the seed is not");
        assert_eq!(stats.recorded, 0);
    }

    /// An empty record emits nothing rather than a payload of pure metadata.
    /// A tag carrying only `id`/`components`/`keepPacked`/position is a
    /// *wrong* payload, which scores worse than the absence this project
    /// already reads as "not yet reproduced".
    #[test]
    fn a_record_that_completes_to_nothing_emits_nothing() {
        let empty = Record::default();
        // `decorated_pot` has no arm in `complete`: its payload is the
        // template's `sherds` and the loot keys, all of them the placer's, so
        // an empty record really does complete to nothing.
        assert!(for_recorded("minecraft:decorated_pot", &empty, 0, 0, 0).is_none());
        // A vault is NOT one of these: every vault has a `VaultConfig.DEFAULT`
        // to save, and the codec's `lenientOptionalFieldOf` fields all omit
        // themselves at their defaults, so it saves `config:{}`.
        let v = for_recorded("minecraft:vault", &empty, 0, 0, 0).expect("a vault emits");
        assert_eq!(
            compound(&v).get("config"),
            Some(&Nbt::Compound(BTreeMap::new())),
            "an all-default config encodes to an empty map, not to no key"
        );
        // A container with no recorded draw is the third: nothing here can
        // say its container is empty, because a data marker can set a table
        // after the template is placed.
        assert!(for_recorded("minecraft:chest", &empty, 0, 0, 0).is_none());

        // ...and the classes whose completion supplies keys of its own still
        // emit, because for those the save really does write something. The
        // spawner's seven constant shorts are written whatever the placer
        // recorded, and a vault WITH a config gains its two empty maps.
        assert!(for_recorded("minecraft:spawner", &empty, 0, 0, 0).is_some());
        let configured = Record {
            nbt: BTreeMap::from([("config".to_string(), Nbt::Compound(BTreeMap::new()))]),
            loot_seed: None,
        };
        assert!(for_recorded("minecraft:vault", &configured, 0, 0, 0).is_some());
    }

    /// The two type ids that are not their block's name. `ender_chest` was
    /// asserted to NOT resolve here until an end city's `third_floor_2`
    /// placed one: it is still not a `RandomizableContainer` (no draw is
    /// spent on it), but it is a block entity vanilla saves.
    #[test]
    fn the_type_id_is_read_off_the_registration_not_the_block_name() {
        assert_eq!(
            entity_id("minecraft:spawner"),
            Some("minecraft:mob_spawner")
        );
        assert_eq!(
            entity_id("minecraft:suspicious_sand"),
            Some("minecraft:brushable_block")
        );
        assert_eq!(
            entity_id("minecraft:suspicious_gravel"),
            Some("minecraft:brushable_block")
        );
        assert_eq!(entity_id("minecraft:chest"), Some("minecraft:chest"));
        assert_eq!(entity_id("minecraft:mob_spawner"), None, "not a block");
        assert_eq!(
            entity_id("minecraft:ender_chest"),
            Some("minecraft:ender_chest")
        );
        assert_eq!(
            entity_id("minecraft:magenta_wall_banner"),
            Some("minecraft:banner")
        );
        assert_eq!(
            entity_id("minecraft:light_gray_banner"),
            Some("minecraft:banner")
        );
        assert_eq!(entity_id("minecraft:banner"), None, "not a block");
        assert_eq!(entity_id("minecraft:grey_banner"), None, "not a colour");
        assert_eq!(entity_id("minecraft:stone"), None);
    }

    /// `newBlockEntity` ignores its `BlockState`, so the *name* is the key.
    /// A caller that hands over a full state string must get nothing, rather
    /// than silently matching — that would be a lookup that works on the one
    /// state the census contained and fails on every other.
    #[test]
    fn the_key_is_the_block_name_not_the_block_state() {
        assert!(is_reproduced("minecraft:potent_sulfur"));
        assert!(
            for_block("minecraft:potent_sulfur[potent_sulfur_state=wet]", 0, 0, 0).is_none(),
            "callers must strip properties; a silent match here would hide a caller bug"
        );
    }

    /// An end city banner at (1342,136,969) in `van-end-20260722-c8362`:
    /// `{patterns:[{color:"black",pattern:"minecraft:triangle_top"},
    /// {color:"black",pattern:"minecraft:triangle_bottom"}], components:{},
    /// keepPacked:0b, id:"minecraft:banner"}` plus the position — the
    /// template's tag minus its `id`.
    #[test]
    fn an_end_city_banner_keeps_its_template_patterns() {
        let layer = |p: &str| {
            Nbt::Compound(BTreeMap::from([
                ("color".to_string(), Nbt::String("black".into())),
                ("pattern".to_string(), Nbt::String(p.into())),
            ]))
        };
        let patterns = Nbt::List(vec![
            layer("minecraft:triangle_top"),
            layer("minecraft:triangle_bottom"),
        ]);
        let record = Record {
            nbt: BTreeMap::from([
                ("patterns".to_string(), patterns.clone()),
                ("id".to_string(), Nbt::String("minecraft:banner".into())),
            ]),
            loot_seed: None,
        };
        let got = for_recorded("minecraft:magenta_wall_banner", &record, 1342, 136, 969)
            .expect("a banner emits");
        let m = compound(&got);
        assert_eq!(m.get("patterns"), Some(&patterns));
        assert_eq!(m.get("id"), Some(&Nbt::String("minecraft:banner".into())));
        assert_eq!(m.len(), 7, "patterns plus the six metadata keys: {m:?}");
    }

    /// A woodland mansion's plain banner and an end city's ender chest save
    /// pure metadata — six keys — and an empty record for either is a payload,
    /// not a placer that knew nothing.
    #[test]
    fn a_plain_banner_and_an_ender_chest_save_only_metadata() {
        for block in ["minecraft:white_wall_banner", "minecraft:ender_chest"] {
            let got = for_recorded(block, &Record::default(), 549, 66, 228).expect(block);
            assert_eq!(compound(&got).len(), 6, "{block}: {got:?}");
        }
        // The mansion's template tag is `{patterns:[]}`, and the save omits
        // an EMPTY layer list.
        let plain = Record {
            nbt: BTreeMap::from([("patterns".to_string(), Nbt::List(Vec::new()))]),
            loot_seed: None,
        };
        let got = for_recorded("minecraft:white_wall_banner", &plain, 549, 66, 228).expect("a");
        assert_eq!(compound(&got).len(), 6, "{got:?}");
    }

    /// The mansion chest at (561,71,231) in `sl2-wm1-a` holds eight alliums
    /// straight out of its template, with no table: vanilla keeps them. An
    /// end city's marker chest has `Items:[]` in its template and gets its
    /// table afterwards; without that second record it stays absent, and
    /// with it the table wins and `Items` is not saved.
    #[test]
    fn a_template_chest_keeps_named_items_and_a_table_suppresses_them() {
        let allium = |slot: i8| {
            Nbt::Compound(BTreeMap::from([
                ("count".to_string(), Nbt::Int(1)),
                ("Slot".to_string(), Nbt::Byte(slot)),
                ("id".to_string(), Nbt::String("minecraft:allium".into())),
            ]))
        };
        let items = Nbt::List(vec![allium(8), allium(10)]);
        let named = Record {
            nbt: BTreeMap::from([("Items".to_string(), items.clone())]),
            loot_seed: Some(42),
        };
        let got = for_recorded("minecraft:chest", &named, 561, 71, 231).expect("emits");
        assert_eq!(compound(&got).get("Items"), Some(&items));
        assert!(!compound(&got).contains_key("LootTableSeed"));

        let empty_items = Record {
            nbt: BTreeMap::from([("Items".to_string(), Nbt::List(Vec::new()))]),
            loot_seed: Some(42),
        };
        let got = for_recorded("minecraft:chest", &empty_items, 0, 0, 0).expect("drawn");
        assert_eq!(
            compound(&got).get("Items"),
            Some(&Nbt::List(Vec::new())),
            "a drawn, tableless chest is empty and saves Items:[] (a mansion storeroom)"
        );
        let undrawn = Record {
            loot_seed: None,
            ..empty_items
        };
        assert!(
            for_recorded("minecraft:chest", &undrawn, 0, 0, 0).is_none(),
            "with no stream threaded, an empty list may be a marker chest's: absent"
        );

        let mut both = loot("minecraft:chests/end_city_treasure", -7);
        both.nbt.insert("Items".into(), items);
        let got = for_recorded("minecraft:chest", &both, 0, 0, 0).expect("emits");
        assert!(!compound(&got).contains_key("Items"));
        assert_eq!(compound(&got).get("LootTableSeed"), Some(&Nbt::Long(-7)));
    }

    /// An end city ship's bow and its brewing stand, from `sl2-ec1-a`:
    /// `{id:"minecraft:skull"}` plus metadata, and the stand's template tag
    /// carried whole — `BrewTime` a short and `Fuel` a byte, as saved.
    #[test]
    fn a_ship_keeps_its_dragon_head_and_its_potions() {
        let head = for_recorded(
            "minecraft:dragon_wall_head",
            &Record::default(),
            1306,
            122,
            -35,
        )
        .expect("a skull emits");
        assert_eq!(
            compound(&head).get("id"),
            Some(&Nbt::String("minecraft:skull".into()))
        );
        assert_eq!(compound(&head).len(), 6);

        let potion = Nbt::Compound(BTreeMap::from([
            ("count".to_string(), Nbt::Int(1)),
            ("Slot".to_string(), Nbt::Byte(0)),
            ("id".to_string(), Nbt::String("minecraft:potion".into())),
        ]));
        let stand = Record {
            nbt: BTreeMap::from([
                ("Items".to_string(), Nbt::List(vec![potion.clone()])),
                ("BrewTime".to_string(), Nbt::Short(0)),
                ("Fuel".to_string(), Nbt::Byte(0)),
                (
                    "id".to_string(),
                    Nbt::String("minecraft:brewing_stand".into()),
                ),
            ]),
            loot_seed: None,
        };
        let got = for_recorded("minecraft:brewing_stand", &stand, 1306, 121, -10).expect("emits");
        let m = compound(&got);
        assert_eq!(m.get("Items"), Some(&Nbt::List(vec![potion])));
        assert_eq!(m.get("BrewTime"), Some(&Nbt::Short(0)));
        assert_eq!(m.get("Fuel"), Some(&Nbt::Byte(0)));
        assert_eq!(
            m.len(),
            9,
            "three saved keys and the six metadata keys: {m:?}"
        );
    }

    /// A pillager outpost's ominous banner, `watchtower.nbt`: the template's
    /// `components` are what vanilla saves, not the empty map.
    #[test]
    fn a_template_banners_components_are_kept() {
        let components = Nbt::Compound(BTreeMap::from([(
            "minecraft:rarity".to_string(),
            Nbt::String("uncommon".into()),
        )]));
        let record = Record {
            nbt: BTreeMap::from([("components".to_string(), components.clone())]),
            loot_seed: None,
        };
        let got = for_recorded("minecraft:white_wall_banner", &record, 272, 83, -956).expect("a");
        assert_eq!(compound(&got).get("components"), Some(&components));
    }

    /// The igloo's basement sign, `igloo/bottom.nbt` against `sl2-igloo2-a`
    /// (323,60,1153): component lines become strings, the matching
    /// `filtered_messages` goes, and `is_waxed:0b` is written.
    #[test]
    fn an_igloo_sign_saves_its_lines_as_strings() {
        let line = |t: &str| {
            Nbt::Compound(BTreeMap::from([(
                "text".to_string(),
                Nbt::String(t.into()),
            )]))
        };
        let lines = Nbt::List(vec![line(""), line("<----"), line("---->"), line("")]);
        let front = Nbt::Compound(BTreeMap::from([
            ("messages".to_string(), lines.clone()),
            ("filtered_messages".to_string(), lines),
            ("color".to_string(), Nbt::String("black".into())),
            ("has_glowing_text".to_string(), Nbt::Byte(0)),
        ]));
        let record = Record {
            nbt: BTreeMap::from([("front_text".to_string(), front)]),
            loot_seed: None,
        };
        let got = for_recorded("minecraft:spruce_wall_sign", &record, 323, 60, 1153).expect("a");
        let m = compound(&got);
        assert_eq!(m.get("id"), Some(&Nbt::String("minecraft:sign".into())));
        assert_eq!(m.get("is_waxed"), Some(&Nbt::Byte(0)));
        let Some(Nbt::Compound(front)) = m.get("front_text") else {
            panic!("front_text: {m:?}")
        };
        assert!(!front.contains_key("filtered_messages"));
        assert_eq!(
            front.get("messages"),
            Some(&Nbt::List(
                ["", "<----", "---->", ""]
                    .map(|s| Nbt::String(s.into()))
                    .to_vec()
            ))
        );
    }

    /// The ancient city's sculk, `sl2-city1-a`, typed: a sensor saves
    /// `last_vibration_frequency:0` and an untouched listener, a shrieker
    /// `warning_level:0` and the same listener, a catalyst an empty
    /// `cursors` — functions of the block, derived with no record.
    #[test]
    fn sculk_block_entities_are_derived_from_the_block() {
        let sensor = for_block("minecraft:sculk_sensor", 1, 2, 3).expect("derived");
        let m = compound(&sensor);
        assert_eq!(m.get("last_vibration_frequency"), Some(&Nbt::Int(0)));
        let Some(Nbt::Compound(listener)) = m.get("listener") else {
            panic!("{m:?}")
        };
        assert_eq!(listener.get("event_delay"), Some(&Nbt::Int(0)));
        assert_eq!(
            listener.get("selector"),
            Some(&Nbt::Compound(BTreeMap::from([(
                "tick".to_string(),
                Nbt::Long(-1)
            )])))
        );
        let shrieker = for_block("minecraft:sculk_shrieker", 1, 2, 3).expect("derived");
        assert_eq!(compound(&shrieker).get("warning_level"), Some(&Nbt::Int(0)));
        let catalyst = for_block("minecraft:sculk_catalyst", 1, 2, 3).expect("derived");
        assert_eq!(
            compound(&catalyst).get("cursors"),
            Some(&Nbt::List(Vec::new()))
        );
        assert!(is_reproduced("minecraft:sculk_catalyst"));
    }
}
