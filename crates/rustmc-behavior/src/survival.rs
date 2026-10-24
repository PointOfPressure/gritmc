//! `BlockState#canSurvive` — what a block needs under it to stay put.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `VegetationBlock#canSurvive`/`#mayPlaceOn`, `CactusBlock`,
//! `BushBlock`.
//!
//! # Why this is code and the shape tables are not
//!
//! `isFaceSturdy` and the face-occlusion bits are cached per state at
//! bootstrap, so they extract to a table. `canSurvive` does not: it reads the
//! *world around* the position, and for some blocks reads several of them.
//! There is no per-state answer to record.
//!
//! What can be recorded is the answer over a controlled neighbourhood, and
//! `data/vanilla/survival.json` does exactly that — every state
//! `would_survive` names, against twenty-three candidate blocks-below, as
//! vanilla answers them with nothing else nearby. `golden_survival` checks
//! this module against it.
//!
//! # Refusing is a result
//!
//! Most of these rules read one block. Some read more: a mushroom checks the
//! light level, sugar cane looks for water beside the block below, a lily pad
//! wants water underneath. The oracle **records a refusal** for those rather
//! than a false, because a harness that cannot see the whole neighbourhood
//! must not pretend it has.
//!
//! This module does the same. [`Survival::Unknown`] is not "no" — it makes
//! the caller fail loudly, so an unported rule shows up as a gap rather than
//! as a world quietly missing its mushrooms.

/// What a `canSurvive` question came back with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Survival {
    Yes,
    No,
    /// This port has not implemented the rule for that block. Never treat as
    /// `No` — see the module docs.
    Unknown,
}

/// Whatever answers `#tag` membership. A trait rather than a concrete type
/// so this crate does not have to own a tag loader — worldgen already has
/// one and the server will have another.
pub trait TagLookup {
    fn contains(&self, tag: &str, block: &str) -> bool;
}

/// The world a survival rule reads.
pub trait SurvivalWorld {
    /// The full block state at a position.
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String;

    /// `state.isFaceSturdy(level, pos, Direction.UP)`, or `None` when this
    /// world cannot say.
    ///
    /// `None` rather than `false`: a world that has no shape table has no
    /// opinion, and a rule that needs one answers [`Survival::Unknown`]
    /// rather than refusing to place anything. The one-block world the
    /// survival oracle uses is exactly that case.
    fn face_is_sturdy_up(&mut self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        None
    }

    /// `BlockState#isAir`, or `None` when this world cannot say. `cave_air`
    /// and `void_air` are air and a name check misses both.
    fn air_at(&mut self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        None
    }

    /// `Block.canSupportCenter(level, pos, direction)` — `SupportType.CENTER`
    /// plus the `#unstable_bottom_center` refusal for DOWN. **Not**
    /// [`Self::face_is_sturdy_up`], which is `SupportType.FULL`.
    fn can_support_center(&mut self, _x: i32, _y: i32, _z: i32, _direction: usize) -> Option<bool> {
        None
    }

    /// `LevelReader#getFluidState`, as a registry name, or
    /// `"minecraft:empty"`. Overridden by worlds that have a block-property
    /// table; the default is the name-shaped guess, which is right for the
    /// only caller here (a water source beside a sugar cane).
    /// `Block.isFaceFull(state.getCollisionShape(level, pos), Direction.UP)`
    /// for the block at `(x, y, z)`.
    ///
    /// The **collision** shape, not the occlusion shape and not
    /// `isFaceSturdy`: a snow layer sits on a slab or a stair whose top face
    /// is solid to walk on, and those answer differently to the three
    /// questions. `None` means the world cannot say, which becomes
    /// [`Survival::Unknown`] rather than a guess.
    fn collision_face_full_up(&mut self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        None
    }

    /// `LevelReader#getRawBrightness(pos, 0)`, or `None` when this world
    /// cannot say.
    ///
    /// Only [`Family::Mushroom`] asks. `None` becomes [`Survival::Unknown`]
    /// rather than a guess, which is what the one-block survival oracle
    /// wants — vanilla's own harness records `null` with
    /// `reads: canSurvive read LevelReader.getRawBrightness/2` for exactly
    /// these cases.
    ///
    /// **A decorating world answers 15 and that is not a stub.** It is read
    /// out of the jar: `ChunkPyramid`'s generation order puts `FEATURES`
    /// before `INITIALIZE_LIGHT` and `LIGHT`, so no sky data exists for the
    /// chunk being decorated, and `SkyLightSectionStorage.getLightValue`
    /// answers **15** when the column has no recorded top section — the
    /// `topSections.get(..) == currentLowestY` branch returns `bipush 15`
    /// without consulting a data layer at all. `LevelLightEngine
    /// .getRawBrightness` is then `max(block 0, sky 15) = 15`, underground
    /// included.
    fn raw_brightness(&mut self, _x: i32, _y: i32, _z: i32) -> Option<i32> {
        None
    }

