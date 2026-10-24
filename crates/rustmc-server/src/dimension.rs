//! Dimension travel — the portal timer, the coordinate scale and the entry
//! position, transcribed from 26.2 bytecode.
//!
//! # The timer is not a countdown and its decay is not one per tick
//!
//! `PortalProcessor` holds `portalTime` and a `insidePortalThisTick` flag that
//! the block sets every tick the entity is in it.
//! `processPortalTeleportation` reads and clears the flag: if it was set, the
//! counter goes **up** by one and the entity teleports when the value *before*
//! the increment reached the transition time; if it was not set, `decayTick`
//! takes four off, floored at zero. So stepping out of a portal loses progress
//! four times faster than standing in it gains it, and one tick outside undoes
//! four inside.
//!
//! The post-increment matters: vanilla's `portalTime++ >= transitionTime`
//! compares the old value, so a transition time of `n` fires on the `n+1`th
//! tick inside. A pre-increment fires one tick early, which is invisible in
//! every test that only asks whether it fires at all.
//!
//! # Only players wait
//!
//! `NetherPortalBlock.getPortalTransitionTime` returns `0` for anything that
//! is not a `Player` (`iconst_0; ireturn` at offset 49) — a thrown item goes
//! through on its first tick. For a player it is
//! `max(0, gamerule)`, and **which** gamerule depends on
//! `abilities.invulnerable`, not on the game mode: creative and spectator are
//! both invulnerable and both take
//! `players_nether_portal_creative_delay` (default 0), everyone else takes
//! `players_nether_portal_default_delay` (default 80).
//!
//! # The scale applies to x and z and not to y
//!
//! `NetherPortalBlock.getPortalDestination` computes
//! `DimensionType.getTeleportationScale(from, to)`, which is
//! `from.coordinateScale / to.coordinateScale`, and calls
//! `worldBorder.clampToBounds(x * scale, y, z * scale)` — **y unscaled**. The
//! nether's `coordinate_scale` is 8.0 and the overworld's is 1.0, so
//! overworld-to-nether is 1/8 and nether-to-overworld is 8, and a
//! transcription that scales y sends everybody to the void or the ceiling.
//!
//! # The clamp's epsilon is a float
//!
//! `clampVec3ToBound` clamps x and z to `[minX, maxX - 1.0E-5f]`, and that
//! literal is a **float widened to a double**: `9.999999747378752E-6`, not
//! `1.0E-5`. It then floors. `y` is neither clamped nor bounded — only
//! floored. `world_border.json`'s `clamps` array is vanilla's own answers on a
//! 32-wide border, including a probe exactly on the upper edge, which is the
//! one the epsilon moves.

use rustmc_data::BlockPos;

use crate::worldborder::{clamp, WorldBorder};

/// `WorldBorder.clampVec3ToBound`'s upper-edge epsilon: `(double)(float)1.0E-5`.
pub const BORDER_CLAMP_EPSILON: f64 = 9.999_999_747_378_752E-6;

/// `NetherPortalBlock`'s two gamerules, and their vanilla defaults.
pub const CREATIVE_DELAY_RULE: (&str, i32) = ("players_nether_portal_creative_delay", 0);
/// See [`CREATIVE_DELAY_RULE`].
pub const DEFAULT_DELAY_RULE: (&str, i32) = ("players_nether_portal_default_delay", 80);

/// The three dimensions a vanilla server runs, and everything about each of
/// them that a served world or a `CommonPlayerSpawnInfo` needs.
///
/// # The registry index is not the order anybody would guess
///
/// `CommonPlayerSpawnInfo.write` opens with
/// `DimensionType.STREAM_CODEC.encode(buf, dimensionType)`, and that codec is
/// a **registry id** — a varint index into the `minecraft:dimension_type`
/// registry as the client received it in the configuration phase. That
/// registry has **four** entries, not three, and they are not in the order
/// the dimensions are usually listed:
///
/// ```text
///   0  minecraft:overworld
///   1  minecraft:overworld_caves      no level uses it; it is still numbered
///   2  minecraft:the_end
///   3  minecraft:the_nether
/// ```
///
/// So `the_nether` is **3** and `the_end` is **2**. Guessing 1 and 2 gives a
/// client that renders the nether with the End's sky and fog, which looks
/// like a client bug and is a one-varint server one.
/// [`tests::the_registry_indices_are_the_captures_own_order`] reads them out
/// of `data/vanilla/registries.bin` rather than trusting this comment.
///
/// # `sea_level` is the CHUNK GENERATOR's, not the dimension type's
///
/// `createCommonSpawnInfo` fills that field from
/// `ServerLevel.getSeaLevel()`, which is
/// `chunkSource.getGenerator().getSeaLevel()` — the `sea_level` of the
/// dimension's **noise settings**, a value that appears nowhere in the
/// `dimension_type` registry entry. Overworld 63, nether 32 (the lava sea),
/// End 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dimension {
    Overworld,
    Nether,
    TheEnd,
}

/// Every dimension, in `Registries.DIMENSION` order — which is the order the
/// login packet's `levels` array carries and is **not** the `dimension_type`
/// registry's order.
pub const DIMENSIONS: [Dimension; 3] = [Dimension::Overworld, Dimension::TheEnd, Dimension::Nether];

