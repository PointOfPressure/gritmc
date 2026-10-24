//! `ItemStack.useOn(UseOnContext)` — the item's own turn at a block it does
//! not place itself.
//!
//! # Where this sits, and the ordering claim it refutes
//!
//! It is easy to describe a hoe as "an item that runs before the block", and
//! that is **backwards**. `ServerPlayerGameMode.useItemOn` is one straight
//! line and the item is last:
//!
//! ```text
//!   126      if (secondary) goto 223                      // sneak + holding
//!   145      r = state.useItemOn(stack, level, player, hand, hit)
//!   152-157  if r.consumesAction()                 -> r
//!   179-187  if r instanceof TryEmptyHandInteraction && hand == MAIN_HAND
//!   196      r2 = state.useWithoutItem(level, player, hit)
//!   203-208  if r2.consumesAction()                -> r2
//!   223      if stack.isEmpty() || onCooldown      -> PASS
//!   245      ctx = new UseOnContext(player, hand, hit)
//!   275/292  r3 = stack.useOn(ctx)                        // <- this module
//!   299-304  if r3.consumesAction()                -> r3
//! ```
//!
//! So `ItemStack.useOn` runs **after** both block calls, at offset 275, and
//! only when neither of them consumed the click. Nothing in this module can
//! pre-empt a lever or a door, and a version that ran it first would place a
//! fire against a chest instead of opening it.
//!
//! Vanilla reaches this by virtual dispatch on the item: `BlockItem.useOn`
//! calls `place(new BlockPlaceContext(ctx))` and `Item.useOn` — the default
//! every other item inherits — returns `PASS`. This server had the
//! `BlockItem` arm inlined into
//! [`crate::world_send::handle_use_item_on`] and nothing else, so a hoe and a
//! flint-and-steel each fell into the placement path, found no block to
//! place, and wrote nothing. That is a **missing dispatch**, not a missing
//! feature of either item, which is why both convert together.
//!
//! # What is here and what is not
//!
//! Two of vanilla's `useOn` overrides are ported. The rest — and there are
//! dozens — return [`UseOn::Pass`] from [`use_on`]'s fallthrough, which is
//! exactly `Item.useOn`'s own body, so an unported item is *correct* rather
//! than merely unhandled. `BlockItem` is deliberately **not** here: its arm
//! stays inlined in the caller, and the caller runs it when this returns
//! `Pass`.
//!
//! Named gaps, so nobody re-derives them:
//!
//! * **`FlintAndSteelItem`'s lighting branch** (offsets 26-134) — campfires,
//!   candles and candle cakes, via `CampfireBlock.canLight`,
//!   `CandleBlock.canLight` and `CandleCakeBlock.canLight`. All three are
//!   `state.is(BlockTags.CAMPFIRES / CANDLES / CANDLE_CAKES)` conjoined with
//!   `!WATERLOGGED` and `!LIT`, and **nothing in this tree reads a block
//!   tag**. Checked rather than assumed, and the check is worth writing down:
//!   `data/vanilla/tags_block.json` *does* exist on the box and *does* carry
//!   all three, but no file under `ci/`, `tools/` or `crates/` mentions it,
//!   `ci/gen-gameplay-tables.sh`'s `generator_for` has no arm for it, and it
//!   carries no provenance stamp — so it is another branch's artefact that
//!   arrived through the shared `data/vanilla` copy, and reading it here
//!   would gate this branch against that one. That is the 2026-08-03
//!   shared-`oracle-work` failure by another door. Guessing the membership
//!   from block names instead would light a `soul_campfire` and miss a modded
//!   one; the branch is absent and [`flint_and_steel`] says so at its call
//!   site.
//! * **`FireBlock.getStateForPlacement`'s connection walk** — reached only
//!   when the block below is neither burnable nor sturdy-faced upward. It
//!   sets `north/east/south/west/up` from `canBurn(neighbour)`, and `canBurn`
//!   is `getIgniteOdds(state) > 0` off a 175-entry map built in
//!   `FireBlock.bootStrap()` that this tree does not extract. See
//!   [`fire_state`] for why the *other* branch needs no such table at all.
//! * **`BaseFireBlock.isPortal`** — the second disjunct of `canBePlacedAt`,
//!   which is nether-portal creation.
//! * **`HoeItem`'s `changeIntoStateAndDropItem`** — `rooted_dirt` becomes
//!   `dirt` *and* pops a `hanging_roots` item. The state change is ported;
//!   the pop is not, because `Block.popResourceFromFace` spawns an item
//!   entity and this server spawns those only from a player's own throw.
//!   [`TILLABLES`] carries the drop so the gap is one call site rather than a
//!   missing row.
//! * **`ItemStack.hurtAndBreak`** — both items damage themselves. There is no
//!   durability in this server and the corpus is creative, where
//!   `hasInfiniteMaterials()` makes vanilla restore the count anyway
//!   (offsets 260-286).

use crate::interact::{BlockPos, Face};

/// `InteractionResult`, restricted to the three arms `useOn` can return.
///
/// The distinction that matters to the caller is between [`Self::Pass`] and
/// [`Self::Fail`]: vanilla's `consumesAction()` is false for both, so neither
/// reaches the advancement trigger — but `Pass` means *this item had no
/// opinion*, and only `Pass` may fall through to the placement path. A
/// flint-and-steel aimed at a wall it cannot light returns `FAIL` and places
/// nothing, and a version that treated the two alike would place a block from
/// an item that is not a `BlockItem`.
#[derive(Debug, Clone, PartialEq)]
pub enum UseOn {
    /// `InteractionResult.PASS` — the item declined. `BlockItem`'s inlined
    /// placement arm in the caller runs next.
    Pass,
    /// `InteractionResult.FAIL` — the item took the click and refused it.
    /// Nothing is written and nothing is placed.
    Fail,
    /// `InteractionResult.SUCCESS`, with the single write it asks for.
    Success(Effect),
}

/// What a successful `useOn` does to the world.
///
/// One write, because both ported items make exactly one `Level.setBlock`
/// call. It is a struct rather than a tuple so that a third item wanting two
/// writes grows a field instead of changing every match arm.
#[derive(Debug, Clone, PartialEq)]
pub struct Effect {
    /// The position written. **Not always the clicked position** — a hoe
    /// tills the block it was clicked on, a flint-and-steel writes to
    /// `clicked.relative(clickedFace)`.
    pub at: BlockPos,
    /// The state written, as `block_props.json` spells it.
    pub state: String,
    /// The `Level.setBlock` flags, from the bytecode rather than from a
    /// constant. Both ported call sites `bipush 11` —
    /// `UPDATE_NEIGHBORS | UPDATE_CLIENTS | 8`, where 8 is `UPDATE_IMMEDIATE`
    /// and is a client-side re-render hint with no server-side meaning. It is
    /// carried rather than masked off so the number in this field is the
    /// number in the jar.
    pub flags: i32,
    /// The sound vanilla plays, or `None` where this call site plays none.
    ///
    /// It read "or `None` where the event is known and its *pitch* is not"
    /// until 2026-09-05, which was a policy rather than a fact about vanilla
    /// and is gone: `crate::sounds::drawn_pitch` produces the right *range*
    /// for every drawn pitch in this server, and the field is not compared.
    pub sound: Option<crate::sounds::Sound>,
    /// An item vanilla pops out of the block, which this server does not
    /// spawn. Recorded so the gap is inspectable rather than silent; the
    /// caller ignores it. Only `rooted_dirt` sets it.
    pub drops: Option<&'static str>,
}

/// The world `useOn` reads. Closures rather than a trait, matching
/// [`crate::placement::Around`], so a unit test can stand up a three-block
/// world without implementing [`crate::edits::Blocks`].
pub struct At<'a> {
    /// `Level.getBlockState(pos)`, as a full state string.
    pub state_at: &'a dyn Fn(BlockPos) -> String,
    /// `BlockState.isFaceSturdy(level, pos, face)`.
    pub face_sturdy: &'a dyn Fn(BlockPos, Face) -> bool,
    /// `Block.defaultBlockState()` for a block name, or `None` if the block
    /// table does not know it.
    pub default_state: &'a dyn Fn(&str) -> Option<String>,
    /// `FireBlock.igniteOdds`, the map `canBurn` reads.
    ///
    /// `None` restores the refusal this field was added to remove: without
    /// the odds, [`fire_state`] cannot answer the connection walk and returns
    /// `None` rather than a guessed state. That is deliberately kept
    /// reachable — a checkout with no vanilla extract must still boot — and
    /// `tests/session_joint.rs` cuts it as its seventh arm.
    pub fire_odds: Option<&'a crate::fire::FireOdds>,
}

/// So that [`crate::fire`]'s placement half runs over an `At` without a
/// second transcription of it. See [`crate::fire::FireLevel`].
impl crate::fire::FireLevel for At<'_> {
    fn state_at(&self, pos: BlockPos) -> String {
        (self.state_at)(pos)
    }
    fn face_sturdy(&self, pos: BlockPos, face: Face) -> bool {
        (self.face_sturdy)(pos, face)
    }
    fn default_state(&self, block: &str) -> Option<String> {
        (self.default_state)(block)
    }
}

