//! The decoration stage — the world a feature sees.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `WorldGenRegion`, `ChunkGenerator#applyBiomeDecoration`.
//!
//! Fourteen feature families ask this crate fourteen slightly different
//! questions about the world, and every one of them reduces to a handful of
//! primitives: what block is here, what is that block *like*, how high is the
//! ground, which biome is this, and would this state survive. [`Decoration`]
//! answers all fourteen traits from those primitives over a [`Canvas`].
//!
//! # Where the block facts come from, and why they are a trait
//!
//! "Is this air", "does this face hold a torch", "is this replaceable" are
//! properties of a *block state*, not of worldgen, and their answers live in
//! `rustmc_behavior::block_props` — which is a **dev**-dependency of this
//! crate and must stay one. So they arrive through [`BlockFacts`], which the
//! caller implements. The same goes for `canSurvive`, which is a whole
//! sub-project of its own.
//!
//! Every one of those methods can answer `None`, and `None` is not "no". A
//! feature that cannot get an answer refuses and is counted; a feature that
//! reads `false` from a missing table places nothing and looks like a working
//! generator with an empty world. That distinction is the reason the
//! decoration gate can say "165 computed, 4 replayed" rather than "165 fine".

use crate::canvas::{Canvas, LiveHeightmap};
use crate::feature::block_predicate::PredicateWorld;
use crate::feature::placement::{HeightmapKind, PlacementWorld};
use crate::tags::BlockTags;

/// What a block state *is*, as far as worldgen cares.
///
/// `None` means the answer is unknown — the extract is stale, or the state is
/// one nothing has ever produced. It is never a polite "no": see the module
/// docs.
pub trait BlockFacts {
    fn is_air(&self, state: &str) -> Option<bool>;
    /// The fluid's registry name, or `minecraft:empty`. **Not** derivable
    /// from the block name: `minecraft:seagrass` is water, and so are 11,792
    /// of the game's 32,366 states.
    fn fluid(&self, state: &str) -> Option<String>;
    fn is_solid_render(&self, state: &str) -> Option<bool>;
    /// `BlockState#isSolid()`, which is not `isSolidRender` — leaves are
    /// solid and do not render solid.
    fn is_solid(&self, state: &str) -> Option<bool>;
    fn is_replaceable(&self, state: &str) -> Option<bool>;
    /// `isFaceSturdy(direction, SupportType.FULL)`, `direction` indexing
    /// `Direction.values()`.
    fn face_is_sturdy(&self, state: &str, direction: usize) -> Option<bool>;

    /// `Block.canSupportCenter` — `SupportType.CENTER`, and a flat refusal
    /// for a DOWN face on `#unstable_bottom_center`. Not answerable from
    /// [`Self::face_is_sturdy`]: a fence has no sturdy *full* face and
    /// supports a centre on all six.
    fn can_support_center(&self, state: &str, direction: usize) -> Option<bool>;
    /// `Block.isFaceFull(state.getFaceOcclusionShape(dir))`.
    fn face_is_full(&self, state: &str, direction: usize) -> Option<bool>;
    /// `Block.isFaceFull(state.getCollisionShape(...), dir)` — a different
    /// question again, and the one that lets glow lichen grow on leaves.
    fn face_is_collision_full(&self, state: &str, direction: usize) -> Option<bool>;
    /// Whether the state counts towards one of the live heightmaps.
    fn matches_heightmap(&self, state: &str, map: LiveHeightmap) -> bool;

    /// `Block.defaultBlockState()`, spelled canonically.
    ///
    /// Defaulted to `None` so a facts implementation that carries no
    /// block-property table still compiles; the jigsaw replacement then
    /// writes a `final_state` verbatim, which is what it did before
    /// 2026-08-06. `TableFacts` answers it.
    fn default_state(&self, _block: &str) -> Option<String> {
        None
    }
}

/// The slice of the world the survival rules read.
///
/// Lent out on its own rather than handing over the whole [`Decoration`],
/// which would be a second mutable borrow of the canvas. It carries *both*
/// questions the rules ask — the block, and whether the block below presents
/// a sturdy up face — because lending only the first is what made seagrass
/// and kelp refuse 3,387 times in a 25-chunk world: not a missing port, a
/// missing argument.
/// What `LevelReader#getRawBrightness(pos, 0)` answers to a feature.
///
/// **15, everywhere, underground included**, and it is derived rather than
/// assumed. `ChunkPyramid`'s generation pyramid orders `FEATURES` before
/// `INITIALIZE_LIGHT` and `LIGHT`, so the chunk being decorated has no sky
/// light data at all. `SkyLightSectionStorage.getLightValue` answers a column
/// with no recorded top section by returning `15` outright — the
/// `topSections.get(getZeroNode(sec)) == currentLowestY` branch — without
/// ever reaching the data layer, and `LevelLightEngine.getRawBrightness` is
/// `max(blockLight 0, skyLight 15 - 0)`.
///
/// Corroborated on the r=16 window at seed 20260722: every one of the 28
/// mushrooms vanilla places there stands on `podzol`, a member of
/// `#overrides_mushroom_light_requirement`, and it places none at the surface
/// in 1,089 chunks.
pub const WORLDGEN_RAW_BRIGHTNESS: i32 = 15;

pub struct SurvivalView<'a> {
    pub block_at: &'a dyn Fn(i32, i32, i32) -> String,
    pub face_is_sturdy_up: &'a dyn Fn(i32, i32, i32) -> Option<bool>,
    /// `BlockState#isAir` — a carpet only asks whether the block below is
    /// there at all.
    pub air_at: &'a dyn Fn(i32, i32, i32) -> Option<bool>,
    /// `Block.isFaceFull(getCollisionShape(..), UP)` — what a snow layer
    /// stands on. The **collision** shape, which is a different question
    /// from occlusion and from `isFaceSturdy`, and the reason a snow layer
    /// sits on a slab.
    pub collision_face_full_up: &'a dyn Fn(i32, i32, i32) -> Option<bool>,
    /// `Block.canSupportCenter(level, pos, direction)`. A spore blossom hangs
    /// from a ceiling by its *centre*, which no sturdy-face answer covers.
    pub can_support_center: &'a dyn Fn(i32, i32, i32, usize) -> Option<bool>,
    /// `LevelReader#getFluidState`, as a registry name.
    ///
    /// Off the block-property table rather than off the block's name: a
    /// waterlogged fence or a seagrass has a **water** fluid state and a name
    /// test sees neither. The lily pad's rule asks for the fluid at its own
    /// position, where the answer decides whether the pad is placed at all.
    pub fluid_at: &'a dyn Fn(i32, i32, i32) -> String,
    /// `LevelReader#getRawBrightness(pos, 0)` — what a mushroom's own rule
    /// asks, and the only caller.
    ///
    /// A decorating world answers **15** everywhere, which is a reading of
    /// the jar and not a stub: `ChunkPyramid` runs `FEATURES` before
    /// `INITIALIZE_LIGHT`, so the chunk has no sky data, and
    /// `SkyLightSectionStorage.getLightValue` returns 15 for a column with no
    /// recorded top section rather than consulting a data layer. See
    /// `rustmc_behavior::survival::SurvivalWorld::raw_brightness`.
    pub raw_brightness: &'a dyn Fn(i32, i32, i32) -> Option<i32>,
    /// `BlockState#isSolidRender` of the block at a position — what
    /// `MushroomBlock#mayPlaceOn` asks of the block below once it is dark
    /// enough.
    pub is_solid_render: &'a dyn Fn(i32, i32, i32) -> Option<bool>,
}

/// `BlockState#canSurvive`, which the caller answers.
pub trait SurvivalRules {
    /// `None` where this port has no rule for the block — distinct from
    /// `Some(false)`, which is a real refusal.
    fn would_survive(
        &self,
        view: &SurvivalView<'_>,
        state: &str,
        pos: (i32, i32, i32),
    ) -> Option<bool>;
}