impl Dimension {
    /// `ResourceKey<Level>` — the string `writeResourceKey` puts on the wire.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Overworld => "minecraft:overworld",
            Self::Nether => "minecraft:the_nether",
            Self::TheEnd => "minecraft:the_end",
        }
    }

    /// Parse a `ResourceKey<Level>`. `None` for anything else, including
    /// `minecraft:overworld_caves`, which is a dimension *type* and not a
    /// level.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "minecraft:overworld" => Some(Self::Overworld),
            "minecraft:the_nether" => Some(Self::Nether),
            "minecraft:the_end" => Some(Self::TheEnd),
            _ => None,
        }
    }

    /// The index into `minecraft:dimension_type` the spawn info carries.
    ///
    /// See the type's own documentation: 0, **3**, **2**.
    #[must_use]
    pub const fn registry_index(self) -> i32 {
        match self {
            Self::Overworld => 0,
            Self::TheEnd => 2,
            Self::Nether => 3,
        }
    }

    /// `dimension_type`'s `min_y` — the bottom of what the dimension
    /// **stores**, which is the bottom of a chunk packet.
    #[must_use]
    pub const fn min_y(self) -> i32 {
        match self {
            Self::Overworld => -64,
            Self::Nether | Self::TheEnd => 0,
        }
    }

    /// `dimension_type`'s `height` — how tall a chunk packet is, in blocks.
    ///
    /// **Not the generator's height.** `nether.json` and `end.json` both
    /// generate 128 blocks and both dimensions store 256; a chunk packet
    /// carries one section per stored 16 blocks whatever the noise made, so
    /// this is the number the wire cares about and the other one is the
    /// generator's business. Conflating them is a 50% parity error and
    /// `tools/worldgen-assembly`'s `NETHER_STORAGE_HEIGHT` comment records
    /// what it cost there.
    #[must_use]
    pub const fn height(self) -> i32 {
        match self {
            Self::Overworld => 384,
            Self::Nether | Self::TheEnd => 256,
        }
    }

    /// `dimension_type`'s `logical_height` — where a chorus fruit or an ender
    /// pearl may land. The nether's is **128** against a stored 256, and it
    /// is the only one of the three where the two differ.
    #[must_use]
    pub const fn logical_height(self) -> i32 {
        match self {
            Self::Overworld => 384,
            Self::Nether => 128,
            Self::TheEnd => 256,
        }
    }

    /// How many 16-block sections a chunk packet in this dimension carries.
    ///
    /// The packet has no count of its own — a decoder reads exactly this many
    /// and a wrong number desyncs the whole buffer rather than truncating it,
    /// which is why [`crate::chunk::LevelChunkWithLight::decode`] takes it as
    /// an argument.
    #[must_use]
    pub const fn sections(self) -> usize {
        (self.height() / 16) as usize
    }

    /// `dimension_type`'s `coordinate_scale`. 8.0 in the nether and 1.0 in
    /// the other two — the number [`teleportation_scale`] divides.
    #[must_use]
    pub const fn coordinate_scale(self) -> f64 {
        match self {
            Self::Nether => 8.0,
            Self::Overworld | Self::TheEnd => 1.0,
        }
    }

    /// `dimension_type`'s `has_skylight`.
    ///
    /// **The End's is `true`**, which surprises people: the End has a sky
    /// light section in every chunk and gets its darkness from
    /// `sky_light_factor: 0.0` and an `ambient_light` of 0.25, not from
    /// having no skylight at all. Only the nether has none.
    #[must_use]
    pub const fn has_skylight(self) -> bool {
        !matches!(self, Self::Nether)
    }

    /// `dimension_type`'s `has_ceiling`. The nether's only.
    #[must_use]
    pub const fn has_ceiling(self) -> bool {
        matches!(self, Self::Nether)
    }

    /// `ServerLevel.getSeaLevel()` — the noise settings' `sea_level`, which is
    /// the field `CommonPlayerSpawnInfo` carries. See the type docs.
    #[must_use]
    pub const fn sea_level(self) -> i32 {
        match self {
            Self::Overworld => 63,
            Self::Nether => 32,
            Self::TheEnd => 0,
        }
    }

    /// The subdirectory of the world folder this dimension saves under.
    ///
    /// **There is no `DIM-1` and no `DIM1` in 26.2, and the overworld is not
    /// the world root.** `LevelStorageSource$LevelStorageAccess.getDimensionPath`
    /// is one call, `DimensionType.getStorageFolder(key, path)`, and that is
    /// `key.identifier().resolveAgainst(path.resolve("dimensions"))` —
    /// `Identifier.resolveAgainst` being `p.resolve(namespace, path)`. So
    /// every dimension, the overworld included, lands at
    /// `<world>/dimensions/<namespace>/<path>/`, with no special case for any
    /// of the three and no legacy folder name anywhere in the method.
    ///
    /// [`crate::tiles::Tiles::configured_region_dir`] was already reading a
    /// real vanilla world on disk and agreed
    /// (`world/dimensions/minecraft/overworld/region`); this is the same
    /// answer taken from the bytecode instead.
    #[must_use]
    pub const fn world_subdir(self) -> &'static str {
        match self {
            Self::Overworld => "dimensions/minecraft/overworld",
            Self::Nether => "dimensions/minecraft/the_nether",
            Self::TheEnd => "dimensions/minecraft/the_end",
        }
    }
}

/// `PortalForcer`'s search and frame constants.
pub mod forcer {
    /// The chunk-ticket radius around a portal being used.
    pub const TICKET_RADIUS: i32 = 3;
    /// How far a nether-side search looks for an existing portal.
    pub const NETHER_PORTAL_RADIUS: i32 = 16;
    /// And an overworld-side one — eight times as far, which is the same ratio
    /// as the coordinate scale and not a coincidence.
    pub const OVERWORLD_PORTAL_RADIUS: i32 = 128;
    pub const FRAME_HEIGHT: i32 = 5;
    pub const FRAME_WIDTH: i32 = 4;
    pub const FRAME_BOX: i32 = 3;
    pub const FRAME_HEIGHT_START: i32 = -1;
    pub const FRAME_HEIGHT_END: i32 = 4;
    pub const FRAME_WIDTH_START: i32 = -1;
    pub const FRAME_WIDTH_END: i32 = 3;
    pub const FRAME_BOX_START: i32 = -1;
    pub const FRAME_BOX_END: i32 = 2;

    use rustmc_data::BlockPos;

    /// `net.minecraft.core.BlockPos.spiralAround(centre, radius, first, second)`.
    ///
    /// The traversal `PortalForcer.createPortal` searches in, and the reason its
    /// answer is a *particular* portal rather than merely a valid one. Every
    /// candidate site is scored by `distSqr` to the clicked block and the smallest
    /// wins, but ties are broken by arrival order — so the order is load-bearing
    /// wherever two sites are equidistant, which around a symmetric centre is
    /// most of them.
    ///
    /// Transcribed from `BlockPos$6`, which is eight fields and one `computeNext`:
    ///
    /// ```text
    ///   directions = [first, second, first.opposite, second.opposite]
    ///   cursor     = centre.move(second)      // ONE STEP OFF CENTRE, at the start
    ///   legs       = 4 * radius
    ///   leg        = -1,  legSize = 0,  legIndex = 0
    ///
    ///   computeNext:
    ///     cursor.set(last).move(directions[(leg + 4) % 4])
    ///     last = cursor
    ///     if legIndex >= legSize:
    ///         if leg >= legs: end
    ///         leg += 1;  legIndex = 0;  legSize = leg / 2 + 1
    ///     legIndex += 1
    ///     return cursor
    /// ```
    ///
    /// **The first position yielded is the centre**, and that is not obvious from
    /// the code: the cursor is initialised one step along `second`, and the first
    /// `computeNext` moves along `directions[(-1 + 4) % 4]` = `second.opposite`,
    /// which undoes it. A transcription that starts the cursor at the centre
    /// yields the centre's *neighbour* first and every subsequent position is off
    /// by one leg.
    ///
    /// The `(leg + 4) % 4` is only ever exercised at `leg == -1`; Java's `%` on a
    /// negative would give `-1` and index out of bounds, which is what the `+ 4`
    /// is for.
    ///
    /// Y never changes: this is a plane walk in the two axes `first` and `second`
    /// span.
    #[must_use]
    pub fn spiral_around(
        centre: BlockPos,
        radius: i32,
        first: Step,
        second: Step,
    ) -> Vec<BlockPos> {
        assert!(
            first.axis() != second.axis(),
            "The two directions cannot be on the same axis"
        );
        let directions = [first, second, first.opposite(), second.opposite()];
        let mut cursor = second.applied(centre, 1);
        let legs = 4 * radius;
        let mut leg = -1i32;
        let mut leg_size = 0i32;
        let mut leg_index = 0i32;
        let mut last = cursor;
        let mut out = Vec::new();
        loop {
            cursor = directions[usize::try_from((leg + 4) % 4).expect("0..3")].applied(last, 1);
            last = cursor;
            if leg_index >= leg_size {
                if leg >= legs {
                    return out;
                }
                leg += 1;
                leg_index = 0;
                leg_size = leg / 2 + 1;
            }
            leg_index += 1;
            out.push(cursor);
        }
    }

