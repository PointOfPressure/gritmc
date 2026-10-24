//! The nether's own decoration features — the five types no other dimension
//! reaches, plus the two `BlockPos` iterators they are written against.
//!
//! Reference (26.2 bytecode, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `GlowstoneFeature`, `BasaltPillarFeature`, `ReplaceBlobsFeature`,
//! `DeltaFeature`, `BasaltColumnsFeature`, and `BlockPos$3` / `BlockPos$4`,
//! the anonymous iterators behind `withinManhattan` and `betweenClosed`.
//!
//! # Why one module and one world trait
//!
//! Every other feature family in this port gets its own trait because each
//! asks a *differently named* question. These five do not: they ask the same
//! four — what block is here, write this block, where does the world end,
//! where is the lava ocean. Five traits with identical bodies would record a
//! distinction that does not exist.
//!
//! None of them is fallible, either, and that is worth saying because most of
//! this port's features are. `BlockState#isAir` is exactly three blocks —
//! `air`, `cave_air`, `void_air`, the only ones vanilla builds with
//! `Properties#air` — so the air test here is a name test rather than a
//! lookup into the block-property table, the same call [`super::ore`] makes
//! and for the same reason. Everything else these five read is a block name.
//!
//! # The iterators are the load-bearing part
//!
//! Three of the five walk a `BlockPos` iterable, and two of those write as
//! they walk, so the *order* is behaviour and not an implementation detail:
//!
//! * `withinManhattan` is an expanding diamond — depth 0, then 1, then 2 —
//!   and within a depth it walks x outward from `-min(reachX, depth)`, then
//!   y, emitting the `+z` and `-z` mirror of each point back to back. It is
//!   **not** a box scan, and a box scan sorted by distance is not it either:
//!   `ReplaceBlobsFeature` and `DeltaFeature` both `break` the moment the
//!   Manhattan distance exceeds `max(radii)`, which only prunes the right
//!   points because the iterator is monotonic in depth.
//! * `betweenClosed` is x fastest, then y, then z. `BasaltColumnsFeature`
//!   writes basalt inside that loop and reads it back at a later position,
//!   so a different traversal is a different column.
//!
//! Both yield the same mutable cursor in vanilla. Nothing here retains a
//! position past the iteration that produced it, so they yield values.

use super::block_predicate::PredicateWorld;
use super::int_provider::IntProvider;
use crate::rng::RandomSource;

/// The world these features read and write.
pub trait NetherWorld: PredicateWorld {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str);

    /// `LevelHeightAccessor#getMinY`.
    fn min_y(&self) -> i32;

    /// `LevelHeightAccessor#getMaxY` — **inclusive**, so `minY + height - 1`.
    fn max_y(&self) -> i32;

    /// `ChunkGenerator#getSeaLevel`. In the nether this is the lava ocean,
    /// at y 32, and it is a real boundary rather than a decoration: a column
    /// grows through lava below it and refuses to grow through lava above.
    fn sea_level(&mut self) -> i32;
}

// ---------------------------------------------------------------------------
// BlockPos iterators
// ---------------------------------------------------------------------------

/// `BlockPos.withinManhattan(origin, reachX, reachY, reachZ)` — `BlockPos$3`.
///
/// Every position within `reachX`/`reachY`/`reachZ` on its own axis whose
/// Manhattan distance from the origin is at most their sum, in order of
/// increasing distance.
#[derive(Debug, Clone)]
pub struct WithinManhattan {
    origin: (i32, i32, i32),
    reach_x: i32,
    reach_y: i32,
    reach_z: i32,
    max_depth: i32,
    depth: i32,
    max_x: i32,
    max_y: i32,
    x: i32,
    y: i32,
    z_mirror: bool,
    cursor: (i32, i32, i32),
}

impl WithinManhattan {
    #[must_use]
    pub const fn new(origin: (i32, i32, i32), reach_x: i32, reach_y: i32, reach_z: i32) -> Self {
        Self {
            origin,
            reach_x,
            reach_y,
            reach_z,
            max_depth: reach_x + reach_y + reach_z,
            // Every counter starts at zero, which is why the *first* call
            // falls straight past `y > maxY` and emits the origin.
            depth: 0,
            max_x: 0,
            max_y: 0,
            x: 0,
            y: 0,
            z_mirror: false,
            cursor: origin,
        }
    }
}