    /// `BlockState#isSolidRender` for the block at `(x, y, z)`, or `None`
    /// when this world has no render-shape table. Only [`Family::Mushroom`]
    /// asks, and only once the light is below 13.
    fn is_solid_render(&mut self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        None
    }

    /// `FireBlock.canBurn(level.getBlockState(pos))` — a positive ignite
    /// odds, and not waterlogged — or `None` when this world has no odds
    /// table. Only [`Family::Fire`] asks, and only when the block below is
    /// not sturdy.
    fn fire_can_burn(&mut self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        None
    }

    fn fluid_at(&mut self, x: i32, y: i32, z: i32) -> String {
        let block = self.block_at(x, y, z);
        let plain = block.split_once('[').map_or(block.as_str(), |(n, _)| n);
        match plain {
            "minecraft:water" => "minecraft:water".to_string(),
            "minecraft:lava" => "minecraft:lava".to_string(),
            _ => "minecraft:empty".to_string(),
        }
    }
}

/// `Direction.Plane.HORIZONTAL`. The order does not matter to any rule here
/// — every use is an `||` over all four and none of them draws — but it is
/// written in vanilla's order anyway so a rule that *does* care can reuse it.
const HORIZONTAL: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

/// The block families whose survival rule this port knows.
///
/// Deliberately a closed list rather than a default: a block that is not here
/// answers [`Survival::Unknown`], and the count of those is the measure of
/// how much of the stage is missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    /// A `VegetationBlock` whose `mayPlaceOn` is a tag test on the block
    /// below, carrying **which** tag. Most are `#supports_vegetation`;
    /// azalea overrides it with `#supports_azalea`, which is a different and
    /// much shorter list. Sharing one rule with a hard-coded tag would put
    /// azaleas on grass.
    VegetationOn(&'static str),
    /// `SoulFireBlock#canSurvive` — one tag test on the block below and
    /// nothing else:
    ///
    /// ```text
    ///   canSurvive(state, level, pos)  =  canSurviveOnBlock(level.getBlockState(pos.below()))
    ///   canSurviveOnBlock(below)       =  below.is(BlockTags.SOUL_FIRE_BASE_BLOCKS)
    /// ```
    ///
    /// `javap -p -c -constants net/minecraft/world/level/block/SoulFireBlock.class`
    /// on the pinned `server-26.2.jar`. The tag is `soul_sand` and
    /// `soul_soil`, two entries.
    ///
    /// It is not [`Family::VegetationOn`] even though the code is the same
    /// shape: `SoulFireBlock` extends `BaseFireBlock`, not `VegetationBlock`,
    /// and folding it into that variant would put a fire block behind a doc
    /// comment describing saplings. Same rule, different method, so the two
    /// stay separable when either changes.
    SoulFire,
    /// `FireBlock#canSurvive`:
    ///
    /// ```text
    ///   below.isFaceSturdy(level, pos.below(), UP) || isValidFireLocation(level, pos)
    ///   isValidFireLocation = any of Direction.values() whose neighbour canBurn
    /// ```
    ///
    /// The second clause needs the ignite-odds table, which this crate does
    /// not own; [`SurvivalWorld::fire_can_burn`] asks the world, and a world
    /// that cannot say turns the answer into [`Survival::Unknown`]. The only
    /// worldgen caller is `patch_fire`, whose own `block_predicate_filter`
    /// requires `netherrack` below — sturdy on top — so decoration is always
    /// answered by the first clause and never reaches the table.
    Fire,
    /// `CarpetBlock#canSurvive` — the block below merely has to not be air.
    /// No tag, no shape, no sturdy face: a carpet sits on anything solid
    /// enough to have a block there at all.
    Carpet,
    /// `MossyCarpetBlock#canSurvive`, **and only for the base state**.
    ///
    /// `pale_moss_carpet` carries `bottom` plus a `WallSide` per horizontal
    /// direction, and its rule reads a different part of the world depending
    /// on which:
    ///
    /// ```text
    ///   bottom=true   the block below
    ///   bottom=false  the four horizontal neighbours, one per side that is
    ///                 not `none`
    /// ```
    ///
    /// **Only the first is ported.** A non-base carpet returns
    /// [`Survival::Unknown`] before the block below is ever read, because
    /// reading below for a state whose rule never looks there is the shape
    /// this module's `hanging` propagule note already warns about: a question
    /// vanilla does not pose, answered anyway.
    ///
    /// That is not a shortcut. Worldgen places exactly one state —
    /// `pale_moss_carpet[bottom=true,east=none,north=none,south=none,west=none]`
    /// — because `pale_moss_patch` is a `vegetation_patch` over a
    /// `simple_block`, which writes a default state and never a wall-attached
    /// one. All 169 declines in window A name that one state.
    ///
    /// The rule for it is taken from the oracle rather than from a class
    /// name: over all twenty-three grounds `data/vanilla/survival.json`
    /// carries, vanilla answers **true on every ground except `air`** —
    /// including `water`, which is what rules out a sturdy-face test and
    /// leaves the `isAir` one.
    ///
    /// It is deliberately **not** folded into [`Family::Carpet`], on this
    /// module's own precedent: `SoulFireBlock` shares `VegetationOn`'s shape
    /// and stays separate because it is a different method that can change
    /// independently. `MossyCarpetBlock` is a different class from
    /// `CarpetBlock` and a much larger one, so the same argument applies with
    /// more force.
    MossyCarpet,
    /// `LeafLitterBlock#canSurvive` — one read and nothing else:
    ///
    /// ```text
    ///   canSurvive(state, level, pos)
    ///     = level.getBlockState(pos.below()).isFaceSturdy(level, below, UP)
    /// ```
    ///
    /// `javap -p -c -constants` on the pinned `server-26.2.jar`: the method is
    /// four instructions and a return, with no tag, no property of its own
    /// state, and no second position.
    ///
    /// The oracle agrees over all twenty-three grounds and the three
    /// disagreeing ones are the interesting ones: `farmland` is **false**,
    /// which no `#supports_vegetation` reading gives — farmland is in that
    /// tag — and `snow_block`, `sand` and `gravel` are **true**, which a
    /// `isSolidRender` reading would not give for all three. It is the sturdy
    /// face and only the sturdy face.
    ///
    /// `facing` and `segment_amount` are carried by every state worldgen
    /// places and read by neither vanilla's rule nor this one.
    LeafLitter,
    /// `SporeBlossomBlock#canSurvive` — hangs from a ceiling that can support
    /// a *centre* on its down face, and refuses to be underwater.
    SporeBlossom,
    /// `SmallDripleafBlock#mayPlaceOn` — `#supports_small_dripleaf`, **or**
    /// a water source directly above and the ordinary vegetation tag below.
    /// The second clause is why dripleaf grows out of shallow water.
    SmallDripleaf,
    /// `VegetationBlock` — saplings, grass, ferns, flowers, bushes.
    /// `canSurvive` is `belowState.is(BlockTags.SUPPORTS_VEGETATION)`: one
    /// tag test on one block, and thirty-four of the fifty-one blocks
    /// worldgen asks about.
    ///
    /// **The mushrooms are NOT in it.** This doc used to say they were, on
    /// the grounds that "their own rule adds a light check, but that check
    /// only widens the answer". Both halves are false and it cost 33 blocks
    /// on the r=16 window — see [`Family::Mushroom`].
    Vegetation,
    /// `MushroomBlock#canSurvive` — a tag that short-circuits, then a
    /// **light** requirement conjoined with `MushroomBlock#mayPlaceOn`.
    ///
    /// ```text
    ///   belowState.is(#overrides_mushroom_light_requirement)  -> true
    ///   else  getRawBrightness(pos, 0) < 13  &&  mayPlaceOn(belowState, ..)
    ///   MushroomBlock#mayPlaceOn = belowState.isSolidRender()
    /// ```
    ///
    /// The tag is four blocks: mycelium, podzol, crimson_nylium,
    /// warped_nylium.
    ///
    /// **This was [`Family::Vegetation`] until 2026-08-10 and that was wrong
    /// twice over.** `VegetationBlock#mayPlaceOn` is
    /// `belowState.is(#supports_vegetation)` and `MushroomBlock` *overrides*
    /// it with `isSolidRender()`, so the support test was the wrong one; and
    /// the light term is a **conjunction**, so it narrows rather than widens.
    /// The two errors do not cancel — `isSolidRender()` is the broader
    /// support test and the light term is the stronger filter — and on the
    /// r=16 overworld window at seed 20260722 the net effect was 33 mushrooms
    /// vanilla does not have: 32 `brown_mushroom` and one surface
    /// `red_mushroom`, every one of them standing on `grass_block`, against
    /// zero that vanilla has and we miss.
    ///
    /// Because a decorating world's [`SurvivalWorld::raw_brightness`] is 15
    /// everywhere (see that method), the light term can never pass during
    /// decoration and **the tag decides every worldgen mushroom**. That is
    /// not an assumption: all 28 mushrooms vanilla places in that window sit
    /// on `podzol`, and it places none at the surface in 1,089 chunks where
    /// `brown_mushroom_normal`'s `rarity_filter` of 256 predicts about four
    /// patches.
    ///
    /// **What this window does not adjudicate:** no case anywhere in it puts
    /// a mushroom over a non-tag block in genuine darkness, so the
    /// `isSolidRender()` half of the rule is unexercised here and is written
    /// from the bytecode alone. It refuses rather than guesses — a world with
    /// a real light value but no solid-render table answers
    /// [`Survival::Unknown`].
    Mushroom,
    /// `SnowLayerBlock#canSurvive` — two tags, then a collision-shape test.
    ///
    /// Worth its own family because it is the single largest thing missing
    /// from a land window: 14,513 blocks of `snow[layers=1]` at seed
    /// 20260722's village, plus 8,053 `grass_block[snowy]` that follow from
    /// them. Every one was a refusal.
    Snow,
    /// `CactusBlock` — sand or red sand below, and this port stops there
    /// because the rest of the rule reads the four horizontal neighbours.
    Cactus,
    /// `SeagrassBlock#mayPlaceOn` and `TallSeagrassBlock#mayPlaceOn`, which
    /// override `VegetationBlock`'s tag test with a shape test: the block
    /// below must present a sturdy up face and must not be in
    /// `#cannot_support_seagrass`. Sand and gravel both qualify and neither
    /// is in `#supports_vegetation`, so borrowing the vegetation rule would
    /// put no seagrass in the ocean at all.
    Seagrass,
    /// `GrowingPlantBlock#canSurvive` with a growth direction of UP — kelp
    /// and its plant body. The block below must not be in the head block's
    /// `#cannot_support_*` tag, and must be either more of the same plant or
    /// a sturdy up face.
    GrowingPlantUp {
        head: &'static str,
        body: &'static str,
        cannot_support: &'static str,
    },
    /// Blocks that override nothing and so inherit `BlockBehaviour`'s
    /// default `canSurvive`, which is an unconditional `true`.
    ///
    /// This is a real answer, not a shrug — the distinction matters, because
    /// `Survival::Unknown` makes a feature refuse itself and place nothing.
    /// `potent_sulfur` sat in that hole: `PotentSulfurBlock` declares
    /// `updateShape`, `getStateForPlacement` and `onPlace` but no
    /// `canSurvive`, so every one of its placements is legal, and answering
    /// "I don't know" left the block absent from the world entirely.
    Unconditional,
    /// `SugarCaneBlock#canSurvive` — more sugar cane below, or a supporting
    /// block below with water (or frosted ice) beside **that** block rather
    /// than beside the cane.
    ///
    /// The two adjacency tags are a trap: `supports_sugar_cane_adjacently`
    /// exists in *both* registries with different contents — the fluid tag
    /// is `#minecraft:water`, the block tag is `minecraft:frosted_ice` — so
    /// the two tests are genuinely different questions and neither subsumes
    /// the other.
    SugarCane,
    /// `MangrovePropaguleBlock#canSurvive`, which is **two rules chosen by a
    /// property**, not one rule reading two blocks:
    ///
    /// ```text
    ///   return !isHanging(state)
    ///        ? super.canSurvive(state, level, pos)          // SaplingBlock
    ///        : level.getBlockState(pos.above())
    ///               .is(BlockTags.SUPPORTS_HANGING_MANGROVE_PROPAGULE);
    /// ```
    ///
    /// `SaplingBlock` overrides neither `canSurvive` nor `mayPlaceOn`, so the
    /// hanging=false branch is exactly [`Family::Vegetation`] — the ordinary
    /// `#supports_vegetation` test — and the hanging branch reads the block
    /// **above** against a one-entry tag whose only member is
    /// `minecraft:mangrove_leaves`.
    ///
    /// This one block was worth more than every structure on the measurement
    /// window. `mangrove_checked` and `tall_mangrove_checked` are gated by a
    /// `block_predicate_filter` on `would_survive(mangrove_propagule[
    /// hanging=false, ...])`, so an `Unknown` here refuses **the whole tree**
    /// — not a propagule. The refusal census on seed 31415926 read
    /// `1345x minecraft:would_survive (minecraft:mangrove_propagule)`, and the
    /// leaves, roots and vines those 1345 trees would have written are the
    /// largest single item in that window's block diff.
    MangrovePropagule,
    /// `LilyPadBlock#mayPlaceOn`, which asks about **two** fluids and one
    /// block:
    ///
    /// ```text
    ///   FluidState f  = level.getFluidState(belowPos);
    ///   FluidState f1 = level.getFluidState(belowPos.above());   // the pad
    ///   return (f.is(FluidTags.SUPPORTS_LILY_PAD)
    ///           || belowState.is(BlockTags.SUPPORTS_LILY_PAD))
    ///        && f1.is(Fluids.EMPTY);
    /// ```
    ///
    /// The fluid tag holds `minecraft:water`; the block tag holds `ice` and
    /// `frosted_ice`, which is why a pad sits on a frozen pond. The second
    /// clause is the one a port is likely to drop: the pad's **own** position
    /// must be dry, so a pad cannot go under water.
    ///
    /// Worth 3,485 refusals on the radius-6 window at seed 31415926, the
    /// second-largest after the mangrove tree.
    LilyPad,
}