    /// One of the four horizontal directions, as `PortalForcer` uses them.
    ///
    /// `Direction.get(POSITIVE, axis)` is EAST for the X axis and SOUTH for the Z
    /// axis, and `getClockWise` on those is SOUTH and WEST — the rotation
    /// `canHostFrame` steps sideways along.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Step {
        North,
        South,
        West,
        East,
    }

    impl Step {
        /// `Direction.get(AxisDirection.POSITIVE, axis)`.
        #[must_use]
        pub const fn positive(axis: crate::portal::Axis) -> Self {
            match axis {
                crate::portal::Axis::X => Self::East,
                crate::portal::Axis::Z => Self::South,
            }
        }

        #[must_use]
        pub const fn axis(self) -> crate::portal::Axis {
            match self {
                Self::West | Self::East => crate::portal::Axis::X,
                Self::North | Self::South => crate::portal::Axis::Z,
            }
        }

        #[must_use]
        pub const fn opposite(self) -> Self {
            match self {
                Self::North => Self::South,
                Self::South => Self::North,
                Self::West => Self::East,
                Self::East => Self::West,
            }
        }

        /// `Direction.getClockWise()` — the horizontal quarter turn.
        #[must_use]
        pub const fn clockwise(self) -> Self {
            match self {
                Self::North => Self::East,
                Self::East => Self::South,
                Self::South => Self::West,
                Self::West => Self::North,
            }
        }

        #[must_use]
        pub const fn step_x(self) -> i32 {
            match self {
                Self::West => -1,
                Self::East => 1,
                Self::North | Self::South => 0,
            }
        }

        #[must_use]
        pub const fn step_z(self) -> i32 {
            match self {
                Self::North => -1,
                Self::South => 1,
                Self::West | Self::East => 0,
            }
        }

        /// `pos.move(this, n)`.
        #[must_use]
        pub const fn applied(self, pos: BlockPos, n: i32) -> BlockPos {
            BlockPos {
                x: pos.x + self.step_x() * n,
                y: pos.y,
                z: pos.z + self.step_z() * n,
            }
        }
    }

    /// What `PortalForcer` asks of the world it is building a portal in.
    ///
    /// Four questions and one write, which is the whole of the level surface the
    /// method touches. Split out as a trait so the search order and the geometry
    /// — the part that is transcription — can be tested against a grid held in
    /// memory, with no `World`, no generation and no extract.
    pub trait PortalLevel {
        /// `BlockState.canBeReplaced() && state.getFluidState().isEmpty()` —
        /// `PortalForcer.canPortalReplaceBlock`, verbatim. Note the fluid clause:
        /// water and lava are `canBeReplaced` and are refused here, so a portal is
        /// never built in a lava lake.
        fn can_portal_replace(&self, pos: BlockPos) -> bool;
        /// `BlockState.isSolid()`, which is what the four rows below a frame have
        /// to be for it to stand.
        fn is_solid(&self, pos: BlockPos) -> bool;
        /// `ServerLevel.getHeight(Heightmap.Types.MOTION_BLOCKING, x, z)`.
        fn motion_blocking_height(&self, x: i32, z: i32) -> i32;
        /// `ServerLevel.getMinY()`.
        fn min_y(&self) -> i32;
        /// `ServerLevel.getMaxY()` — the last y that exists, not the count.
        fn max_y(&self) -> i32;
        /// `ServerLevel.getLogicalHeight()`.
        fn logical_height(&self) -> i32;
        /// `WorldBorder.isWithinBounds(pos)`.
        fn within_border(&self, pos: BlockPos) -> bool;
        /// `WorldBorder.clampToBounds(pos)`.
        fn clamp_to_border(&self, pos: BlockPos) -> BlockPos;
        /// One write. `flags` is vanilla's own: 3 for the obsidian frame
        /// (`setBlock(.., 3)`), 18 for the portal blocks (`setBlock(.., 18)`),
        /// and the forced platform uses `setBlockAndUpdate`, which is 3.
        fn set_block(&mut self, pos: BlockPos, block: &str, flags: i32);
    }

    /// `PortalForcer.canHostFrame(at, scratch, right, lateral)`.
    ///
    /// A 3x5 slab, `lateral` blocks sideways of `at` along `right.getClockWise()`:
    /// the four rows at `j < 0` must be **solid** and the fifteen at `j >= 0`
    /// must be replaceable. `createPortal` calls it three times per candidate —
    /// at 0, then at −1 and +1 — and a site that passes all three is preferred
    /// over one that passes only the middle.
    ///
    /// The iteration is `i` in `-1..3` outer, `j` in `-1..4` inner, and it
    /// **returns on the first failure**, so the order is observable to a level
    /// that counts its reads. That is not a hypothetical: it is the difference
    /// between one `getBlockState` and twenty on a candidate that fails at once.
    fn can_host_frame(level: &impl PortalLevel, at: BlockPos, right: Step, lateral: i32) -> bool {
        let cw = right.clockwise();
        for i in -1..3 {
            for j in -1..4 {
                let p = BlockPos {
                    x: at.x + right.step_x() * i + cw.step_x() * lateral,
                    y: at.y + j,
                    z: at.z + right.step_z() * i + cw.step_z() * lateral,
                };
                if j < 0 && !level.is_solid(p) {
                    return false;
                }
                if j >= 0 && !level.can_portal_replace(p) {
                    return false;
                }
            }
        }
        true
    }

    /// `BlockUtil$FoundRectangle` — the bottom-left corner of the portal that was
    /// built, and its 2x3 extent.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FoundRectangle {
        pub min: BlockPos,
        pub axis1_size: i32,
        pub axis2_size: i32,
    }

    /// `PortalForcer.createPortal(clicked, axis)` — build a portal near `clicked`,
    /// and say where.
    ///
    /// # It never fails to build one, except at the top of the world
    ///
    /// The only `Optional.empty()` in the method is the height check in the
    /// forcing branch: if `maxY - 9 < max(minY + 1, 70)` there is nowhere to put
    /// a 5-tall frame and it gives up. Everywhere else, a search that finds no
    /// site **carves** one — a 2x3x4 box of air over a 2x1x4 obsidian floor at
    /// `clamp(clicked.y, max(minY + 1, 70), maxY - 9)` — and then builds on it.
    /// So "no portal was created" is not a state a caller has to handle for any
    /// world taller than 79 blocks.
    ///
    /// # The search has a preferred answer and a fallback, and they are separate
    ///
    /// Two `(distance, position)` pairs are kept:
    ///
    /// * `best` — a site where the frame stands at lateral −1, 0 **and** +1, so
    ///   there is a block of clear ground either side of it;
    /// * `fallback` — a site where only lateral 0 works.
    ///
    /// A fallback is only *recorded* while `best` is still unset (`bestDist ==
    /// -1.0` guards the second block), and it is only *used* if the whole spiral
    /// finished with no `best`. Collapsing the two into one "closest that fits"
    /// gives a different portal wherever a tight site is nearer than a roomy one.
    ///
    /// # The vertical walk, which is where a rewrite goes wrong
    ///
    /// For each column the walk starts at `min(maxY, motionBlockingHeight)` and
    /// steps **down**. At each y that is replaceable it records `yTop = y`, then
    /// slides down through every further replaceable block; the resulting `gap =
    /// yTop - y` is refused when it is `1` or `2` — a cavity too short to stand a
    /// frame in — and accepted at `0` (solid immediately below) or `>= 3`. It
    /// then refuses `y + 4 > maxY`. Both of those are `continue`s that decrement
    /// `y` and go round again, not `break`s.
    ///
    /// # Errors
    /// Never — the `Option` is vanilla's own `Optional.empty()`, and the doc
    /// above says when.
    #[must_use]
    pub fn create_portal(
        level: &mut impl PortalLevel,
        clicked: BlockPos,
        axis: crate::portal::Axis,
    ) -> Option<FoundRectangle> {
        let right = Step::positive(axis);
        let mut best_dist = -1.0f64;
        let mut best: Option<BlockPos> = None;
        let mut fallback_dist = -1.0f64;
        let mut fallback: Option<BlockPos> = None;
        let max_y = level
            .max_y()
            .min(level.min_y() + level.logical_height() - 1);

        for p in spiral_around(clicked, NETHER_PORTAL_RADIUS, Step::East, Step::South) {
            let top = max_y.min(level.motion_blocking_height(p.x, p.z));
            if !level.within_border(p) {
                continue;
            }
            // The border is asked about the position ONE STEP ALONG `right` as
            // well, because that is where the portal's second column lands.
            if !level.within_border(right.applied(p, 1)) {
                continue;
            }
            let mut y = top;
            while y >= level.min_y() {
                let at = BlockPos { x: p.x, y, z: p.z };
                if !level.can_portal_replace(at) {
                    y -= 1;
                    continue;
                }
                let y_top = y;
                while y > level.min_y()
                    && level.can_portal_replace(BlockPos {
                        x: p.x,
                        y: y - 1,
                        z: p.z,
                    })
                {
                    y -= 1;
                }
                if y + 4 > max_y {
                    y -= 1;
                    continue;
                }
                let gap = y_top - y;
                if gap > 0 && gap < 3 {
                    y -= 1;
                    continue;
                }
                let site = BlockPos { x: p.x, y, z: p.z };
                if can_host_frame(level, site, right, 0) {
                    let d = dist_sqr(clicked, site);
                    if can_host_frame(level, site, right, -1)
                        && can_host_frame(level, site, right, 1)
                        && (best_dist == -1.0 || best_dist > d)
                    {
                        best_dist = d;
                        best = Some(site);
                    }
                    if best_dist == -1.0 && (fallback_dist == -1.0 || fallback_dist > d) {
                        fallback_dist = d;
                        fallback = Some(site);
                    }
                }
                y -= 1;
            }
        }

        if best_dist == -1.0 && fallback_dist != -1.0 {
            best = fallback;
            best_dist = fallback_dist;
        }

        let origin = if best_dist == -1.0 {
            // Nothing anywhere in the spiral: carve a site.
            let lo = (level.min_y() + 1).max(70);
            let hi = max_y - 9;
            if hi < lo {
                return None;
            }
            let forced = level.clamp_to_border(BlockPos {
                x: clicked.x - right.step_x(),
                y: clicked.y.clamp(lo, hi),
                z: clicked.z - right.step_z(),
            });
            let cw = right.clockwise();
            for a in -1..2 {
                for b in 0..2 {
                    for c in -1..3 {
                        let block = if c < 0 {
                            crate::portal::FRAME_BLOCK
                        } else {
                            "minecraft:air"
                        };
                        let p = BlockPos {
                            x: forced.x + b * right.step_x() + a * cw.step_x(),
                            y: forced.y + c,
                            z: forced.z + b * right.step_z() + a * cw.step_z(),
                        };
                        // `setBlockAndUpdate` is `setBlock(pos, state, 3)`.
                        level.set_block(p, block, 3);
                    }
                }
            }
            forced
        } else {
            best.expect("bestDist != -1.0 implies a position")
        };

        // The frame: the rim of a 4x5 rectangle, `setBlock(.., 3)`.
        for i in -1..3 {
            for j in -1..4 {
                if i == -1 || i == 2 || j == -1 || j == 3 {
                    let p = BlockPos {
                        x: origin.x + i * right.step_x(),
                        y: origin.y + j,
                        z: origin.z + i * right.step_z(),
                    };
                    level.set_block(p, crate::portal::FRAME_BLOCK, 3);
                }
            }
        }

        // The opening: 2 wide, 3 tall, `setBlock(.., 18)`. Flag 18 is 16|2 —
        // `UPDATE_CLIENTS` without `UPDATE_NEIGHBORS`, plus `UPDATE_SUPPRESS_DROPS`
        // — so lighting a portal does not re-trigger the neighbour updates the
        // frame's own writes already did.
        for i in 0..2 {
            for j in 0..3 {
                let p = BlockPos {
                    x: origin.x + i * right.step_x(),
                    y: origin.y + j,
                    z: origin.z + i * right.step_z(),
                };
                level.set_block(p, crate::portal::PORTAL_BLOCK, 18);
            }
        }

        Some(FoundRectangle {
            min: origin,
            axis1_size: 2,
            axis2_size: 3,
        })
    }

    /// `BlockPos.distSqr(Vec3i)` — integer differences, accumulated as a double.
    #[must_use]
    pub fn dist_sqr(a: BlockPos, b: BlockPos) -> f64 {
        let dx = f64::from(a.x - b.x);
        let dy = f64::from(a.y - b.y);
        let dz = f64::from(a.z - b.z);
        dx * dx + dy * dy + dz * dz
    }
}