impl Iterator for WithinManhattan {
    type Item = (i32, i32, i32);

    fn next(&mut self) -> Option<Self::Item> {
        // The mirrored point comes out on the call *after* its twin, and is
        // computed from the cursor rather than from the counters — which is
        // why the cursor is state and not a local.
        if self.z_mirror {
            self.z_mirror = false;
            self.cursor.2 = self.origin.2 - (self.cursor.2 - self.origin.2);
            return Some(self.cursor);
        }
        loop {
            if self.y > self.max_y {
                self.x += 1;
                if self.x > self.max_x {
                    self.depth += 1;
                    if self.depth > self.max_depth {
                        return None;
                    }
                    self.max_x = self.reach_x.min(self.depth);
                    self.x = -self.max_x;
                }
                self.max_y = self.reach_y.min(self.depth - self.x.abs());
                self.y = -self.max_y;
            }
            let (x, y) = (self.x, self.y);
            let z = self.depth - x.abs() - y.abs();
            let mut out = None;
            if z <= self.reach_z {
                self.z_mirror = z != 0;
                self.cursor = (self.origin.0 + x, self.origin.1 + y, self.origin.2 + z);
                out = Some(self.cursor);
            }
            self.y += 1;
            if out.is_some() {
                return out;
            }
        }
    }
}

/// `BlockPos.betweenClosed(x1, y1, z1, x2, y2, z2)` — `BlockPos$4`. X fastest,
/// then Y, then Z, both ends inclusive.
#[derive(Debug, Clone)]
pub struct BetweenClosed {
    min: (i32, i32, i32),
    width: i32,
    height: i32,
    index: i32,
    end: i32,
}

impl BetweenClosed {
    #[must_use]
    pub const fn new(min: (i32, i32, i32), max: (i32, i32, i32)) -> Self {
        let width = max.0 - min.0 + 1;
        let height = max.1 - min.1 + 1;
        let depth = max.2 - min.2 + 1;
        Self {
            min,
            width,
            height,
            index: 0,
            end: width * height * depth,
        }
    }
}

impl Iterator for BetweenClosed {
    type Item = (i32, i32, i32);

    fn next(&mut self) -> Option<Self::Item> {
        if self.index == self.end {
            return None;
        }
        let dx = self.index % self.width;
        let rest = self.index / self.width;
        let dy = rest % self.height;
        let dz = rest / self.height;
        self.index += 1;
        Some((self.min.0 + dx, self.min.1 + dy, self.min.2 + dz))
    }
}

/// `Vec3i#distManhattan`.
const fn dist_manhattan(a: (i32, i32, i32), b: (i32, i32, i32)) -> i32 {
    (a.0 - b.0).abs() + (a.1 - b.1).abs() + (a.2 - b.2).abs()
}

/// The registry name of a state string, without its properties.
fn plain(state: &str) -> &str {
    state.split_once('[').map_or(state, |(name, _)| name)
}