/// `HoeItem.TILLABLES` — the static `ImmutableMap.of(...)` at offsets 0-109
/// of `<clinit>`, in its own order.
///
/// Each row is `(clicked block, predicate, resulting block, popped item)`.
/// The predicate is the `Pair.getFirst()`; four of the five rows carry
/// `lambda$static$0`, whose whole body is `iconst_1; ireturn`, and only
/// `rooted_dirt` carries `HoeItem::onlyIfAirAbove`. Resolved through the
/// `BootstrapMethods` constant pool (`javap -v`), because
/// `// InvokeDynamic #2:test` is the same text for both and reading the
/// disassembly alone would have made all five unconditional.
///
/// **`onlyIfAirAbove` on `grass_block` would be the plausible wrong answer**
/// and it is not what 26.2 does: a hoe tills grass through its underside.
static TILLABLES: &[(&str, Predicate, &str, Option<&'static str>)] = &[
    (
        "minecraft:grass_block",
        Predicate::Always,
        "minecraft:farmland",
        None,
    ),
    (
        "minecraft:dirt_path",
        Predicate::Always,
        "minecraft:farmland",
        None,
    ),
    (
        "minecraft:dirt",
        Predicate::Always,
        "minecraft:farmland",
        None,
    ),
    (
        "minecraft:coarse_dirt",
        Predicate::Always,
        "minecraft:dirt",
        None,
    ),
    (
        "minecraft:rooted_dirt",
        Predicate::OnlyIfAirAbove,
        "minecraft:dirt",
        Some("minecraft:hanging_roots"),
    ),
];

/// The `Predicate<UseOnContext>` half of a [`TILLABLES`] row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Predicate {
    /// `HoeItem.lambda$static$0` — `iconst_1; ireturn`.
    Always,
    /// `HoeItem.onlyIfAirAbove`:
    ///
    /// ```text
    ///   return ctx.getClickedFace() != Direction.DOWN
    ///       && ctx.getLevel().getBlockState(ctx.getClickedPos().above()).isAir();
    /// ```
    OnlyIfAirAbove,
}

impl Predicate {
    fn test(self, pos: BlockPos, face: Face, w: &At<'_>) -> bool {
        match self {
            Self::Always => true,
            Self::OnlyIfAirAbove => {
                face != Face::Down
                    && crate::blockentity::block_of(&(w.state_at)((pos.0, pos.1 + 1, pos.2)))
                        == "minecraft:air"
            }
        }
    }
}

/// The seven `HoeItem` registrations in `net.minecraft.world.item.Items`.
///
/// Counted two ways because a hardcoded list is exactly the kind of thing
/// that silently loses a member: seven `new HoeItem` sites in `Items`'
/// disassembly, and seven ids ending `_hoe` in vanilla's own registry report.
/// **`copper_hoe` is new in 26.2** and is the reason this is not the six-name
/// list an older memory would produce.
const HOES: &[&str] = &[
    "minecraft:copper_hoe",
    "minecraft:diamond_hoe",
    "minecraft:golden_hoe",
    "minecraft:iron_hoe",
    "minecraft:netherite_hoe",
    "minecraft:stone_hoe",
    "minecraft:wooden_hoe",
];

/// The seven `ShovelItem` registrations, counted the way [`HOES`] is.
///
/// Read off the **class column** of `data/vanilla/item_props.json` rather
/// than off a `_shovel` suffix: the suffix is a naming convention and the
/// class is what vanilla dispatches `useOn` through, and this branch added
/// that column precisely so a list like this stops being a guess. Seven
/// items, `copper_shovel` among them for the same reason `copper_hoe` is in
/// [`HOES`].
const SHOVELS: &[&str] = &[
    "minecraft:copper_shovel",
    "minecraft:diamond_shovel",
    "minecraft:golden_shovel",
    "minecraft:iron_shovel",
    "minecraft:netherite_shovel",
    "minecraft:stone_shovel",
    "minecraft:wooden_shovel",
];

/// `ShovelItem.FLATTENABLES` — the `ImmutableMap.Builder` chain at offsets
/// 0-88 of `<clinit>`, in its own order.
///
/// Six rows, every one of them `Blocks.DIRT_PATH.defaultBlockState()`. It is
/// a map to a **`BlockState`** rather than to a `Block` — unlike
/// [`TILLABLES`], whose consumer calls `defaultBlockState()` itself — so the
/// state written is the default and never one derived from the clicked
/// block: a `grass_block[snowy=true]` flattens to a plain `dirt_path`.
///
/// The order is preserved even though nothing can read two rows: the map is
/// keyed by block and the lookup is `get`, so a duplicate key would be the
/// only way order could matter, and there is none. Kept in bytecode order
/// because a reordered list is a list somebody has edited.
static FLATTENABLES: &[(&str, &str)] = &[
    ("minecraft:grass_block", "minecraft:dirt_path"),
    ("minecraft:dirt", "minecraft:dirt_path"),
    ("minecraft:podzol", "minecraft:dirt_path"),
    ("minecraft:coarse_dirt", "minecraft:dirt_path"),
    ("minecraft:mycelium", "minecraft:dirt_path"),
    ("minecraft:rooted_dirt", "minecraft:dirt_path"),
];

/// The seven `AxeItem` registrations, read off the class column of
/// `data/vanilla/item_props.json` exactly as [`SHOVELS`] is.
const AXES: &[&str] = &[
    "minecraft:copper_axe",
    "minecraft:diamond_axe",
    "minecraft:golden_axe",
    "minecraft:iron_axe",
    "minecraft:netherite_axe",
    "minecraft:stone_axe",
    "minecraft:wooden_axe",
];

/// `AxeItem.STRIPPABLES` — the 23 `ImmutableMap.Builder.put` pairs of
/// `<clinit>`, in bytecode order.
///
/// Block to block, not block to state: `AxeItem.lambda$getStripped$0` builds
/// the state itself as
/// `stripped.defaultBlockState().setValue(RotatedPillarBlock.AXIS,
/// clicked.getValue(AXIS))`, so **the axis is carried over from the clicked
/// block** and a table of states could not express it. That is the one thing
/// in this function a plausible implementation gets wrong: stripping a
/// horizontal log with the default state would silently stand it upright.
///
/// All 23 are `RotatedPillarBlock`s and every one therefore has an `axis`, so
/// the lookup below never has to cope with a missing property. `bamboo_block`
/// is in the list and is the only member that is not a log, wood, stem or
/// hyphae.
static STRIPPABLES: &[(&str, &str)] = &[
    ("minecraft:oak_wood", "minecraft:stripped_oak_wood"),
    ("minecraft:oak_log", "minecraft:stripped_oak_log"),
    (
        "minecraft:dark_oak_wood",
        "minecraft:stripped_dark_oak_wood",
    ),
    ("minecraft:dark_oak_log", "minecraft:stripped_dark_oak_log"),
    (
        "minecraft:pale_oak_wood",
        "minecraft:stripped_pale_oak_wood",
    ),
    ("minecraft:pale_oak_log", "minecraft:stripped_pale_oak_log"),
    ("minecraft:acacia_wood", "minecraft:stripped_acacia_wood"),
    ("minecraft:acacia_log", "minecraft:stripped_acacia_log"),
    ("minecraft:cherry_wood", "minecraft:stripped_cherry_wood"),
    ("minecraft:cherry_log", "minecraft:stripped_cherry_log"),
    ("minecraft:birch_wood", "minecraft:stripped_birch_wood"),
    ("minecraft:birch_log", "minecraft:stripped_birch_log"),
    ("minecraft:jungle_wood", "minecraft:stripped_jungle_wood"),
    ("minecraft:jungle_log", "minecraft:stripped_jungle_log"),
    ("minecraft:spruce_wood", "minecraft:stripped_spruce_wood"),
    ("minecraft:spruce_log", "minecraft:stripped_spruce_log"),
    ("minecraft:warped_stem", "minecraft:stripped_warped_stem"),
    (
        "minecraft:warped_hyphae",
        "minecraft:stripped_warped_hyphae",
    ),
    ("minecraft:crimson_stem", "minecraft:stripped_crimson_stem"),
    (
        "minecraft:crimson_hyphae",
        "minecraft:stripped_crimson_hyphae",
    ),
    (
        "minecraft:mangrove_wood",
        "minecraft:stripped_mangrove_wood",
    ),
    ("minecraft:mangrove_log", "minecraft:stripped_mangrove_log"),
    ("minecraft:bamboo_block", "minecraft:stripped_bamboo_block"),
];

/// The `GrowingPlantHeadBlock`s — the four blocks `ShearsItem.useOn` acts on.
///
/// **Counted two independent ways, because this is a `instanceof` dispatch
/// and there is no table of block classes in this tree to read it from.**
/// Four classes in the jar have `GrowingPlantHeadBlock` as their direct
/// superclass — `CaveVinesBlock`, `KelpBlock`, `TwistingVinesBlock`,
/// `WeepingVinesBlock` — and exactly four blocks in Mojang's own
/// `generated/reports/blocks.json` carry an `age` property of `0..=25`. The
/// two lists are the same four ids. A single-source list here would be the
/// kind that silently loses a member at the next version; two sources that
/// agree is what this project asks for instead.
///
/// The **body** blocks (`cave_vines_plant`, `kelp_plant`, …) are
/// `GrowingPlantBodyBlock` and are deliberately absent: they carry no `age`
/// and vanilla's `instanceof` does not match them, so shearing the middle of
/// a vine does nothing.
const GROWING_PLANT_HEADS: &[&str] = &[
    "minecraft:cave_vines",
    "minecraft:kelp",
    "minecraft:twisting_vines",
    "minecraft:weeping_vines",
];