/// The biome and climate questions decoration asks.
pub trait BiomeFacts {
    /// The biome at a **block** position, resolved through its 4x4x4 cell.
    fn biome_at(&mut self, x: i32, y: i32, z: i32) -> String;
    /// `BiomeFilter#shouldPlace` — does the biome here list the feature
    /// currently being placed?
    fn biome_lists(&mut self, biome: &str, feature: &str) -> bool;
    /// `Biome.BIOME_INFO_NOISE.getValue(x, z, false)`, on already-divided
    /// coordinates.
    fn info_noise(&mut self, x: f64, z: f64) -> f64;
    /// `Biome#coldEnoughToSnow`.
    fn cold_enough_to_snow(&mut self, biome: &str, x: i32, y: i32, z: i32) -> bool;
    /// `Biome#getPrecipitationAt(pos, seaLevel) == SNOW`.
    fn precipitation_is_snow(&mut self, biome: &str, x: i32, y: i32, z: i32) -> bool;
}

/// The world a feature is handed.
pub struct Decoration<'a> {
    canvas: &'a mut Canvas,
    facts: &'a dyn BlockFacts,
    survival: &'a dyn SurvivalRules,
    biomes: &'a mut dyn BiomeFacts,
    tags: &'a BlockTags,
    sea_level: i32,
    /// `WorldGenerationContext#getGenDepth` — the **generator's** height, not
    /// the canvas's.
    ///
    /// `new WorldGenerationContext(generator, level)` takes
    /// `min(level.getHeight(), generator.getGenDepth())`, and in the nether
    /// those disagree: the dimension stores 256 blocks and the noise settings
    /// generate 128. Every vertical anchor a placement resolves — every
    /// `below_top`, every `above_bottom` — is measured against this and not
    /// against the canvas.
    ///
    /// It is carried rather than derived because the overworld's two numbers
    /// are equal, so a canvas-derived value looks right in every overworld
    /// test and is wrong by 128 in the nether. `stage::surface::GenExtent`
    /// learned the same lesson the same way, for 232,634 blocks.
    gen_depth: i32,
    /// The placed feature currently running, which `BiomeFilter` asks about.
    running: String,
    /// How far outside its own chunk a feature may write. Vanilla's feature
    /// region is 3x3, so one.
    write_radius: i32,
    /// The chunk whose features are running, for the write mask.
    center: (i32, i32),
    /// The world seed. The geode's noise is built from it directly rather
    /// than from the feature stream, so it has to reach this far in.
    world_seed: i64,
    /// `RandomState.getOrCreateRandomFactory(minecraft:worldgen_region_random)`,
    /// or `None` for a world that has not been given one.
    ///
    /// `None` is not a default value, it is an absence: every reader turns it
    /// into a refusal. A test world that never sets it keeps the behaviour it
    /// had before this field existed.
    region_random_factory: Option<crate::rng::AnyPositionalFactory>,
    /// `WorldGenRegion.getRandom()` for the chunk currently being decorated.
    ///
    /// **One stream per generating chunk, shared by every feature in it**,
    /// because that is what a `WorldGenRegion` is: the constructor takes
    /// `factory.at(center.getPos().getWorldPosition())` once, and every
    /// feature placed for that chunk calls `getRandom()` on the same object.
    /// Re-deriving it per feature would be a different stream that looks the
    /// same on any chunk where only one feature draws from it.
    region_random: Option<crate::rng::AnyRandomSource>,
    /// What `getRawBrightness(pos, 0)` answers here: [`WORLDGEN_RAW_BRIGHTNESS`]
    /// in a dimension with sky light, 0 in one without. See
    /// [`Self::without_sky_light`].
    raw_brightness: i32,
}

impl<'a> Decoration<'a> {
    // Eight, and every one of them is a distinct fact about the world this
    // feature sees. A builder or a config struct would only move the same
    // eight somewhere else, and the two that are easiest to confuse —
    // `sea_level` and `gen_depth` — are exactly the two a caller must be made
    // to state, so defaulting either is the wrong direction.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        canvas: &'a mut Canvas,
        facts: &'a dyn BlockFacts,
        survival: &'a dyn SurvivalRules,
        biomes: &'a mut dyn BiomeFacts,
        tags: &'a BlockTags,
        sea_level: i32,
        gen_depth: i32,
        world_seed: i64,
    ) -> Self {
        Self {
            canvas,
            facts,
            survival,
            biomes,
            tags,
            sea_level,
            gen_depth,
            running: String::new(),
            write_radius: 1,
            center: (0, 0),
            world_seed,
            region_random_factory: None,
            region_random: None,
            raw_brightness: WORLDGEN_RAW_BRIGHTNESS,
        }
    }

    /// A dimension whose `DimensionType.hasSkyLight` is false. In 26.2 that is
    /// the nether alone: `the_end.json` says `has_skylight: true`.
    ///
    /// `LevelLightEngine` is built with `hasSkyLight` as its third argument and
    /// leaves `skyEngine` **null** when it is false, and `getRawBrightness` is
    /// `max(blockEngine == null ? 0 : blockLight, skyEngine == null ? 0 :
    /// skyLight - amount)` (offsets 0..49 in the pinned jar). With no block
    /// light computed yet during `FEATURES`, that is **0**, not the 15 a sky-lit
    /// chunk with no sky data answers.
    ///
    /// The one rule that reads it is `MushroomBlock.canSurvive`: off a
    /// `#mushroom_grow_block`, a mushroom survives only below brightness 13 and
    /// on a solid-render block. At 15 every nether mushroom on netherrack was
    /// refused; vanilla places them.
    #[must_use]
    pub const fn without_sky_light(mut self) -> Self {
        self.raw_brightness = 0;
        self
    }

    /// Give this world the level's own random, as a `WorldGenRegion` has one.
    ///
    /// Separate from [`Self::new`] rather than a ninth argument, and that is a
    /// judgement rather than convenience: the eight are facts every caller
    /// must state, and this is a capability a caller may not have. A world
    /// without it refuses the features that need it, which is the behaviour
    /// every caller had before the field existed.
    ///
    /// `legacy_random_source` comes from the dimension's noise settings, the
    /// same boolean `SurfaceNoises::for_dimension` takes: the overworld is
    /// Xoroshiro, the nether and the end are legacy.
    #[must_use]
    pub fn with_level_random(mut self, world_seed: i64, legacy_random_source: bool) -> Self {
        self.region_random_factory = Some(
            crate::rng::AnyPositionalFactory::for_world(world_seed, legacy_random_source)
                .fork_from_hash_of("minecraft:worldgen_region_random"),
        );
        self.begin_chunk(self.center.0, self.center.1);
        self
    }

    /// Point the world at the chunk whose features are about to run.
    ///
    /// This is also where the level's random is taken, because a
    /// `WorldGenRegion` is built per generating chunk and takes its own in the
    /// constructor:
    ///
    /// ```text
    ///   this.random = level.getChunkSource().randomState()
    ///       .getOrCreateRandomFactory(minecraft:worldgen_region_random)
    ///       .at(center.getPos().getWorldPosition())
    /// ```
    ///
    /// `ChunkPos.getWorldPosition()` is `(minBlockX, 0, minBlockZ)` — the y is
    /// a literal zero and not the chunk's minimum, which is the one part of
    /// this a reader is likely to assume rather than check.
    pub fn begin_chunk(&mut self, chunk_x: i32, chunk_z: i32) {
        self.center = (chunk_x, chunk_z);
        use crate::rng::PositionalRandomFactory;
        self.region_random = self
            .region_random_factory
            .as_ref()
            .map(|factory| factory.at(chunk_x * 16, 0, chunk_z * 16));
    }

    /// `WorldGenLevel.getSeed()`.
    #[must_use]
    pub const fn world_seed(&self) -> i64 {
        self.world_seed
    }

    /// `BlockState.isSolid()` off the block-property table.
    #[must_use]
    pub fn block_is_solid(&self, state: &str) -> Option<bool> {
        self.facts.is_solid(state)
    }

    /// `isFaceSturdy(direction, FULL)` off the block-property table.
    #[must_use]
    pub fn block_face_is_sturdy(&self, state: &str, direction: usize) -> Option<bool> {
        self.facts.face_is_sturdy(state, direction)
    }

    /// `Block.defaultBlockState()` for the jigsaw replacement's
    /// `parseForBlock`, delegated to the block facts.
    #[must_use]
    pub fn default_state(&self, block: &str) -> Option<String> {
        self.facts.default_state(block)
    }

    /// `Heightmap.Types.WORLD_SURFACE_WG` — the noise-stage fossil, frozen
    /// before decoration began. The `GravityProcessor` of a terrain-matching
    /// jigsaw piece reads it and nothing else.
    #[must_use]
    pub fn world_surface_wg(&self, x: i32, z: i32) -> i32 {
        self.canvas.world_surface_wg(x, z)
    }

    /// The resolved block tags, for a processor rule's `tag_match`.
    #[must_use]
    pub const fn block_tags(&self) -> &BlockTags {
        self.tags
    }

    /// Write, and raise the live heightmaps if the block took.
    fn write(&mut self, x: i32, y: i32, z: i32, state: &str) {
        // The write mask. A feature reaches one chunk out and no further —
        // vanilla's region simply has no chunk there, so the write is
        // dropped rather than refused, and a canvas with a wider margin
        // would silently accept it.
        let (dx, dz) = ((x >> 4) - self.center.0, (z >> 4) - self.center.1);
        if dx.abs() > self.write_radius || dz.abs() > self.write_radius {
            return;
        }
        if self.canvas.set_block(x, y, z, state) {
            let facts = self.facts;
            self.canvas
                .update_heightmaps(x, y, z, &FactsAsPredicates(facts));
        }
    }
}

