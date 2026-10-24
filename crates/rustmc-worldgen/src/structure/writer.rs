//! How a structure piece writes blocks.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `StructurePiece.{placeBlock, getBlock, isInterior, generateBox,
//! generateMaybeBox, maybeGenerateBlock}`,
//! `MineshaftPieces.MineShaftPiece.{canBeReplaced, isInInvalidLocation}`.
//!
//! Shared by every structure in the game. A piece describes itself in local
//! coordinates and this turns that into writes, clipped to the chunk being
//! generated — which is why the same piece is written once per chunk it
//! overlaps and must produce the same blocks each time.
//!
//! # The draw count is the box volume, not the blocks placed
//!
//! [`Writer::generate_maybe_box`] draws a `nextFloat` for **every position in
//! the box**, before it tests anything else — before the air check, before
//! the interior check, before the edge test. So a maybe-box over a 3x3x40
//! corridor spends 360 values whether it writes 360 blocks or none, and
//! moving the cheap tests in front of the draw — the obvious optimisation —
//! desynchronises everything after it.
//!
//! # Two chance comparisons that are not the same comparison
//!
//! `generateMaybeBox` places when `nextFloat() <= chance`; `maybeGenerateBlock`
//! places when `nextFloat() < chance`. They differ at exact equality, and
//! they compile to different comparison instructions (`fcmpl` against
//! `fcmpg`) which also differ on NaN. Two methods, ten lines apart, doing
//! *almost* the same thing.
//!
//! # Iteration order is Y, then X, then Z
//!
//! Which is invisible for a plain `generateBox` and decisive for a maybe-box,
//! since it fixes which position each drawn value belongs to.

// Read once, not per piece written. See `crate::trace_switch`.
crate::trace_switch!(trace_mineshaft, "RUSTMC_TRACE_MINESHAFT");
use super::pieces::{BoundingBox, Facing, Mirror, Piece, Rotation};
use crate::rng::RandomSource;

/// What a structure needs of the world while it writes.
pub trait StructureWorld: crate::block_entity::BlockEntityRecorder {
    /// The full block state at a world position.
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String;

    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str);

    /// `Heightmap.Types.OCEAN_FLOOR_WG` for this column — the fossil the
    /// noise stage recorded, not a live heightmap.
    fn ocean_floor_wg(&mut self, x: i32, z: i32) -> i32;

    /// Whether the biome here is in `#minecraft:mineshaft_blocking`.
    fn biome_blocks_mineshafts(&mut self, x: i32, y: i32, z: i32) -> bool;

    /// `BlockState#liquid`.
    fn is_liquid(&mut self, x: i32, y: i32, z: i32) -> bool;

    /// `ChunkAccess#markPosForPostprocessing`.
    ///
    /// Vanilla marks a position whenever the block it just wrote is in
    /// `StructurePiece.SHAPE_CHECK_BLOCKS`, and a later pass re-derives that
    /// block from its neighbours. A default no-op so a world that does not
    /// run the pass is explicit about it rather than silently correct.
    fn mark_post_processing(&mut self, _x: i32, _y: i32, _z: i32) {}

    /// `WorldGenLevel.addFreshEntity` for an entity a piece creates — a
    /// mineshaft's chest minecart. A default no-op: a world that keeps no
    /// entities drops it, and the draws that made it are spent either way.
    fn add_fresh_entity(&mut self, _entity: rustmc_world::nbt::Nbt) {}

    /// `WorldGenLevel.getRandom()` — the `WorldGenRegion` random a spawned
    /// mob's `finalizeSpawn` draws from — or `None` for a world without one,
    /// which then spawns nothing.
    fn level_random(&mut self) -> Option<&mut crate::rng::AnyRandomSource> {
        None
    }

    /// `state.isFaceSturdy(level, pos, direction)`, where the direction is
    /// given as a unit offset. A world with no shape table should answer
    /// conservatively rather than guess.
    fn is_sturdy_face(&mut self, x: i32, y: i32, z: i32, dx: i32, dy: i32, dz: i32) -> bool;

    /// `Block.canSupportCenter(level, pos, DOWN) && !(block instanceof
    /// FallingBlock)` — whether a chain may hang from this block.
    fn can_hang_chain_below(&mut self, x: i32, y: i32, z: i32) -> bool;

    /// `BlockState#isSolidRender` — is the state's occlusion shape a full
    /// cube.
    ///
    /// Asked about a **state**, not a position, because the only caller —
    /// the corridor's rail pass — asks it about the state `getBlock` gave
    /// it, which is `air` outside the clip and not the world's block. A
    /// position-taking version would answer about the wrong block at every
    /// chunk border.
    ///
    /// It gates a `maybeGenerateBlock` draw, so a world that guesses at it
    /// from block names desynchronises the stream: a fence, a chain, a
    /// spawner and a pointed dripstone all look solid to a name test and
    /// none of them occlude.
    fn state_is_solid_render(&self, state: &str) -> bool;

    /// `level.getHeight(MOTION_BLOCKING_NO_LEAVES, x, z)` on the live map —
    /// what `ScatteredFeaturePiece` averages or minimises to seat a temple.
    ///
    /// `None` is "this world keeps no live heightmap", and a scattered piece
    /// then declines to place rather than seating itself on a made-up ground;
    /// the mineshaft, which never asks, keeps working in the toy worlds.
    fn motion_blocking_no_leaves_height(&mut self, _x: i32, _z: i32) -> Option<i32> {
        None
    }

    /// `level.getMinY()`.
    fn min_y(&self) -> i32 {
        -64
    }

    /// `level.getRandom().nextBoolean()` — the `WorldGenRegion`'s own random,
    /// which the desert pyramid's cellar draws from. `None` when this world
    /// carries none, and the piece then declines rather than inventing one.
    fn level_random_next_bool(&mut self) -> Option<bool> {
        None
    }

    /// `level.getRandom().nextFloat()`. See [`Self::level_random_next_bool`].
    fn level_random_next_f32(&mut self) -> Option<f32> {
        None
    }
}