/// `GrowingPlantHeadBlock.MAX_AGE` — `public static final int MAX_AGE = 25`,
/// and the `bipush 25` in both `isMaxAge` and `getMaxAgeState`.
const GROWING_PLANT_MAX_AGE: &str = "25";

/// `Level.setBlockAndUpdate(pos, state)` — `setBlock(pos, state, 3)`.
///
/// `UPDATE_NEIGHBORS | UPDATE_CLIENTS`, and **not** the 11 that
/// [`SET_BLOCK_FLAGS`] carries: `UPDATE_IMMEDIATE` is absent. Two ported
/// items in this module write with 11 and this one writes with 3, from three
/// different call sites in the jar, which is why the flags are a per-effect
/// field rather than a module constant.
const SET_BLOCK_AND_UPDATE_FLAGS: i32 = 3;

/// `BlockTags.SOUL_FIRE_BASE_BLOCKS`, read out of the pinned jar's own
/// datapack (`data/minecraft/tags/block/soul_fire_base_blocks.json`).
///
/// Two members, and it is inlined rather than extracted because it is the
/// only tag either ported item needs and a two-name table is not worth a
/// generator. The tags [`flint_and_steel`]'s lighting branch would need are a
/// different matter and are named in this module's header.
const SOUL_FIRE_BASE_BLOCKS: &[&str] = &["minecraft:soul_sand", "minecraft:soul_soil"];

/// `SoulFireBlock.canSurviveOnBlock(BlockState)` — the whole method,
/// `state.is(BlockTags.SOUL_FIRE_BASE_BLOCKS)`.
///
/// Exposed because [`crate::fire`]'s port of `BaseFireBlock.getState` asks
/// the same question, and two copies of a two-name tag is exactly how the
/// second one goes stale.
#[must_use]
pub fn is_soul_fire_base(state: &str) -> bool {
    SOUL_FIRE_BASE_BLOCKS.contains(&crate::blockentity::block_of(state))
}

/// `bipush 11` at `HoeItem.lambda$changeIntoState$0:9` and at
/// `FlintAndSteelItem.useOn:201`. See [`Effect::flags`].
const SET_BLOCK_FLAGS: i32 = 11;

/// `ItemStack.useOn(UseOnContext)` — dispatch on the item, `PASS` by default.
///
/// `player` is the connection id of whoever used the item, which both ported
/// sounds **exclude**: `Level.playSound(Entity except, ...)` is the
/// player-excluding overload and both call sites pass `ctx.getPlayer()`. That
/// is not incidental — it is why neither conversion depends on this server
/// getting a sound right, and why `310` and `320` observe no `sound` at all.
#[must_use]
pub fn use_on(item: &str, pos: BlockPos, face: Face, player: u64, w: &At<'_>) -> UseOn {
    if HOES.contains(&item) {
        return hoe(pos, face, player, w);
    }
    if item == "minecraft:flint_and_steel" {
        return flint_and_steel(pos, face, player, w);
    }
    if SHOVELS.contains(&item) {
        return shovel(pos, face, player, w);
    }
    if AXES.contains(&item) {
        return axe(pos, player, w);
    }
    if item == "minecraft:shears" {
        return shears(pos, player, w);
    }
    if item == "minecraft:fire_charge" {
        return fire_charge(pos, face, w);
    }
    // `Item.useOn`'s own body: `getstatic PASS; areturn`.
    UseOn::Pass
}

/// `HoeItem.useOn` — offsets 0-136.
///
/// ```text
///     0-33   pair = TILLABLES.get(level.getBlockState(clickedPos).getBlock())
///    33-39   if (pair == null) return PASS
///    42-68   if (!pair.getFirst().test(ctx)) goto 133
///    72-89   level.playSound(player, clickedPos, HOE_TILL, BLOCKS, 1.0F, 1.0F)
///    93-96   if (level.isClientSide) goto 129
///   102-108  pair.getSecond().accept(ctx)          // setBlock(pos, state, 11)
///   113-126  stack.hurtAndBreak(1, player, slot)
///       129  return SUCCESS
///       133  return PASS
/// ```
///
/// **The sound is outside the `isClientSide` guard and the write is inside
/// it**, which on a dedicated server means both happen and in that order.
/// Ported as the two statements they are rather than merged, because the
/// order is the observable one: a `playSound` reaches the broadcast with
/// nothing queued in front of it while the block change waits for the tick
/// boundary, so the sound leads on the wire either way — but only one of the
/// two orders is what the jar says.
///
/// The `Consumer` is `changeIntoState(FARMLAND.defaultBlockState())` for four
/// rows: the **default** state, not a state derived from the clicked block,
/// so `farmland[moisture=0]` regardless of what the grass looked like.
#[must_use]
pub fn hoe(pos: BlockPos, face: Face, player: u64, w: &At<'_>) -> UseOn {
    let clicked = (w.state_at)(pos);
    let Some(&(_, predicate, into, drops)) = TILLABLES
        .iter()
        .find(|(block, _, _, _)| *block == crate::blockentity::block_of(&clicked))
    else {
        return UseOn::Pass;
    };
    if !predicate.test(pos, face, w) {
        return UseOn::Pass;
    }
    let Some(state) = (w.default_state)(into) else {
        // The block table does not know `farmland`, which cannot happen with
        // a real `block_props.json` and is not worth writing an unknown state
        // for: an id-less state fails the *next* whole-chunk encode rather
        // than this placement. `PASS`, not `FAIL`, so the caller behaves as
        // it did before this module existed.
        return UseOn::Pass;
    };
    UseOn::Success(Effect {
        at: pos,
        state,
        flags: SET_BLOCK_FLAGS,
        sound: Some(crate::sounds::Sound {
            event: "minecraft:item.hoe.till".to_string(),
            source: crate::sounds::SOURCE_BLOCKS,
            at: crate::sounds::Eighths::block(pos),
            pos: crate::sounds::block_centre(pos),
            // `fconst_1`, `fconst_1` at offsets 87-88. Exact, and the reason
            // this sound is emitted where flint-and-steel's is not.
            volume: 1.0,
            pitch: 1.0,
            except: Some(player),
        }),
        drops,
    })
}

/// `ShovelItem.useOn` — offsets 0-238.
///
/// ```text
///    17-24   if (ctx.getClickedFace() == Direction.DOWN) return PASS      // 235
///    33-49   flattened = FLATTENABLES.get(state.getBlock())               // a BlockState
///    51-52   result = null
///    54-70   if (flattened != null && level.getBlockState(pos.above()).isAir())
///    73-85       level.playSound(player, pos, SHOVEL_FLATTEN, BLOCKS, 1.0F, 1.0F)
///    88-92       result = flattened
///   95-120   else if (state.getBlock() instanceof CampfireBlock && state.getValue(LIT))
///  123-137       if (!level.isClientSide) level.levelEvent(null, 1009, pos, 0)
///  140-150       CampfireBlock.dowse(ctx.getPlayer(), level, pos, state)
///  151-166       result = state.setValue(LIT, false)
///  168-170   if (result == null) return PASS                              // 231
///  173-189   if (!level.isClientSide) level.setBlock(pos, result, 11)
///  190-202       level.gameEvent(BLOCK_CHANGE, pos, Context.of(player, result))
///  205-224       if (player != null) stack.hurtAndBreak(1, player, slot)
///       227   return SUCCESS
/// ```
///
/// # Three things that are not the obvious reading
///
/// * **The `DOWN` refusal is the whole method's first act**, at offset 17,
///   *before* the block is even looked up. It is not a per-row predicate the
///   way [`Predicate::OnlyIfAirAbove`] is for a hoe, and it applies to the
///   campfire branch too: a lit campfire clicked from underneath is not
///   dowsed.
/// * **The air-above test guards only the flatten branch.** A `grass_block`
///   with something on top falls *through* to the campfire test rather than
///   returning, which matters because both branches are followed by the same
///   `result == null` check. A transcription that returned `PASS` from the
///   failed air test would be right today only because grass is not a
///   campfire.
/// * **The sound plays before the write and outside the `isClientSide`
///   guard**, exactly as in [`hoe`], and it excludes `ctx.getPlayer()` for
///   the same reason: `Level.playSound(Entity, ...)` is the
///   player-excluding overload.
///
/// # The campfire branch is refused, not ported
///
/// `CampfireBlock.dowse` needs three things this server does not have: the
/// `levelEvent(1009)` for the extinguish sound and particles (there is a
/// `crate::interact::level_event_body` but [`Effect`] carries no level event,
/// and adding a field for one call site would be a change to every match arm
/// for a mechanic with no other user), a **block entity** to clear the
/// cooking items on, and `state.setValue(LIT, false)` — a state edit of the
/// clicked block rather than a named default, which [`Effect::state`] can
/// express but the other two cannot.
///
/// So a lit campfire clicked with a shovel returns [`UseOn::Pass`] here, and
/// the caller falls through to the placement path, which finds nothing to
/// place. **That is the same answer this server gave before this function
/// existed**, so the branch is unported rather than newly wrong, and no
/// scenario can regress on it. The corpus contains no campfire.
#[must_use]
pub fn shovel(pos: BlockPos, face: Face, player: u64, w: &At<'_>) -> UseOn {
    // Offset 17. Before the block lookup, and it covers both branches.
    if face == Face::Down {
        return UseOn::Pass;
    }
    let clicked_state = (w.state_at)(pos);
    let clicked = crate::blockentity::block_of(&clicked_state);
    let Some(&(_, into)) = FLATTENABLES.iter().find(|(block, _)| *block == clicked) else {
        // Offsets 95-120, the campfire branch, refused above.
        return UseOn::Pass;
    };
    let above = (w.state_at)((pos.0, pos.1 + 1, pos.2));
    if crate::blockentity::block_of(&above) != "minecraft:air" {
        return UseOn::Pass;
    }
    let Some(state) = (w.default_state)(into) else {
        // As in `hoe`: the block table does not know `dirt_path`, which
        // cannot happen with a real `block_props.json`.
        return UseOn::Pass;
    };
    UseOn::Success(Effect {
        at: pos,
        state,
        flags: SET_BLOCK_FLAGS,
        sound: Some(crate::sounds::Sound {
            // `SoundEvents.SHOVEL_FLATTEN`, whose registration string is
            // `item.shovel.flatten` (`SoundEvents.<clinit>` offset 12720).
            // Read out of the jar rather than composed from the field name,
            // which is how `AXE_WAX_OFF` would come out as `item.axe.wax_off`
            // by luck and `ARROW_SHOOT` would not.
            event: "minecraft:item.shovel.flatten".to_string(),
            source: crate::sounds::SOURCE_BLOCKS,
            at: crate::sounds::Eighths::block(pos),
            pos: crate::sounds::block_centre(pos),
            // `fconst_1`, `fconst_1` at offsets 83-84.
            volume: 1.0,
            pitch: 1.0,
            except: Some(player),
        }),
        drops: None,
    })
}