/// `net.minecraft.world.entity.PortalProcessor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalProcessor {
    entry_position: BlockPos,
    portal_time: i32,
    inside_portal_this_tick: bool,
}

impl PortalProcessor {
    /// The constructor: `insidePortalThisTick` starts **true**, so the tick a
    /// processor is created is already counted.
    #[must_use]
    pub fn new(entry_position: BlockPos) -> Self {
        Self {
            entry_position,
            portal_time: 0,
            inside_portal_this_tick: true,
        }
    }

    #[must_use]
    pub fn portal_time(&self) -> i32 {
        self.portal_time
    }

    #[must_use]
    pub fn entry_position(&self) -> BlockPos {
        self.entry_position
    }

    pub fn update_entry_position(&mut self, p: BlockPos) {
        self.entry_position = p;
    }

    /// `Entity.setAsInsidePortal` calls this each tick the entity is in the
    /// block.
    pub fn set_as_inside_portal_this_tick(&mut self, v: bool) {
        self.inside_portal_this_tick = v;
    }

    #[must_use]
    pub fn is_inside_portal_this_tick(&self) -> bool {
        self.inside_portal_this_tick
    }

    /// `hasExpired()` — `portalTime <= 0`.
    #[must_use]
    pub fn has_expired(&self) -> bool {
        self.portal_time <= 0
    }

    /// `decayTick()` — `max(portalTime - 4, 0)`.
    fn decay_tick(&mut self) {
        self.portal_time = (self.portal_time - 4).max(0);
    }

    /// `processPortalTeleportation(level, entity, canTeleport)`.
    ///
    /// Returns whether the entity teleports this tick.
    pub fn process(&mut self, can_teleport: bool, transition_time: i32) -> bool {
        if !self.inside_portal_this_tick {
            self.decay_tick();
            return false;
        }
        self.inside_portal_this_tick = false;
        if !can_teleport {
            return false;
        }
        // `portalTime++ >= transitionTime` — the OLD value is compared.
        let before = self.portal_time;
        self.portal_time += 1;
        before >= transition_time
    }
}

/// `NetherPortalBlock.getPortalTransitionTime(level, entity)`.
///
/// `creative_delay` and `default_delay` are the two gamerules' current values;
/// the choice between them is `abilities.invulnerable`.
#[must_use]
pub fn nether_portal_transition_time(
    is_player: bool,
    invulnerable: bool,
    creative_delay: i32,
    default_delay: i32,
) -> i32 {
    if !is_player {
        return 0;
    }
    let rule = if invulnerable {
        creative_delay
    } else {
        default_delay
    };
    // `Math.max(0, rule)` — a datapack can set a negative delay and vanilla
    // floors it rather than treating it as "never".
    rule.max(0)
}

/// `DimensionType.getTeleportationScale(from, to)` — `from.coordinateScale /
/// to.coordinateScale`, in that order.
///
/// Overworld to nether is `1.0 / 8.0`; nether to overworld is `8.0`. The
/// values come from the dimension-type registry, which is datapack data
/// (`data/minecraft/dimension_type/the_nether.json` carries
/// `"coordinate_scale": 8.0`) and reaches this server through the registry
/// sync, so they are not constants here.
#[must_use]
pub fn teleportation_scale(from_coordinate_scale: f64, to_coordinate_scale: f64) -> f64 {
    from_coordinate_scale / to_coordinate_scale
}