/// `StructurePiece.isReplaceableByStructures`.
///
/// Air, any fluid, and three plants — glow lichen and the two seagrasses.
/// The plants are not an accident: a mine's supports are allowed to grow
/// through them.
/// `BlockState#isAir`, which is **not** "the block is named air".
///
/// `cave_air` and `void_air` answer yes, and a mineshaft interior is full of
/// `cave_air` — so a name comparison silently flips every `isAir()` test
/// inside a mine. Vanilla's structure code asks this five separate times
/// (`generateBox`'s and `generateMaybeBox`'s `skipAir`, `isSupportingBox`,
/// and the sphere writer's `excludeAir`) and all five are this question.
/// `StructurePiece.SHAPE_CHECK_BLOCKS` — the closed list of blocks whose
/// state has to be re-derived from their neighbours after the structure is
/// written.
///
/// A fence is placed with `west=true` by `placeSupport` and then loses that
/// connection if nothing to its west supports one. Without the pass the
/// explicitly-set connection survives, which is a *state* difference on a
/// block that is otherwise in the right place — 144 of them at seed
/// 31415926, and none at all at seed 20260722.
#[must_use]
pub fn needs_shape_check(state: &str) -> bool {
    // `StructurePiece.CAVE_AIR` is **not** a member. It is a separate
    // `BlockState` field holding the fill block, and its `putstatic` sits
    // immediately above `SHAPE_CHECK_BLOCKS`' builder in the same static
    // initialiser — which is how it got in here. A mineshaft writes cave_air
    // by the thousand, so marking it put thousands of spurious positions in
    // the post-processing list; that was invisible for as long as the pass
    // only folded fences, and became three wrong speleothems the moment it
    // folded anything else.
    matches!(
        plain(state),
        "minecraft:torch"
            | "minecraft:wall_torch"
            | "minecraft:ladder"
            | "minecraft:iron_bars"
            | "minecraft:oak_fence"
            | "minecraft:spruce_fence"
            | "minecraft:birch_fence"
            | "minecraft:jungle_fence"
            | "minecraft:acacia_fence"
            | "minecraft:dark_oak_fence"
            | "minecraft:pale_oak_fence"
            | "minecraft:nether_brick_fence"
    )
}

#[must_use]
pub fn is_air(state: &str) -> bool {
    matches!(
        plain(state),
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}

#[must_use]
pub fn is_replaceable_by_structures(state: &str) -> bool {
    matches!(
        plain(state),
        "minecraft:air"
            | "minecraft:cave_air"
            | "minecraft:void_air"
            | "minecraft:water"
            | "minecraft:lava"
            | "minecraft:glow_lichen"
            | "minecraft:seagrass"
            | "minecraft:tall_seagrass"
    )
}

/// A piece being written into one chunk.
///
/// `clip` is the box writes are limited to — vanilla passes the chunk being
/// generated, so a piece spanning four chunks is written four times, each
/// time with a different clip. Every position outside it is silently
/// dropped, which is also why `get_block` reports **air** out there rather
/// than reading the world: a piece must not make decisions from blocks it
/// cannot see.
pub struct Writer<'a, W: StructureWorld + ?Sized> {
    pub world: &'a mut W,
    pub piece: &'a Piece,
    pub clip: BoundingBox,
    /// Blocks this piece will not overwrite — its own timbers. Empty for a
    /// structure with no such rule.
    pub protected: &'a [&'a str],
}

const fn facing_name(facing: Facing) -> &'static str {
    match facing {
        Facing::North => "north",
        Facing::South => "south",
        Facing::East => "east",
        Facing::West => "west",
    }
}

fn plain(state: &str) -> &str {
    state.split_once('[').map_or(state, |(n, _)| n)
}

