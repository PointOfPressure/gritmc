//! `randomTick` — the bodies, for the blocks that grow, spread, decay and
//! melt.
//!
//! [`crate::randomtick`] chooses the positions; this module is what a chosen
//! block does. Every body here is transcribed from the 26.2 jar with
//! `javap -p -c -constants`, and the offsets quoted beside each are that
//! method's.
//!
//! # Dispatch is by declaring class, exactly as the JVM's is
//!
//! `BlockState.randomTick` is a virtual call, so what runs is the override
//! nearest the block's own class. [`Kind::of`] walks the block's class and
//! then its superclasses (`data/vanilla/set_placed_by.json`'s `classes` and
//! `superclasses`) and stops at the **first class that declares
//! `randomTick`** — ported or not. Stopping at an unported declarer matters:
//! `FrostedIceBlock` extends `IceBlock` and overrides it, so a walk that
//! skipped unported classes would melt frosted ice by `IceBlock`'s rule.
//!
//! [`DECLARERS`] is the jar's own list — every class in
//! `net.minecraft.world.level.block` whose `javap` shows a `randomTick`
//! method — and [`PORTED`] is the subset this module answers. The rest are
//! counted on the boot line, never approximated.
//!
//! # The random source is `Level.random`
//!
//! `tickChunk` passes `this.random` to every body, and `CocoaBlock` asks for
//! `level.getRandom()` — the same field. So every draw here comes off
//! [`crate::weather::level_random`], the one generator `/weather` and the
//! weather cycle already share. It is unseeded in vanilla
//! (`RandomSource.create()`), so no draw here can be compared to a vanilla
//! draw; what is exact is **which** draws are taken, in what order, with what
//! bounds, which is what decides the distribution a growth window produces.

use rustmc_behavior::place_survive::{block_of, property};
use rustmc_worldgen::rng::RandomSource;

use crate::brightness::Light;
use crate::edits::Blocks;
use crate::interact::BlockPos;
use crate::placement::with_property;
use crate::shapes::{Effects, ShapeTable, RECURSION_LIMIT, UPDATE_ALL, UPDATE_CLIENTS};

/// `Block.UPDATE_INVISIBLE | UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS` — `sipush
/// 260`, the flags sugar cane, cactus and a sapling's stage use. No
/// `UPDATE_CLIENTS`, so the change never reaches a client.
const INVISIBLE: i32 = 260;

/// Every class in `net.minecraft.world.level.block` that declares
/// `randomTick(BlockState, ServerLevel, BlockPos, RandomSource)`, read off
/// the jar. A class not in this list inherits.
pub const DECLARERS: &[&str] = &[
    "BambooSaplingBlock",
    "BambooStalkBlock",
    "BeetrootBlock",
    "BuddingAmethystBlock",
    "CactusBlock",
    "ChorusFlowerBlock",
    "CocoaBlock",
    "CropBlock",
    "DriedGhastBlock",
    "EyeblossomBlock",
    "FarmlandBlock",
    "FlowerPotBlock",
    "GrowingPlantHeadBlock",
    "IceBlock",
    "LeavesBlock",
    "LiquidBlock",
    "MangrovePropaguleBlock",
    "MushroomBlock",
    "NetherPortalBlock",
    "NetherWartBlock",
    "NyliumBlock",
    "PitcherCropBlock",
    "PointedDripstoneBlock",
    "RedStoneOreBlock",
    "SaplingBlock",
    "SnowLayerBlock",
    "SpeleothemBlock",
    "SpreadingSnowyBlock",
    "StemBlock",
    "SugarCaneBlock",
    "SweetBerryBushBlock",
    "TorchflowerCropBlock",
    "TurtleEggBlock",
    "VineBlock",
    "WeatheringCopperBarsBlock",
    "WeatheringCopperBulbBlock",
    "WeatheringCopperChainBlock",
    "WeatheringCopperChestBlock",
    "WeatheringCopperDoorBlock",
    "WeatheringCopperFullBlock",
    "WeatheringCopperGolemStatueBlock",
    "WeatheringCopperGrateBlock",
    "WeatheringCopperSlabBlock",
    "WeatheringCopperStairBlock",
    "WeatheringCopperTrapDoorBlock",
    "WeatheringLanternBlock",
    "WeatheringLightningRodBlock",
];

/// The declarers this module answers.
pub const PORTED: &[&str] = &[
    "BambooSaplingBlock",
    "BambooStalkBlock",
    "BeetrootBlock",
    "CactusBlock",
    "CocoaBlock",
    "CropBlock",
    "FarmlandBlock",
    "GrowingPlantHeadBlock",
    "IceBlock",
    "LeavesBlock",
    "NetherWartBlock",
    "NyliumBlock",
    "PitcherCropBlock",
    "SaplingBlock",
    "SnowLayerBlock",
    "SpreadingSnowyBlock",
    "StemBlock",
    "SugarCaneBlock",
    "SweetBerryBushBlock",
    "TorchflowerCropBlock",
];

/// What one block's `randomTick` is, resolved once per block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `CropBlock.randomTick` with the class's `getMaxAge`. `gate` is the
    /// `nextInt(3) != 0` that `BeetrootBlock` and `TorchflowerCropBlock`
    /// put in front of `super.randomTick`.
    Crop {
        max_age: i32,
        gate: bool,
    },
    Pitcher,
    Stem,
    SweetBerry,
    Cocoa,
    Sapling,
    SugarCane,
    Cactus,
    BambooSapling,
    BambooStalk,
    Spreading,
    Leaves,
    Ice,
    SnowLayer,
    Farmland,
    NetherWart,
    Nylium,
    /// `GrowingPlantHeadBlock` — kelp and the three vines.
    PlantHead,
    /// The chain reached `BlockBehaviour`, whose `randomTick` is empty.
    Nothing,
}