/// `WorldBorder.clampToBounds(double, double, double)`.
///
/// `Mth.clamp` on x and z against `[minX, maxX - eps]`, y untouched, then
/// `BlockPos.containing`, which floors. Not rounds: `-15.5` floors to `-16`.
#[must_use]
pub fn clamp_to_bounds(border: &WorldBorder, x: f64, y: f64, z: f64) -> BlockPos {
    let cx = clamp(
        x,
        border.min_x(0.0),
        border.max_x(0.0) - BORDER_CLAMP_EPSILON,
    );
    let cz = clamp(
        z,
        border.min_z(0.0),
        border.max_z(0.0) - BORDER_CLAMP_EPSILON,
    );
    #[allow(clippy::cast_possible_truncation)]
    BlockPos {
        x: cx.floor() as i32,
        y: y.floor() as i32,
        z: cz.floor() as i32,
    }
}

/// The scaled, clamped entry position `NetherPortalBlock.getPortalDestination`
/// hands to the portal search.
///
/// x and z are multiplied by the scale, **y is not**, and the result is
/// clamped to the *destination* level's border.
#[must_use]
pub fn scaled_entry_position(
    destination_border: &WorldBorder,
    x: f64,
    y: f64,
    z: f64,
    scale: f64,
) -> BlockPos {
    clamp_to_bounds(destination_border, x * scale, y, z * scale)
}

/// End portals.
///
/// # An end portal has no delay and a nether portal has eighty
///
/// `Portal.getPortalTransitionTime` is a **default method returning 0**, and
/// `EndPortalBlock` does not override it — only `NetherPortalBlock` does. So an
/// end portal fires on the first tick an entity is inside it, for players as
/// well as everything else, and the `players_nether_portal_*` gamerules do not
/// reach it. `Portal.getLocalTransition` is likewise a default returning
/// `NONE`, which `NetherPortalBlock` overrides to `CONFUSION` — the nausea a
/// nether portal gives and an end portal does not.
///
/// # Where it sends you
///
/// `EndPortalBlock.getPortalDestination` branches on the level it is in:
///
/// * **in the End** — the destination is the level's own respawn data, its
///   dimension and position, which is normally the overworld spawn;
/// * **anywhere else** — the End, at [`END_SPAWN_POINT`].
///
/// and then on what is travelling:
///
/// * a `ServerPlayer` goes through `findRespawnPositionAndUseSpawnBlock(false,
///   DO_NOTHING)`, which consults their bed or respawn anchor;
/// * anything else lands at `Vec3.atBottomCenterOf(adjustSpawnLocation(...))`
///   with a zero delta and the respawn data's yaw and pitch, relative in
///   `DELTA` and `ROTATION`.
///
/// Both branches then run `PLAY_PORTAL_SOUND.then(PLACE_PORTAL_TICKET)`.
///
/// The player branch needs a respawn position this module does not own, so it
/// is named here and not implemented: [`end_portal_target`] answers the part
/// that is a constant and says which part is not.
pub mod end_portal {
    use rustmc_data::BlockPos;

    /// `ServerLevel.END_SPAWN_POINT` — `(100, 50, 0)`, the obsidian platform.
    pub const END_SPAWN_POINT: BlockPos = BlockPos {
        x: 100,
        y: 50,
        z: 0,
    };

    /// `Portal.getPortalTransitionTime`'s default, which `EndPortalBlock`
    /// inherits: **zero ticks**.
    pub const TRANSITION_TIME: i32 = 0;

    /// Where an end portal sends something, as far as this module can say.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Target {
        /// Entering the End from anywhere else: the obsidian platform.
        EndPlatform(BlockPos),
        /// Leaving the End. The dimension and position are the level's respawn
        /// data, and for a `ServerPlayer` they are overridden by
        /// `findRespawnPositionAndUseSpawnBlock` — neither is a constant, so
        /// this variant carries nothing and exists to make the branch visible
        /// rather than to answer it.
        LevelRespawnData,
    }

    /// The branch `getPortalDestination` takes.
    #[must_use]
    pub fn end_portal_target(in_the_end: bool) -> Target {
        if in_the_end {
            Target::LevelRespawnData
        } else {
            Target::EndPlatform(END_SPAWN_POINT)
        }
    }

    /// Where a **player** entering the End lands, and which way they face.
    ///
    /// `EndPortalBlock.getPortalDestination`, offsets 81..160, in order:
    ///
    /// ```text
    ///   81  pos = Vec3.atBottomCenterOf(END_SPAWN_POINT)     (100.5, 50.0, 0.5)
    ///   93  createEndPlatform(endLevel, BlockPos.containing(pos).below(), true)
    ///  107  yRot = Direction.WEST.toYRot()                    90.0f
    ///  115  xRot = 0.0f
    ///  118  relatives = Relative.union(DELTA, Set.of(X_ROT))
    ///  142  if (entity instanceof ServerPlayer) pos = pos.subtract(0, 1, 0)
    /// ```
    ///
    /// **The player is dropped one block, and only the player.** A non-player
    /// takes the `adjustSpawnLocation` branch instead and keeps the
    /// bottom-centre y. So a thrown item arrives at y 50 and a player at
    /// **49**, standing on the obsidian the platform's `dy == -1` layer just
    /// wrote at y 48.
    ///
    /// The platform's origin is `containing(pos).below()` = (100, **49**, 0),
    /// not `END_SPAWN_POINT` itself — one block lower, which puts its
    /// obsidian at 48 rather than 49 and its three layers of air at 49, 50
    /// and 51.
    ///
    /// `DELTA` is relative and `X_ROT` is relative, so the arriving entity
    /// keeps its pitch and its velocity and is given an absolute yaw of 90.
    #[must_use]
    pub fn player_end_arrival() -> Arrival {
        Arrival {
            x: 100.5,
            y: 49.0,
            z: 0.5,
            y_rot: 90.0,
            x_rot: 0.0,
            platform_origin: BlockPos {
                x: 100,
                y: 49,
                z: 0,
            },
        }
    }

    /// Where something arriving in the End is put.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Arrival {
        pub x: f64,
        pub y: f64,
        pub z: f64,
        /// Absolute — `Direction.WEST.toYRot()`, which is 90.
        pub y_rot: f32,
        /// Relative: `X_ROT` is in the relatives set, so this is a delta of
        /// zero and the arriving entity keeps the pitch it had.
        pub x_rot: f32,
        /// What `createEndPlatform` is called with.
        pub platform_origin: BlockPos,
    }

    /// `EndPlatformFeature.createEndPlatform(level, origin, destroy)` — the
    /// hundred positions it writes, in the order it writes them.
    ///
    /// Three nested loops, and **the outer one is z**:
    ///
    /// ```text
    ///   for dz in -2..=2          // slot 4, the LAST argument to move
    ///     for dx in -2..=2        // slot 5, the FIRST
    ///       for dy in -1..3       // slot 6, the SECOND
    ///          block = dy == -1 ? OBSIDIAN : AIR
    ///          if !level.getBlockState(p).is(block):
    ///              if destroy: level.destroyBlock(p, true, null)
    ///              level.setBlock(p, block.defaultBlockState(), 3)
    /// ```
    ///
    /// `move(III)` takes `(dx, dy, dz)` and the bytecode pushes slots 5, 6, 4
    /// in that order, so the loop nesting is z-major, then x, then y — which
    /// is not the order the argument list reads in and is what a transcription
    /// copying the signature gets wrong. It matters because `destroyBlock`
    /// drops items and fires neighbour updates, so the write order is
    /// observable.
    ///
    /// 5 x 5 x 4 = **100 positions**: 25 obsidian at `dy == -1` and 75 air.
    /// A position that already holds the right block is skipped entirely —
    /// no destroy, no set.
    #[must_use]
    pub fn end_platform_positions(origin: BlockPos) -> Vec<(BlockPos, &'static str)> {
        let mut out = Vec::with_capacity(100);
        for dz in -2..=2 {
            for dx in -2..=2 {
                for dy in -1..3 {
                    let block = if dy == -1 {
                        "minecraft:obsidian"
                    } else {
                        "minecraft:air"
                    };
                    out.push((
                        BlockPos {
                            x: origin.x + dx,
                            y: origin.y + dy,
                            z: origin.z + dz,
                        },
                        block,
                    ));
                }
            }
        }
        out
    }
}