/// `BlockState#isAir` — three blocks, not one. See [`super::ore`] for what
/// getting this wrong cost there.
fn is_air_name(name: &str) -> bool {
    matches!(
        name,
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}

/// `BlockGetter#isEmptyBlock`.
fn empty<W: NetherWorld + ?Sized>(world: &mut W, x: i32, y: i32, z: i32) -> bool {
    let state = world.block_at(x, y, z);
    is_air_name(plain(&state))
}

/// `Blocks.GLOWSTONE.defaultBlockState()`. No properties, so the name is the
/// state.
const GLOWSTONE: &str = "minecraft:glowstone";

/// `Blocks.BASALT.defaultBlockState()` — **with its axis**.
///
/// Basalt is a `RotatedPillarBlock`, so its default state is
/// `basalt[axis=y]`, and the canvas stores canonical state strings. Writing
/// the bare name produces a state that is not a state: it compares unequal to
/// vanilla's basalt everywhere it is written *and* unequal to the
/// `basalt[axis=y]` that `basalt_blobs` lays down through the ordinary
/// `state_spec::canonical` path, so the two features stop recognising each
/// other's output as well as vanilla's.
///
/// Measured: the bare name cost **47,718 blocks** over the r=7 nether window
/// (seed 20260722, centre (64,64), 225 chunks, 7,372,800 blocks, y 0..127) —
/// more than every one of these five features is worth put together. The
/// features taking a config never had the bug, because their states come from
/// the datapack through `canonical`; the three that take
/// `NoneFeatureConfiguration` have no datapack state to canonicalise and so
/// are the only place a hand-written name can appear at all.
const BASALT: &str = "minecraft:basalt[axis=y]";

/// `Blocks.BASALT` itself, for a `BlockState#is(Block)` test — which compares
/// blocks and ignores the axis, so it must not be [`BASALT`].
const BASALT_BLOCK: &str = "minecraft:basalt";

// ---------------------------------------------------------------------------
// glowstone_blob
// ---------------------------------------------------------------------------

/// `GlowstoneFeature#place`.
///
/// One seed block hung under a netherrack/basalt/blackstone ceiling, then
/// **1,500** attempts to grow it. Every attempt draws five times whether or
/// not it places anything, so the body costs a flat 7,500 draws and the two
/// refusals before it are the only cheap exit.
///
/// The growth rule is "exactly one glowstone neighbour", which is what makes
/// a blob a blob rather than a cube: a candidate touching two blocks that are
/// already glowstone is rejected, so the deposit branches instead of filling.
pub fn place_glowstone<W: NetherWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    random: &mut R,
    origin: (i32, i32, i32),
) -> bool {
    let (ox, oy, oz) = origin;
    if !empty(world, ox, oy, oz) {
        return false;
    }
    let above = world.block_at(ox, oy + 1, oz);
    if !matches!(
        plain(&above),
        "minecraft:netherrack" | "minecraft:basalt" | "minecraft:blackstone"
    ) {
        return false;
    }
    world.set_block(ox, oy, oz, GLOWSTONE);

    for _ in 0..1500 {
        // Five draws, always, before anything is tested. `-nextInt(12)` is
        // the one asymmetric axis: a blob only grows downward from its
        // ceiling.
        let dx = random.next_i32_bound(8) - random.next_i32_bound(8);
        let dy = -random.next_i32_bound(12);
        let dz = random.next_i32_bound(8) - random.next_i32_bound(8);
        let (x, y, z) = (ox + dx, oy + dy, oz + dz);
        if !empty(world, x, y, z) {
            continue;
        }
        let mut touching = 0;
        for face in 0..6 {
            let (nx, ny, nz) = super::multiface_growth::offset_of((x, y, z), face);
            let state = world.block_at(nx, ny, nz);
            if plain(&state) == GLOWSTONE {
                touching += 1;
            }
            if touching > 1 {
                break;
            }
        }
        if touching == 1 {
            world.set_block(x, y, z, GLOWSTONE);
        }
    }
    true
}

// ---------------------------------------------------------------------------
// basalt_pillar
// ---------------------------------------------------------------------------