/// Which family a block's rule belongs to.
///
/// **Derived from the oracle, not from reading class names.** For each block
/// `data/vanilla/survival.json` names, vanilla's answers over the
/// twenty-three candidate grounds were compared against
/// `#supports_vegetation` membership; the thirty-four below match it exactly
/// on every ground vanilla would answer for. `golden_survival` re-checks
/// that, so a block listed here whose rule is really something else fails
/// rather than quietly placing flowers on stone.
///
/// Sixteen do *not* match, and each reads more than the block below: azalea,
/// cactus, dead_bush, fire, flowering_azalea, leaf_litter, lily_pad, melon,
/// moss_carpet, pale_moss_carpet, pumpkin, short_dry_grass, soul_fire,
/// spore_blossom, sugar_cane, tall_dry_grass.
///
/// **"Do not match `#supports_vegetation`" is not "unported", and this
/// paragraph said it was until 2026-08-18.** Fourteen of the sixteen have had
/// their own family for some time — each one is a variant below with its own
/// rule and its own doc — and the sentence had drifted into naming them as
/// gaps. It is the same defect the module is otherwise careful about: a
/// comment describing a state of the code that no line holds it to.
///
/// **Every subject in `data/vanilla/survival.json` has a family now.** `fire`
/// was the last without one until 2026-10-02; its second clause still needs
/// the ignite-odds table, which the world supplies through
/// [`SurvivalWorld::fire_can_burn`] or declines. Re-derive this rather than
/// editing it by hand: the list is every subject that [`family_of`] answers
/// `None` for.
///
/// `mangrove_propagule` used to be the seventeenth, on the ground that it
/// "reads more than the block below". It does not: it reads a *different*
/// block below or above depending on `hanging`, which is a property of the
/// state and known before the world is touched. See
/// [`Family::MangrovePropagule`] for what that cost.
fn family_of(block: &str) -> Option<Family> {
    match block {
        "minecraft:snow" => Some(Family::Snow),
        "minecraft:soul_fire" => Some(Family::SoulFire),
        "minecraft:fire" => Some(Family::Fire),
        "minecraft:acacia_sapling"
        | "minecraft:allium"
        | "minecraft:azure_bluet"
        | "minecraft:birch_sapling"
        | "minecraft:blue_orchid"
        | "minecraft:bush"
        | "minecraft:cherry_sapling"
        | "minecraft:closed_eyeblossom"
        | "minecraft:cornflower"
        | "minecraft:dandelion"
        | "minecraft:dark_oak_sapling"
        | "minecraft:fern"
        | "minecraft:firefly_bush"
        | "minecraft:jungle_sapling"
        | "minecraft:large_fern"
        | "minecraft:lily_of_the_valley"
        | "minecraft:oak_sapling"
        | "minecraft:orange_tulip"
        | "minecraft:oxeye_daisy"
        | "minecraft:pale_oak_sapling"
        | "minecraft:pink_petals"
        | "minecraft:pink_tulip"
        | "minecraft:poppy"
        | "minecraft:red_tulip"
        | "minecraft:short_grass"
        | "minecraft:spruce_sapling"
        | "minecraft:sunflower"
        | "minecraft:sweet_berry_bush"
        | "minecraft:tall_grass"
        | "minecraft:white_tulip"
        | "minecraft:wildflowers" => Some(Family::Vegetation),
        // `NetherRootsBlock`, `NetherFungusBlock` and `NetherSproutsBlock`
        // each override `mayPlaceOn` with **their own** tag, passed to the
        // constructor in `Blocks`' static initialiser — not
        // `#supports_vegetation`. All five tags add `#nylium` and
        // `soul_soil` to it, which is the whole floor of a nether forest and
        // half the floor of a soul sand valley.
        //
        // `crimson_roots` sat in the vegetation family above until
        // 2026-10-02, and the survival golden could not see it: its grounds
        // held neither nylium nor soul soil, and on every ground it did hold
        // the two rules agree. So every `patch_crimson_roots` on soul soil
        // was refused, and the golden stayed green.
        "minecraft:crimson_roots" => Some(Family::VegetationOn("minecraft:supports_crimson_roots")),
        "minecraft:warped_roots" => Some(Family::VegetationOn("minecraft:supports_warped_roots")),
        "minecraft:nether_sprouts" => {
            Some(Family::VegetationOn("minecraft:supports_nether_sprouts"))
        }
        "minecraft:crimson_fungus" => {
            Some(Family::VegetationOn("minecraft:supports_crimson_fungus"))
        }
        "minecraft:warped_fungus" => Some(Family::VegetationOn("minecraft:supports_warped_fungus")),
        "minecraft:brown_mushroom" | "minecraft:red_mushroom" => Some(Family::Mushroom),
        "minecraft:cactus" => Some(Family::Cactus),
        "minecraft:mangrove_propagule" => Some(Family::MangrovePropagule),
        // `DryVegetationBlock#mayPlaceOn` — one tag on the block below, and
        // the tag is `#supports_dry_vegetation`: `#sand`, `#terracotta` and
        // `#supports_vegetation`. A dead bush is not a `VegetationBlock` and
        // borrowing that family would refuse every bush in a desert.
        "minecraft:dead_bush" | "minecraft:short_dry_grass" | "minecraft:tall_dry_grass" => {
            Some(Family::VegetationOn("minecraft:supports_dry_vegetation"))
        }
        // `PumpkinBlock` and the melon both `extends Block` and override
        // neither `canSurvive` nor `mayPlaceOn` — there is no `MelonBlock`
        // class at all — so their `canSurvive` is `BlockBehaviour`'s
        // unconditional `true`. Answering "I don't know" refused 14 pumpkins
        // on the measurement window for a rule that does not exist.
        "minecraft:potent_sulfur" | "minecraft:pumpkin" | "minecraft:melon" => {
            Some(Family::Unconditional)
        }
        "minecraft:lily_pad" => Some(Family::LilyPad),
        "minecraft:azalea" | "minecraft:flowering_azalea" => {
            Some(Family::VegetationOn("minecraft:supports_azalea"))
        }
        "minecraft:moss_carpet" => Some(Family::Carpet),
        "minecraft:pale_moss_carpet" => Some(Family::MossyCarpet),
        "minecraft:leaf_litter" => Some(Family::LeafLitter),
        "minecraft:spore_blossom" => Some(Family::SporeBlossom),
        "minecraft:small_dripleaf" => Some(Family::SmallDripleaf),
        "minecraft:sugar_cane" => Some(Family::SugarCane),
        "minecraft:seagrass" | "minecraft:tall_seagrass" => Some(Family::Seagrass),
        "minecraft:kelp" | "minecraft:kelp_plant" => Some(Family::GrowingPlantUp {
            head: "minecraft:kelp",
            body: "minecraft:kelp_plant",
            cannot_support: "minecraft:cannot_support_kelp",
        }),
        _ => None,
    }
}