/// `PortalForcer.findClosestPortalPosition(from, toNether, border)` — the
/// filter chain and the comparator, over candidates a caller supplies.
///
/// # What is ported and what is not, stated apart
///
/// The **selection** is ported in full and is what this function is. The
/// **candidate source** is not: vanilla asks
/// `PoiManager.getInSquare(NETHER_PORTAL, from, r, ANY)`, and this server has
/// no point-of-interest index. A caller passes the portal positions it knows
/// about instead, and the one observable difference is named below rather
/// than left implicit.
///
/// # The chain, in order, from the bytecode
///
/// ```text
///    9  r = toNether ? 16 : 128
///   23  poi.ensureLoadedAndValid(level, from, r)
///   35  poi.getInSquare(h -> h.is(PoiTypes.NETHER_PORTAL), from, r, ANY)
///   51    .map(PoiRecord::getPos)
///   61    .filter(border::isWithinBounds)
///   77    .filter(p -> level.getBlockState(p).hasProperty(HORIZONTAL_AXIS))
///   88    .min(comparingDouble(p -> from.distSqr(p))
///              .thenComparingInt(BlockPos::getY))
/// ```
///
/// Three things a reading gets wrong:
///
/// * **the radius is 16 nether-side and 128 overworld-side**, and the argument
///   is `toNether`, so the *small* radius is the one used when arriving in the
///   nether. It is the same 8:1 as the coordinate scale, and it is not a
///   coincidence: a 128-block overworld square maps onto a 16-block nether one.
/// * **`getInSquare`'s final filter is Chebyshev in x and z with NO y bound**
///   (`abs(dx) <= r && abs(dz) <= r`), so a portal two hundred blocks above
///   the arrival point is a candidate. Y enters only as the tie-break.
/// * **the second filter is `hasProperty(HORIZONTAL_AXIS)`, not
///   `is(NETHER_PORTAL)`.** A POI record survives the block being broken until
///   the section is revalidated, and this is the re-check — any block with a
///   horizontal-axis property passes it, which is a weaker test than the block
///   identity and is the one vanilla makes.
///
/// # The one place the candidate order is observable
///
/// `Stream.min` keeps the **first** minimum it sees, so two candidates with
/// the same `distSqr` *and* the same y are separated by arrival order and by
/// nothing else. Everywhere else the comparator decides, and any candidate
/// order gives the same answer. So the missing POI index costs exact
/// (distSqr, y) ties and costs nothing else — which is a bounded statement
/// rather than "close enough", and is why this is ported rather than refused.
#[must_use]
pub fn find_closest_portal_position(
    candidates: &[BlockPos],
    from: BlockPos,
    to_nether: bool,
    within_border: &dyn Fn(BlockPos) -> bool,
    has_horizontal_axis: &dyn Fn(BlockPos) -> bool,
) -> Option<BlockPos> {
    let r = if to_nether {
        forcer::NETHER_PORTAL_RADIUS
    } else {
        forcer::OVERWORLD_PORTAL_RADIUS
    };
    let mut best: Option<(f64, i32, BlockPos)> = None;
    for &p in candidates {
        // `getInSquare`'s own final filter: Chebyshev in x and z, no y bound.
        if (p.x - from.x).abs() > r || (p.z - from.z).abs() > r {
            continue;
        }
        if !within_border(p) || !has_horizontal_axis(p) {
            continue;
        }
        let key = (forcer::dist_sqr(from, p), p.y);
        // `Stream.min` replaces only on a STRICT improvement, so the first of
        // two equal candidates wins.
        match best {
            Some((d, y, _)) if (d, y) <= key => {}
            _ => best = Some((key.0, key.1, p)),
        }
    }
    best.map(|(_, _, p)| p)
}

/// Leaving the End — `ServerPlayer.findRespawnPositionAndUseSpawnBlock`.
///
/// This is the branch `EndPortalBlock.getPortalDestination` takes for a
/// `ServerPlayer` standing on an end portal **inside** the End, and it is not
/// the entering journey with a different position: it is a different
/// `TeleportTransition`, built by a different method, with a different
/// relatives set.
pub mod end_return {
    use rustmc_data::BlockPos;

    /// The three outcomes, which differ in one boolean and in nothing else.
    ///
    /// ```text
    ///   findRespawnPositionAndUseSpawnBlock(bool, post):
    ///     config = getRespawnConfig()
    ///     level  = server.getLevel(RespawnConfig.getDimensionOrDefault(config))
    ///     if level != null && config != null:
    ///         r = findRespawnAndUseSpawnBlock(level, config, bool)
    ///         if r.isPresent(): return new TeleportTransition(level, r.position(),
    ///                                       Vec3.ZERO, r.yaw(), r.pitch(), post)
    ///         return TeleportTransition.missingRespawnBlock(player, post)   // 83
    ///     return TeleportTransition.createDefault(player, post)             // 89
    /// ```
    ///
    /// `createDefault` and `missingRespawnBlock` are byte-for-byte the same
    /// method apart from `iconst_0` against `iconst_1` at offset 37 — the
    /// `missingRespawnBlock` flag — and that flag is what makes
    /// `ServerPlayer.teleport` send `game_event NO_RESPAWN_BLOCK_AVAILABLE`
    /// at its offset 16, before anything else.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Outcome {
        /// The player has a respawn block and it is still there: the
        /// transition goes to that block's own position and angles.
        RespawnBlock,
        /// The player has a respawn block and it is gone. Same destination as
        /// [`Self::WorldSpawn`], plus the `NO_RESPAWN_BLOCK_AVAILABLE` game
        /// event.
        MissingRespawnBlock,
        /// The player has no respawn block at all: the world spawn, silently.
        WorldSpawn,
    }

    impl Outcome {
        /// Whether `TeleportTransition.missingRespawnBlock()` is true, which
        /// is the only thing the two spawn-going outcomes disagree about.
        #[must_use]
        pub const fn sends_no_respawn_block_event(self) -> bool {
            matches!(self, Self::MissingRespawnBlock)
        }

        /// `ServerPlayer.findRespawnPositionAndUseSpawnBlock`'s branch, given
        /// what the player has.
        #[must_use]
        pub const fn of(has_respawn_config: bool, block_still_there: bool) -> Self {
            if !has_respawn_config {
                Self::WorldSpawn
            } else if block_still_there {
                Self::RespawnBlock
            } else {
                Self::MissingRespawnBlock
            }
        }
    }

    /// Which level a player with no respawn block returns to.
    ///
    /// `MinecraftServer.findRespawnDimension()` reads the **overworld level
    /// data's** respawn data — not the player's, and not the level the player
    /// is standing in — takes the dimension it names, and falls back to
    /// `overworld()` if that level does not exist. So a default world always
    /// returns a player to the overworld, and it does so by looking up a
    /// dimension key rather than by naming one.
    #[must_use]
    pub fn respawn_dimension(
        world_respawn_dimension: Option<super::Dimension>,
    ) -> super::Dimension {
        world_respawn_dimension.unwrap_or(super::Dimension::Overworld)
    }

    /// Where that player lands: `findAdjustedSharedSpawnPos`, which is
    /// `Vec3.atBottomCenterOf(entity.adjustSpawnLocation(level, respawnData.pos()))`.
    ///
    /// `atBottomCenterOf` adds **0.5 to x and z and nothing to y** — the same
    /// function `EndPortalBlock` uses on `END_SPAWN_POINT`, and the reason a
    /// spawn position that looks like an integer arrives as a half.
    ///
    /// `adjustSpawnLocation` is `Entity`'s default for a player and returns
    /// its argument unchanged; the overriding implementations belong to
    /// entities this server does not spawn.
    #[must_use]
    pub fn bottom_centre_of(pos: BlockPos) -> (f64, f64, f64) {
        (
            f64::from(pos.x) + 0.5,
            f64::from(pos.y),
            f64::from(pos.z) + 0.5,
        )
    }

    /// **The arrival is fully absolute**, unlike the End entry's.
    ///
    /// `createDefault` and `missingRespawnBlock` both pass `Set.of()` as the
    /// relatives — `invokestatic Set.of()` at offset 39, with no arguments —
    /// where `EndPortalBlock`'s entering branch passes
    /// `Relative.union(DELTA, Set.of(X_ROT))`. So a player entering the End
    /// keeps its velocity and its pitch, and a player leaving it keeps
    /// neither: the delta is `Vec3.ZERO` absolute, and the yaw and pitch are
    /// the world spawn's.
    #[must_use]
    pub const fn relatives_are_empty() -> bool {
        true
    }
}