/// Adapts [`BlockFacts`] to the canvas's own heightmap trait, so the caller
/// implements one thing rather than two that must agree.
struct FactsAsPredicates<'a>(&'a dyn BlockFacts);

impl crate::canvas::HeightmapPredicates for FactsAsPredicates<'_> {
    fn matches(&self, state: &str, map: LiveHeightmap) -> bool {
        self.0.matches_heightmap(state, map)
    }
}

impl PredicateWorld for Decoration<'_> {
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
        self.canvas.block_at(x, y, z).to_string()
    }

    fn fluid_at(&mut self, x: i32, y: i32, z: i32) -> String {
        let state = self.canvas.block_at(x, y, z).to_string();
        self.facts
            .fluid(&state)
            .unwrap_or_else(|| "minecraft:empty".to_string())
    }

    fn would_survive(&mut self, state: &str, x: i32, y: i32, z: i32) -> Option<bool> {
        // The rules need to read the world, and the world is `self` — so the
        // canvas and the facts are lent out on their own rather than the
        // whole struct, which would be a second mutable borrow.
        let canvas = &*self.canvas;
        let facts = self.facts;
        let read = |bx: i32, by: i32, bz: i32| canvas.block_at(bx, by, bz).to_string();
        // `Direction.UP` is index 1 in `Direction.values()`.
        let sturdy =
            |bx: i32, by: i32, bz: i32| facts.face_is_sturdy(canvas.block_at(bx, by, bz), 1);
        let air = |bx: i32, by: i32, bz: i32| facts.is_air(canvas.block_at(bx, by, bz));
        let centre = |bx: i32, by: i32, bz: i32, d: usize| {
            facts.can_support_center(canvas.block_at(bx, by, bz), d)
        };
        // `Direction.UP` is index 1 in `Direction.values()`.
        let collision_up = |bx: i32, by: i32, bz: i32| {
            facts.face_is_collision_full(canvas.block_at(bx, by, bz), 1)
        };
        let fluid = |bx: i32, by: i32, bz: i32| {
            facts
                .fluid(canvas.block_at(bx, by, bz))
                .unwrap_or_else(|| "minecraft:empty".to_string())
        };
        // Not a constant folded into the rule: the rule belongs to the
        // block and this value belongs to the *stage*, and a server placing a
        // mushroom at runtime must answer with real light instead.
        let level = self.raw_brightness;
        let brightness = move |_: i32, _: i32, _: i32| Some(level);
        let solid_render =
            |bx: i32, by: i32, bz: i32| facts.is_solid_render(canvas.block_at(bx, by, bz));
        let view = SurvivalView {
            block_at: &read,
            face_is_sturdy_up: &sturdy,
            air_at: &air,
            can_support_center: &centre,
            collision_face_full_up: &collision_up,
            fluid_at: &fluid,
            raw_brightness: &brightness,
            is_solid_render: &solid_render,
        };
        self.survival.would_survive(&view, state, (x, y, z))
    }

    fn face_is_sturdy(&mut self, x: i32, y: i32, z: i32, direction: usize) -> Option<bool> {
        let state = self.canvas.block_at(x, y, z);
        self.facts.face_is_sturdy(state, direction)
    }

    fn can_support_center(&mut self, x: i32, y: i32, z: i32, direction: usize) -> Option<bool> {
        let state = self.canvas.block_at(x, y, z);
        self.facts.can_support_center(state, direction)
    }

    fn face_is_collision_full(&mut self, x: i32, y: i32, z: i32, direction: usize) -> Option<bool> {
        let state = self.canvas.block_at(x, y, z);
        self.facts.face_is_collision_full(state, direction)
    }

    fn replaceable_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        let state = self.canvas.block_at(x, y, z);
        self.facts.is_replaceable(state)
    }

    fn air_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        let state = self.canvas.block_at(x, y, z);
        self.facts.is_air(state)
    }

    fn inside_build_height(&self, y: i32) -> Option<bool> {
        Some(y >= self.canvas.min_y() && y <= self.canvas.max_y())
    }

    fn is_solid_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        let state = self.canvas.block_at(x, y, z);
        self.facts.is_solid(state)
    }

    fn solid_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        let state = self.canvas.block_at(x, y, z);
        self.facts.is_solid_render(state)
    }
}

impl PlacementWorld for Decoration<'_> {
    fn block_tags(&self) -> &BlockTags {
        self.tags
    }

    fn biome_info_noise(&mut self, x: f64, z: f64) -> f64 {
        self.biomes.info_noise(x, z)
    }

    fn min_y(&self) -> i32 {
        self.canvas.min_y()
    }

    fn gen_depth(&self) -> i32 {
        self.gen_depth
    }

    fn heightmap(&mut self, kind: HeightmapKind, x: i32, z: i32) -> i32 {
        match kind {
            // The two fossils, from the noise stage.
            HeightmapKind::OceanFloorWg => self.canvas.ocean_floor_wg(x, z),
            HeightmapKind::WorldSurfaceWg => self.canvas.world_surface_wg(x, z),
            // The live ones, which rise as features write.
            HeightmapKind::OceanFloor => self.canvas.height_of(LiveHeightmap::OceanFloor, x, z),
            HeightmapKind::WorldSurface => self.canvas.height_of(LiveHeightmap::WorldSurface, x, z),
            HeightmapKind::MotionBlocking => {
                self.canvas.height_of(LiveHeightmap::MotionBlocking, x, z)
            }
            HeightmapKind::MotionBlockingNoLeaves => {
                self.canvas
                    .height_of(LiveHeightmap::MotionBlockingNoLeaves, x, z)
            }
        }
    }

    fn set_running_feature(&mut self, name: &str) {
        self.running = name.to_string();
    }

    fn biome_lists_this_feature(&mut self, x: i32, y: i32, z: i32) -> bool {
        let biome = self.biomes.biome_at(x, y, z);
        let running = self.running.clone();
        self.biomes.biome_lists(&biome, &running)
    }
}