/// `AxeItem.useOn` — offsets 0-146 — and `evaluateNewBlockState`'s first of
/// three branches.
///
/// ```text
///   AxeItem.useOn
///    16-26   if (playerHasBlockingItemUseIntent(ctx)) return PASS
///    27-53   newState = evaluateNewBlockState(level, pos, player, state)
///              if (newState.isEmpty()) return PASS
///    60-86   ITEM_USED_ON_BLOCK trigger
///    86-101  level.setBlock(pos, newState.get(), 11)
///   102-123  level.gameEvent(BLOCK_CHANGE, pos, Context.of(player, newState))
///   123-143  if (player != null) stack.hurtAndBreak(1, player, slot)
///       143  return SUCCESS
///
///   AxeItem.evaluateNewBlockState
///     0-13   stripped = getStripped(state)
///    16-32   if present: playSound(player, pos, AXE_STRIP, BLOCKS, 1, 1); return it
///    33-45   previous = WeatheringCopper.getPrevious(state)
///    48-64   if present: spawnSoundAndParticle(..., AXE_SCRAPE, 3005); return it
///    65-109  waxedOff = WAX_OFF_BY_BLOCK.get(state.getBlock())
///                          .map(b -> b.withPropertiesOf(state))
///   112-128  if present: spawnSoundAndParticle(..., AXE_WAX_OFF, 3004); return it
///       129  return Optional.empty()
/// ```
///
/// # The axis is carried, and that is the whole subtlety of the strip
///
/// `getStripped` is `STRIPPABLES.get(block)` mapped through
/// `lambda$getStripped$0`, whose body is
/// `stripped.defaultBlockState().setValue(AXIS, clicked.getValue(AXIS))`.
/// A horizontal log stripped with the default state would stand upright, and
/// there is no gate anywhere in this tree that would notice: the block id
/// would be right and only one property wrong. That is why
/// [`crate::placement::property_of`] and
/// [`crate::placement::with_property`] are used here rather than
/// `default_state` alone, and why the test sweeps all three axes.
///
/// # Two branches refused, and one guard
///
/// * **`WeatheringCopper.getPrevious`** (the scrape) and
///   **`HoneycombItem.WAX_OFF_BY_BLOCK`** (the wax-off) both end in
///   `Block.withPropertiesOf(state)`, which copies *every* property the two
///   blocks share — a general cross-block property transfer, not a named
///   substitution like the axis above. The copper family is stairs, slabs,
///   doors, trapdoors, grates and bulbs, so the shared property sets are
///   large and differ per pair. Neither map is extracted by this tree and
///   `withPropertiesOf` has no analogue here, so both return
///   [`UseOn::Pass`], which is what an axe on a copper block did before this
///   function existed.
/// * **`playerHasBlockingItemUseIntent`** is
///   `hand == MAIN_HAND && offhand.has(DataComponents.BLOCKS_ATTACKS)
///   && !player.isSecondaryUseActive()`. **Exactly one item in the 1,537-item
///   registry carries `minecraft:blocks_attacks`, and it is
///   `minecraft:shield`** (counted off `data/vanilla/item_props.json`), so
///   the guard is reachable only for a player holding a shield in the offhand
///   and not sneaking. This server tracks an offhand stack
///   ([`crate::player::Hotbar`]) but models neither the component nor
///   `isSecondaryUseActive`, so the guard is omitted and the count is written
///   here so the size of the omission is one item rather than unknown.
#[must_use]
pub fn axe(pos: BlockPos, player: u64, w: &At<'_>) -> UseOn {
    let clicked = (w.state_at)(pos);
    let block = crate::blockentity::block_of(&clicked);
    let Some(&(_, stripped)) = STRIPPABLES.iter().find(|(from, _)| *from == block) else {
        // The scrape and wax-off branches, refused above. `Optional.empty()`
        // at offset 129 is `PASS` at offset 50.
        return UseOn::Pass;
    };
    let Some(default) = (w.default_state)(stripped) else {
        return UseOn::Pass;
    };
    // `lambda$getStripped$0`: the stripped block's DEFAULT state, then the
    // clicked block's axis written over it. `with_property` leaves a state
    // without the property alone, so a strippable that stopped being a
    // pillar would silently keep the default rather than gain a property
    // vanilla's state does not have.
    let state = crate::placement::property_of(&clicked, "axis").map_or(default.clone(), |axis| {
        crate::placement::with_property(&default, "axis", axis)
    });
    UseOn::Success(Effect {
        at: pos,
        state,
        flags: SET_BLOCK_FLAGS,
        sound: Some(crate::sounds::Sound {
            // `SoundEvents.AXE_STRIP`, registration string `item.axe.strip`
            // (`SoundEvents.<clinit>` offset 792).
            event: "minecraft:item.axe.strip".to_string(),
            source: crate::sounds::SOURCE_BLOCKS,
            at: crate::sounds::Eighths::block(pos),
            pos: crate::sounds::block_centre(pos),
            // `fconst_1`, `fconst_1` at offsets 25-26 of
            // `evaluateNewBlockState`.
            volume: 1.0,
            pitch: 1.0,
            except: Some(player),
        }),
        drops: None,
    })
}

/// `ShearsItem.useOn` — offsets 0-165.
///
/// ```text
///    24-29   if (!(block instanceof GrowingPlantHeadBlock)) goto 160
///    39-46   if (head.isMaxAge(state)) goto 160
///    61-87   ITEM_USED_ON_BLOCK trigger
///    87-99   level.playSound(player, pos, GROWING_PLANT_CROP, BLOCKS, 1, 1)
///   102-118  level.setBlockAndUpdate(pos, head.getMaxAgeState(state))
///   119-136  level.gameEvent(BLOCK_CHANGE, pos, Context.of(player, newState))
///   136-156  if (player != null) stack.hurtAndBreak(1, player, slot)
///       156  return SUCCESS
///   160-165  return super.useOn(ctx)                       // Item.useOn: PASS
/// ```
///
/// `getMaxAgeState` is `state.setValue(AGE, 25)` — **the clicked state with
/// one property changed**, not a default. That matters for `cave_vines`,
/// whose `berries` property must survive: shearing a berried vine must not
/// pick its berries. The other three heads have only `age` and cannot show
/// the difference, so the test uses the one that can.
///
/// `isMaxAge` is `getValue(AGE) == 25` — equality, not `>=`. There is no
/// state above 25 for the property to hold, so the two agree; transcribed as
/// the comparison in the bytecode.
///
/// The write is `setBlockAndUpdate`, flags **3**, where [`hoe`] and
/// [`flint_and_steel`] write with 11.
///
/// **Not ported: `ShearsItem.mineBlock`**, which is the other half of this
/// class and is about drops rather than about a click.
#[must_use]
pub fn shears(pos: BlockPos, player: u64, w: &At<'_>) -> UseOn {
    let clicked = (w.state_at)(pos);
    if !GROWING_PLANT_HEADS.contains(&crate::blockentity::block_of(&clicked)) {
        return UseOn::Pass;
    }
    // `isMaxAge`. A head with no `age` cannot happen — all four carry it —
    // and if one ever did, vanilla would throw where this passes, which is
    // the safe direction for a disagreement nothing can currently reach.
    if crate::placement::property_of(&clicked, "age") == Some(GROWING_PLANT_MAX_AGE) {
        return UseOn::Pass;
    }
    UseOn::Success(Effect {
        at: pos,
        state: crate::placement::with_property(&clicked, "age", GROWING_PLANT_MAX_AGE),
        flags: SET_BLOCK_AND_UPDATE_FLAGS,
        sound: Some(crate::sounds::Sound {
            // `SoundEvents.GROWING_PLANT_CROP`, registration string
            // `block.growing_plant.crop` (`SoundEvents.<clinit>` offset 6624).
            event: "minecraft:block.growing_plant.crop".to_string(),
            source: crate::sounds::SOURCE_BLOCKS,
            at: crate::sounds::Eighths::block(pos),
            pos: crate::sounds::block_centre(pos),
            // `fconst_1`, `fconst_1` at offsets 97-98.
            volume: 1.0,
            pitch: 1.0,
            except: Some(player),
        }),
        drops: None,
    })
}