#[cfg(test)]
mod forcer_tests {
    use super::forcer::{create_portal, PortalLevel, Step};
    use crate::portal::Axis;
    use rustmc_data::BlockPos;

    /// A world held in a map, answering the seven questions
    /// [`PortalLevel`] asks and recording every write in order.
    ///
    /// **This scores the transcription against its bytecode, not against a
    /// run.** `PortalForcer.createPortal` takes a `ServerLevel`, and driving
    /// one inside the pinned jar is a different order of work from
    /// `ci/portal-golden.sh`'s in-memory `BlockGetter` — `PortalShape` reads a
    /// `BlockGetter`, this writes to a level and asks it for a heightmap. So
    /// what these tests pin is the geometry, the search order, the gap rule
    /// and the two write flags; they catch a change and they do not prove
    /// parity. The one part that IS scored against vanilla is the traversal,
    /// in `tests/portal_shape.rs::the_spiral_is_vanillas_own`.
    struct Grid {
        /// Everything at or below this y is solid stone; above it is air.
        surface: i32,
        min_y: i32,
        max_y: i32,
        logical_height: i32,
        /// Positions forced to be non-replaceable regardless of the surface.
        blocked: Vec<BlockPos>,
        writes: Vec<(BlockPos, String, i32)>,
    }

    impl Grid {
        fn new(surface: i32) -> Self {
            Self {
                surface,
                min_y: 0,
                max_y: 255,
                logical_height: 256,
                blocked: Vec::new(),
                writes: Vec::new(),
            }
        }

        fn wrote(&self, block: &str) -> Vec<BlockPos> {
            self.writes
                .iter()
                .filter(|(_, b, _)| b == block)
                .map(|(p, _, _)| *p)
                .collect()
        }
    }

    impl PortalLevel for Grid {
        fn can_portal_replace(&self, pos: BlockPos) -> bool {
            !self.blocked.contains(&pos) && pos.y > self.surface
        }
        fn is_solid(&self, pos: BlockPos) -> bool {
            pos.y <= self.surface
        }
        fn motion_blocking_height(&self, _x: i32, _z: i32) -> i32 {
            self.surface + 1
        }
        fn min_y(&self) -> i32 {
            self.min_y
        }
        fn max_y(&self) -> i32 {
            self.max_y
        }
        fn logical_height(&self) -> i32 {
            self.logical_height
        }
        fn within_border(&self, _pos: BlockPos) -> bool {
            true
        }
        fn clamp_to_border(&self, pos: BlockPos) -> BlockPos {
            pos
        }
        fn set_block(&mut self, pos: BlockPos, block: &str, flags: i32) {
            self.writes.push((pos, block.to_string(), flags));
        }
    }

    /// A portal built on open flat ground: the site is the clicked column
    /// itself, because the spiral reaches it first and nothing nearer can tie.
    #[test]
    fn a_portal_lands_on_the_clicked_column() {
        let mut g = Grid::new(63);
        let clicked = BlockPos { x: 8, y: 70, z: -3 };
        let r = create_portal(&mut g, clicked, Axis::X).expect("a portal is built");
        assert_eq!(
            r.min,
            BlockPos { x: 8, y: 64, z: -3 },
            "the frame's bottom-left sits on the first air block above the surface"
        );
        assert_eq!((r.axis1_size, r.axis2_size), (2, 3));
    }

    /// The opening is 2x3 `nether_portal` at flag 18, and the frame is the rim
    /// of a 4x5 rectangle of obsidian at flag 3.
    ///
    /// The flags are checked because they are the part a rewrite drops:
    /// `setBlock(.., 18)` is `UPDATE_CLIENTS | UPDATE_SUPPRESS_DROPS` with
    /// **no** `UPDATE_NEIGHBORS`, and writing the portal blocks at 3 instead
    /// re-triggers the neighbour updates the frame's own writes already did.
    #[test]
    fn the_frame_is_the_rim_and_the_opening_is_two_by_three() {
        let mut g = Grid::new(63);
        let r = create_portal(&mut g, BlockPos { x: 0, y: 70, z: 0 }, Axis::X)
            .expect("a portal is built");

        let portal = g.wrote(crate::portal::PORTAL_BLOCK);
        assert_eq!(portal.len(), 6, "2 wide, 3 tall");
        for p in &portal {
            assert!(
                p.x >= r.min.x && p.x < r.min.x + 2,
                "the opening is 2 along x"
            );
            assert!(p.y >= r.min.y && p.y < r.min.y + 3, "the opening is 3 tall");
            assert_eq!(p.z, r.min.z);
        }
        assert!(
            g.writes
                .iter()
                .filter(|(_, b, _)| b == crate::portal::PORTAL_BLOCK)
                .all(|(_, _, f)| *f == 18),
            "the portal blocks go in at flag 18"
        );

        // The frame writes are the rim of i in -1..3, j in -1..4 — 20 of the
        // 20 rim cells, of which 6 are the opening's own footprint written
        // over. 4x5 = 20 cells, minus the 2x3 interior = 14.
        let frame = g.wrote(crate::portal::FRAME_BLOCK);
        assert_eq!(frame.len(), 14, "the rim of a 4x5 rectangle");
        assert!(
            g.writes
                .iter()
                .filter(|(_, b, _)| b == crate::portal::FRAME_BLOCK)
                .all(|(_, _, f)| *f == 3),
            "the frame goes in at flag 3"
        );
        // The frame is written BEFORE the opening — the opening's blocks sit
        // inside the rim and a reversed order would have the frame overwrite
        // two of them at the corners it shares. It shares none, but the order
        // is vanilla's and is what a level with neighbour updates observes.
        let last_frame = g
            .writes
            .iter()
            .rposition(|(_, b, _)| b == crate::portal::FRAME_BLOCK)
            .expect("frame");
        let first_portal = g
            .writes
            .iter()
            .position(|(_, b, _)| b == crate::portal::PORTAL_BLOCK)
            .expect("portal");
        assert!(last_frame < first_portal, "the frame goes in first");
    }