// ---------------------------------------------------------------------------
// The fourteen feature worlds, every one of them over the same primitives.
//
// They are separate traits because each feature asks a *differently named*
// question — `ore.can_write` and the disk's `mark_above_for_post_processing`
// are not the same idea — and collapsing them into one would lose the record
// of which feature needs what. Here they all reduce to the canvas plus
// `BlockFacts`, which is the point: fourteen vocabularies, five primitives.
// ---------------------------------------------------------------------------

impl crate::feature::ore::OreWorld for Decoration<'_> {
    fn ocean_floor(&mut self, x: i32, z: i32) -> i32 {
        // `OCEAN_FLOOR_WG`, the fossil. The ore chains read the pre-feature
        // ground, not the canopy.
        self.canvas.ocean_floor_wg(x, z)
    }

    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
        self.canvas.block_at(x, y, z).to_string()
    }

    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
        self.write(x, y, z, name);
    }

    fn outside_build_height(&self, y: i32) -> bool {
        y < self.canvas.min_y() || y > self.canvas.max_y()
    }

    fn can_write(&self, x: i32, y: i32, z: i32) -> bool {
        let (dx, dz) = ((x >> 4) - self.center.0, (z >> 4) - self.center.1);
        dx.abs() <= self.write_radius
            && dz.abs() <= self.write_radius
            && y >= self.canvas.min_y()
            && y <= self.canvas.max_y()
    }
}

impl crate::feature::disk::DiskWorld for Decoration<'_> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
        self.write(x, y, z, name);
    }

    /// `Feature#markAboveForPostProcessing`, which is the whole walk and not
    /// a single mark: two positions **upward** from the block just placed,
    /// returning at the first air, marking each.
    ///
    /// The position handed in is never itself marked. Over a sea-floor disk
    /// the two above it are water, which is why this one method is the
    /// largest single block of the marks vanilla makes and we did not — see
    /// `docs/ORACLE.md` §23. It had a no-op default on the trait and the
    /// production world never overrode it, so every disk and fallen-tree mark
    /// was being dropped silently.
    fn mark_above_for_post_processing(&mut self, x: i32, y: i32, z: i32) {
        for step in 1..=2 {
            let above = y + step;
            let state = self.canvas.block_at(x, above, z);
            // `BlockState#isAir` — the three air blocks, not a name check.
            if state == "minecraft:air"
                || state == "minecraft:cave_air"
                || state == "minecraft:void_air"
            {
                return;
            }
            self.canvas.mark_post_processing(x, above, z);
        }
    }
}

impl crate::feature::underwater_magma::MagmaWorld for Decoration<'_> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
        self.write(x, y, z, name);
    }

    fn face_is_full(&mut self, x: i32, y: i32, z: i32, direction: usize) -> Option<bool> {
        let state = self.canvas.block_at(x, y, z);
        self.facts.face_is_full(state, direction)
    }

    fn is_air(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        let state = self.canvas.block_at(x, y, z);
        self.facts.is_air(state)
    }
}

impl crate::feature::vegetation_patch::PatchWorld for Decoration<'_> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        self.write(x, y, z, state);
    }

    fn is_empty_block(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        self.facts.is_air(self.canvas.block_at(x, y, z))
    }

    fn face_is_sturdy(&mut self, x: i32, y: i32, z: i32, direction: usize) -> Option<bool> {
        let state = self.canvas.block_at(x, y, z);
        self.facts.face_is_sturdy(state, direction)
    }
}

impl crate::feature::simple_block::SimpleBlockWorld for Decoration<'_> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
        self.write(x, y, z, name);
    }

    fn level_random_next_boolean(&mut self) -> Option<bool> {
        use crate::rng::RandomSource;
        self.region_random.as_mut().map(RandomSource::next_bool)
    }

    fn is_empty_block(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        let state = self.canvas.block_at(x, y, z);
        self.facts.is_air(state)
    }
}

impl crate::feature::block_pile::BlockPileWorld for Decoration<'_> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
        self.write(x, y, z, name);
    }

    fn is_empty_block(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        let state = self.canvas.block_at(x, y, z);
        self.facts.is_air(state)
    }

    fn min_y(&self) -> i32 {
        self.canvas.min_y()
    }
}

impl crate::feature::spring::SpringWorld for Decoration<'_> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
        self.write(x, y, z, name);
    }

    fn is_empty_block(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        let state = self.canvas.block_at(x, y, z);
        self.facts.is_air(state)
    }
}

impl crate::feature::freeze_top_layer::FreezeWorld for Decoration<'_> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
        self.write(x, y, z, name);
    }

    fn motion_blocking_height(&mut self, x: i32, z: i32) -> i32 {
        self.canvas.height_of(LiveHeightmap::MotionBlocking, x, z)
    }

    fn cold_enough_to_snow(&mut self, x: i32, y: i32, z: i32) -> bool {
        let biome = self.biomes.biome_at(x, y, z);
        self.biomes.cold_enough_to_snow(&biome, x, y, z)
    }

    fn precipitation_is_snow(&mut self, x: i32, y: i32, z: i32) -> bool {
        let biome = self.biomes.biome_at(x, y, z);
        self.biomes.precipitation_is_snow(&biome, x, y, z)
    }

    /// **Source water only, and that is deliberate — do NOT widen this to the
    /// fluid tag.** This one feeds the freeze path, and vanilla's
    /// `Biome.shouldFreeze` is `getFluidState(pos).is(Fluids.WATER)`: fluid
    /// *identity* against the source fluid, not `is(FluidTags.WATER)`. A
    /// `flowing_water` state's type is `Fluids.FLOWING_WATER`, so vanilla
    /// answers false for it here and so must we.
    ///
    /// Its neighbours in this file are tag-based (`is_water_fluid`) because
    /// they implement `LevelReader#isWaterAt`, which is a different method.
    /// Two predicates that look identical and must not be: checked against
    /// the bytecode 2026-08-10 rather than unified on sight.
    fn is_water_at(&mut self, x: i32, y: i32, z: i32) -> bool {
        let state = self.canvas.block_at(x, y, z).to_string();
        self.facts.fluid(&state).as_deref() == Some("minecraft:water")
    }

    fn snow_can_survive(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        <Self as PredicateWorld>::would_survive(self, "minecraft:snow[layers=1]", x, y, z)
    }
}

impl crate::feature::block_column::ColumnWorld for Decoration<'_> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
        self.write(x, y, z, name);
    }
}

impl crate::feature::multiface_growth::MultifaceWorld for Decoration<'_> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        self.write(x, y, z, state);
    }

    /// The position itself, not the column above it — this is
    /// `ChunkAccess#markPosForPostProcessing` direct, where the disk and the
    /// lake go through `Feature#markAboveForPostProcessing`.
    fn mark_pos_for_post_processing(&mut self, x: i32, y: i32, z: i32) {
        self.canvas.mark_post_processing(x, y, z);
    }
}

impl crate::feature::aquatic::AquaticWorld for Decoration<'_> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        self.write(x, y, z, state);
    }

    fn ocean_floor_height(&mut self, x: i32, z: i32) -> i32 {
        // `OCEAN_FLOOR`, the **live** one — seagrass and kelp grow out of
        // whatever the steps before them left, not out of the fossil.
        self.canvas.height_of(LiveHeightmap::OceanFloor, x, z)
    }
}

impl Decoration<'_> {
    /// Record an entity this pass added, in the chunk its `Pos` stands in.
    pub fn record_entity(&mut self, entity: rustmc_world::nbt::Nbt) {
        let block = |i: usize| match entity.get("Pos") {
            Some(rustmc_world::nbt::Nbt::List(p)) => match p.get(i) {
                #[allow(clippy::cast_possible_truncation)]
                Some(rustmc_world::nbt::Nbt::Double(v)) => v.floor() as i32,
                _ => 0,
            },
            _ => 0,
        };
        let (cx, cz) = (block(0) >> 4, block(2) >> 4);
        self.canvas.record_entity(cx, cz, entity);
    }

    /// `WorldGenRegion.getRandom()` — the level random a structure's
    /// `finalizeSpawn` draws from — when this world was given one.
    pub fn level_random(&mut self) -> Option<&mut crate::rng::AnyRandomSource> {
        self.region_random.as_mut()
    }
}