/// `BasaltPillarFeature#place`.
///
/// A column of basalt dropped from a two-block gap until it lands, with a
/// ragged skirt hung off its four sides. The four skirt flags are
/// **short-circuited** — once a side stops hanging it never draws again — so
/// the draw count depends on the terrain and not only on the drop height,
/// which is why the flags are carried rather than recomputed per level.
pub fn place_basalt_pillar<W: NetherWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    random: &mut R,
    origin: (i32, i32, i32),
) -> bool {
    let (ox, oy, oz) = origin;
    // **The origin is air and the block ABOVE it is solid.** A basalt pillar
    // hangs from a ceiling and grows down; it does not start in open air.
    //
    // The port had the second test inverted — it asked for two empty blocks —
    // and that is why this feature was disabled rather than fixed. Starting a
    // pillar anywhere with two blocks of headroom lets it begin high in a
    // cavern and fall until it hits something, so the drop loop below ran
    // ~134 blocks instead of ~7. Measured over the r=7 nether window, seed
    // 20260722: our pillars produced 48,115 basalt against vanilla's ~2,400.
    //
    // `BasaltPillarFeature.place`, offsets 16-41:
    //
    //   16: isEmptyBlock(pos)          23: ifeq 39   -> not empty, return false
    //   26: isEmptyBlock(pos.above())  36: ifeq 41   -> NOT empty, carry on
    //   39: iconst_0; ireturn                        -> above IS empty, refuse
    //
    // The fall-through at 36 is the whole rule: reaching `return false` needs
    // `isEmptyBlock(above)` to be **true**.
    if !empty(world, ox, oy, oz) || empty(world, ox, oy + 1, oz) {
        return false;
    }

    let mut y = oy;
    let (mut north, mut south, mut west, mut east) = (true, true, true, true);
    while empty(world, ox, y, oz) {
        if y < world.min_y() || y > world.max_y() {
            return true;
        }
        world.set_block(ox, y, oz, BASALT);
        north = north && hang_off(world, random, (ox, y, oz - 1));
        south = south && hang_off(world, random, (ox, y, oz + 1));
        west = west && hang_off(world, random, (ox - 1, y, oz));
        east = east && hang_off(world, random, (ox + 1, y, oz));
        y -= 1;
    }

    // Back up onto the last block written — the base skirt goes around the
    // bottom of the pillar, not around the floor it landed on — then down
    // again, because the apron below is laid at the floor's level.
    y += 1;
    base_hang_off(world, random, (ox, y, oz - 1));
    base_hang_off(world, random, (ox, y, oz + 1));
    base_hang_off(world, random, (ox - 1, y, oz));
    base_hang_off(world, random, (ox + 1, y, oz));
    y -= 1;

    // The 7x7 apron. `nextInt(10)` is drawn for all 49 cells whether or not
    // one can pass: `|dx| * |dz|` is 9 at a corner, so a corner needs
    // `nextInt(10) < 1` and almost never gets it, but it still costs a draw.
    for dx in -3_i32..4 {
        for dz in -3_i32..4 {
            let weight = dx.abs() * dz.abs();
            if random.next_i32_bound(10) >= 10 - weight {
                continue;
            }
            let (x, mut cursor_y, z) = (ox + dx, y, oz + dz);
            let mut steps = 3;
            while empty(world, x, cursor_y - 1, z) {
                cursor_y -= 1;
                steps -= 1;
                if steps <= 0 {
                    break;
                }
            }
            if !empty(world, x, cursor_y - 1, z) {
                world.set_block(x, cursor_y, z, BASALT);
            }
        }
    }
    true
}

/// `BasaltPillarFeature#placeHangOff` — nine times in ten.
fn hang_off<W: NetherWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    random: &mut R,
    pos: (i32, i32, i32),
) -> bool {
    if random.next_i32_bound(10) != 0 {
        world.set_block(pos.0, pos.1, pos.2, BASALT);
        return true;
    }
    false
}

/// `BasaltPillarFeature#placeBaseHangOff` — a coin flip, and no return value:
/// the base skirt is the last thing that reads the flags, so nothing depends
/// on whether it landed.
fn base_hang_off<W: NetherWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    random: &mut R,
    pos: (i32, i32, i32),
) {
    if random.next_bool() {
        world.set_block(pos.0, pos.1, pos.2, BASALT);
    }
}

// ---------------------------------------------------------------------------
// netherrack_replace_blobs
// ---------------------------------------------------------------------------

/// `ReplaceSphereConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplaceSphereConfig {
    /// The block the blob eats.
    pub target: String,
    /// The block it leaves.
    pub state: String,
    pub radius: IntProvider,
}

impl ReplaceSphereConfig {
    /// # Errors
    /// If a field is missing or a state is not in the block-state extract.
    pub fn parse(
        config: &serde_json::Value,
        states: &dyn super::state_spec::DefaultStates,
    ) -> Result<Self, String> {
        let state_field = |name: &str| -> Result<String, String> {
            super::state_spec::canonical(
                config
                    .get(name)
                    .ok_or_else(|| format!("replace_blobs has no `{name}`"))?,
                states,
            )
        };
        Ok(Self {
            target: state_field("target")?,
            state: state_field("state")?,
            radius: IntProvider::parse(
                config
                    .get("radius")
                    .ok_or_else(|| "replace_blobs has no `radius`".to_string())?,
            )?,
        })
    }
}