/// `FlintAndSteelItem.useOn` — offsets 135-269, the fire branch.
///
/// ```text
///   135-144  relPos = clickedPos.relative(ctx.getClickedFace())
///   146-156  if (!BaseFireBlock.canBePlacedAt(level, relPos, ctx.getHorizontalDirection()))
///                goto 266
///   159-185  level.playSound(player, relPos, FLINTANDSTEEL_USE, BLOCKS,
///                            1.0F, level.getRandom().nextFloat() * 0.4F + 0.8F)
///   188-206  level.setBlock(relPos, BaseFireBlock.getState(level, relPos), 11)
///   207-214  level.gameEvent(player, BLOCK_PLACE, relPos)
///   218-259  PLACED_BLOCK trigger, then hurtAndBreak
///       262  return SUCCESS
///       266  return FAIL
/// ```
///
/// **The sound is at `relPos`, not at the clicked block.** It was dropped
/// here until 2026-09-05, on two grounds, and the second of them was checked
/// and is false:
///
/// * *the pitch is a draw this server does not model.* True, and no longer a
///   reason. `level.getRandom().nextFloat() * 0.4F + 0.8F` is the same shape
///   as the lever's, the chest's and the piston's, and
///   [`crate::sounds::drawn_pitch`] is what this server does with all of
///   them: produce the right *range* from a generator of its own and let
///   `tools/oracle-runner` go on not comparing the field, which it has not
///   done since the commit that added `crate::sounds`. Emitting nothing is
///   not the conservative choice over emitting an uncompared number — it is
///   one wrong observation on every stream instead of none.
/// * *`ctx.getPlayer()` is excluded from it either way, so no scenario in
///   this corpus can hear it.* **Refuted by the recordings.** The exclusion
///   is real — `aload` the player, not `aconst_null` — but it excludes the
///   *actor*, and the corpus reads both streams:
///   `flint-and-steel-lights-a-fire` carries this event on the **observer's**
///   stream 30 times across the recordings on this box, at `[2.5, 1.5, 0.5]`,
///   which is `relPos`'s centre. `./ci/sound-emission-gap.py`.
///
/// So it is emitted, at `relPos`, volume 1.0, excluding the player.
///
/// `getHorizontalDirection()` is read only by `isPortal`, which is absent, so
/// it is not a parameter here.
///
/// **The lighting branch above this (offsets 26-134) is not ported** — see
/// the module header. It means a flint-and-steel used on an unlit campfire or
/// candle reaches *this* branch and tries to put fire against it, rather than
/// lighting it. Stated because it is a wrong answer rather than a missing
/// one, and because the corpus contains neither block.
#[must_use]
pub fn flint_and_steel(pos: BlockPos, face: Face, player: u64, w: &At<'_>) -> UseOn {
    let rel = face.relative(pos);
    let Some(state) = can_be_placed_at(rel, w) else {
        return UseOn::Fail;
    };
    UseOn::Success(Effect {
        at: rel,
        state,
        flags: SET_BLOCK_FLAGS,
        // Offsets 159-185, and every argument of it is load-bearing: the
        // position is `relPos` and not the clicked block, the entity is the
        // player and not `null` so the lighter does not hear their own
        // lighter, and the pitch scale is 0.4 onto 0.8 — a fourth pair of
        // constants, different again from the lever's, the chest's and both
        // of the piston's.
        //
        // It is INSIDE the `canBePlacedAt` guard: offsets 146-156 jump to the
        // `return FAIL` at 266 before reaching it, so a flint-and-steel aimed
        // at a wall it cannot light is silent. That is `UseOn::Fail` above,
        // which carries no sound, so the ordering is structural here rather
        // than a condition somebody has to keep.
        sound: Some(crate::sounds::Sound {
            event: "minecraft:item.flintandsteel.use".to_string(),
            source: crate::sounds::SOURCE_BLOCKS,
            at: crate::sounds::Eighths::block(rel),
            pos: crate::sounds::block_centre(rel),
            volume: 1.0,
            pitch: crate::sounds::drawn_pitch(0.4, 0.8),
            except: Some(player),
        }),
        drops: None,
    })
}

/// `FireChargeItem.useOn` — offsets 89-162, the fire branch.
///
/// ```text
///    89-97   pos = clickedPos.relative(ctx.getClickedFace())
///   98-107   if (!BaseFireBlock.canBePlacedAt(level, pos, horizontalDirection)) goto 142
///  110-115   playSound(level, pos)
///  116-126   level.setBlockAndUpdate(pos, BaseFireBlock.getState(level, pos))
///  127-139   level.gameEvent(player, BLOCK_PLACE, pos); lit = true
///  142-158   if (lit) { stack.shrink(1); return SUCCESS }
///       159  return FAIL
/// ```
///
/// **The same branch [`flint_and_steel`] ports, and three things differ.**
/// They are written out because "it is like the flint and steel" is exactly
/// the reading that would get each of them wrong:
///
/// * **The write is `setBlockAndUpdate`, flags 3**, where flint-and-steel is
///   `setBlock(..., 11)`. `UPDATE_IMMEDIATE` is absent, which has no server
///   meaning, so the two agree on what happens — but the number in the field
///   is the number in the jar and these are two different numbers.
/// * **The sound excludes nobody.** `FireChargeItem.playSound` passes
///   `aconst_null` as `Level.playSound`'s entity (offset 6), so the user
///   hears it; flint-and-steel passes `ctx.getPlayer()` and the user does
///   not. It is dropped here anyway, for flint-and-steel's reason: the pitch
///   is `1.0 + (nextFloat() - nextFloat()) * 0.2`, **two** draws from the
///   level `RandomSource` this server does not model, and emitting a
///   plausible number that is not vanilla's is the failure mode
///   `crate::useblock`'s header argues against.
/// * **The item is consumed** — `stack.shrink(1)` at offset 151, where
///   flint-and-steel calls `hurtAndBreak`. Neither is ported; this server has
///   no durability and creative does not consume.
///
/// **The candle/campfire branch (offsets 20-86) is not ported**, for
/// [`flint_and_steel`]'s reason and with the same consequence: a fire charge
/// used on an unlit campfire reaches *this* branch and tries to put fire
/// against it. `CampfireBlock.canLight`, `CandleBlock.canLight` and
/// `CandleCakeBlock.canLight` are all block-tag reads and nothing in this
/// tree reads a block tag.
#[must_use]
pub fn fire_charge(pos: BlockPos, face: Face, w: &At<'_>) -> UseOn {
    let rel = face.relative(pos);
    let Some(state) = can_be_placed_at(rel, w) else {
        return UseOn::Fail;
    };
    UseOn::Success(Effect {
        at: rel,
        state,
        flags: SET_BLOCK_AND_UPDATE_FLAGS,
        sound: None,
        drops: None,
    })
}