/// `state.getValue(property) == value`, off the canonical `name[a=b,c=d]`
/// spelling.
///
/// Matched as a whole comma-separated field rather than with `contains`: the
/// substring `hanging=true` also appears inside a hypothetical
/// `not_hanging=true`, and a property test that can be fooled by a prefix is a
/// silent wrong answer rather than a loud one.
fn has_property(state: &str, property: &str, value: &str) -> bool {
    let Some(body) = state.split_once('[').map(|(_, rest)| rest) else {
        return false;
    };
    let body = body.strip_suffix(']').unwrap_or(body);
    body.split(',')
        .any(|pair| pair.split_once('=') == Some((property, value)))
}

/// Vanilla's own `canSurvive` answers, written by `ci/survival-golden.sh`
/// and read by `tests/golden_survival.rs`.
///
/// A quoted path on purpose: `ci/gen-gameplay-tables.sh` derives the tables a
/// tree needs from exactly these literals, and until 2026-10-03 nothing named
/// this one, so no gate ever regenerated it. A gate directory then held
/// whichever copy it was seeded with, and a change to `SurvivalGolden.java`
/// (more grounds, a dark pass) reached no gate: the golden compared this
/// branch's rules against another branch's oracle.
pub const TABLE_PATH: &str = "data/vanilla/survival.json";