impl<W: StructureWorld + ?Sized> Writer<'_, W> {
    fn inside_clip(&self, x: i32, y: i32, z: i32) -> bool {
        let (wx, wy, wz) = self.piece.world_pos(x, y, z);
        wx >= self.clip.min_x
            && wx <= self.clip.max_x
            && wy >= self.clip.min_y
            && wy <= self.clip.max_y
            && wz >= self.clip.min_z
            && wz <= self.clip.max_z
    }

    /// `StructurePiece.getBlock` — **air** outside the clip, not the world's
    /// block. See the struct docs.
    pub fn get_block(&mut self, x: i32, y: i32, z: i32) -> String {
        if !self.inside_clip(x, y, z) {
            return "minecraft:air".to_string();
        }
        let (wx, wy, wz) = self.piece.world_pos(x, y, z);
        self.world.block_at(wx, wy, wz)
    }

    /// `MineShaftPiece.canBeReplaced` — a mineshaft never overwrites its own
    /// planks, logs, fences or chains. The base class returns true for
    /// everything; this is the override, and without it a corridor crossing
    /// an older one eats its supports.
    fn can_be_replaced(&mut self, x: i32, y: i32, z: i32) -> bool {
        if self.protected.is_empty() {
            return true;
        }
        let state = self.get_block(x, y, z);
        let name = plain(&state).to_string();
        !self.protected.iter().any(|p| *p == name)
    }

    /// `StructurePiece.placeBlock`.
    ///
    /// The state is **mirrored then rotated** into the piece's frame before
    /// it is written, in that order. A corridor describes its fence as
    /// connecting west; facing west, that becomes a fence connecting south.
    pub fn place_block(&mut self, state: &str, x: i32, y: i32, z: i32) {
        if !self.inside_clip(x, y, z) {
            return;
        }
        if !self.can_be_replaced(x, y, z) {
            return;
        }
        let oriented = self.orient(state);
        let (wx, wy, wz) = self.piece.world_pos(x, y, z);
        self.world.set_block(wx, wy, wz, &oriented);
        if needs_shape_check(&oriented) {
            self.world.mark_post_processing(wx, wy, wz);
        }
    }

    /// Record what this piece knew about the block entity it just placed —
    /// `docs/DETERMINISM.md` §42.
    ///
    /// Takes **piece-local** coordinates, like [`Self::place_block`] beside
    /// it, and converts them once here. A caller that had to convert first
    /// would be one `world_pos` away from recording a payload at a position
    /// no block of that class occupies, which the emitter would then drop as
    /// an orphan — a silent loss that looks exactly like a class nobody has
    /// ported.
    ///
    /// Unlike `place_block` this does **not** consult the clip or the
    /// protected list. It cannot: a payload is recorded for a block the
    /// caller has already asked to be placed, and re-deciding here would put
    /// two independent copies of that judgement in the tree. What keeps a
    /// record honest when the write was refused is the emitter, which uses a
    /// record only if the finished canvas holds a block of that class at that
    /// position.
    pub fn record_block_entity(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        record: crate::block_entity::Record,
    ) {
        let (wx, wy, wz) = self.piece.world_pos(x, y, z);
        self.world.record_block_entity(wx, wy, wz, record);
    }

    /// `state.mirror(mirror).rotate(rotation)` for the properties a
    /// structure actually places.
    ///
    /// Covers the four fence connections, a `facing`, a rail `shape` and an
    /// `axis` — which is every directional property a mineshaft writes. A
    /// state carrying something else passes through unchanged, and that is a
    /// **named gap** rather than a claim: a structure placing a stair or a
    /// door would need its own case here, and would be wrong without one.
    fn orient(&self, state: &str) -> String {
        let Some(facing) = self.piece.facing else {
            return state.to_string();
        };
        let (mirror, rotation) = facing.frame();
        if mirror == Mirror::None && rotation == Rotation::None {
            return state.to_string();
        }
        let Some((name, rest)) = state.split_once('[') else {
            return state.to_string();
        };
        let pairs: Vec<(String, String)> = rest
            .trim_end_matches(']')
            .split(',')
            .filter_map(|p| {
                p.split_once('=')
                    .map(|(k, v)| (k.to_string(), v.to_string()))
            })
            .collect();

        let moved = |d: Facing| rotation.apply(mirror.apply(d));
        let mut out: Vec<(String, String)> = Vec::new();
        // The four fence booleans move as a set: each connection is carried
        // to wherever its direction ends up.
        let connection_names = ["north", "south", "east", "west"];
        let has_connections = pairs
            .iter()
            .filter(|(k, _)| connection_names.contains(&k.as_str()))
            .count()
            == 4;
        for (key, value) in &pairs {
            if has_connections && connection_names.contains(&key.as_str()) {
                let from = match key.as_str() {
                    "north" => Facing::North,
                    "south" => Facing::South,
                    "east" => Facing::East,
                    _ => Facing::West,
                };
                let to = moved(from);
                out.push((facing_name(to).to_string(), value.clone()));
            } else if key == "facing" {
                let from = match value.as_str() {
                    "north" => Some(Facing::North),
                    "south" => Some(Facing::South),
                    "east" => Some(Facing::East),
                    "west" => Some(Facing::West),
                    _ => None,
                };
                match from {
                    Some(d) => out.push((key.clone(), facing_name(moved(d)).to_string())),
                    // `up`/`down` do not move under a horizontal rotation.
                    None => out.push((key.clone(), value.clone())),
                }
            } else if key == "hinge" && mirror != Mirror::None {
                // `DoorBlock.mirror`: any mirror at all is
                // `rotate(mirror.getRotation(facing)).cycle(HINGE)` — the
                // hinge flips whatever the facing's axis. The stronghold's
                // south- and west-facing doors were its first users: 94
                // `hinge` positions in seed 12345's (110,23)
                // stronghold before this.
                let flipped = match value.as_str() {
                    "left" => "right",
                    "right" => "left",
                    other => other,
                };
                out.push((key.clone(), flipped.to_string()));
            } else if key == "shape"
                && mirror == Mirror::LeftRight
                && name.ends_with("_stairs")
                && pairs
                    .iter()
                    .any(|(k, v)| k == "facing" && (v == "north" || v == "south"))
            {
                // `StairBlock.mirror(LEFT_RIGHT)` on a z-axis stair is
                // `rotate(CLOCKWISE_180)` with left and right exchanged; an
                // x-axis stair falls through to `Block.mirror` and keeps its
                // shape. The facing half is the `facing` arm above. Rotation
                // never touches a stair's shape (`StairBlock.rotate` sets
                // FACING only), so this is the one place a placed stair's
                // shape moves — first exercised by the swamp hut's roof
                // corners, which are the first `outer_*` stairs any piece here
                // places.
                let swapped = match value.as_str() {
                    "inner_left" => "inner_right",
                    "inner_right" => "inner_left",
                    "outer_left" => "outer_right",
                    "outer_right" => "outer_left",
                    other => other,
                };
                out.push((key.clone(), swapped.to_string()));
            } else if key == "shape"
                && rotation == Rotation::Clockwise90
                && !name.ends_with("_stairs")
            {
                let rotated = match value.as_str() {
                    "north_south" => "east_west",
                    "east_west" => "north_south",
                    "ascending_east" => "ascending_south",
                    "ascending_south" => "ascending_west",
                    "ascending_west" => "ascending_north",
                    "ascending_north" => "ascending_east",
                    other => other,
                };
                out.push((key.clone(), rotated.to_string()));
            } else if key == "axis" && rotation == Rotation::Clockwise90 {
                let swapped = match value.as_str() {
                    "x" => "z",
                    "z" => "x",
                    other => other,
                };
                out.push((key.clone(), swapped.to_string()));
            } else {
                out.push((key.clone(), value.clone()));
            }
        }
        // Sorted, because a state's canonical spelling in this port is
        // alphabetical and the connection names were just permuted.
        out.sort();
        let joined: Vec<String> = out.into_iter().map(|(k, v)| format!("{k}={v}")).collect();
        format!("{name}[{}]", joined.join(","))
    }

    /// `StructurePiece.isInterior` — is the position **one above** this one
    /// below the world surface.
    ///
    /// The `y + 1` is the reference's and is not an off-by-one: the question
    /// asked is whether the block above is underground, which is what decides
    /// whether a cobweb is in a cave or hanging in the open air.
    /// The world position of a local coordinate, and whether the clip admits
    /// it — for tracing which of a piece's per-chunk passes actually writes.
    #[must_use]
    pub fn traced_target(&self, x: i32, y: i32, z: i32) -> ((i32, i32, i32), bool) {
        (self.piece.world_pos(x, y, z), self.inside_clip(x, y, z))
    }

    pub fn is_interior(&mut self, x: i32, y: i32, z: i32) -> bool {
        if !self.inside_clip(x, y + 1, z) {
            return false;
        }
        let (wx, wy, wz) = self.piece.world_pos(x, y + 1, z);
        wy < self.world.ocean_floor_wg(wx, wz)
    }

    /// `StructurePiece.generateBox` — `edge` on the shell, `inside` within.
    #[expect(clippy::too_many_arguments, reason = "the reference's own signature")]
    pub fn generate_box(
        &mut self,
        min_x: i32,
        min_y: i32,
        min_z: i32,
        max_x: i32,
        max_y: i32,
        max_z: i32,
        edge: &str,
        inside: &str,
        skip_air: bool,
    ) {
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                for z in min_z..=max_z {
                    if skip_air && is_air(&self.get_block(x, y, z)) {
                        continue;
                    }
                    let on_edge = y == min_y
                        || y == max_y
                        || x == min_x
                        || x == max_x
                        || z == min_z
                        || z == max_z;
                    let state = if on_edge { edge } else { inside };
                    self.place_block(state, x, y, z);
                }
            }
        }
    }

    /// `generateAirBox` — a box of `Blocks.AIR` with no shell distinction.
    ///
    /// It said `cave_air` until the jungle temple became its first caller and
    /// read 323 and 319 `air -> cave_air` inside its two measured boxes:
    /// `StructurePiece.generateAirBox` offset 34 is `getstatic Blocks.AIR`.
    pub fn generate_air_box(
        &mut self,
        min_x: i32,
        min_y: i32,
        min_z: i32,
        max_x: i32,
        max_y: i32,
        max_z: i32,
    ) {
        self.generate_box(
            min_x,
            min_y,
            min_z,
            max_x,
            max_y,
            max_z,
            "minecraft:air",
            "minecraft:air",
            false,
        );
    }

    /// `StructurePiece.generateMaybeBox`.
    ///
    /// One draw per position, taken **first**. See the module docs.
    #[expect(clippy::too_many_arguments, reason = "the reference's own signature")]
    pub fn generate_maybe_box<R: RandomSource + ?Sized>(
        &mut self,
        random: &mut R,
        chance: f32,
        min_x: i32,
        min_y: i32,
        min_z: i32,
        max_x: i32,
        max_y: i32,
        max_z: i32,
        edge: &str,
        inside: &str,
        skip_air: bool,
        require_interior: bool,
    ) {
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                for z in min_z..=max_z {
                    // Unconditional, and before every other test.
                    if random.next_f32() > chance {
                        continue;
                    }
                    if skip_air && is_air(&self.get_block(x, y, z)) {
                        continue;
                    }
                    if require_interior && !self.is_interior(x, y, z) {
                        continue;
                    }
                    let on_edge = y == min_y
                        || y == max_y
                        || x == min_x
                        || x == max_x
                        || z == min_z
                        || z == max_z;
                    let state = if on_edge { edge } else { inside };
                    self.place_block(state, x, y, z);
                }
            }
        }
    }

    /// `StructurePiece.maybeGenerateBlock` — one draw, and the comparison is
    /// strict where the maybe-box's is not.
    pub fn maybe_generate_block<R: RandomSource + ?Sized>(
        &mut self,
        random: &mut R,
        chance: f32,
        x: i32,
        y: i32,
        z: i32,
        state: &str,
    ) {
        if random.next_f32() < chance {
            self.place_block(state, x, y, z);
        }
    }

    /// `setBlock` without the `canBeReplaced` test.
    ///
    /// `setPlanksBlock` and the pillar/chain fill call the world's setter
    /// directly rather than `placeBlock`, so they overwrite a piece's own
    /// timbers where `placeBlock` would not. Two write paths, and which one
    /// a call site uses is behaviour.
    pub fn place_block_ignoring_protection(&mut self, state: &str, x: i32, y: i32, z: i32) {
        if !self.inside_clip(x, y, z) {
            return;
        }
        let (wx, wy, wz) = self.piece.world_pos(x, y, z);
        self.world.set_block(wx, wy, wz, state);
    }

    /// The **world's** block at a local position, with no clipping.
    ///
    /// `getBlock(.., box)` is the usual reader and answers air outside the
    /// box; a handful of call sites go to `level.getBlockState` directly
    /// instead, after gating on `box.isInside` themselves. `createChest` is
    /// one, and it reads the block *below* the gated position — which can
    /// be outside the box while the position it was told about is not.
    pub fn world_block(&mut self, x: i32, y: i32, z: i32) -> String {
        let (wx, wy, wz) = self.piece.world_pos(x, y, z);
        self.world.block_at(wx, wy, wz)
    }

    /// `box.isInside(getWorldPos(x, y, z))`.
    pub fn inside_box(&self, x: i32, y: i32, z: i32) -> bool {
        self.inside_clip(x, y, z)
    }

    /// `BlockState#isSolidRender` for a state `getBlock` already returned.
    pub fn state_is_solid_render(&self, state: &str) -> bool {
        self.world.state_is_solid_render(state)
    }

    /// `state.isFaceSturdy(level, pos, UP)` at a local position, against the
    /// **world's** block rather than the clipped one.
    ///
    /// `setPlanksBlock` reads `level.getBlockState(pos)` directly, not
    /// `getBlock(.., box)`, so this deliberately does not clip. It is
    /// already inside the box: `isInterior` gated the call.
    pub fn is_sturdy_face_up(&mut self, x: i32, y: i32, z: i32) -> bool {
        let (wx, wy, wz) = self.piece.world_pos(x, y, z);
        self.world.is_sturdy_face(wx, wy, wz, 0, 1, 0)
    }

    /// `MineShaftCorridor.hasSturdyNeighbours` — at least `required` of the
    /// six neighbours present a sturdy face towards this position.
    ///
    /// Returns as soon as the count is reached, so it does not visit all six
    /// — irrelevant to the answer, and it is what the reference does.
    pub fn has_sturdy_neighbours(&mut self, x: i32, y: i32, z: i32, required: i32) -> bool {
        let (wx, wy, wz) = self.piece.world_pos(x, y, z);
        let mut found = 0;
        // `Direction.values()` order: down, up, north, south, west, east.
        for (dx, dy, dz) in [
            (0, -1, 0),
            (0, 1, 0),
            (0, 0, -1),
            (0, 0, 1),
            (-1, 0, 0),
            (1, 0, 0),
        ] {
            let (nx, ny, nz) = (wx + dx, wy + dy, wz + dz);
            if nx < self.clip.min_x
                || nx > self.clip.max_x
                || ny < self.clip.min_y
                || ny > self.clip.max_y
                || nz < self.clip.min_z
                || nz > self.clip.max_z
            {
                continue;
            }
            if self.world.is_sturdy_face(nx, ny, nz, -dx, -dy, -dz) {
                found += 1;
                if found >= required {
                    return true;
                }
            }
        }
        false
    }

    /// `MineShaftPiece.isSupportingBox` — nothing along the run one above
    /// `max_y` may be air.
    pub fn is_supporting_box(&mut self, min_x: i32, max_x: i32, max_y: i32, z: i32) -> bool {
        for x in min_x..=max_x {
            if is_air(&self.get_block(x, max_y + 1, z)) {
                return false;
            }
        }
        true
    }

    /// `MineShaftCorridor.fillPillarDownOrChainUp` — walk outwards from a
    /// floor plank until something can hold a pillar below or a chain above.
    ///
    /// Both directions advance together, one step per iteration, and either
    /// can finish the call. The limits are different — 20 down, 50 up — and
    /// the downward walk also refuses to pass through lava, which is what
    /// stops a mine dropping a support into a lava lake.
    pub fn fill_pillar_down_or_chain_up(
        &mut self,
        wood: &str,
        fence: &str,
        chain: &str,
        x: i32,
        y: i32,
        z: i32,
    ) {
        if !self.inside_clip(x, y, z) {
            return;
        }
        let (wx, start_y, wz) = self.piece.world_pos(x, y, z);
        let (mut down, mut up) = (true, true);
        let mut distance = 1;
        while down || up {
            if down {
                let at = start_y - distance;
                let state = self.world.block_at(wx, at, wz);
                let replaceable =
                    is_replaceable_by_structures(&state) && plain(&state) != "minecraft:lava";
                if !replaceable && self.world.is_sturdy_face(wx, at, wz, 0, 1, 0) {
                    for fill in (at + 1)..start_y {
                        self.world.set_block(wx, fill, wz, wood);
                    }
                    return;
                }
                down = distance <= 20 && replaceable && at > self.clip.min_y + 1;
            }
            if up {
                let at = start_y + distance;
                let state = self.world.block_at(wx, at, wz);
                let replaceable = is_replaceable_by_structures(&state);
                if !replaceable && self.world.can_hang_chain_below(wx, at, wz) {
                    self.world.set_block(wx, start_y + 1, wz, fence);
                    for fill in (start_y + 2)..at {
                        self.world.set_block(wx, fill, wz, chain);
                    }
                    return;
                }
                up = distance <= 50 && replaceable && at < self.clip.max_y;
            }
            distance += 1;
        }
    }

    /// `StructurePiece.generateUpperHalfSphere` — the domed ceiling of a
    /// mineshaft room.
    ///
    /// Every value here is `f32`, and the threshold is **1.05**, not 1: the
    /// dome is slightly larger than the ellipsoid it is named after, which
    /// is what stops a ring of blocks being left at the equator. Computing
    /// it in `f64` and comparing at the end gives a subtly different rim.
    #[expect(clippy::too_many_arguments, reason = "the reference's own signature")]
    pub fn generate_upper_half_sphere(
        &mut self,
        min_x: i32,
        min_y: i32,
        min_z: i32,
        max_x: i32,
        max_y: i32,
        max_z: i32,
        state: &str,
        exclude_air: bool,
    ) {
        #[expect(clippy::cast_precision_loss, reason = "a span bounded by a structure")]
        let (x_span, y_span, z_span) = (
            (max_x - min_x + 1) as f32,
            (max_y - min_y + 1) as f32,
            (max_z - min_z + 1) as f32,
        );
        #[expect(clippy::cast_precision_loss, reason = "a structure coordinate")]
        let (centre_x, centre_z) = (min_x as f32 + x_span / 2.0, min_z as f32 + z_span / 2.0);
        for y in min_y..=max_y {
            #[expect(clippy::cast_precision_loss, reason = "a structure coordinate")]
            let fy = (y - min_y) as f32 / y_span;
            for x in min_x..=max_x {
                #[expect(clippy::cast_precision_loss, reason = "a structure coordinate")]
                let fx = (x as f32 - centre_x) / (x_span * 0.5);
                for z in min_z..=max_z {
                    #[expect(clippy::cast_precision_loss, reason = "a structure coordinate")]
                    let fz = (z as f32 - centre_z) / (z_span * 0.5);
                    if exclude_air && is_air(&self.get_block(x, y, z)) {
                        continue;
                    }
                    if fx * fx + fy * fy + fz * fz <= 1.05 {
                        self.place_block(state, x, y, z);
                    }
                }
            }
        }
    }

    /// `MineShaftPiece.isInInvalidLocation` — a mine refuses to build in a
    /// blocking biome, or anywhere its shell would touch liquid.
    ///
    /// The shell is the piece grown by one on every side and then **clipped
    /// to the chunk**, so the answer depends on which chunk is being written
    /// — a piece can be invalid in one of its chunks and fine in another.
    /// That is the reference's behaviour, not an approximation of it.
    pub fn is_in_invalid_location(&mut self) -> bool {
        let bb = self.piece.bounding_box;
        let min_x = (bb.min_x - 1).max(self.clip.min_x);
        let min_y = (bb.min_y - 1).max(self.clip.min_y);
        let min_z = (bb.min_z - 1).max(self.clip.min_z);
        let max_x = (bb.max_x + 1).min(self.clip.max_x);
        let max_y = (bb.max_y + 1).min(self.clip.max_y);
        let max_z = (bb.max_z + 1).min(self.clip.max_z);

        // `RUSTMC_TRACE_MINESHAFT=1` reports every piece this refuses and
        // why. A refusal returns before any draw is taken, so one piece
        // refused on one side and placed on the other shifts every later
        // piece's random stream — which is a whole-corridor difference that
        // looks nothing like a bug in the piece that shows it.
        //
        // **All four refusal paths report.** Two of them used to return
        // `true` silently, so the trace could only ever name a biome refusal
        // or a y-face one, and "the trace reports nothing" was recorded as
        // "we refuse no piece" — which it did not establish. Reconciling this
        // against `worldgen-dump mineshaft-ceiling`, which reads refusals out
        // of the blocks instead, is what exposed it: the blocks said eight
        // and the trace said six.
        let trace = trace_mineshaft();
        let refuse = |reason: &str, clip: BoundingBox| {
            if trace {
                println!(
                    "MINESHAFT chunk ({}, {}) refuse ({reason}) box {},{},{}..{},{},{}",
                    clip.min_x >> 4,
                    clip.min_z >> 4,
                    bb.min_x,
                    bb.min_y,
                    bb.min_z,
                    bb.max_x,
                    bb.max_y,
                    bb.max_z
                );
            }
            true
        };

        if self.world.biome_blocks_mineshafts(
            (min_x + max_x) / 2,
            (min_y + max_y) / 2,
            (min_z + max_z) / 2,
        ) {
            return refuse("biome", self.clip);
        }
        // The six faces, in the reference's order: bottom/top, then
        // north/south, then west/east. Every one is world coordinates —
        // unlike everything else here, this walks the box directly.
        for x in min_x..=max_x {
            for z in min_z..=max_z {
                if self.world.is_liquid(x, min_y, z) || self.world.is_liquid(x, max_y, z) {
                    return refuse(&format!("liquid y-face at {x},{z}"), self.clip);
                }
            }
        }
        for x in min_x..=max_x {
            for y in min_y..=max_y {
                if self.world.is_liquid(x, y, min_z) || self.world.is_liquid(x, y, max_z) {
                    return refuse(&format!("liquid z-face at {x},{y}"), self.clip);
                }
            }
        }
        for z in min_z..=max_z {
            for y in min_y..=max_y {
                if self.world.is_liquid(min_x, y, z) || self.world.is_liquid(max_x, y, z) {
                    return refuse(&format!("liquid x-face at {y},{z}"), self.clip);
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;
    use crate::structure::pieces::Facing;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Flat {
        blocks: BTreeMap<(i32, i32, i32), String>,
        surface: i32,
        blocking: bool,
        liquid: Vec<(i32, i32, i32)>,
    }

    impl crate::block_entity::BlockEntityRecorder for Flat {
        fn record_block_entity(
            &mut self,
            _x: i32,
            _y: i32,
            _z: i32,
            _record: crate::block_entity::Record,
        ) {
            // These tests are about placeBlock's orientation and clipping, and
            // no case in this module places a block that carries an entity.
            // Dropped explicitly rather than by inheriting a default no-op.
        }
    }

    impl StructureWorld for Flat {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.blocks
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:stone".to_string())
        }
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
            self.blocks.insert((x, y, z), state.to_string());
        }
        fn ocean_floor_wg(&mut self, _: i32, _: i32) -> i32 {
            self.surface
        }
        fn biome_blocks_mineshafts(&mut self, _: i32, _: i32, _: i32) -> bool {
            self.blocking
        }
        fn is_liquid(&mut self, x: i32, y: i32, z: i32) -> bool {
            self.liquid.contains(&(x, y, z))
        }
        fn is_sturdy_face(&mut self, x: i32, y: i32, z: i32, _: i32, _: i32, _: i32) -> bool {
            self.block_at(x, y, z) == "minecraft:stone"
        }
        fn state_is_solid_render(&self, state: &str) -> bool {
            state == "minecraft:stone"
        }
        fn can_hang_chain_below(&mut self, x: i32, y: i32, z: i32) -> bool {
            self.block_at(x, y, z) == "minecraft:stone"
        }
    }

    fn piece() -> Piece {
        Piece {
            id: "test",
            bounding_box: BoundingBox::new(0, 60, 0, 2, 62, 9),
            facing: Some(Facing::South),
            depth: 0,
        }
    }

    const EVERYWHERE: BoundingBox = BoundingBox::new(-64, -64, -64, 64, 320, 64);

    #[test]
    fn a_generated_box_puts_the_edge_state_on_the_shell() {
        let mut world = Flat::default();
        let p = piece();
        let mut w = Writer {
            world: &mut world,
            piece: &p,
            clip: EVERYWHERE,
            protected: &[],
        };
        w.generate_box(
            0,
            0,
            0,
            2,
            2,
            2,
            "minecraft:oak_planks",
            "minecraft:cave_air",
            false,
        );
        // A 3x3x3 has exactly one interior block.
        let inside = world.block_at(1, 61, 1);
        assert_eq!(
            inside, "minecraft:cave_air",
            "the middle is the inside state"
        );
        assert_eq!(world.block_at(0, 60, 0), "minecraft:oak_planks", "a corner");
        assert_eq!(world.block_at(1, 60, 1), "minecraft:oak_planks", "a face");
    }

    /// The draw count is the volume, whatever gets written.
    #[test]
    fn a_maybe_box_draws_once_per_position_regardless() {
        let run = |chance: f32| {
            let mut world = Flat::default();
            let p = piece();
            let mut random = WorldgenRandom::new(LegacyRandomSource::new(7));
            let mut w = Writer {
                world: &mut world,
                piece: &p,
                clip: EVERYWHERE,
                protected: &[],
            };
            w.generate_maybe_box(
                &mut random,
                chance,
                0,
                0,
                0,
                2,
                2,
                4,
                "minecraft:cobweb",
                "minecraft:cobweb",
                false,
                false,
            );
            let placed = world
                .blocks
                .values()
                .filter(|b| *b == "minecraft:cobweb")
                .count();
            (random.next_i64(), placed)
        };
        let (stream_none, placed_none) = run(0.0);
        let (stream_all, placed_all) = run(1.0);
        assert_eq!(placed_none, 0, "nothing should be placed at chance 0");
        assert_eq!(placed_all, 45, "3x3x5 should all be placed at chance 1");
        assert_eq!(
            stream_none, stream_all,
            "the stream must end in the same place whether or not anything was written"
        );
    }

    /// ...and moving the cheap tests before the draw would break exactly
    /// that. Skipping air still costs the draw.
    #[test]
    fn skipping_air_still_costs_the_draw() {
        let mut world = Flat::default();
        // Make the whole area air, so every position is skipped.
        for y in 60..=62 {
            for x in 0..=2 {
                for z in 0..=4 {
                    world.blocks.insert((x, y, z), "minecraft:air".to_string());
                }
            }
        }
        let p = piece();
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(7));
        {
            let mut w = Writer {
                world: &mut world,
                piece: &p,
                clip: EVERYWHERE,
                protected: &[],
            };
            w.generate_maybe_box(
                &mut random,
                1.0,
                0,
                0,
                0,
                2,
                2,
                4,
                "minecraft:cobweb",
                "minecraft:cobweb",
                true,
                false,
            );
        }
        assert!(
            !world.blocks.values().any(|b| b == "minecraft:cobweb"),
            "everything was air, so nothing should be written"
        );
        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(7));
        for _ in 0..45 {
            probe.next_f32();
        }
        assert_eq!(random.next_i64(), probe.next_i64(), "45 draws all the same");
    }

    /// The two chance comparisons differ at equality. Pinned with a
    /// hand-built source so the boundary is actually hit.
    #[test]
    fn the_two_chance_comparisons_disagree_at_equality() {
        // `generateMaybeBox` places when nextFloat() <= chance; a chance of
        // 1.0 therefore always places, since nextFloat() < 1.0 always.
        // `maybeGenerateBlock` places when nextFloat() < chance. The visible
        // difference is at chance 0: a nextFloat() of exactly 0.0 places in
        // the box form and does not in the single-block form.
        let mut world = Flat::default();
        let p = piece();
        struct Zero;
        impl crate::rng::RandomSource for Zero {
            fn next_i32(&mut self) -> i32 {
                0
            }
            fn next_i32_bound(&mut self, _: i32) -> i32 {
                0
            }
            fn next_i64(&mut self) -> i64 {
                0
            }
            fn next_bool(&mut self) -> bool {
                false
            }
            fn next_f32(&mut self) -> f32 {
                0.0
            }
            fn next_f64(&mut self) -> f64 {
                0.0
            }
            fn next_gaussian(&mut self) -> f64 {
                0.0
            }
            fn set_seed(&mut self, _: i64) {}
        }
        let mut w = Writer {
            world: &mut world,
            piece: &p,
            clip: EVERYWHERE,
            protected: &[],
        };
        w.generate_maybe_box(
            &mut Zero,
            0.0,
            0,
            0,
            0,
            0,
            0,
            0,
            "minecraft:cobweb",
            "minecraft:cobweb",
            false,
            false,
        );
        w.maybe_generate_block(&mut Zero, 0.0, 0, 1, 0, "minecraft:torch");
        assert_eq!(
            world.block_at(0, 60, 0),
            "minecraft:cobweb",
            "the box form places at nextFloat() == chance"
        );
        assert_eq!(
            world.block_at(0, 61, 0),
            "minecraft:stone",
            "the single-block form does not"
        );
    }

    /// A piece does not overwrite its own timbers.
    #[test]
    fn protected_blocks_are_not_replaced() {
        let mut world = Flat::default();
        world
            .blocks
            .insert((1, 61, 1), "minecraft:oak_fence[north=true]".to_string());
        let p = piece();
        let mut w = Writer {
            world: &mut world,
            piece: &p,
            clip: EVERYWHERE,
            protected: &["minecraft:oak_planks", "minecraft:oak_fence"],
        };
        w.place_block("minecraft:cave_air", 1, 1, 1);
        w.place_block("minecraft:cave_air", 0, 1, 1);
        assert!(
            world.block_at(1, 61, 1).starts_with("minecraft:oak_fence"),
            "the fence survives — and the property spelling must not matter"
        );
        assert_eq!(
            world.block_at(0, 61, 1),
            "minecraft:cave_air",
            "stone does not"
        );
    }

    /// Writes outside the clip are dropped, and reads outside it report air
    /// rather than the world.
    #[test]
    fn the_clip_bounds_both_reading_and_writing() {
        let mut world = Flat::default();
        let p = piece();
        let mut w = Writer {
            world: &mut world,
            piece: &p,
            // One chunk only: x 0..15, z 0..3.
            clip: BoundingBox::new(0, -64, 0, 15, 320, 3),
            protected: &[],
        };
        assert_eq!(
            w.get_block(0, 0, 9),
            "minecraft:air",
            "outside the clip reads as air, not as the stone that is there"
        );
        w.place_block("minecraft:cave_air", 0, 0, 9);
        assert_eq!(
            world.block_at(0, 60, 9),
            "minecraft:stone",
            "and the write was dropped"
        );
    }

    /// A blocking biome refuses the whole piece, before any liquid is
    /// checked.
    #[test]
    fn a_blocking_biome_invalidates_the_location() {
        let mut world = Flat {
            blocking: true,
            ..Flat::default()
        };
        let p = piece();
        let mut w = Writer {
            world: &mut world,
            piece: &p,
            clip: EVERYWHERE,
            protected: &[],
        };
        assert!(w.is_in_invalid_location());
    }

    /// So does liquid anywhere on the grown shell — including one block
    /// outside the piece, which is the point of growing it.
    #[test]
    fn liquid_touching_the_shell_invalidates_the_location() {
        let p = piece();
        let bb = p.bounding_box;
        let mut clean = Flat::default();
        assert!(!Writer {
            world: &mut clean,
            piece: &p,
            clip: EVERYWHERE,
            protected: &[],
        }
        .is_in_invalid_location());

        let mut wet = Flat {
            // One block beyond the piece's own corner.
            liquid: vec![(bb.min_x - 1, bb.min_y - 1, bb.min_z - 1)],
            ..Flat::default()
        };
        assert!(
            Writer {
                world: &mut wet,
                piece: &p,
                clip: EVERYWHERE,
                protected: &[],
            }
            .is_in_invalid_location(),
            "liquid one block outside the piece still invalidates it"
        );
    }

    /// `isInterior` asks about the block **above**.
    #[test]
    fn interior_is_decided_one_block_up() {
        let mut world = Flat {
            // Surface at y=62: local y=1 maps to world 61, and the question
            // is asked about 62 — not below it.
            surface: 62,
            ..Flat::default()
        };
        let p = piece();
        let mut w = Writer {
            world: &mut world,
            piece: &p,
            clip: EVERYWHERE,
            protected: &[],
        };
        assert!(w.is_interior(0, 0, 0), "world y 61 < 62");
        assert!(!w.is_interior(0, 1, 0), "world y 62 is not < 62");
    }
}