/// `ReplaceBlobsFeature#place`.
///
/// Walks **down** from the origin looking for the target block, then eats a
/// Manhattan ball of it. The search happens before any sampling, so a blob
/// that finds no netherrack costs **zero draws** — which is exactly why this
/// feature's refusal moved ore in the measured window instead of only
/// omitting basalt: a refusal that would have drawn nothing still consumed
/// its index in the step, but one that would have drawn three left the whole
/// remainder of the step reading a shifted stream.
pub fn place_replace_blobs<W: NetherWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    config: &ReplaceSphereConfig,
    random: &mut R,
    origin: (i32, i32, i32),
) -> bool {
    let target = plain(&config.target).to_string();

    // `origin.mutable().clamp(Y, minY + 1, maxY)` — the clamp is on where the
    // search *starts*, and `findTarget` then refuses to descend past
    // `minY + 1`, so the bedrock floor is never a blob's target.
    let start_y = origin.1.clamp(world.min_y() + 1, world.max_y());
    let Some(found) = find_target(world, (origin.0, start_y, origin.2), &target) else {
        return false;
    };

    let rx = config.radius.sample(random);
    let ry = config.radius.sample(random);
    let rz = config.radius.sample(random);
    let max_reach = rx.max(ry.max(rz));

    let mut placed = false;
    for pos in WithinManhattan::new(found, rx, ry, rz) {
        if dist_manhattan(pos, found) > max_reach {
            break;
        }
        let state = world.block_at(pos.0, pos.1, pos.2);
        if plain(&state) == target {
            world.set_block(pos.0, pos.1, pos.2, &config.state);
            placed = true;
        }
    }
    placed
}

/// `ReplaceBlobsFeature#findTarget`.
fn find_target<W: NetherWorld + ?Sized>(
    world: &mut W,
    from: (i32, i32, i32),
    target: &str,
) -> Option<(i32, i32, i32)> {
    let (x, mut y, z) = from;
    while y > world.min_y() + 1 {
        let state = world.block_at(x, y, z);
        if plain(&state) == target {
            return Some((x, y, z));
        }
        y -= 1;
    }
    None
}

// ---------------------------------------------------------------------------
// delta_feature
// ---------------------------------------------------------------------------

/// `DeltaFeatureConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct DeltaConfig {
    pub contents: String,
    pub rim: String,
    pub size: IntProvider,
    pub rim_size: IntProvider,
}

impl DeltaConfig {
    /// # Errors
    /// If a field is missing or a state is not in the block-state extract.
    pub fn parse(
        config: &serde_json::Value,
        states: &dyn super::state_spec::DefaultStates,
    ) -> Result<Self, String> {
        let state_field = |name: &str| -> Result<String, String> {
            super::state_spec::canonical(
                config
                    .get(name)
                    .ok_or_else(|| format!("delta_feature has no `{name}`"))?,
                states,
            )
        };
        let provider = |name: &str| -> Result<IntProvider, String> {
            IntProvider::parse(
                config
                    .get(name)
                    .ok_or_else(|| format!("delta_feature has no `{name}`"))?,
            )
        };
        Ok(Self {
            contents: state_field("contents")?,
            rim: state_field("rim")?,
            size: provider("size")?,
            rim_size: provider("rim_size")?,
        })
    }
}

/// The blocks a delta refuses to overwrite — `DeltaFeature.CANNOT_REPLACE`.
const DELTA_CANNOT_REPLACE: [&str; 7] = [
    "minecraft:bedrock",
    "minecraft:nether_bricks",
    "minecraft:nether_brick_fence",
    "minecraft:nether_brick_stairs",
    "minecraft:nether_wart",
    "minecraft:chest",
    "minecraft:spawner",
];