impl crate::block_entity::BlockEntityRecorder for Decoration<'_> {
    fn record_block_entity(&mut self, x: i32, y: i32, z: i32, record: crate::block_entity::Record) {
        // Straight through to the canvas, which decides whether anything is
        // recording. Unconditional here on purpose: see
        // `Canvas::record_block_entity`.
        self.canvas.record_block_entity(x, y, z, record);
    }
}

impl crate::feature::monster_room::MonsterRoomWorld for Decoration<'_> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        self.write(x, y, z, state);
    }
    fn can_replace(&mut self, x: i32, y: i32, z: i32) -> bool {
        // `Feature.isReplaceable(BlockTags.FEATURES_CANNOT_REPLACE)`.
        let state = self.canvas.block_at(x, y, z).to_string();
        let plain = state.split_once('[').map_or(state.as_str(), |(n, _)| n);
        !self
            .tags
            .contains("minecraft:features_cannot_replace", plain)
    }
    fn min_y(&self) -> i32 {
        self.canvas.min_y()
    }
}

impl crate::feature::speleothem::SpeleothemWorld for Decoration<'_> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        self.write(x, y, z, state);
    }
    fn is_water_at(&mut self, x: i32, y: i32, z: i32) -> bool {
        // `LevelReader#isWaterAt` is `getFluidState(pos).is(FluidTags.WATER)`,
        // and that tag holds `flowing_water` too — see `is_water_fluid`.
        self.facts
            .fluid(self.canvas.block_at(x, y, z))
            .as_deref()
            .is_some_and(crate::feature::speleothem::is_water_fluid)
    }
}

impl crate::feature::large_dripstone::LargeDripstoneWorld for Decoration<'_> {
    fn world_surface_wg(&mut self, x: i32, z: i32) -> i32 {
        // The same fossil heightmap `HeightmapKind::WorldSurfaceWg` reads —
        // written by the noise stage and never updated by decoration, which is
        // why a stalagmite is cut off at the *pre-decoration* surface rather
        // than at whatever the last feature left there.
        self.canvas.world_surface_wg(x, z)
    }
}

impl crate::feature::lake::LakeWorld for Decoration<'_> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        self.write(x, y, z, state);
    }

    fn min_y(&self) -> i32 {
        self.canvas.min_y()
    }

    fn is_liquid(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        // `BlockState#liquid()` — the block *is* a fluid. A waterlogged fence
        // has a fluid state and is not liquid, which is why this asks the
        // block rather than the fluid table.
        let state = self.canvas.block_at(x, y, z).to_string();
        let plain = state.split_once('[').map_or(state.as_str(), |(n, _)| n);
        Some(plain == "minecraft:water" || plain == "minecraft:lava")
    }

    /// `Biome#shouldFreeze(level, pos, mustBeAtEdge)`, which `LakeFeature`'s
    /// ice pass calls with `mustBeAtEdge = false`.
    ///
    /// # This was `cold_enough_to_snow` and nothing else
    ///
    /// Vanilla's predicate is five tests (26.2 bytecode, `Biome.shouldFreeze`,
    /// offsets 0..89), and only the first was here:
    ///
    /// ```text
    ///    0  warmEnoughToRain(pos, seaLevel)              -> false
    ///   16  !isInsideBuildHeight(pos.getY())             -> false
    ///   29  getBrightness(LightLayer.BLOCK, pos) >= 10   -> false
    ///   53  !getFluidState(pos).is(Fluids.WATER)         -> false
    ///   73  !(getBlockState(pos).getBlock()
    ///          instanceof LiquidBlock)                   -> false
    ///   84  mustBeAtEdge == false                        -> TRUE
    /// ```
    ///
    /// The two fluid tests are the ones that were load-bearing here.
    /// `LakeFeature`'s ice pass walks the **entire 16x16 footprint at a single
    /// y** — `origin.y + 4`, vanilla offsets 1246..1347 — and never consults
    /// the lake's own shape, so the water test is the only thing that keeps it
    /// inside the lake. Without it, a water lake in a cold biome laid a flat
    /// one-block sheet of `ice` right across its footprint, over whatever was
    /// there. Measured on seed 12345 r=16 it overwrote `deepslate`,
    /// `cave_air`, `sulfur`, `tuff` and `air` alike, in a plane at one y per
    /// lake — 131 blocks at y=-11 in chunk (60,67) alone.
    ///
    /// The lake that reaches this in an overworld cave is 26.2's
    /// `sulfur_pool`, whose fluid is water and whose barrier is
    /// `minecraft:sulfur`; the lava lakes are gated out one level up by
    /// `fluid_is_water`, which was already right.
    ///
    /// `Fluids.WATER` is the **source** fluid, so flowing water fails, and
    /// `instanceof LiquidBlock` excludes a waterlogged block — which has a
    /// water fluid state but is not a water block. That is why this asks for
    /// `minecraft:water` at `level=0` rather than for anything merely wet.
    ///
    /// The brightness test is not implemented for the reason
    /// `feature::freeze_top_layer` gives: no light has been computed while
    /// features run — `initializeLight` is a later chunk status — so it reads
    /// 0 on a real server too and the branch is always taken.
    fn should_freeze(&mut self, x: i32, y: i32, z: i32) -> bool {
        let biome = self.biomes.biome_at(x, y, z);
        if !self.biomes.cold_enough_to_snow(&biome, x, y, z) {
            return false;
        }
        if y < self.canvas.min_y() || y > self.canvas.max_y() {
            return false;
        }
        let state = self.canvas.block_at(x, y, z).to_string();
        let plain = state.split_once('[').map_or(state.as_str(), |(n, _)| n);
        if plain != "minecraft:water" || state.contains("level=") && !state.contains("level=0") {
            return false;
        }
        true
    }

    /// One walk, one implementation — `Feature#markAboveForPostProcessing` is
    /// a method on `Feature` and every caller gets the same one.
    fn mark_above_for_post_processing(&mut self, x: i32, y: i32, z: i32) {
        <Self as crate::feature::disk::DiskWorld>::mark_above_for_post_processing(self, x, y, z);
    }
}

impl crate::feature::blue_ice::BlueIceWorld for Decoration<'_> {
    fn sea_level(&mut self) -> i32 {
        self.sea_level
    }
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        self.write(x, y, z, state);
    }
}

impl crate::feature::iceberg::IcebergWorld for Decoration<'_> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        self.write(x, y, z, state);
    }
    /// `ChunkGenerator#getSeaLevel`, which is the same 63 the level reports —
    /// vanilla reads it off a different object here and the value is one.
    fn sea_level(&mut self) -> i32 {
        self.sea_level
    }
}

impl crate::feature::huge_mushroom::HugeMushroomWorld for Decoration<'_> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        self.write(x, y, z, state);
    }

    fn min_y(&self) -> i32 {
        self.canvas.min_y()
    }

    /// **Inclusive**, and the canvas's — the same reading the nether feature
    /// takes, for the same reason: `isValidPosition` asks a *level* question
    /// about how much headroom the mushroom has.
    fn max_y(&self) -> i32 {
        self.canvas.max_y()
    }
}

impl crate::feature::nether::NetherWorld for Decoration<'_> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        self.write(x, y, z, state);
    }

    fn min_y(&self) -> i32 {
        self.canvas.min_y()
    }

    /// **Inclusive**, and the canvas's rather than the generator's: the
    /// nether generates 128 blocks of noise into 256 blocks of storage, and
    /// `isOutsideBuildHeight` is a *level* question. A basalt pillar that
    /// used the generator's 127 would stop dead at the noise ceiling.
    fn max_y(&self) -> i32 {
        self.canvas.max_y()
    }

    fn sea_level(&mut self) -> i32 {
        self.sea_level
    }
}