impl Kind {
    /// The override that runs for `block`, or `None` when the nearest
    /// declarer is one this module has not ported.
    #[must_use]
    pub fn of(classes: &crate::placedby::PlacedByTable, block: &str) -> Option<Self> {
        let own = classes.class_of(block)?;
        let chain = std::iter::once(own).chain(classes.supers_of(own).iter().map(String::as_str));
        for class in chain {
            let kind = match class {
                "CropBlock" => Self::Crop {
                    max_age: 7,
                    gate: false,
                },
                // `BeetrootBlock.getMaxAge` is `iconst_3`, and its
                // `randomTick` is `if (nextInt(3) != 0) super.randomTick`.
                "BeetrootBlock" => Self::Crop {
                    max_age: 3,
                    gate: true,
                },
                // `TorchflowerCropBlock.getMaxAge` is `iconst_2`, the same gate.
                "TorchflowerCropBlock" => Self::Crop {
                    max_age: 2,
                    gate: true,
                },
                "PitcherCropBlock" => Self::Pitcher,
                "StemBlock" => Self::Stem,
                "SweetBerryBushBlock" => Self::SweetBerry,
                "CocoaBlock" => Self::Cocoa,
                "SaplingBlock" => Self::Sapling,
                "SugarCaneBlock" => Self::SugarCane,
                "CactusBlock" => Self::Cactus,
                "BambooSaplingBlock" => Self::BambooSapling,
                "BambooStalkBlock" => Self::BambooStalk,
                "SpreadingSnowyBlock" => Self::Spreading,
                "LeavesBlock" => Self::Leaves,
                "IceBlock" => Self::Ice,
                "SnowLayerBlock" => Self::SnowLayer,
                "FarmlandBlock" => Self::Farmland,
                "NetherWartBlock" => Self::NetherWart,
                "NyliumBlock" => Self::Nylium,
                "GrowingPlantHeadBlock" => Self::PlantHead,
                "BlockBehaviour" => Self::Nothing,
                c if DECLARERS.contains(&c) => return None,
                _ => continue,
            };
            return Some(kind);
        }
        Some(Self::Nothing)
    }
}

/// Everything one body needs, for one random-tick pass.
pub struct Ctx<'a, R: RandomSource> {
    pub world: &'a dyn Blocks,
    pub shapes: Option<&'a ShapeTable>,
    pub light: &'a Light<'a>,
    pub rng: &'a mut R,
    pub fx: &'a mut Effects,
    /// `Level.isRaining()`, for `FarmlandBlock`'s `isRainingAt`.
    pub raining: bool,
}

impl<R: RandomSource> Ctx<'_, R> {
    fn get(&self, pos: BlockPos) -> String {
        self.world.state_at(pos)
    }

    /// `Level.setBlock(pos, state, flags)`.
    fn set(&mut self, pos: BlockPos, state: &str, flags: i32) {
        crate::shapes::set_block(
            self.world,
            self.shapes,
            pos,
            state,
            flags,
            RECURSION_LIMIT,
            self.fx,
        );
    }

    fn default_of(&self, block: &str) -> Option<String> {
        self.world.default_state(block)
    }

    /// `Level.removeBlock(pos, false)` —
    /// `setBlock(pos, getFluidState(pos).createLegacyBlock(), 3)`.
    fn remove(&mut self, pos: BlockPos) {
        let fluid = self.world.legacy_fluid_at(pos);
        self.set(pos, &fluid, UPDATE_ALL);
    }

    fn tag(&self, tag: &str, state: &str) -> bool {
        crate::fallable::tags().is_some_and(|t| t.contains(tag, block_of(state)))
    }
}

fn above(p: BlockPos) -> BlockPos {
    (p.0, p.1 + 1, p.2)
}

fn below(p: BlockPos, n: i32) -> BlockPos {
    (p.0, p.1 - n, p.2)
}