/// `DeltaFeature#place`.
///
/// A flat lava pool with a magma rim, drawn as **two offset copies of one
/// diamond**: the rim goes at `p` and the lava at `p + (rimX, 0, rimZ)`. The
/// offset is sampled once for the whole feature, so the pool sits *beside*
/// its rim rather than inside it, which is where basalt deltas get their
/// crescents.
///
/// The rim samples are drawn whenever the 0.9 roll passes, including when the
/// result cannot be used: `rimActive` needs both offsets non-zero, and a zero
/// offset costs its draw regardless.
pub fn place_delta<W: NetherWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    config: &DeltaConfig,
    random: &mut R,
    origin: (i32, i32, i32),
) -> bool {
    let has_rim = random.next_f64() < 0.9;
    let rim_x = if has_rim {
        config.rim_size.sample(random)
    } else {
        0
    };
    let rim_z = if has_rim {
        config.rim_size.sample(random)
    } else {
        0
    };
    let rim_active = has_rim && rim_x != 0 && rim_z != 0;

    let size_x = config.size.sample(random);
    let size_z = config.size.sample(random);
    let max_reach = size_x.max(size_z);

    let contents = plain(&config.contents).to_string();
    let mut placed = false;
    for pos in WithinManhattan::new(origin, size_x, 0, size_z) {
        if dist_manhattan(pos, origin) > max_reach {
            break;
        }
        if !delta_is_clear(world, pos, &contents) {
            continue;
        }
        if rim_active {
            placed = true;
            world.set_block(pos.0, pos.1, pos.2, &config.rim);
        }
        let inner = (pos.0 + rim_x, pos.1, pos.2 + rim_z);
        if delta_is_clear(world, inner, &contents) {
            placed = true;
            world.set_block(inner.0, inner.1, inner.2, &config.contents);
        }
    }
    placed
}