/// `BlockState#canSurvive` for the families this port knows.
pub fn can_survive<W: SurvivalWorld + ?Sized, T: TagLookup + ?Sized>(
    world: &mut W,
    tags: &T,
    state: &str,
    pos: (i32, i32, i32),
) -> Survival {
    let block = state.split_once('[').map_or(state, |(n, _)| n);
    let Some(family) = family_of(block) else {
        return Survival::Unknown;
    };
    // Answered before the world is touched: the default rule reads nothing,
    // so asking for the block below would be a question vanilla never poses.
    if matches!(family, Family::Unconditional) {
        return Survival::Yes;
    }
    let (x, y, z) = pos;
    // A hanging propagule reads the block **above** and never the one below,
    // so it is answered before the `below` read rather than inside the match:
    // asking for a block vanilla does not ask for is a question this port
    // would then have to explain to anyone reading a trace.
    if matches!(family, Family::MangrovePropagule) && has_property(state, "hanging", "true") {
        let above = world.block_at(x, y + 1, z);
        let above_block = above.split_once('[').map_or(above.as_str(), |(n, _)| n);
        return if tags.contains("minecraft:supports_hanging_mangrove_propagule", above_block) {
            Survival::Yes
        } else {
            Survival::No
        };
    }
    // A wall-attached pale moss carpet reads its four horizontal neighbours
    // and never the block below, so it is declined here rather than inside the
    // match, for the same reason the hanging propagule is answered above: the
    // `below` read on the next line is a question vanilla does not pose for
    // this state, and a trace that shows it would have to be explained.
    // Worldgen never places one — `pale_moss_patch` is a `vegetation_patch`
    // over a `simple_block`, which writes the default state.
    if matches!(family, Family::MossyCarpet) && !has_property(state, "bottom", "true") {
        return Survival::Unknown;
    }
    let below = world.block_at(x, y - 1, z);
    let below_block = below.split_once('[').map_or(below.as_str(), |(n, _)| n);
    match family {
        Family::Unconditional => Survival::Yes,
        // `LilyPadBlock#mayPlaceOn`. Both halves are load-bearing and the
        // second is the easy one to lose: the pad's **own** position must have
        // no fluid, so a pad cannot be placed under water.
        Family::LilyPad => {
            let supported = world.fluid_at(x, y - 1, z) == "minecraft:water"
                || tags.contains("minecraft:supports_lily_pad", below_block);
            if supported && world.fluid_at(x, y, z) == "minecraft:empty" {
                Survival::Yes
            } else {
                Survival::No
            }
        }
        // `MushroomBlock#canSurvive`. The tag is checked first and returns
        // outright, exactly as the bytecode does, so a podzol mushroom never
        // asks the world for a light level it may not have.
        Family::Mushroom => {
            if tags.contains(
                "minecraft:overrides_mushroom_light_requirement",
                below_block,
            ) {
                return Survival::Yes;
            }
            match world.raw_brightness(x, y, z) {
                None => Survival::Unknown,
                Some(light) if light >= 13 => Survival::No,
                // Dark enough: `MushroomBlock#mayPlaceOn`, which is
                // `belowState.isSolidRender()` (offset 1 of the pinned jar's
                // method) — neither `#supports_vegetation` nor a face test.
                // Reached in the nether, whose decorating world answers 0:
                // no sky engine, and no block light computed yet.
                Some(_) => match world.is_solid_render(x, y - 1, z) {
                    Some(true) => Survival::Yes,
                    Some(false) => Survival::No,
                    None => Survival::Unknown,
                },
            }
        }
        // `VegetationBlock#mayPlaceOn`.
        Family::Vegetation => {
            if tags.contains("minecraft:supports_vegetation", below_block) {
                Survival::Yes
            } else {
                Survival::No
            }
        }
        // Not hanging — `SaplingBlock.canSurvive`, which is
        // `VegetationBlock.canSurvive` calling **the propagule's own**
        // `mayPlaceOn`. That override is the whole difference and it was
        // nearly missed: `MangrovePropaguleBlock#mayPlaceOn` tests
        // `#supports_mangrove_propagule`, which is `#supports_vegetation`
        // **plus `minecraft:clay`**. Reading the class hierarchy said
        // `#supports_vegetation`; `golden_survival`'s first run said
        // `mangrove_propagule on minecraft:clay: port says false, vanilla says
        // true` and named the missing entry outright.
        Family::MangrovePropagule => {
            if tags.contains("minecraft:supports_mangrove_propagule", below_block) {
                Survival::Yes
            } else {
                Survival::No
            }
        }
        // `CactusBlock#canSurvive` begins with the ground test and then
        // checks that nothing solid touches its sides. Only the first half is
        // ported, so a *positive* ground is not yet a positive answer.
        Family::Cactus => {
            if below_block == "minecraft:sand" || below_block == "minecraft:red_sand" {
                Survival::Unknown
            } else {
                Survival::No
            }
        }
        Family::LeafLitter => match world.face_is_sturdy_up(x, y - 1, z) {
            Some(true) => Survival::Yes,
            Some(false) => Survival::No,
            None => Survival::Unknown,
        },
        Family::Seagrass => {
            if tags.contains("minecraft:cannot_support_seagrass", below_block) {
                return Survival::No;
            }
            match world.face_is_sturdy_up(x, y - 1, z) {
                Some(true) => Survival::Yes,
                Some(false) => Survival::No,
                None => Survival::Unknown,
            }
        }
        Family::VegetationOn(tag) => {
            if tags.contains(tag, below_block) {
                Survival::Yes
            } else {
                Survival::No
            }
        }
        // `SoulFireBlock#canSurviveOnBlock` — the block below is in
        // `#soul_fire_base_blocks`, and there is no second clause. No shape
        // read, no fluid read, no light read: this answers on every world
        // that can name the block below, including the one-block oracle.
        Family::Fire => match world.face_is_sturdy_up(x, y - 1, z) {
            Some(true) => Survival::Yes,
            None => Survival::Unknown,
            Some(false) => {
                // `Direction.values()`: down, up, north, south, west, east.
                const DIRECTIONS: [(i32, i32, i32); 6] = [
                    (0, -1, 0),
                    (0, 1, 0),
                    (0, 0, -1),
                    (0, 0, 1),
                    (-1, 0, 0),
                    (1, 0, 0),
                ];
                for (dx, dy, dz) in DIRECTIONS {
                    match world.fire_can_burn(x + dx, y + dy, z + dz) {
                        Some(true) => return Survival::Yes,
                        Some(false) => {}
                        None => return Survival::Unknown,
                    }
                }
                Survival::No
            }
        },
        Family::SoulFire => {
            if tags.contains("minecraft:soul_fire_base_blocks", below_block) {
                Survival::Yes
            } else {
                Survival::No
            }
        }
        Family::Snow => {
            // Read from `SnowLayerBlock#canSurvive`'s bytecode, in order:
            // the refusing tag first, then the overriding tag, then the
            // shape test. The order matters — a block in both tags is
            // refused, and swapping them would place snow on it.
            let below = world.block_at(x, y - 1, z);
            let name = below.split('[').next().unwrap_or(&below).to_string();
            if tags.contains("minecraft:cannot_support_snow_layer", &name) {
                return Survival::No;
            }
            if tags.contains("minecraft:support_override_snow_layer", &name) {
                return Survival::Yes;
            }
            match world.collision_face_full_up(x, y - 1, z) {
                Some(true) => Survival::Yes,
                None => Survival::Unknown,
                // A full snow layer supports another: this is how snow
                // deepens past one block.
                Some(false) => {
                    if name == "minecraft:snow" && below.contains("layers=8") {
                        Survival::Yes
                    } else {
                        Survival::No
                    }
                }
            }
        }
        Family::Carpet | Family::MossyCarpet => match world.air_at(x, y - 1, z) {
            Some(true) => Survival::No,
            Some(false) => Survival::Yes,
            None => Survival::Unknown,
        },
        Family::SporeBlossom => {
            // The ceiling is asked about, not the block below.
            match world.can_support_center(x, y + 1, z, 0) {
                Some(false) => Survival::No,
                None => Survival::Unknown,
                Some(true) => {
                    if world.fluid_at(x, y, z) == "minecraft:water" {
                        Survival::No
                    } else {
                        Survival::Yes
                    }
                }
            }
        }
        Family::SmallDripleaf => {
            if tags.contains("minecraft:supports_small_dripleaf", below_block) {
                return Survival::Yes;
            }
            // `getFluidState(pos.above()).isSourceOfType(WATER)` — a source,
            // so flowing water above does not qualify. This port's fluid
            // answer is the block's own fluid, which for `water[level=0]` is
            // the source and for flowing water is not.
            let above = world.block_at(x, y + 1, z);
            let above_source = above == "minecraft:water[level=0]";
            if above_source && tags.contains("minecraft:supports_vegetation", below_block) {
                Survival::Yes
            } else {
                Survival::No
            }
        }
        Family::SugarCane => {
            if below_block == "minecraft:sugar_cane" {
                return Survival::Yes;
            }
            if !tags.contains("minecraft:supports_sugar_cane", below_block) {
                return Survival::No;
            }
            // Neighbours of the block *below*, not of the cane.
            for (dx, dz) in HORIZONTAL {
                let (nx, ny, nz) = (x + dx, y - 1, z + dz);
                if world.fluid_at(nx, ny, nz) == "minecraft:water" {
                    return Survival::Yes;
                }
                let beside = world.block_at(nx, ny, nz);
                let beside = beside.split_once('[').map_or(beside.as_str(), |(n, _)| n);
                if tags.contains("minecraft:supports_sugar_cane_adjacently", beside) {
                    return Survival::Yes;
                }
            }
            Survival::No
        }
        Family::GrowingPlantUp {
            head,
            body,
            cannot_support,
        } => {
            if tags.contains(cannot_support, below_block) {
                return Survival::No;
            }
            if below_block == head || below_block == body {
                return Survival::Yes;
            }
            match world.face_is_sturdy_up(x, y - 1, z) {
                Some(true) => Survival::Yes,
                Some(false) => Survival::No,
                None => Survival::Unknown,
            }
        }
    }
}