impl crate::feature::end::EndWorld for Decoration<'_> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        self.write(x, y, z, state);
    }

    /// The **canvas's** min y, not the generator's. `placeSpike`'s box starts
    /// at `level.getMinY()`, which is a level question, and in the End the two
    /// happen to agree at 0 — so this is the right answer for the wrong reason
    /// unless it is written down. The nether's `max_y` note above is the same
    /// distinction, and there they differ by 128.
    fn min_y(&self) -> i32 {
        self.canvas.min_y()
    }

    fn world_seed(&mut self) -> i64 {
        self.world_seed
    }

    fn add_fresh_entity(&mut self, entity: rustmc_world::nbt::Nbt) {
        Decoration::record_entity(self, entity);
    }
}

impl crate::feature::vines::VineWorld for Decoration<'_> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        self.write(x, y, z, state);
    }
}

impl crate::feature::tree::TreeWorld for Decoration<'_> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        self.write(x, y, z, state);
    }

    fn min_y(&self) -> i32 {
        self.canvas.min_y()
    }

    fn max_y(&self) -> i32 {
        self.canvas.max_y()
    }

    fn motion_blocking_no_leaves(&mut self, x: i32, z: i32) -> i32 {
        self.canvas
            .height_of(LiveHeightmap::MotionBlockingNoLeaves, x, z)
    }
}

/// Structures write through the same world features do.
///
/// The write mask is deliberately **not** applied here. A feature may only
/// reach one chunk out because vanilla's feature region is a 3x3; a
/// structure is written once per chunk it overlaps, with the chunk itself as
/// the clip, so the clip already bounds it and a second mask would erase the
/// parts of a mineshaft that belong to the neighbours.
impl crate::structure::writer::StructureWorld for Decoration<'_> {
    fn level_random(&mut self) -> Option<&mut crate::rng::AnyRandomSource> {
        self.region_random.as_mut()
    }
    fn add_fresh_entity(&mut self, entity: rustmc_world::nbt::Nbt) {
        Decoration::record_entity(self, entity);
    }

    fn mark_post_processing(&mut self, x: i32, y: i32, z: i32) {
        self.canvas.mark_post_processing(x, y, z);
    }

    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
        self.canvas.block_at(x, y, z).to_string()
    }

    /// A structure's write, with the live heightmaps raised behind it.
    ///
    /// `ProtoChunk.setBlockState` updates every map in
    /// `getPersistedStatus().heightmapsAfter()`, and during FEATURES the
    /// persisted status is still CARVERS, whose `heightmapsAfter` is
    /// `FINAL_HEIGHTMAPS` — all four live maps. A structure written in a
    /// decoration step is no different from a feature in that respect.
    ///
    /// This used to write without updating them, which was invisible while the
    /// only structures were a mineshaft and an ocean ruin: one is underground
    /// and the other underwater, so neither moves a surface height that a
    /// later step reads. A village does — its roofs are the terrain as far as
    /// `vegetal_decoration` is concerned — and without this the trees of steps
    /// 9 and 10 grow through the houses.
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        if self.canvas.set_block(x, y, z, state) {
            let facts = self.facts;
            self.canvas
                .update_heightmaps(x, y, z, &FactsAsPredicates(facts));
        }
    }

    fn ocean_floor_wg(&mut self, x: i32, z: i32) -> i32 {
        self.canvas.ocean_floor_wg(x, z)
    }

    fn biome_blocks_mineshafts(&mut self, x: i32, y: i32, z: i32) -> bool {
        let biome = self.biomes.biome_at(x, y, z);
        // `#minecraft:mineshaft_blocking` — deep dark only, in 26.2.
        biome == "minecraft:deep_dark"
    }

    fn is_liquid(&mut self, x: i32, y: i32, z: i32) -> bool {
        let fluid = <Self as PredicateWorld>::fluid_at(self, x, y, z);
        fluid != "minecraft:empty"
    }

    fn is_sturdy_face(&mut self, x: i32, y: i32, z: i32, dx: i32, dy: i32, dz: i32) -> bool {
        // `Direction.values()` order, which is what the props table indexes.
        let direction = match (dx, dy, dz) {
            (0, -1, 0) => 0,
            (0, 1, 0) => 1,
            (0, 0, -1) => 2,
            (0, 0, 1) => 3,
            (-1, 0, 0) => 4,
            _ => 5,
        };
        <Self as PredicateWorld>::face_is_sturdy(self, x, y, z, direction).unwrap_or(false)
    }

    fn motion_blocking_no_leaves_height(&mut self, x: i32, z: i32) -> Option<i32> {
        Some(
            self.canvas
                .height_of(LiveHeightmap::MotionBlockingNoLeaves, x, z),
        )
    }

    fn min_y(&self) -> i32 {
        self.canvas.min_y()
    }

    fn level_random_next_bool(&mut self) -> Option<bool> {
        use crate::rng::RandomSource;
        self.region_random.as_mut().map(RandomSource::next_bool)
    }

    fn level_random_next_f32(&mut self) -> Option<f32> {
        use crate::rng::RandomSource;
        self.region_random.as_mut().map(RandomSource::next_f32)
    }

    fn state_is_solid_render(&self, state: &str) -> bool {
        // A state the table does not carry answers "not solid", which costs
        // a draw rather than spending one — the same direction every other
        // unknown in this file falls, so a missing entry shows up as a
        // missing rail and not as a stream that silently ran on.
        self.facts.is_solid_render(state).unwrap_or(false)
    }

    fn can_hang_chain_below(&mut self, x: i32, y: i32, z: i32) -> bool {
        // `Block.canSupportCenter(level, pos, DOWN) && !(block instanceof
        // FallingBlock)`.
        //
        // `canSupportCenter` is `SupportType.CENTER`, **not** the FULL form
        // `face_is_sturdy` answers, and the difference is the whole point
        // here: a fence's sturdy-face bits are `000000` and its centre bits
        // are `111111`. Answering this with FULL refuses every chain, and a
        // mineshaft that cannot hang a chain fills a pillar of logs downward
        // instead — 117 blocks of it in one r7 sample.
        let name = self.canvas.block_at(x, y, z).to_string();
        let plain = name.split_once('[').map_or(name.as_str(), |(n, _)| n);
        // `instanceof FallingBlock`, which no column of the table spells;
        // the tag `#unstable_bottom_center` is a different set and is
        // handled inside `can_support_center`.
        if matches!(
            plain,
            "minecraft:sand"
                | "minecraft:red_sand"
                | "minecraft:gravel"
                | "minecraft:suspicious_sand"
                | "minecraft:suspicious_gravel"
                | "minecraft:anvil"
                | "minecraft:chipped_anvil"
                | "minecraft:damaged_anvil"
                | "minecraft:dragon_egg"
                | "minecraft:pointed_dripstone"
                | "minecraft:white_concrete_powder"
        ) {
            return false;
        }
        <Self as PredicateWorld>::can_support_center(self, x, y, z, 0).unwrap_or(false)
    }
}

impl crate::feature::geode::GeodeWorld for Decoration<'_> {
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
        self.canvas.block_at(x, y, z).to_string()
    }
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        self.write(x, y, z, state);
    }
    fn is_air(&mut self, x: i32, y: i32, z: i32) -> bool {
        <Self as PredicateWorld>::air_at(self, x, y, z).unwrap_or(false)
    }
    fn is_fluid_source(&mut self, x: i32, y: i32, z: i32) -> bool {
        // `FluidState#isSource` — a full block of fluid, not flowing. The
        // canvas spells a source as `level=0`.
        let state = self.canvas.block_at(x, y, z).to_string();
        state == "minecraft:water[level=0]" || state == "minecraft:lava[level=0]"
    }
    fn has_fluid(&mut self, x: i32, y: i32, z: i32) -> bool {
        <Self as PredicateWorld>::fluid_at(self, x, y, z) != "minecraft:empty"
    }
    fn world_seed(&mut self) -> i64 {
        self.world_seed
    }
}