    /// The axis chooses `right`, and a Z-axis portal is 2 blocks along **z**.
    ///
    /// `Direction.get(POSITIVE, axis)` is EAST for X and SOUTH for Z, and
    /// every offset in the method is `right.getStepX()` / `getStepZ()`, so
    /// getting this wrong builds a portal at ninety degrees to the one that
    /// was lit.
    #[test]
    fn the_axis_turns_the_portal() {
        let mut gx = Grid::new(63);
        let rx = create_portal(&mut gx, BlockPos { x: 0, y: 70, z: 0 }, Axis::X).expect("x");
        let mut gz = Grid::new(63);
        let rz = create_portal(&mut gz, BlockPos { x: 0, y: 70, z: 0 }, Axis::Z).expect("z");

        let spread = |g: &Grid| {
            let ps = g.wrote(crate::portal::PORTAL_BLOCK);
            let dx = ps.iter().map(|p| p.x).max().unwrap_or(0)
                - ps.iter().map(|p| p.x).min().unwrap_or(0);
            let dz = ps.iter().map(|p| p.z).max().unwrap_or(0)
                - ps.iter().map(|p| p.z).min().unwrap_or(0);
            (dx, dz)
        };
        assert_eq!(spread(&gx), (1, 0), "an X-axis portal is 2 wide along x");
        assert_eq!(spread(&gz), (0, 1), "a Z-axis portal is 2 wide along z");
        assert_eq!(rx.min.y, rz.min.y, "the same ground either way");
    }

    /// `Step::positive` and `getClockWise` — the two rotations the whole
    /// method is expressed in.
    #[test]
    fn the_directions_are_vanillas() {
        assert_eq!(Step::positive(Axis::X), Step::East);
        assert_eq!(Step::positive(Axis::Z), Step::South);
        assert_eq!(Step::East.clockwise(), Step::South);
        assert_eq!(Step::South.clockwise(), Step::West);
        assert_eq!(Step::West.clockwise(), Step::North);
        assert_eq!(Step::North.clockwise(), Step::East);
        // SOUTH is POSITIVE z and WEST is NEGATIVE x, which is the pair a
        // transcription mirrors by accident.
        assert_eq!(Step::South.step_z(), 1);
        assert_eq!(Step::West.step_x(), -1);
    }

    /// A world with nowhere to put a portal still gets one: the forcing
    /// branch carves a 2x3x4 box of air over an obsidian floor and builds on
    /// that.
    ///
    /// Driven by making **every** position non-replaceable, which is what a
    /// solid world is: `canPortalReplaceBlock` then answers false for every
    /// candidate the spiral offers, the search ends with `bestDist == -1.0`
    /// and no fallback, and the method carves.
    #[test]
    fn a_solid_world_gets_a_carved_portal() {
        // Surface above every y the walk can reach, so nothing is replaceable.
        let mut g = Grid::new(255);
        let clicked = BlockPos { x: 5, y: 100, z: 5 };
        let r = create_portal(&mut g, clicked, Axis::X).expect("a portal is forced");

        // `clamp(clicked.y, max(minY + 1, 70), maxY - 9)` with minY 0 and
        // maxY 255 is `clamp(100, 70, 246)` = 100, and the origin steps one
        // block back along `right`.
        assert_eq!(r.min, BlockPos { x: 4, y: 100, z: 5 });

        // The carve: 3 x 2 x 4 positions, obsidian at the bottom layer only.
        let carved_obsidian = g
            .writes
            .iter()
            .take(24)
            .filter(|(_, b, _)| b == crate::portal::FRAME_BLOCK)
            .count();
        assert_eq!(carved_obsidian, 6, "the carve floors 3x2 with obsidian");
        assert_eq!(
            g.writes
                .iter()
                .take(24)
                .filter(|(_, b, _)| b == "minecraft:air")
                .count(),
            18,
            "and 3x2x3 of air above it"
        );
    }

    /// A world too short to hold a frame is the one case that returns `None`,
    /// and it is the only one.
    ///
    /// `maxY - 9 < max(minY + 1, 70)`. With `minY = 0` the floor is 70, so a
    /// world whose `maxY` is under 79 has nowhere to force one.
    #[test]
    fn a_world_under_seventy_nine_tall_refuses() {
        let mut g = Grid::new(255);
        g.max_y = 78;
        g.logical_height = 79;
        assert!(create_portal(&mut g, BlockPos { x: 0, y: 40, z: 0 }, Axis::X).is_none());

        g.max_y = 79;
        g.logical_height = 80;
        assert!(
            create_portal(&mut g, BlockPos { x: 0, y: 40, z: 0 }, Axis::X).is_some(),
            "one block taller and it forces one"
        );
    }

    /// The gap rule: a cavity 1 or 2 blocks deep is refused, 0 and 3 are not.
    ///
    /// This is the clause a rewrite drops, because it reads as an
    /// optimisation and is not: `gap = yTop - y` where `y` has slid to the
    /// bottom of the run of replaceable blocks, and `gap > 0 && gap < 3`
    /// continues. The effect is that a portal never lands in a two-block
    /// crawlspace.
    #[test]
    fn a_two_block_cavity_is_refused() {
        // A ceiling: solid at y = 66, so the column has air at 64..65 (a
        // 2-deep cavity over the surface at 63) and open sky above 66.
        let mut g = Grid::new(63);
        g.blocked.push(BlockPos { x: 0, y: 66, z: 0 });
        // The heightmap now reports the ceiling rather than the ground.
        struct Ceiling(Grid);
        impl PortalLevel for Ceiling {
            fn can_portal_replace(&self, pos: BlockPos) -> bool {
                self.0.can_portal_replace(pos)
            }
            fn is_solid(&self, pos: BlockPos) -> bool {
                pos.y <= self.0.surface || self.0.blocked.contains(&pos)
            }
            fn motion_blocking_height(&self, x: i32, z: i32) -> i32 {
                if (x, z) == (0, 0) {
                    67
                } else {
                    self.0.motion_blocking_height(x, z)
                }
            }
            fn min_y(&self) -> i32 {
                self.0.min_y()
            }
            fn max_y(&self) -> i32 {
                self.0.max_y()
            }
            fn logical_height(&self) -> i32 {
                self.0.logical_height()
            }
            fn within_border(&self, p: BlockPos) -> bool {
                self.0.within_border(p)
            }
            fn clamp_to_border(&self, p: BlockPos) -> BlockPos {
                self.0.clamp_to_border(p)
            }
            fn set_block(&mut self, p: BlockPos, b: &str, f: i32) {
                self.0.set_block(p, b, f);
            }
        }
        let mut c = Ceiling(g);
        let r = create_portal(&mut c, BlockPos { x: 0, y: 70, z: 0 }, Axis::X)
            .expect("a portal is built");
        // Column (0,0) offers only the 2-deep cavity at y 64..65, which the
        // gap rule refuses, so the portal must land on a NEIGHBOURING column.
        assert_ne!(
            (r.min.x, r.min.z),
            (0, 0),
            "a 2-block cavity must not host a portal"
        );
    }

    /// `BlockPos.distSqr` — integer differences accumulated as a double, and
    /// the value the whole search is ordered by.
    #[test]
    fn dist_sqr_is_the_squared_distance() {
        let a = BlockPos { x: 0, y: 0, z: 0 };
        let b = BlockPos { x: 3, y: 4, z: 12 };
        assert!((super::forcer::dist_sqr(a, b) - 169.0).abs() < f64::EPSILON);
        assert!((super::forcer::dist_sqr(b, a) - 169.0).abs() < f64::EPSILON);
    }
}