/// `BlockState.isAir()` — the three air blocks.
#[must_use]
pub fn is_air(state: &str) -> bool {
    matches!(
        block_of(state),
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}

fn int(state: &str, name: &str) -> i32 {
    property(state, name)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// Run `kind`'s `randomTick` for `state` at `pos`.
pub fn random_tick<R: RandomSource>(cx: &mut Ctx<'_, R>, kind: Kind, pos: BlockPos, state: &str) {
    match kind {
        Kind::Crop { max_age, gate } => {
            // `BeetrootBlock.randomTick` offsets 0-17: `nextInt(3) == 0`
            // returns without calling `super` — two in three ticks reach it.
            if gate && cx.rng.next_i32_bound(3) == 0 {
                return;
            }
            crop(cx, pos, state, max_age);
        }
        Kind::Pitcher => pitcher(cx, pos, state),
        Kind::Stem => stem(cx, pos, state),
        Kind::SweetBerry => sweet_berry(cx, pos, state),
        Kind::Cocoa => cocoa(cx, pos, state),
        Kind::Sapling => sapling(cx, pos, state),
        Kind::SugarCane => sugar_cane(cx, pos, state),
        Kind::Cactus => cactus(cx, pos, state),
        Kind::BambooSapling => bamboo_sapling(cx, pos),
        Kind::BambooStalk => bamboo_stalk(cx, pos, state),
        Kind::Spreading => spreading(cx, pos, state),
        Kind::Leaves => leaves(cx, pos, state),
        Kind::Ice => ice(cx, pos, state),
        Kind::SnowLayer => snow_layer(cx, pos),
        Kind::Farmland => farmland(cx, pos, state),
        Kind::NetherWart => nether_wart(cx, pos, state),
        Kind::Nylium => nylium(cx, pos, state),
        Kind::PlantHead => grow_plant_head(cx, pos, state),
        Kind::Nothing => {}
    }
}

/// `CropBlock.getGrowthSpeed(block, level, pos)`, offsets 0-324.
///
/// The 3x3 of blocks under the crop: each in `BlockTags.GROWS_CROPS` adds 1,
/// or 3 if its `moisture` is above 0 (`getValueOrElse(MOISTURE, 0)`, so a
/// tagged block without the property counts as dry); the eight around the
/// centre add a quarter of that. Then the speed halves when the same block
/// grows on both axes beside it, or — only if that did not fire — on any
/// diagonal. Both reads use `BlockState.is(Block)`, the crop's own block.
#[must_use]
pub fn growth_speed(
    world: &dyn Blocks,
    block: &str,
    pos: BlockPos,
    grows_crops: impl Fn(&str) -> bool,
) -> f32 {
    let mut f = 1.0f32;
    let under = below(pos, 1);
    for i in -1..=1 {
        for j in -1..=1 {
            let mut g = 0.0f32;
            let s = world.state_at((under.0 + i, under.1, under.2 + j));
            if grows_crops(&s) {
                g = 1.0;
                if int(&s, "moisture") > 0 {
                    g = 3.0;
                }
            }
            if i != 0 || j != 0 {
                g /= 4.0;
            }
            f += g;
        }
    }
    let is = |p: BlockPos| block_of(&world.state_at(p)) == block;
    let (x, y, z) = pos;
    let north = (x, y, z - 1);
    let south = (x, y, z + 1);
    let west = (x - 1, y, z);
    let east = (x + 1, y, z);
    let x_axis = is(west) || is(east);
    let z_axis = is(north) || is(south);
    if x_axis && z_axis {
        f /= 2.0;
    } else {
        let diagonal = is((west.0, y, west.2 - 1))
            || is((east.0, y, east.2 - 1))
            || is((east.0, y, east.2 + 1))
            || is((west.0, y, west.2 + 1));
        if diagonal {
            f /= 2.0;
        }
    }
    f
}

/// `nextInt((int)(25.0f / f) + 1)` — `fdiv`, `f2i`, `iconst_1`, `iadd`.
#[allow(clippy::cast_possible_truncation)]
fn growth_bound(f: f32) -> i32 {
    (25.0f32 / f) as i32 + 1
}

fn growth_draw<R: RandomSource>(cx: &mut Ctx<'_, R>, block: &str, pos: BlockPos) -> bool {
    let f = growth_speed(cx.world, block, pos, |s| {
        crate::fallable::tags().is_some_and(|t| t.contains("minecraft:grows_crops", block_of(s)))
    });
    cx.rng.next_i32_bound(growth_bound(f)) == 0
}

/// `CropBlock.getStateForAge(age)` — `defaultBlockState().setValue(AGE,
/// age)`, except `TorchflowerCropBlock`'s, which at 2 is the torchflower.
#[must_use]
pub fn crop_state_for_age(world: &dyn Blocks, block: &str, age: i32) -> Option<String> {
    if block == "minecraft:torchflower_crop" && age == 2 {
        return world.default_state("minecraft:torchflower");
    }
    let default = world.default_state(block)?;
    Some(with_property(&default, "age", &age.to_string()))
}

/// `CropBlock.randomTick`, offsets 0-68:
///
/// ```text
///   if (level.getRawBrightness(pos, 0) >= 9) {
///     int age = getAge(state);
///     if (age < getMaxAge()) {
///       float f = getGrowthSpeed(this, level, pos);
///       if (random.nextInt((int)(25.0F / f) + 1) == 0)
///         level.setBlock(pos, getStateForAge(age + 1), 2);
///   } }
/// ```
///
/// The light check comes **before** the draw, so a crop in the dark takes
/// none.
fn crop<R: RandomSource>(cx: &mut Ctx<'_, R>, pos: BlockPos, state: &str, max_age: i32) {
    if cx.light.raw(pos, 0) < 9 {
        return;
    }
    let age = int(state, "age");
    if age >= max_age {
        return;
    }
    let block = block_of(state).to_string();
    if growth_draw(cx, &block, pos) {
        if let Some(next) = crop_state_for_age(cx.world, &block, age + 1) {
            cx.set(pos, &next, UPDATE_CLIENTS);
        }
    }
}

/// `PitcherCropBlock.randomTick`, offsets 0-46, and `grow(level, state, pos,
/// 1)`, offsets 0-93. No light check here — `canGrow` makes it, through
/// `sufficientLight`, **after** the draw.
fn pitcher<R: RandomSource>(cx: &mut Ctx<'_, R>, pos: BlockPos, state: &str) {
    let f = growth_speed(cx.world, "minecraft:pitcher_crop", pos, |s| {
        crate::fallable::tags().is_some_and(|t| t.contains("minecraft:grows_crops", block_of(s)))
    });
    if cx.rng.next_i32_bound(growth_bound(f)) != 0 {
        return;
    }
    pitcher_grow(cx, pos, state, 1);
}

/// `PitcherCropBlock.grow(level, state, pos, by)`.
pub fn pitcher_grow<R: RandomSource>(cx: &mut Ctx<'_, R>, pos: BlockPos, state: &str, by: i32) {
    let age = (int(state, "age") + by).min(4);
    if !pitcher_can_grow(cx.world, cx.light, pos, state, age) {
        return;
    }
    let next = with_property(state, "age", &age.to_string());
    cx.set(pos, &next, UPDATE_CLIENTS);
    // `isDouble(age)` is `age >= 3`.
    if age >= 3 {
        let upper = with_property(&next, "half", "upper");
        cx.set(above(pos), &upper, UPDATE_ALL);
    }
}

/// `PitcherCropBlock.canGrow(level, pos, state, age)`.
fn pitcher_can_grow(
    world: &dyn Blocks,
    light: &Light<'_>,
    pos: BlockPos,
    state: &str,
    age: i32,
) -> bool {
    // `isMaxAge` is `age >= 4`; `sufficientLight` is `CropBlock
    // .hasSufficientLight`, raw brightness >= 8.
    if int(state, "age") >= 4 || light.raw(pos, 0) < 8 {
        return false;
    }
    if age >= 3 {
        let up = world.state_at(above(pos));
        return is_air(&up) || block_of(&up) == "minecraft:pitcher_crop";
    }
    true
}

/// `StemBlock.randomTick`, offsets 0-244.
///
/// Below age 7 it is a crop with `getGrowthSpeed`'s draw. At 7 the **same**
/// successful draw instead picks a side with
/// `Direction.Plane.HORIZONTAL.getRandomDirection` — `Util.getRandom` over
/// `[NORTH, EAST, SOUTH, WEST]`, one `nextInt(4)` — and fruits there only if
/// that side is air over a block in the stem's fruit-support tag. A blocked
/// side is a wasted draw, not a retry.
fn stem<R: RandomSource>(cx: &mut Ctx<'_, R>, pos: BlockPos, state: &str) {
    if cx.light.raw(pos, 0) < 9 {
        return;
    }
    let block = block_of(state).to_string();
    if !growth_draw(cx, &block, pos) {
        return;
    }
    let age = int(state, "age");
    if age < 7 {
        let next = with_property(state, "age", &(age + 1).to_string());
        cx.set(pos, &next, UPDATE_CLIENTS);
        return;
    }
    let (dir, dx, dz) = match cx.rng.next_i32_bound(4) {
        0 => ("north", 0, -1),
        1 => ("east", 1, 0),
        2 => ("south", 0, 1),
        _ => ("west", -1, 0),
    };
    let target = (pos.0 + dx, pos.1, pos.2 + dz);
    let soil = cx.get(below(target, 1));
    if !is_air(&cx.get(target)) {
        return;
    }
    // `Blocks` constructs `melon_stem` with `SUPPORTS_MELON_STEM_FRUIT` and
    // `pumpkin_stem` with `SUPPORTS_PUMPKIN_STEM_FRUIT`, fruit and attached
    // stem by `BlockIds`.
    let (fruit, attached, support) = match block.as_str() {
        "minecraft:melon_stem" => (
            "minecraft:melon",
            "minecraft:attached_melon_stem",
            "minecraft:supports_melon_stem_fruit",
        ),
        "minecraft:pumpkin_stem" => (
            "minecraft:pumpkin",
            "minecraft:attached_pumpkin_stem",
            "minecraft:supports_pumpkin_stem_fruit",
        ),
        _ => return,
    };
    if !cx.tag(support, &soil) {
        return;
    }
    let (Some(fruit), Some(attached)) = (cx.default_of(fruit), cx.default_of(attached)) else {
        return;
    };
    cx.set(target, &fruit, UPDATE_ALL);
    cx.set(pos, &with_property(&attached, "facing", dir), UPDATE_ALL);
}

/// `SweetBerryBushBlock.randomTick`, offsets 0-87: the draw comes **first**,
/// then the light at `pos.above()`.
fn sweet_berry<R: RandomSource>(cx: &mut Ctx<'_, R>, pos: BlockPos, state: &str) {
    let age = int(state, "age");
    if age >= 3 || cx.rng.next_i32_bound(5) != 0 || cx.light.raw(above(pos), 0) < 9 {
        return;
    }
    let next = with_property(state, "age", &(age + 1).to_string());
    cx.set(pos, &next, UPDATE_CLIENTS);
}

/// `CocoaBlock.randomTick`, offsets 0-58: `level.getRandom().nextInt(5)` —
/// the level's generator by name, the same one the pass hands in.
fn cocoa<R: RandomSource>(cx: &mut Ctx<'_, R>, pos: BlockPos, state: &str) {
    if cx.rng.next_i32_bound(5) != 0 {
        return;
    }
    let age = int(state, "age");
    if age < 2 {
        let next = with_property(state, "age", &(age + 1).to_string());
        cx.set(pos, &next, UPDATE_CLIENTS);
    }
}

/// `SaplingBlock.randomTick`, offsets 0-34, and `advanceTree`'s first arm.
///
/// Stage 0 cycles to 1 with flags 260 — **invisible to every client**. Stage
/// 1 calls `TreeGrower.growTree`, which places a configured tree feature off
/// `Level.random`; that arm is not ported, and the stage-1 sapling is left
/// standing, which is the one outcome vanilla never produces from a draw
/// that succeeded. The draw is still taken, so the stream is vanilla's.
fn sapling<R: RandomSource>(cx: &mut Ctx<'_, R>, pos: BlockPos, state: &str) {
    if cx.light.max_local_raw(above(pos)) < 9 || cx.rng.next_i32_bound(7) != 0 {
        return;
    }
    if int(state, "stage") == 0 {
        let next = with_property(state, "stage", "1");
        cx.set(pos, &next, INVISIBLE);
    }
}

/// `SugarCaneBlock.randomTick`, offsets 0-130. No draw at all: a cane grows
/// on every random tick it receives, sixteen ages to a block, three blocks
/// tall. Only the new block above is visible; the age writes are flags 260.
fn sugar_cane<R: RandomSource>(cx: &mut Ctx<'_, R>, pos: BlockPos, state: &str) {
    if !is_air(&cx.get(above(pos))) {
        return;
    }
    let block = block_of(state).to_string();
    let mut i = 1;
    while block_of(&cx.get(below(pos, i))) == block {
        i += 1;
    }
    if i >= 3 {
        return;
    }
    let age = int(state, "age");
    if age == 15 {
        if let Some(default) = cx.default_of(&block) {
            cx.set(above(pos), &default, UPDATE_ALL);
        }
        cx.set(pos, &with_property(state, "age", "0"), INVISIBLE);
    } else {
        cx.set(
            pos,
            &with_property(state, "age", &(age + 1).to_string()),
            INVISIBLE,
        );
    }
}

/// `CactusBlock.randomTick`, offsets 0-232.
///
/// ```text
///   if (!level.isEmptyBlock(above)) return;
///   i = 1; age = AGE;
///   while (below(i) is cactus) { if (++i == 3 && age == 15) return; }
///   if (age == 8 && canSurvive(default, above)) {
///     chance = i >= 3 ? 0.25 : 0.1;
///     if (random.nextDouble() <= chance) setBlockAndUpdate(above, CACTUS_FLOWER);
///   } else if (age == 15 && i < 3) {
///     setBlockAndUpdate(above, default);
///     s = state.setValue(AGE, 0); setBlock(pos, s, 260);
///     level.neighborChanged(s, above, this, null, false);
///   }
///   if (age < 15) setBlock(pos, state.setValue(AGE, age + 1), 260);
/// ```
///
/// The `neighborChanged` at offset 196 is not ported: it re-runs the new
/// block's survival check, which `setBlockAndUpdate` has just made with the
/// same neighbours. `canSurvive` for the flower position is approximated by
/// "every horizontal neighbour is air and the block below is the cactus",
/// which is `CactusBlock.canSurvive` for a position the cactus itself
/// supports.
fn cactus<R: RandomSource>(cx: &mut Ctx<'_, R>, pos: BlockPos, state: &str) {
    let up = above(pos);
    if !is_air(&cx.get(up)) {
        return;
    }
    let block = block_of(state).to_string();
    let age = int(state, "age");
    let mut i = 1;
    while block_of(&cx.get(below(pos, i))) == block {
        i += 1;
        if i == 3 && age == 15 {
            return;
        }
    }
    if age == 8 && cactus_can_survive(cx, up) {
        let chance = if i >= 3 { 0.25 } else { 0.1 };
        if cx.rng.next_f64() <= chance {
            if let Some(flower) = cx.default_of("minecraft:cactus_flower") {
                cx.set(up, &flower, UPDATE_ALL);
            }
        }
    } else if age == 15 && i < 3 {
        if let Some(default) = cx.default_of(&block) {
            cx.set(up, &default, UPDATE_ALL);
        }
        cx.set(pos, &with_property(state, "age", "0"), INVISIBLE);
    }
    if age < 15 {
        cx.set(
            pos,
            &with_property(state, "age", &(age + 1).to_string()),
            INVISIBLE,
        );
    }
}

fn cactus_can_survive<R: RandomSource>(cx: &Ctx<'_, R>, pos: BlockPos) -> bool {
    let (x, y, z) = pos;
    [(x, y, z - 1), (x, y, z + 1), (x - 1, y, z), (x + 1, y, z)]
        .iter()
        .all(|&p| is_air(&cx.get(p)))
}

/// `BambooSaplingBlock.randomTick`, offsets 0-42, and `growBamboo`:
/// `setBlock(above, BAMBOO[leaves=small], 3)`.
fn bamboo_sapling<R: RandomSource>(cx: &mut Ctx<'_, R>, pos: BlockPos) {
    if cx.rng.next_i32_bound(3) != 0
        || !is_air(&cx.get(above(pos)))
        || cx.light.raw(above(pos), 0) < 9
    {
        return;
    }
    bamboo_sapling_grow(cx, pos);
}

/// `BambooSaplingBlock.growBamboo`, also `performBonemeal`'s whole body.
pub fn bamboo_sapling_grow<R: RandomSource>(cx: &mut Ctx<'_, R>, pos: BlockPos) {
    if let Some(bamboo) = cx.default_of("minecraft:bamboo") {
        cx.set(
            above(pos),
            &with_property(&bamboo, "leaves", "small"),
            UPDATE_ALL,
        );
    }
}

fn bamboo_run(world: &dyn Blocks, pos: BlockPos, step: i32) -> i32 {
    let mut i = 0;
    while i < 16
        && block_of(&world.state_at((pos.0, pos.1 + step * (i + 1), pos.2))) == "minecraft:bamboo"
    {
        i += 1;
    }
    i
}

/// `BambooStalkBlock.randomTick`, offsets 0-81.
fn bamboo_stalk<R: RandomSource>(cx: &mut Ctx<'_, R>, pos: BlockPos, state: &str) {
    if int(state, "stage") != 0 {
        return;
    }
    if cx.rng.next_i32_bound(3) != 0
        || !is_air(&cx.get(above(pos)))
        || cx.light.raw(above(pos), 0) < 9
    {
        return;
    }
    let height = bamboo_run(cx.world, pos, -1) + 1;
    if height < 16 {
        bamboo_stalk_grow(cx, pos, state, height);
    }
}

/// `BambooStalkBlock.growBamboo(state, level, pos, random, height)`, offsets
/// 0-278.
pub fn bamboo_stalk_grow<R: RandomSource>(
    cx: &mut Ctx<'_, R>,
    pos: BlockPos,
    state: &str,
    height: i32,
) {
    let below1 = cx.get(below(pos, 1));
    let below2_pos = below(pos, 2);
    let below2 = cx.get(below2_pos);
    let is_bamboo = |s: &str| block_of(s) == "minecraft:bamboo";
    let mut leaves = "none";
    if height >= 1 {
        if !is_bamboo(&below1) || property(&below1, "leaves") == Some("none") {
            leaves = "small";
        } else {
            leaves = "large";
            if is_bamboo(&below2) {
                cx.set(
                    below(pos, 1),
                    &with_property(&below1, "leaves", "small"),
                    UPDATE_ALL,
                );
                cx.set(
                    below2_pos,
                    &with_property(&below2, "leaves", "none"),
                    UPDATE_ALL,
                );
            }
        }
    }
    let age = i32::from(int(state, "age") == 1 || is_bamboo(&below2));
    let stage = i32::from((height >= 11 && cx.rng.next_f32() < 0.25) || height == 15);
    if let Some(default) = cx.default_of("minecraft:bamboo") {
        let s = with_property(&default, "age", &age.to_string());
        let s = with_property(&s, "leaves", leaves);
        let s = with_property(&s, "stage", &stage.to_string());
        cx.set(above(pos), &s, UPDATE_ALL);
    }
}

/// `SpreadingSnowyBlock.canStayAlive(state, level, pos)`.
fn can_stay_alive(world: &dyn Blocks, light: &Light<'_>, state: &str, pos: BlockPos) -> bool {
    let up = world.state_at(above(pos));
    if block_of(&up) == "minecraft:snow" && int(&up, "layers") == 1 {
        return true;
    }
    if fluid_full(world, &up) {
        return false;
    }
    let _ = light;
    dampening_into(world, state, &up) < 15
}

/// `LightEngine.getLightDampeningInto(state, above, UP,
/// above.getLightDampening())`: 16 when the two occlusion shapes close the
/// face between them, otherwise the upper state's dampening. With no light
/// table it answers 0 — every block alive — which is what this server did
/// before the table existed.
fn dampening_into(world: &dyn Blocks, state: &str, up: &str) -> i32 {
    let Some(props) = crate::brightness::props() else {
        return 0;
    };
    let id = |s: &str| {
        world
            .state_id(s)
            .and_then(|i| u16::try_from(i).ok())
            .unwrap_or(0)
    };
    let (from, to) = (id(state), id(up));
    if crate::light_engine::shape_occludes(props, from, to, 1) {
        16
    } else {
        i32::from(props.dampening(to))
    }
}

/// `FluidState.isFull()` — `getAmount() == 8`: a source, or a falling flow.
fn fluid_full(world: &dyn Blocks, state: &str) -> bool {
    let Some(fluid) = world
        .place_props()
        .and_then(|p| p.get(state))
        .and_then(|p| p.fluid.clone())
    else {
        return false;
    };
    match fluid.as_str() {
        "minecraft:water" | "minecraft:lava" => true,
        _ => int(state, "level") >= 8,
    }
}

/// `SpreadingSnowyBlock.randomTick`, offsets 0-197.
///
/// Dies to the base block (`dirt`, for grass and mycelium alike) when it
/// cannot stay alive, and returns. Otherwise, with
/// `getMaxLocalRawBrightness(above) >= 9`, four attempts, each **three**
/// draws in argument order — `nextInt(3) - 1`, `nextInt(5) - 3`,
/// `nextInt(3) - 1` — at a base-block position the spreading block could stay
/// alive on with no water above it.
fn spreading<R: RandomSource>(cx: &mut Ctx<'_, R>, pos: BlockPos, state: &str) {
    let Some(base) = cx.default_of("minecraft:dirt") else {
        return;
    };
    if !can_stay_alive(cx.world, cx.light, state, pos) {
        cx.set(pos, &base, UPDATE_ALL);
        return;
    }
    if cx.light.max_local_raw(above(pos)) < 9 {
        return;
    }
    let block = block_of(state).to_string();
    let Some(default) = cx.default_of(&block) else {
        return;
    };
    for _ in 0..4 {
        let dx = cx.rng.next_i32_bound(3) - 1;
        let dy = cx.rng.next_i32_bound(5) - 3;
        let dz = cx.rng.next_i32_bound(3) - 1;
        let target = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
        let here = cx.get(target);
        if block_of(&here) != "minecraft:dirt" {
            continue;
        }
        // `canPropagate`: alive there, and no water in the block above.
        if !can_stay_alive(cx.world, cx.light, &default, target) || cx.world.water_at(above(target))
        {
            continue;
        }
        let snowy = cx.tag("minecraft:snow", &cx.get(above(target)));
        let next = with_property(&default, "snowy", if snowy { "true" } else { "false" });
        cx.set(target, &next, UPDATE_ALL);
    }
}

/// `LeavesBlock.randomTick`: `decaying(state)` — not persistent, distance 7
/// — then `dropResources` and `removeBlock(pos, false)`. The drops are not
/// ported: this server has no loot-table path from a tick.
fn leaves<R: RandomSource>(cx: &mut Ctx<'_, R>, pos: BlockPos, state: &str) {
    if property(state, "persistent") == Some("false") && int(state, "distance") == 7 {
        cx.remove(pos);
    }
}

/// `IceBlock.randomTick`: `getBrightness(BLOCK, pos) > 11 -
/// state.getLightDampening()` melts. `melt` in a dimension where water does
/// not evaporate is `setBlockAndUpdate(pos, WATER)` then
/// `neighborChanged(pos, WATER, null)`; the second is water's own
/// `LiquidBlock.neighborChanged`, which schedules a fluid tick, and this
/// server runs no fluid ticks.
fn ice<R: RandomSource>(cx: &mut Ctx<'_, R>, pos: BlockPos, state: &str) {
    let dampening = crate::brightness::props()
        .zip(cx.world.state_id(state).and_then(|i| u16::try_from(i).ok()))
        .map_or(0, |(p, id)| i32::from(p.dampening(id)));
    if cx.light.block(pos) <= 11 - dampening {
        return;
    }
    // `EnvironmentAttributes.WATER_EVAPORATES` is true in the nether alone.
    let evaporates = crate::sky::active().is_some_and(|s| s.dimension() == "minecraft:the_nether");
    if evaporates {
        cx.remove(pos);
    } else if let Some(water) = cx.default_of("minecraft:water") {
        cx.set(pos, &water, UPDATE_ALL);
    }
}

/// `SnowLayerBlock.randomTick`: block light above 11 removes it. Drops not
/// ported, as for leaves.
fn snow_layer<R: RandomSource>(cx: &mut Ctx<'_, R>, pos: BlockPos) {
    if cx.light.block(pos) > 11 {
        cx.remove(pos);
    }
}

/// `FarmlandBlock.randomTick`, offsets 0-113.
///
/// Water within `betweenClosed(-4,0,-4 .. 4,1,4)` or rain at `pos.above()`
/// sets moisture 7 (if not already); otherwise moisture falls by one, and at
/// 0 the block turns to dirt unless what stands on it is in
/// `MAINTAINS_FARMLAND`. `turnToDirt`'s `pushEntitiesUp` is not ported.
fn farmland<R: RandomSource>(cx: &mut Ctx<'_, R>, pos: BlockPos, state: &str) {
    let moisture = int(state, "moisture");
    let wet = near_water(cx.world, pos) || raining_at(cx, above(pos));
    if wet {
        if moisture < 7 {
            cx.set(pos, &with_property(state, "moisture", "7"), UPDATE_CLIENTS);
        }
    } else if moisture > 0 {
        cx.set(
            pos,
            &with_property(state, "moisture", &(moisture - 1).to_string()),
            UPDATE_CLIENTS,
        );
    } else if !cx.tag("minecraft:maintains_farmland", &cx.get(above(pos))) {
        if let Some(dirt) = cx.default_of("minecraft:dirt") {
            cx.set(pos, &dirt, UPDATE_ALL);
        }
    }
}

/// `FarmlandBlock.isNearWater` — any `FluidTags.WATER` in the 9x2x9 box at
/// and above the block.
fn near_water(world: &dyn Blocks, pos: BlockPos) -> bool {
    for dy in 0..=1 {
        for dz in -4..=4 {
            for dx in -4..=4 {
                if world.water_at((pos.0 + dx, pos.1 + dy, pos.2 + dz)) {
                    return true;
                }
            }
        }
    }
    false
}

/// `Level.isRainingAt(pos)`, for the plains-like biomes this server serves:
/// raining, and the position sees the sky. The biome's precipitation and the
/// `MOTION_BLOCKING` height test are folded into the sky test.
fn raining_at<R: RandomSource>(cx: &Ctx<'_, R>, pos: BlockPos) -> bool {
    cx.raining && cx.light.sky_open(pos)
}

/// `NetherWartBlock.randomTick`: `age < 3 && nextInt(10) == 0` -> age + 1,
/// flags 2.
fn nether_wart<R: RandomSource>(cx: &mut Ctx<'_, R>, pos: BlockPos, state: &str) {
    let age = int(state, "age");
    if age < 3 && cx.rng.next_i32_bound(10) == 0 {
        cx.set(
            pos,
            &with_property(state, "age", &(age + 1).to_string()),
            UPDATE_CLIENTS,
        );
    }
}

/// `NyliumBlock.randomTick`: `canBeNylium` false — the dampening into the
/// block above is 15 or more, the same `getLightDampeningInto` arithmetic as
/// grass but with no snow-layer and no fluid arm — turns it to netherrack
/// with `setBlockAndUpdate`. Nylium never spreads on a random tick; that is
/// bone meal's.
fn nylium<R: RandomSource>(cx: &mut Ctx<'_, R>, pos: BlockPos, state: &str) {
    if dampening_into(cx.world, state, &cx.get(above(pos))) < 15 {
        return;
    }
    if let Some(netherrack) = cx.default_of("minecraft:netherrack") {
        cx.set(pos, &netherrack, UPDATE_ALL);
    }
}

/// A growing plant's head: its growth direction, its chance per random tick,
/// and the body block a head turns into once something grows past it — the
/// four `GrowingPlantHeadBlock` constructors' arguments, read off their
/// `<init>`s.
#[must_use]
pub fn plant_head(block: &str) -> Option<(i32, f64, &'static str)> {
    Some(match block {
        // `KelpBlock.<init>`: `Direction.UP`, `iconst_1` (fluid ticks), 0.14.
        "minecraft:kelp" => (1, 0.14, "minecraft:kelp_plant"),
        // `TwistingVinesBlock.<init>`: UP, 0.1.
        "minecraft:twisting_vines" => (1, 0.1, "minecraft:twisting_vines_plant"),
        // `WeepingVinesBlock.<init>` and `CaveVinesBlock.<init>`: DOWN, 0.1.
        "minecraft:weeping_vines" => (-1, 0.1, "minecraft:weeping_vines_plant"),
        "minecraft:cave_vines" => (-1, 0.1, "minecraft:cave_vines_plant"),
        _ => return None,
    })
}

/// `GrowingPlantHeadBlock.randomTick`, offsets 0-72:
///
/// ```text
///   if (AGE < 25 && random.nextDouble() < growPerTickProbability) {
///     growPos = pos.relative(growthDirection);
///     if (canGrowInto(level.getBlockState(growPos)))
///       level.setBlockAndUpdate(growPos, getGrowIntoState(state, level.getRandom()));
///   }
/// ```
///
/// `canGrowInto` is `is(WATER)` for kelp and `isAir()` for the vines;
/// `getGrowIntoState` is `state.cycle(AGE)`, and `CaveVinesBlock` adds a
/// `nextFloat() < 0.11F` for `BERRIES` after it. The head left behind turns
/// into the body by its own `updateShape` — `crate::shapes` carries that
/// rule.
fn grow_plant_head<R: RandomSource>(cx: &mut Ctx<'_, R>, pos: BlockPos, state: &str) {
    let block = block_of(state);
    let Some((dir, chance, _)) = plant_head(block) else {
        return;
    };
    let age = int(state, "age");
    if age >= 25 || cx.rng.next_f64() >= chance {
        return;
    }
    let grow = (pos.0, pos.1 + dir, pos.2);
    let there = cx.get(grow);
    let fits = if block == "minecraft:kelp" {
        block_of(&there) == "minecraft:water"
    } else {
        is_air(&there)
    };
    if !fits {
        return;
    }
    let mut next = with_property(state, "age", &(age + 1).to_string());
    if block == "minecraft:cave_vines" {
        let berries = cx.rng.next_f32() < 0.11;
        next = with_property(&next, "berries", if berries { "true" } else { "false" });
    }
    cx.set(grow, &next, UPDATE_ALL);
}

/// What bone meal does to a block this module owns the growth of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Meal {
    /// Not one of this module's blocks — `crate::bonemeal`'s crop arm, or
    /// its refusal, decides.
    NotMine,
    /// `isValidBonemealTarget` is false: `growCrop` returns false and the
    /// stack is untouched.
    Pass,
    /// `performBonemeal` ran and the writes are in the context's effects.
    Grown,
}

/// `BoneMealItem.growCrop` for the bonemealable blocks whose growth lives in
/// this module — the ones whose `isBonemealSuccess` is `iconst_1; ireturn`
/// (no draw), and the stems, whose `performBonemeal` draws one
/// `Mth.nextInt(random, 2, 5)`.
///
/// ```text
///   SweetBerryBushBlock  valid: age < 3     perform: setBlock(age + 1, 2)
///   CocoaBlock           valid: age < 2     perform: setBlock(age + 1, 2)
///   BambooSaplingBlock   valid: above.isAir() && insideBuildHeight(above)
///                        perform: growBamboo — setBlock(above, BAMBOO[leaves=small], 3)
///   PitcherCropBlock     valid: canGrow(lower, lowerState, lowerAge + 1)
///                        perform: grow(level, lowerState, lowerPos, 1)
///   StemBlock            valid: age != 7
///                        perform: i = min(7, age + nextInt(2, 5));
///                                 setBlock(i, 2); if (i == 7) state.randomTick(...)
/// ```
pub fn bone_meal<R: RandomSource>(cx: &mut Ctx<'_, R>, pos: BlockPos, state: &str) -> Meal {
    let age = int(state, "age");
    match block_of(state) {
        "minecraft:sweet_berry_bush" => {
            if age >= 3 {
                return Meal::Pass;
            }
            cx.set(
                pos,
                &with_property(state, "age", &(age + 1).min(3).to_string()),
                UPDATE_CLIENTS,
            );
            Meal::Grown
        }
        "minecraft:cocoa" => {
            if age >= 2 {
                return Meal::Pass;
            }
            cx.set(
                pos,
                &with_property(state, "age", &(age + 1).to_string()),
                UPDATE_CLIENTS,
            );
            Meal::Grown
        }
        "minecraft:bamboo_sapling" => {
            let top = crate::sky::active().map_or(320, crate::sky::SkyModel::top_y);
            if !is_air(&cx.get(above(pos))) || above(pos).1 >= top {
                return Meal::Pass;
            }
            bamboo_sapling_grow(cx, pos);
            Meal::Grown
        }
        "minecraft:pitcher_crop" => {
            // `getLowerHalf`: this block if it is the lower half, else the
            // block below if that is a lower pitcher crop, else nothing.
            let (lower_pos, lower) = if property(state, "half") == Some("lower") {
                (pos, state.to_string())
            } else {
                let b = below(pos, 1);
                let s = cx.get(b);
                if block_of(&s) != "minecraft:pitcher_crop" || property(&s, "half") != Some("lower")
                {
                    return Meal::Pass;
                }
                (b, s)
            };
            let next = int(&lower, "age") + 1;
            if !pitcher_can_grow(cx.world, cx.light, lower_pos, &lower, next) {
                return Meal::Pass;
            }
            pitcher_grow(cx, lower_pos, &lower, 1);
            Meal::Grown
        }
        // `SaplingBlock`: valid while the tree's minimum height fits under
        // the build limit; `isBonemealSuccess` is `nextFloat() < 0.45`
        // (`level.getRandom()`, offsets 0-22); `performBonemeal` is
        // `advanceTree`, whose stage-0 arm cycles STAGE with flags 260 —
        // invisible — and whose stage-1 arm grows the tree, which is not
        // ported (see [`sapling`]). `growCrop` returns true either way, so
        // the stack shrinks and `useOn` sends 1505 on every use: a failed
        // roll and an invisible stage look the same on the wire.
        b if b.ends_with("_sapling") && b != "minecraft:bamboo_sapling" => {
            let top = crate::sky::active().map_or(320, crate::sky::SkyModel::top_y);
            if pos.1 + 1 >= top {
                return Meal::Pass;
            }
            if f64::from(cx.rng.next_f32()) < 0.45 && int(state, "stage") == 0 {
                cx.set(pos, &with_property(state, "stage", "1"), INVISIBLE);
            }
            Meal::Grown
        }
        "minecraft:melon_stem" | "minecraft:pumpkin_stem" => {
            if age == 7 {
                return Meal::Pass;
            }
            let i = (age + cx.rng.next_i32_bound(4) + 2).min(7);
            let next = with_property(state, "age", &i.to_string());
            cx.set(pos, &next, UPDATE_CLIENTS);
            if i == 7 {
                stem(cx, pos, &next);
            }
            Meal::Grown
        }
        _ => Meal::NotMine,
    }
}

#[cfg(test)]
mod tests;