/// `BaseFireBlock.canBePlacedAt(level, pos, direction)` fused with
/// `BaseFireBlock.getState(level, pos)` — `Some(state)` where vanilla returns
/// true and would then write that state, `None` where it returns false.
///
/// ```text
///    0-14   if (!level.getBlockState(pos).isAir()) return false
///   15-25   if (getState(level, pos).canSurvive(level, pos)) return true
///   28-34   return isPortal(level, pos, direction)
/// ```
///
/// The two are fused because vanilla calls `getState` twice — once inside
/// `canBePlacedAt` and again at offset 191 to build the write — with nothing
/// between them that could change the answer.
///
/// # The branch that needs no flammability table, and the one that does
///
/// `getState` picks soul fire when the block below is in
/// [`SOUL_FIRE_BASE_BLOCKS`] and otherwise asks
/// `FireBlock.getStateForPlacement`, whose whole body is:
///
/// ```text
///   if (!canBurn(below) && !below.isFaceSturdy(getter, belowPos, UP)) {
///       <the connection walk>
///   }
///   return defaultBlockState();
/// ```
///
/// and `canSurvive` is
/// `below.isFaceSturdy(level, belowPos, UP) || isValidFireLocation(level, pos)`.
/// **A sturdy face below satisfies both**: it makes `canSurvive` true without
/// consulting `isValidFireLocation`, and it makes the connection walk's guard
/// false without consulting `canBurn`. So the sturdy path is exact, and it is
/// the only path this corpus reaches — `320` puts fire on top of a stone
/// block. Anything else needs `getIgniteOdds`, which this tree has no table
/// for, and returns `None` so that the item **fails** rather than writing a
/// fire whose connection booleans were guessed.
///
/// **That over-refusal is gone when [`At::fire_odds`] is set**, which is
/// production. It stood while nothing in this tree extracted `igniteOdds`;
/// `crate::fire` does now, and [`fire_state`] hands the walk to
/// `crate::fire::state_for_placement`. The refusal survives only for a
/// checkout with no vanilla extract, where it is still the smaller of the two
/// errors available: a missing fire is one wrong observation, a fire with the
/// wrong `north`/`east`/`south`/`west`/`up` is one wrong observation that
/// also spreads.
pub(crate) fn can_be_placed_at(pos: BlockPos, w: &At<'_>) -> Option<String> {
    if crate::blockentity::block_of(&(w.state_at)(pos)) != "minecraft:air" {
        return None;
    }
    fire_state(pos, w)
}