/// `DeltaFeature#isClear`.
///
/// The neighbour loop is a **roof** test written as two exclusions: air on
/// any side other than UP disqualifies the position, and anything *but* air
/// above it disqualifies it too. So a delta only spreads through solid ground
/// that is open above — the floor of a cavern, never a wall of one.
fn delta_is_clear<W: NetherWorld + ?Sized>(
    world: &mut W,
    pos: (i32, i32, i32),
    contents: &str,
) -> bool {
    let here = world.block_at(pos.0, pos.1, pos.2);
    let here = plain(&here);
    if here == contents {
        return false;
    }
    if DELTA_CANNOT_REPLACE.contains(&here) {
        return false;
    }
    for face in 0..6 {
        let (nx, ny, nz) = super::multiface_growth::offset_of(pos, face);
        let state = world.block_at(nx, ny, nz);
        let air = is_air_name(plain(&state));
        // Face 1 is UP in `Direction.values()` order.
        let up = face == 1;
        if air != up {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// basalt_columns
// ---------------------------------------------------------------------------

/// `ColumnFeatureConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnConfig {
    pub reach: IntProvider,
    pub height: IntProvider,
}

impl ColumnConfig {
    /// # Errors
    /// If a field is missing or malformed.
    pub fn parse(config: &serde_json::Value) -> Result<Self, String> {
        let provider = |name: &str| -> Result<IntProvider, String> {
            IntProvider::parse(
                config
                    .get(name)
                    .ok_or_else(|| format!("basalt_columns has no `{name}`"))?,
            )
        };
        Ok(Self {
            reach: provider("reach")?,
            height: provider("height")?,
        })
    }
}

/// The blocks a basalt column refuses to stand on — `CANNOT_PLACE_ON`. It is
/// `DELTA_CANNOT_REPLACE` plus lava, magma and soul sand: a column may not
/// grow out of the very things a delta lays down, which is what stops the two
/// from stacking where their biomes overlap.
const COLUMN_CANNOT_PLACE_ON: [&str; 10] = [
    "minecraft:lava",
    "minecraft:bedrock",
    "minecraft:magma_block",
    "minecraft:soul_sand",
    "minecraft:nether_bricks",
    "minecraft:nether_brick_fence",
    "minecraft:nether_brick_stairs",
    "minecraft:nether_wart",
    "minecraft:chest",
    "minecraft:spawner",
];

/// `BasaltColumnsFeature#place`.
///
/// A *cluster* of columns, not one. The origin only decides where to scatter
/// 50 or 15 random seeds; each seed then grows its own column, whose height
/// falls off with Manhattan distance from the origin.
///
/// The `nextFloat() < 0.9` roll swaps **both** the reach and the count
/// together — 5 and 50, or 8 and 15 — so the two shapes differ in density as
/// well as in size, and the common one is the dense small one.
pub fn place_basalt_columns<W: NetherWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    config: &ColumnConfig,
    random: &mut R,
    origin: (i32, i32, i32),
) -> bool {
    let sea_level = world.sea_level();
    if !can_place_at(world, sea_level, origin) {
        return false;
    }

    let height = config.height.sample(random);
    let clustered = random.next_f32() < 0.9;
    let reach = height.min(if clustered { 5 } else { 8 });
    let count = if clustered { 50 } else { 15 };

    // `BlockPos.randomBetweenClosed(random, count, ...)` over a box that is
    // one block tall. The y draw is `nextInt(1)`, which is always 0 and still
    // advances the stream — dropping it would desynchronise every column
    // after the first.
    let span = 2 * reach + 1;
    let mut placed = false;
    for _ in 0..count {
        let seed = (
            origin.0 - reach + random.next_i32_bound(span),
            origin.1 + random.next_i32_bound(1),
            origin.2 - reach + random.next_i32_bound(span),
        );
        let remaining = height - dist_manhattan(seed, origin);
        if remaining < 0 {
            continue;
        }
        // Sampled only for a seed that survives, and after that seed's three
        // position draws — the iterator is lazy, so the two interleave.
        let spread = config.reach.sample(random);
        placed |= place_column(world, sea_level, seed, remaining, spread);
    }
    placed
}

/// `BasaltColumnsFeature#placeColumn`.
fn place_column<W: NetherWorld + ?Sized>(
    world: &mut W,
    sea_level: i32,
    centre: (i32, i32, i32),
    height: i32,
    spread: i32,
) -> bool {
    let mut placed = false;
    let min = (centre.0 - spread, centre.1, centre.2 - spread);
    let max = (centre.0 + spread, centre.1, centre.2 + spread);
    for pos in BetweenClosed::new(min, max) {
        let distance = dist_manhattan(pos, centre);
        // An open cell searches *down* for ground; a solid one searches *up*
        // for the open cell above it. Both budgets are the distance from the
        // column's centre, so the skirt thins outward instead of chasing the
        // terrain arbitrarily far.
        let base = if is_air_or_lava_ocean(world, sea_level, pos) {
            find_surface(world, sea_level, pos, distance)
        } else {
            find_air(world, pos, distance)
        };
        let Some((x, mut y, z)) = base else { continue };

        let mut remaining = height - distance / 2;
        while remaining >= 0 {
            if is_air_or_lava_ocean(world, sea_level, (x, y, z)) {
                world.set_block(x, y, z, BASALT);
                y += 1;
                placed = true;
            } else {
                let state = world.block_at(x, y, z);
                if plain(&state) != BASALT_BLOCK {
                    break;
                }
                // Growing through a neighbouring column rather than stopping
                // at it — this is what welds a cluster into one mass.
                y += 1;
            }
            remaining -= 1;
        }
    }
    placed
}

/// `BasaltColumnsFeature#findSurface` — the first standable spot at or below
/// `from`, within `budget` steps.
fn find_surface<W: NetherWorld + ?Sized>(
    world: &mut W,
    sea_level: i32,
    from: (i32, i32, i32),
    mut budget: i32,
) -> Option<(i32, i32, i32)> {
    let (x, mut y, z) = from;
    while y > world.min_y() + 1 && budget > 0 {
        budget -= 1;
        if can_place_at(world, sea_level, (x, y, z)) {
            return Some((x, y, z));
        }
        y -= 1;
    }
    None
}

/// `BasaltColumnsFeature#findAir` — the first air at or above `from`, within
/// `budget` steps, abandoning the search entirely on a forbidden block rather
/// than stepping over it.
fn find_air<W: NetherWorld + ?Sized>(
    world: &mut W,
    from: (i32, i32, i32),
    mut budget: i32,
) -> Option<(i32, i32, i32)> {
    let (x, mut y, z) = from;
    while y <= world.max_y() && budget > 0 {
        budget -= 1;
        let state = world.block_at(x, y, z);
        let name = plain(&state);
        if COLUMN_CANNOT_PLACE_ON.contains(&name) {
            return None;
        }
        if is_air_name(name) {
            return Some((x, y, z));
        }
        y += 1;
    }
    None
}

/// `BasaltColumnsFeature#canPlaceAt` — open here, standing on something that
/// is neither air nor forbidden.
fn can_place_at<W: NetherWorld + ?Sized>(
    world: &mut W,
    sea_level: i32,
    pos: (i32, i32, i32),
) -> bool {
    if !is_air_or_lava_ocean(world, sea_level, pos) {
        return false;
    }
    let below = world.block_at(pos.0, pos.1 - 1, pos.2);
    let name = plain(&below);
    !is_air_name(name) && !COLUMN_CANNOT_PLACE_ON.contains(&name)
}

/// `BasaltColumnsFeature#isAirOrLavaOcean`. Lava counts only *at or below*
/// the sea level: a lava fall hanging in a cavern is not the ocean, and a
/// column will not grow through one.
fn is_air_or_lava_ocean<W: NetherWorld + ?Sized>(
    world: &mut W,
    sea_level: i32,
    pos: (i32, i32, i32),
) -> bool {
    let state = world.block_at(pos.0, pos.1, pos.2);
    let name = plain(&state);
    is_air_name(name) || (name == "minecraft:lava" && pos.1 <= sea_level)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first point out of `withinManhattan` is the origin, and the walk
    /// never returns inward — the property both callers' `break` depends on.
    #[test]
    fn within_manhattan_starts_at_the_origin_and_never_goes_back_inward() {
        let points: Vec<_> = WithinManhattan::new((10, 20, 30), 2, 2, 2).collect();
        assert_eq!(points[0], (10, 20, 30));
        let mut last = 0;
        for p in &points {
            let d = dist_manhattan(*p, (10, 20, 30));
            assert!(d >= last, "{p:?} at {d} came after {last}");
            last = d;
        }
    }

    /// Every point inside the clipped diamond, once, and nothing outside it.
    #[test]
    fn within_manhattan_is_the_diamond_and_only_the_diamond() {
        let points: Vec<_> = WithinManhattan::new((0, 0, 0), 2, 1, 2).collect();
        let mut expected = Vec::new();
        for x in -2_i32..=2 {
            for y in -1_i32..=1 {
                for z in -2_i32..=2 {
                    if x.abs() + y.abs() + z.abs() <= 5 {
                        expected.push((x, y, z));
                    }
                }
            }
        }
        let mut sorted = points.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), points.len(), "a point came out twice");
        expected.sort_unstable();
        assert_eq!(sorted, expected);
    }

    /// The `+z`/`-z` twins come out back to back, which is the order a
    /// write-as-you-walk feature sees them in.
    #[test]
    fn within_manhattan_mirrors_z_immediately() {
        let points: Vec<_> = WithinManhattan::new((0, 0, 0), 0, 0, 3).collect();
        assert_eq!(
            points,
            vec![
                (0, 0, 0),
                (0, 0, 1),
                (0, 0, -1),
                (0, 0, 2),
                (0, 0, -2),
                (0, 0, 3),
                (0, 0, -3),
            ]
        );
    }

    /// No reach on any axis is exactly one point — not zero, and not a loop
    /// that never ends.
    #[test]
    fn within_manhattan_with_no_reach_is_the_origin_alone() {
        let points: Vec<_> = WithinManhattan::new((4, 5, 6), 0, 0, 0).collect();
        assert_eq!(points, vec![(4, 5, 6)]);
    }

    /// X fastest, then Y, then Z — the order `placeColumn` writes in.
    #[test]
    fn between_closed_runs_x_fastest_then_y_then_z() {
        let points: Vec<_> = BetweenClosed::new((0, 0, 0), (1, 1, 1)).collect();
        assert_eq!(
            points,
            vec![
                (0, 0, 0),
                (1, 0, 0),
                (0, 1, 0),
                (1, 1, 0),
                (0, 0, 1),
                (1, 0, 1),
                (0, 1, 1),
                (1, 1, 1),
            ]
        );
    }
}