/// `BubbleColumnBlock.updateColumn`, applied as a sweep after decoration.
///
/// **The mechanism is not vanilla's.** A real server marks the magma block
/// for post-processing and the column forms when `updateShape` runs on the
/// finished chunk — a scheduled-tick subsystem this port does not have. What
/// is reproduced here is that pass's *outcome*: from each source block, walk
/// up through water and replace it.
///
/// Named rather than presented as the real thing, because it is the only
/// place in this generator where a result is reached by a different route.
/// It is a sweep, so it cannot depend on tick order — and bubble columns do
/// not: a column is the contiguous water above one source block, and two
/// sources cannot share one.
///
/// `enables_bubble_column_drag_down` is magma (drag true), and
/// `enables_bubble_column_push_up` is soul sand (drag false).
pub fn form_bubble_columns(canvas: &mut Canvas, area: crate::pipeline::Area) {
    let min_y = canvas.min_y();
    let max_y = min_y + canvas.height() - 1;

    // **On the pool, one chunk at a time.** This pass is the most chunk-local
    // thing in the generator and it was the least parallel: measured
    // 2026-08-16 at seed 20260722, r=32, it walked 4,761 chunks x 256 columns
    // x 384 blocks — 468 million reads — in 13.38 s at 1.00 cores, 18.6% of
    // the whole pipeline, hidden inside the `decoration` row because it runs
    // after the last stage mark.
    //
    // # Why splitting it cannot move a block
    //
    // Every read and every write is at the *same* `(x, z)` as its source, so
    // a column never touches another chunk. That is the property
    // `Canvas::chunk_writers` turns into disjoint `&mut` borrows the compiler
    // checks, and it is the same argument the noise stage rests on.
    //
    // Two orderings still matter and both are preserved. The scan collects
    // writes and applies them **after** the whole chunk has been read, so a
    // bubble column written by one source cannot be seen by a later source's
    // `canOccupy` walk — the deferred-write shape is kept exactly, per chunk,
    // and since no column crosses a chunk boundary the per-chunk deferral is
    // the whole-canvas deferral. And a chunk's palette is interned in write
    // order, so the loop keeps `local_z`, then `local_x`, then ascending `y`:
    // the writes reaching any one chunk arrive in the order they always did.
    let writers: Vec<crate::canvas::ChunkWriter<'_>> = canvas
        .chunk_writers()
        .into_iter()
        .filter(|w| {
            let (cx, cz) = w.position();
            area.contains(cx, cz)
        })
        .collect();
    let threads = crate::pipeline::worker_threads().min(writers.len().max(1));
    let queue = std::sync::Mutex::new(writers.into_iter());
    let run = |queue: &std::sync::Mutex<std::vec::IntoIter<crate::canvas::ChunkWriter<'_>>>| {
        let mut writes: Vec<(i32, i32, i32, &'static str)> = Vec::new();
        loop {
            let next = queue.lock().expect("the bubble-column queue lock").next();
            let Some(mut writer) = next else { break };
            let (chunk_x, chunk_z) = writer.position();
            writes.clear();
            for local_z in 0..16 {
                for local_x in 0..16 {
                    let (x, z) = (chunk_x * 16 + local_x, chunk_z * 16 + local_z);
                    for y in min_y..=max_y {
                        let source = writer.block_at(x, y, z);
                        let drag = match source {
                            "minecraft:magma_block" => "true",
                            "minecraft:soul_sand" => "false",
                            _ => continue,
                        };
                        let column = if drag == "true" {
                            "minecraft:bubble_column[drag=true]"
                        } else {
                            "minecraft:bubble_column[drag=false]"
                        };
                        // `canOccupy` — water, or more of the same column.
                        let mut above = y + 1;
                        while above <= max_y {
                            let here = writer.block_at(x, above, z);
                            if here != "minecraft:water[level=0]"
                                && !here.starts_with("minecraft:bubble_column")
                            {
                                break;
                            }
                            writes.push((x, above, z, column));
                            above += 1;
                        }
                    }
                }
            }
            for &(x, y, z, state) in &writes {
                writer.set_block(x, y, z, state);
            }
        }
    };
    if threads <= 1 {
        run(&queue);
    } else {
        std::thread::scope(|scope| {
            for _ in 0..threads {
                scope.spawn(|| run(&queue));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::configured::FeatureWorld;
    use crate::pipeline::{Area, Pipeline};
    use std::collections::BTreeMap;

    /// Enough facts to move: air is air, stone is solid, everything else is
    /// itself. The real answers come from the block-props extract.
    struct Facts;

    impl BlockFacts for Facts {
        fn is_air(&self, state: &str) -> Option<bool> {
            Some(state == "minecraft:air" || state == "minecraft:cave_air")
        }
        fn fluid(&self, state: &str) -> Option<String> {
            let plain = state.split_once('[').map_or(state, |(n, _)| n);
            Some(match plain {
                "minecraft:water" => "minecraft:water".to_string(),
                "minecraft:lava" => "minecraft:lava".to_string(),
                _ => "minecraft:empty".to_string(),
            })
        }
        fn is_solid_render(&self, state: &str) -> Option<bool> {
            self.is_air(state).map(|air| !air)
        }
        fn is_solid(&self, state: &str) -> Option<bool> {
            self.is_air(state).map(|air| !air)
        }
        fn is_replaceable(&self, state: &str) -> Option<bool> {
            self.is_air(state)
        }
        fn can_support_center(&self, state: &str, direction: usize) -> Option<bool> {
            self.face_is_sturdy(state, direction)
        }

        fn face_is_sturdy(&self, state: &str, _: usize) -> Option<bool> {
            self.is_air(state).map(|air| !air)
        }
        fn face_is_full(&self, state: &str, _: usize) -> Option<bool> {
            self.is_air(state).map(|air| !air)
        }
        fn face_is_collision_full(&self, state: &str, _: usize) -> Option<bool> {
            self.is_air(state).map(|air| !air)
        }
        fn matches_heightmap(&self, state: &str, _: LiveHeightmap) -> bool {
            state != "minecraft:air"
        }
    }

    struct NoSurvival;

    impl SurvivalRules for NoSurvival {
        fn would_survive(&self, _: &SurvivalView<'_>, _: &str, _: (i32, i32, i32)) -> Option<bool> {
            None
        }
    }

    struct OneBiome;

    impl BiomeFacts for OneBiome {
        fn biome_at(&mut self, _: i32, _: i32, _: i32) -> String {
            "minecraft:plains".to_string()
        }
        fn biome_lists(&mut self, _: &str, _: &str) -> bool {
            true
        }
        fn info_noise(&mut self, _: f64, _: f64) -> f64 {
            0.0
        }
        fn cold_enough_to_snow(&mut self, _: &str, _: i32, _: i32, _: i32) -> bool {
            false
        }
        fn precipitation_is_snow(&mut self, _: &str, _: i32, _: i32, _: i32) -> bool {
            false
        }
    }

    fn tags() -> BlockTags {
        BlockTags::from_entries(BTreeMap::new()).expect("tags")
    }

    /// The level random is the factory, the name and the position — and each
    /// of the three is pinned here because each is a choice read off the jar.
    ///
    /// The *primitives* are already golden: `golden_rng` replays
    /// `forkPositional.at(x,y,z)` and `forkPositional.fromHashOf(name)`
    /// against JVM vectors, and `AnyPositionalFactory::fork_from_hash_of` is
    /// the same call the surface system's gradient factories go through. What
    /// this branch added is which name, which position, and how long the
    /// stream lives — none of which a stream test can see.
    ///
    /// So the assertion is a **mutation** one. Each of the three wrong
    /// answers a reader might reasonably write instead is constructed here and
    /// required to differ:
    ///
    /// * the bare path `worldgen_region_random`, without the namespace —
    ///   `fromHashOf(Identifier)` is `fromHashOf(id.toString())`;
    /// * `y` at the chunk's minimum instead of the literal 0 that
    ///   `ChunkPos.getWorldPosition()` uses;
    /// * one stream for the whole run instead of one per chunk.
    #[test]
    fn the_level_random_is_named_positioned_and_scoped_the_way_the_jar_says() {
        use crate::feature::simple_block::SimpleBlockWorld;
        use crate::rng::{AnyPositionalFactory, PositionalRandomFactory, RandomSource};

        let seed = 20260722i64;
        let (cx, cz) = (-1062i32, -671i32);
        let take = |source: &mut crate::rng::AnyRandomSource| {
            (0..12).map(|_| source.next_bool()).collect::<Vec<bool>>()
        };

        let mut pipeline = Pipeline::new(Area::square(cx, cz, 0), -64, 384, "minecraft:air");
        let facts = Facts;
        let survival = NoSurvival;
        let mut biomes = OneBiome;
        let tags = tags();
        let mut world = Decoration::new(
            pipeline.canvas_mut(),
            &facts,
            &survival,
            &mut biomes,
            &tags,
            63,
            384,
            seed,
        )
        .with_level_random(seed, false);
        world.begin_chunk(cx, cz);
        let got: Vec<bool> = (0..12)
            .map(|_| {
                SimpleBlockWorld::level_random_next_boolean(&mut world)
                    .expect("the world was given a level random")
            })
            .collect();

        let root = AnyPositionalFactory::for_world(seed, false);
        let want = take(
            &mut root
                .fork_from_hash_of("minecraft:worldgen_region_random")
                .at(cx * 16, 0, cz * 16),
        );
        assert_eq!(got, want, "the level random is not the documented stream");

        // The namespace is load-bearing.
        assert_ne!(
            want,
            take(
                &mut root
                    .fork_from_hash_of("worldgen_region_random")
                    .at(cx * 16, 0, cz * 16)
            ),
            "the bare path must not produce the same stream as the full Identifier"
        );
        // The y is a literal zero, not the chunk's minimum.
        assert_ne!(
            want,
            take(
                &mut root
                    .fork_from_hash_of("minecraft:worldgen_region_random")
                    .at(cx * 16, -64, cz * 16)
            ),
            "y = 0 must not agree with y = minY, or the position is untested"
        );
        // And it is per chunk: the neighbour is a different stream, and
        // re-entering this chunk restarts it rather than continuing.
        let neighbour = take(
            &mut root
                .fork_from_hash_of("minecraft:worldgen_region_random")
                .at((cx + 1) * 16, 0, cz * 16),
        );
        assert_ne!(want, neighbour, "each chunk has its own stream");
        world.begin_chunk(cx, cz);
        let again: Vec<bool> = (0..12)
            .map(|_| SimpleBlockWorld::level_random_next_boolean(&mut world).expect("still armed"))
            .collect();
        assert_eq!(
            again, want,
            "`begin_chunk` must take a fresh stream, as a new WorldGenRegion does"
        );
    }

    /// A world that was never given a level random refuses, and the refusal is
    /// what a caller must turn into an `Unplaceable`.
    #[test]
    fn a_world_without_a_level_random_answers_none() {
        use crate::feature::simple_block::SimpleBlockWorld;

        let mut pipeline = Pipeline::new(Area::square(0, 0, 0), -64, 384, "minecraft:air");
        let facts = Facts;
        let survival = NoSurvival;
        let mut biomes = OneBiome;
        let tags = tags();
        let mut world = Decoration::new(
            pipeline.canvas_mut(),
            &facts,
            &survival,
            &mut biomes,
            &tags,
            63,
            384,
            0,
        );
        world.begin_chunk(0, 0);
        assert_eq!(
            SimpleBlockWorld::level_random_next_boolean(&mut world),
            None,
            "a world with no stream must answer None, never a branch"
        );
    }

    /// The whole point: one type satisfies every feature's world.
    #[test]
    fn a_decoration_world_is_a_feature_world() {
        fn assert_feature_world<W: FeatureWorld>() {}
        assert_feature_world::<Decoration<'_>>();
    }

    /// A write raises the live heightmaps, which is what the next feature in
    /// the step reads.
    #[test]
    fn a_write_raises_the_live_heightmaps() {
        let mut pipeline = Pipeline::new(Area::square(0, 0, 0), -64, 384, "minecraft:air");
        let facts = Facts;
        let survival = NoSurvival;
        let mut biomes = OneBiome;
        let tags = tags();
        let mut world = Decoration::new(
            pipeline.canvas_mut(),
            &facts,
            &survival,
            &mut biomes,
            &tags,
            63,
            // genDepth == the canvas height in these overworld-shaped tests.
            384,
            0,
        );
        world.begin_chunk(0, 0);
        crate::feature::ore::OreWorld::set_block(&mut world, 3, 70, 3, "minecraft:stone");
        assert_eq!(
            world.heightmap(HeightmapKind::MotionBlocking, 3, 3),
            71,
            "one past the block just written"
        );
    }

    /// A feature reaches one chunk out and no further. Vanilla's region
    /// simply has no chunk there, so the write is dropped — and a canvas with
    /// a wider margin would happily take it.
    #[test]
    fn a_write_two_chunks_out_is_dropped() {
        let mut pipeline = Pipeline::new(Area::square(0, 0, 2), -64, 384, "minecraft:air");
        let facts = Facts;
        let survival = NoSurvival;
        let mut biomes = OneBiome;
        let tags = tags();
        {
            let mut world = Decoration::new(
                pipeline.canvas_mut(),
                &facts,
                &survival,
                &mut biomes,
                &tags,
                63,
                384,
                0,
            );
            world.begin_chunk(0, 0);
            // One chunk out: kept.
            crate::feature::ore::OreWorld::set_block(&mut world, 20, 70, 3, "minecraft:stone");
            // Two chunks out: dropped.
            crate::feature::ore::OreWorld::set_block(&mut world, 40, 70, 3, "minecraft:stone");
        }
        assert_eq!(pipeline.canvas().block_at(20, 70, 3), "minecraft:stone");
        assert_eq!(pipeline.canvas().block_at(40, 70, 3), "minecraft:air");
    }

    /// The ore chains read the fossil and the aquatic features read the live
    /// map, and this is where the two are told apart.
    #[test]
    fn the_ore_floor_is_the_fossil_and_the_kelp_floor_is_not() {
        let mut pipeline = Pipeline::new(Area::square(0, 0, 0), -64, 384, "minecraft:air");
        pipeline
            .canvas_mut()
            .set_wg_heightmaps(0, 0, &[64; 256], &[62; 256]);
        let facts = Facts;
        let survival = NoSurvival;
        let mut biomes = OneBiome;
        let tags = tags();
        let mut world = Decoration::new(
            pipeline.canvas_mut(),
            &facts,
            &survival,
            &mut biomes,
            &tags,
            63,
            // genDepth == the canvas height in these overworld-shaped tests.
            384,
            0,
        );
        world.begin_chunk(0, 0);
        // A feature raises the live map well above the fossil.
        crate::feature::ore::OreWorld::set_block(&mut world, 0, 100, 0, "minecraft:stone");

        assert_eq!(
            crate::feature::ore::OreWorld::ocean_floor(&mut world, 0, 0),
            62,
            "the ore chain still sees the pre-feature ground"
        );
        assert_eq!(
            crate::feature::aquatic::AquaticWorld::ocean_floor_height(&mut world, 0, 0),
            101,
            "while kelp grows out of what the features left"
        );
    }
}