/// `BaseFireBlock.getState` — offsets 0-26 and its tail — restricted to the
/// branch [`can_be_placed_at`] documents.
fn fire_state(pos: BlockPos, w: &At<'_>) -> Option<String> {
    // With the odds, this is `BaseFireBlock.getState` entire — the soul-fire
    // test and `FireBlock.getStateForPlacement` including its connection
    // walk. One transcription, in `crate::fire`.
    if let Some(odds) = w.fire_odds {
        return crate::fire::base_fire_state(odds, w, pos);
    }
    // Without it — a checkout with no vanilla extract — the sturdy branch is
    // still exact and everything else refuses. See this function's docs.
    let below = (pos.0, pos.1 - 1, pos.2);
    let state_below = (w.state_at)(below);
    if is_soul_fire_base(&state_below) {
        return (w.default_state)("minecraft:soul_fire");
    }
    if !(w.face_sturdy)(below, Face::Up) {
        return None;
    }
    (w.default_state)("minecraft:fire")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A world of named states, air everywhere else. Small on purpose:
    /// everything either item reads is one of three positions.
    type World = Vec<(BlockPos, &'static str)>;

    fn state_of(w: &World, p: BlockPos) -> String {
        w.iter()
            .find(|(q, _)| *q == p)
            .map_or_else(|| "minecraft:air".to_string(), |(_, s)| (*s).to_string())
    }

    /// The three blocks these tests use that have a sturdy upward face. Not
    /// the real column — `block_props.json` carries it per state — but the
    /// distinction the fire branch turns on: `oak_fence` is the negative.
    fn sturdy_of(w: &World, p: BlockPos) -> bool {
        matches!(
            crate::blockentity::block_of(&state_of(w, p)),
            "minecraft:stone" | "minecraft:grass_block" | "minecraft:dirt"
        )
    }

    /// The block table, keeping the two-part states the corpus actually
    /// observes so a test cannot pass on a state string vanilla never emits.
    fn defaults(block: &str) -> Option<String> {
        Some(
            match block {
                "minecraft:farmland" => "minecraft:farmland[moisture=0]",
                "minecraft:dirt" => "minecraft:dirt",
                "minecraft:dirt_path" => "minecraft:dirt_path",
                // Every `STRIPPABLES` target is a `RotatedPillarBlock`, whose
                // default is `axis=y`. Built from the name so the sweep below
                // covers all 23 without twenty-three lines of fixture.
                b if b.starts_with("minecraft:stripped_") => return Some(format!("{b}[axis=y]")),
                "minecraft:fire" => {
                    "minecraft:fire[age=0,east=false,north=false,south=false,up=false,west=false]"
                }
                "minecraft:soul_fire" => "minecraft:soul_fire",
                _ => return None,
            }
            .to_string(),
        )
    }

    /// Runs `f` with an [`At`] over `w`. A closure taking a closure, because
    /// [`At`]'s fields borrow and a helper *returning* one would have to
    /// outlive them.
    fn with<R>(w: &World, f: impl FnOnce(&At<'_>) -> R) -> R {
        let state_at = |p| state_of(w, p);
        let face_sturdy = |p, _f| sturdy_of(w, p);
        f(&At {
            state_at: &state_at,
            face_sturdy: &face_sturdy,
            default_state: &defaults,
            // The tests in this module are about the *item* dispatch, and
            // they run the no-extract arm on purpose: `crate::fire`'s tests
            // cover the walk against the real table, and a fixture here would
            // make an item test fail for a table's reason.
            fire_odds: None,
        })
    }

    /// `310-hoe-turns-grass-into-farmland`, from the gate's own recording:
    /// vanilla writes `minecraft:farmland[moisture=0]` at the clicked
    /// position and this server wrote nothing.
    #[test]
    fn a_hoe_turns_grass_into_farmland_at_the_clicked_position() {
        let w: World = vec![((1, -1, 0), "minecraft:grass_block[snowy=false]")];
        with(&w, |at| {
            let r = use_on("minecraft:iron_hoe", (1, -1, 0), Face::Up, 7, at);
            let UseOn::Success(e) = r else {
                panic!("{r:?}")
            };
            assert_eq!(e.at, (1, -1, 0));
            assert_eq!(e.state, "minecraft:farmland[moisture=0]");
            assert_eq!(e.flags, 11);
            assert_eq!(e.drops, None);
            let s = e.sound.expect("HOE_TILL");
            assert!((s.pitch - 1.0).abs() < f32::EPSILON);
            assert!((s.volume - 1.0).abs() < f32::EPSILON);
            // The player-excluding overload, which is why `310` sees no sound.
            assert_eq!(s.except, Some(7));
        });
    }

    /// All seven, because the list is hand-written and `copper_hoe` is the
    /// one a pre-26.2 memory drops.
    #[test]
    fn every_hoe_in_the_registry_tills() {
        let w: World = vec![((0, 0, 0), "minecraft:dirt")];
        with(&w, |at| {
            for hoe in HOES {
                assert!(
                    matches!(use_on(hoe, (0, 0, 0), Face::Up, 1, at), UseOn::Success(_)),
                    "{hoe}"
                );
            }
        });
        assert_eq!(HOES.len(), 7);
    }

    /// The fallthrough is `Item.useOn`'s own body, and it has to be `Pass`
    /// rather than `Fail` or the caller stops placing blocks.
    #[test]
    fn an_unported_item_passes_so_the_placement_path_still_runs() {
        let w: World = vec![((0, 0, 0), "minecraft:grass_block[snowy=false]")];
        with(&w, |at| {
            assert_eq!(
                use_on("minecraft:stone", (0, 0, 0), Face::Up, 1, at),
                UseOn::Pass
            );
            assert_eq!(
                use_on("minecraft:diamond_sword", (0, 0, 0), Face::Up, 1, at),
                UseOn::Pass
            );
        });
    }

    /// A hoe on a block with no `TILLABLES` row passes, so a hoe clicked on
    /// stone still reaches the placement path rather than eating the click.
    #[test]
    fn a_hoe_on_an_untillable_block_passes() {
        let w: World = vec![((0, 0, 0), "minecraft:stone")];
        with(&w, |at| {
            assert_eq!(
                use_on("minecraft:iron_hoe", (0, 0, 0), Face::Up, 1, at),
                UseOn::Pass
            );
        });
    }

    /// `coarse_dirt` is the row that does **not** produce farmland, and
    /// `rooted_dirt` is the only one carrying a predicate and a drop.
    #[test]
    fn the_two_rows_that_are_not_farmland() {
        let w: World = vec![
            ((0, 0, 0), "minecraft:coarse_dirt"),
            ((2, 0, 0), "minecraft:rooted_dirt"),
        ];
        with(&w, |at| {
            let UseOn::Success(e) = use_on("minecraft:iron_hoe", (0, 0, 0), Face::Up, 1, at) else {
                panic!()
            };
            assert_eq!(e.state, "minecraft:dirt");
            let UseOn::Success(e) = use_on("minecraft:iron_hoe", (2, 0, 0), Face::Up, 1, at) else {
                panic!()
            };
            assert_eq!(e.state, "minecraft:dirt");
            assert_eq!(e.drops, Some("minecraft:hanging_roots"));
        });
    }

    /// `onlyIfAirAbove` is on `rooted_dirt` **and nowhere else**, which is
    /// the reading the disassembly alone cannot give: both predicates print
    /// as `InvokeDynamic #2:test` / `#3:test` and only the `BootstrapMethods`
    /// table separates them. Grass clicked from below still tills; rooted
    /// dirt with a block above it does not.
    #[test]
    fn only_rooted_dirt_carries_the_air_above_predicate() {
        let w: World = vec![
            ((0, 0, 0), "minecraft:rooted_dirt"),
            ((0, 1, 0), "minecraft:stone"),
            ((2, 0, 0), "minecraft:grass_block[snowy=false]"),
            ((2, 1, 0), "minecraft:stone"),
        ];
        with(&w, |at| {
            assert_eq!(
                use_on("minecraft:iron_hoe", (0, 0, 0), Face::Up, 1, at),
                UseOn::Pass
            );
            assert!(matches!(
                use_on("minecraft:iron_hoe", (2, 0, 0), Face::Down, 1, at),
                UseOn::Success(_)
            ));
        });
        // The face half of the same predicate, on a clear column.
        let w: World = vec![((0, 0, 0), "minecraft:rooted_dirt")];
        with(&w, |at| {
            assert_eq!(
                use_on("minecraft:iron_hoe", (0, 0, 0), Face::Down, 1, at),
                UseOn::Pass
            );
            assert!(matches!(
                use_on("minecraft:iron_hoe", (0, 0, 0), Face::Up, 1, at),
                UseOn::Success(_)
            ));
        });
    }

    /// `320-flint-and-steel-lights-a-fire`, from the gate's own recording:
    /// vanilla writes
    /// `fire[age=0,east=false,north=false,south=false,up=false,west=false]`
    /// at `[2,1,0]` after a click on `[2,0,0]`'s up face, and this server
    /// wrote `air`.
    #[test]
    fn flint_and_steel_puts_default_fire_against_the_clicked_face() {
        let w: World = vec![((2, 0, 0), "minecraft:stone")];
        with(&w, |at| {
            let r = use_on("minecraft:flint_and_steel", (2, 0, 0), Face::Up, 3, at);
            let UseOn::Success(e) = r else {
                panic!("{r:?}")
            };
            assert_eq!(e.at, (2, 1, 0));
            assert_eq!(
                e.state,
                "minecraft:fire[age=0,east=false,north=false,south=false,up=false,west=false]"
            );
            assert_eq!(e.flags, 11);
            // Offsets 159-185. Four things, and each of them was wrong in
            // some version of this port: the event, the position (`relPos`,
            // the fire's own block, not the stone that was clicked), the
            // exclusion (the player who struck it does not hear it — the
            // corpus reads this event off the OBSERVER's stream), and the
            // pitch range `nextFloat() * 0.4F + 0.8F`.
            let sound = e.sound.expect("a fire that lights is heard");
            assert_eq!(sound.event, "minecraft:item.flintandsteel.use");
            assert_eq!(sound.pos, (2.5, 1.5, 0.5));
            assert_eq!(sound.except, Some(3));
            assert_eq!(sound.volume, 1.0);
            assert!(
                (0.8..1.2).contains(&sound.pitch),
                "pitch {} outside 0.8..1.2",
                sound.pitch
            );
        });
    }

    /// The guard the sound sits behind: offsets 146-156 jump past the
    /// `playSound` to `return FAIL`, so a flint-and-steel aimed at something
    /// it cannot light is silent as well as ineffective.
    #[test]
    fn a_flint_and_steel_that_cannot_light_is_silent() {
        // Air below the target position, so `canSurvive` fails and
        // `can_be_placed_at` returns `None`.
        let w: World = vec![];
        with(&w, |at| {
            assert!(matches!(
                use_on("minecraft:flint_and_steel", (0, 0, 0), Face::Up, 1, at),
                UseOn::Fail
            ));
        });
    }

    /// Soul sand below turns the write into `soul_fire`, which is the one
    /// branch of `BaseFireBlock.getState` that is not `FireBlock`'s.
    #[test]
    fn soul_sand_below_makes_it_soul_fire() {
        for base in SOUL_FIRE_BASE_BLOCKS {
            let w: World = vec![((0, 0, 0), base)];
            with(&w, |at| {
                let r = use_on("minecraft:flint_and_steel", (0, 0, 0), Face::Up, 1, at);
                let UseOn::Success(e) = r else {
                    panic!("{base} {r:?}")
                };
                assert_eq!(e.state, "minecraft:soul_fire");
                assert_eq!(e.at, (0, 1, 0));
            });
        }
    }

    /// The two refusals, and they are `FAIL` rather than `PASS`: an item that
    /// is not a `BlockItem` must not fall through to the placement path.
    #[test]
    fn flint_and_steel_fails_rather_than_passing() {
        // Target occupied — `canBePlacedAt` offsets 0-14.
        let w: World = vec![
            ((0, 0, 0), "minecraft:stone"),
            ((0, 1, 0), "minecraft:stone"),
        ];
        with(&w, |at| {
            assert_eq!(
                use_on("minecraft:flint_and_steel", (0, 0, 0), Face::Up, 1, at),
                UseOn::Fail
            );
        });
        // Nothing sturdy below: the branch that would need `getIgniteOdds`.
        let w: World = vec![((0, 0, 0), "minecraft:oak_fence")];
        with(&w, |at| {
            assert_eq!(
                use_on("minecraft:flint_and_steel", (0, 0, 0), Face::Up, 1, at),
                UseOn::Fail
            );
        });
    }

    /// The clicked face decides where the fire goes, and a side face is the
    /// case the scenario name is about. The target's own support is the block
    /// below *it*, not the one that was clicked.
    #[test]
    fn the_fire_goes_against_the_face_that_was_clicked() {
        let w: World = vec![
            ((0, 0, 0), "minecraft:stone"),
            ((0, -1, -1), "minecraft:stone"),
        ];
        with(&w, |at| {
            let r = use_on("minecraft:flint_and_steel", (0, 0, 0), Face::North, 1, at);
            let UseOn::Success(e) = r else {
                panic!("{r:?}")
            };
            assert_eq!(e.at, (0, 0, -1));
        });
    }

    /// The shovel's flatten branch: `grass_block` with air above becomes
    /// `dirt_path` at the clicked position, with the flatten sound.
    #[test]
    fn a_shovel_flattens_grass_into_a_path() {
        let w: World = vec![((4, 70, 4), "minecraft:grass_block")];
        let got = with(&w, |at| shovel((4, 70, 4), Face::Up, 7, at));
        let UseOn::Success(effect) = got else {
            panic!("expected a write, got {got:?}");
        };
        assert_eq!(
            effect.at,
            (4, 70, 4),
            "the clicked block, not the one above"
        );
        assert_eq!(effect.state, "minecraft:dirt_path");
        assert_eq!(effect.flags, SET_BLOCK_FLAGS);
        let sound = effect.sound.expect("the flatten sound");
        assert_eq!(sound.event, "minecraft:item.shovel.flatten");
        assert!(
            (sound.pitch - 1.0).abs() < f32::EPSILON && (sound.volume - 1.0).abs() < f32::EPSILON,
            "fconst_1 twice"
        );
        assert_eq!(
            sound.except,
            Some(7),
            "playSound(Entity, ...) excludes them"
        );
    }

    /// All six `FLATTENABLES` rows go to `dirt_path`, and a seventh block
    /// does not. Written as a sweep because a five-row transcription of a
    /// six-row map passes every single-block test that will ever be written.
    #[test]
    fn every_flattenable_becomes_a_path_and_nothing_else_does() {
        for (block, _) in FLATTENABLES {
            let w: World = vec![((0, 64, 0), block)];
            let got = with(&w, |at| shovel((0, 64, 0), Face::Up, 1, at));
            let UseOn::Success(effect) = got else {
                panic!("{block} is in FLATTENABLES and did not flatten: {got:?}");
            };
            assert_eq!(effect.state, "minecraft:dirt_path", "{block}");
        }
        assert_eq!(FLATTENABLES.len(), 6, "the ImmutableMap has six rows");
        let w: World = vec![((0, 64, 0), "minecraft:stone")];
        assert_eq!(
            with(&w, |at| shovel((0, 64, 0), Face::Up, 1, at)),
            UseOn::Pass,
            "stone is not flattenable"
        );
    }

    /// **The two refusals, which are different offsets and different
    /// answers.** `DOWN` is refused at offset 17 before the block is looked
    /// up at all; a non-air block above is refused inside the flatten branch
    /// at offset 70. Both come out as `PASS` here, so the test that
    /// distinguishes them is that the `DOWN` case refuses a block that would
    /// otherwise succeed.
    #[test]
    fn a_shovel_refuses_the_down_face_and_a_covered_block() {
        let w: World = vec![((2, 70, 2), "minecraft:grass_block")];
        assert_eq!(
            with(&w, |at| shovel((2, 70, 2), Face::Down, 1, at)),
            UseOn::Pass,
            "offset 17: the DOWN face is refused before anything is read"
        );
        assert!(
            matches!(
                with(&w, |at| shovel((2, 70, 2), Face::Up, 1, at)),
                UseOn::Success(_)
            ),
            "the same block from above is the positive control for that refusal"
        );

        let covered: World = vec![
            ((2, 70, 2), "minecraft:grass_block"),
            ((2, 71, 2), "minecraft:stone"),
        ];
        assert_eq!(
            with(&covered, |at| shovel((2, 70, 2), Face::Up, 1, at)),
            UseOn::Pass,
            "offset 70: something on top and the flatten branch is skipped"
        );
    }

    /// The dispatch, not the body: all seven shovels reach [`shovel`] and an
    /// item that is not one of them does not. [`SHOVELS`] is a hand-written
    /// list of a class column, so the thing worth testing is that it has
    /// seven members and that `use_on` routes by it.
    #[test]
    fn all_seven_shovels_dispatch_and_nothing_else_does() {
        let w: World = vec![((0, 64, 0), "minecraft:grass_block")];
        assert_eq!(SHOVELS.len(), 7, "seven ShovelItem registrations in 26.2");
        for item in SHOVELS {
            assert!(
                matches!(
                    with(&w, |at| use_on(item, (0, 64, 0), Face::Up, 1, at)),
                    UseOn::Success(_)
                ),
                "{item} did not reach shovel()"
            );
        }
        assert_eq!(
            with(&w, |at| use_on(
                "minecraft:stick",
                (0, 64, 0),
                Face::Up,
                1,
                at
            )),
            UseOn::Pass,
            "Item.useOn's own body"
        );
    }

    /// The strip, and the property that a state table could not carry: the
    /// **clicked block's axis** wins over the stripped block's default.
    ///
    /// Swept over all three axes and all 23 rows, because `axis=y` is both
    /// the commonest case and the stripped block's own default, so a
    /// transcription that dropped the `setValue` passes every `axis=y` test
    /// that will ever be written and stands every horizontal log upright.
    #[test]
    fn an_axe_strips_a_log_and_keeps_its_axis() {
        assert_eq!(STRIPPABLES.len(), 23, "23 put() pairs in AxeItem.<clinit>");
        for (from, to) in STRIPPABLES {
            for axis in ["x", "y", "z"] {
                let clicked = format!("{from}[axis={axis}]");
                let w: World = vec![((0, 64, 0), Box::leak(clicked.into_boxed_str()))];
                let got = with(&w, |at| axe((0, 64, 0), 3, at));
                let UseOn::Success(effect) = got else {
                    panic!("{from} at axis={axis} did not strip: {got:?}");
                };
                assert_eq!(
                    effect.state,
                    format!("{to}[axis={axis}]"),
                    "lambda$getStripped$0 carries the clicked block's axis"
                );
                assert_eq!(effect.at, (0, 64, 0));
                assert_eq!(effect.flags, SET_BLOCK_FLAGS);
            }
        }
    }

    /// The sound, and the dispatch. Separated from the sweep above so a
    /// failure names one thing.
    #[test]
    fn all_seven_axes_dispatch_and_the_strip_sound_is_item_axe_strip() {
        assert_eq!(AXES.len(), 7, "seven AxeItem registrations in 26.2");
        let w: World = vec![((0, 64, 0), "minecraft:oak_log[axis=y]")];
        for item in AXES {
            let got = with(&w, |at| use_on(item, (0, 64, 0), Face::Up, 5, at));
            let UseOn::Success(effect) = got else {
                panic!("{item} did not reach axe(): {got:?}");
            };
            let sound = effect.sound.expect("the strip sound");
            assert_eq!(sound.event, "minecraft:item.axe.strip");
            assert_eq!(sound.except, Some(5));
        }
    }

    /// **The refused branches answer `PASS`, not `FAIL`**, so the caller
    /// still falls through to the placement path — which is what an axe on a
    /// copper block did before `axe` existed, and is why adding this function
    /// cannot regress anything it does not handle.
    #[test]
    fn an_axe_on_something_it_cannot_strip_passes() {
        for block in [
            "minecraft:stone",
            "minecraft:oxidized_copper",
            "minecraft:waxed_copper_block",
            "minecraft:stripped_oak_log[axis=y]",
        ] {
            let w: World = vec![((0, 64, 0), block)];
            assert_eq!(
                with(&w, |at| axe((0, 64, 0), 1, at)),
                UseOn::Pass,
                "{block}"
            );
        }
    }

    /// Shears stop a growing vine by pinning its age to 25, **keeping every
    /// other property**. `cave_vines` is the only one of the four heads that
    /// has another property, so it is the only one that can show the
    /// difference between `setValue(AGE, 25)` and a default state — and
    /// picking the berries off a sheared vine is exactly the bug a default
    /// would cause.
    #[test]
    fn shears_pin_a_vines_age_and_keep_its_berries() {
        let w: World = vec![((1, 68, 1), "minecraft:cave_vines[age=7,berries=true]")];
        let got = with(&w, |at| shears((1, 68, 1), 9, at));
        let UseOn::Success(effect) = got else {
            panic!("expected a write, got {got:?}");
        };
        assert_eq!(effect.state, "minecraft:cave_vines[age=25,berries=true]");
        assert_eq!(effect.at, (1, 68, 1));
        assert_eq!(
            effect.flags, SET_BLOCK_AND_UPDATE_FLAGS,
            "setBlockAndUpdate is 3, not the 11 the hoe and the flint use"
        );
        let sound = effect.sound.expect("the crop sound");
        assert_eq!(sound.event, "minecraft:block.growing_plant.crop");
        assert_eq!(sound.except, Some(9));
    }

    /// All four heads, and the two refusals: an already-maxed head, and a
    /// **body** block. The body is the one worth a case — `cave_vines_plant`
    /// is the block a player actually clicks in the middle of a vine, it is
    /// `GrowingPlantBodyBlock` rather than a head, and vanilla's `instanceof`
    /// does not match it.
    #[test]
    fn every_head_shears_and_nothing_else_does() {
        assert_eq!(GROWING_PLANT_HEADS.len(), 4, "four GrowingPlantHeadBlocks");
        for head in GROWING_PLANT_HEADS {
            let w: World = vec![(
                (0, 64, 0),
                Box::leak(format!("{head}[age=0]").into_boxed_str()),
            )];
            let got = with(&w, |at| shears((0, 64, 0), 1, at));
            let UseOn::Success(effect) = got else {
                panic!("{head} did not shear: {got:?}");
            };
            assert_eq!(effect.state, format!("{head}[age=25]"));

            let maxed: World = vec![(
                (0, 64, 0),
                Box::leak(format!("{head}[age=25]").into_boxed_str()),
            )];
            assert_eq!(
                with(&maxed, |at| shears((0, 64, 0), 1, at)),
                UseOn::Pass,
                "{head} at MAX_AGE: isMaxAge returns and Item.useOn passes"
            );
        }
        for other in [
            "minecraft:cave_vines_plant[berries=true]",
            "minecraft:oak_leaves[distance=1,persistent=false,waterlogged=false]",
            "minecraft:stone",
        ] {
            let w: World = vec![((0, 64, 0), other)];
            assert_eq!(
                with(&w, |at| shears((0, 64, 0), 1, at)),
                UseOn::Pass,
                "{other}"
            );
        }
        // The dispatch: only `minecraft:shears` reaches it.
        let w: World = vec![((0, 64, 0), "minecraft:kelp[age=3]")];
        assert!(matches!(
            with(&w, |at| use_on(
                "minecraft:shears",
                (0, 64, 0),
                Face::Up,
                1,
                at
            )),
            UseOn::Success(_)
        ));
        assert_eq!(
            with(&w, |at| use_on(
                "minecraft:iron_axe",
                (0, 64, 0),
                Face::Up,
                1,
                at
            )),
            UseOn::Pass,
            "an axe has no rule for kelp"
        );
    }

    /// A fire charge puts fire against a sturdy face, like a flint and steel,
    /// and **writes with different flags**. The pair is the test: the same
    /// world, the two items, the same position and state, different numbers.
    #[test]
    fn a_fire_charge_lights_like_a_flint_but_with_flags_three() {
        let w: World = vec![((0, 63, 0), "minecraft:stone")];
        let charge = with(&w, |at| fire_charge((0, 63, 0), Face::Up, at));
        let flint = with(&w, |at| flint_and_steel((0, 63, 0), Face::Up, 1, at));
        let (UseOn::Success(c), UseOn::Success(f)) = (charge.clone(), flint) else {
            panic!("both should light: {charge:?}");
        };
        assert_eq!(c.at, (0, 64, 0), "the position along the clicked face");
        assert_eq!(c.at, f.at);
        assert_eq!(c.state, f.state, "BaseFireBlock.getState, the same call");
        assert_eq!(c.flags, SET_BLOCK_AND_UPDATE_FLAGS, "setBlockAndUpdate");
        assert_eq!(f.flags, SET_BLOCK_FLAGS, "setBlock(..., 11)");
        assert_ne!(c.flags, f.flags, "and that is the point of this test");
        assert!(c.sound.is_none(), "two nextFloat draws we cannot reproduce");
    }

    /// Where it cannot light, it **fails** rather than passing — so nothing
    /// falls through to the placement path and tries to place a fire charge
    /// as a block. Same distinction `flint_and_steel` carries.
    #[test]
    fn a_fire_charge_that_cannot_light_fails_and_does_not_pass() {
        let w: World = vec![((0, 63, 0), "minecraft:oak_fence")];
        assert_eq!(
            with(&w, |at| fire_charge((0, 63, 0), Face::Up, at)),
            UseOn::Fail
        );
        assert_eq!(
            with(&w, |at| use_on(
                "minecraft:fire_charge",
                (0, 63, 0),
                Face::Up,
                1,
                at
            )),
            UseOn::Fail,
            "and it reaches that answer through the dispatch"
        );
    }
}
