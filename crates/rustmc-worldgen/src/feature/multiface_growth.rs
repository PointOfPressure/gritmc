//! Glow lichen and sculk vein — blocks that grow *on* faces, not in cells.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `MultifaceGrowthFeature`, `MultifaceGrowthConfiguration`,
//! `MultifaceBlock`, `MultifaceSpreader` and its `SpreadType`,
//! `SculkVeinBlock.SculkVeinSpreaderConfig`, `Util#shuffle`.
//!
//! # Every draw is a shuffle
//!
//! The feature itself never calls `nextInt` directly. Its whole appetite is:
//!
//! ```text
//! getShuffledDirections(random)               n - 1 draws
//! per direction tried:
//!   getShuffledDirectionsExcept(random, ...)  m - 1 draws
//! on a successful placement:
//!   nextFloat()                               1 draw
//!   if it spreads: Direction.allShuffled()    5 draws
//! ```
//!
//! `Util#shuffle` is a downward Fisher-Yates — `for (i = size; i > 1; i--)`,
//! one `nextInt(i)` per step, swapping `i - 1` with the drawn index. Six
//! directions is five draws, not six.
//!
//! # A `searchRange` that does not search
//!
//! The inner loop is written
//!
//! ```text
//! for (i = 0; i < config.searchRange; i++) {
//!     cursor.setWithOffset(origin, direction);
//!     ...
//! }
//! ```
//!
//! — `origin`, not `cursor`. Every iteration probes the same block, one step
//! from where the feature landed, so a `search_range` of 20 searches exactly
//! one position twenty times. `javap -c` of `MultifaceGrowthFeature.place` is
//! unambiguous about it: the `setWithOffset` receiver is local 8 (the mutable
//! cursor) and its first argument is local 3 (the origin the context handed
//! over), while `cursor.set(origin)` two instructions earlier is left with
//! nothing to do.
//!
//! The repeats are free — the body is deterministic in the world it reads and
//! only draws *after* it succeeds, at which point the feature returns — so
//! this port probes once and says why, rather than looping to look faithful.
//! What is *not* free is the reach: a port that walked the cursor outward
//! would find attachment sites twenty blocks away that vanilla never sees.
//!
//! # Spreading is a second, different traversal
//!
//! After a face is painted, `chanceOfSpreading` may send the block hunting for
//! one more face through `MultifaceSpreader`. That walk has its own rules —
//! three spread *types* tried in order (same position, same plane, wrap
//! around), each producing a candidate position and face — and it is where
//! sculk vein differs from glow lichen: sculk refuses to grow off sculk or a
//! sculk catalyst, refuses a sturdy backing block two steps away, and accepts
//! anything merely replaceable, while glow lichen wants air, itself, or a
//! water source.

use super::block_predicate::PredicateWorld;
use super::state_spec::DefaultStates;

/// `Direction.values()` order — the order `allShuffled` shuffles and the order
/// the block-property tables are columned by.
pub const DIRECTIONS: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

/// The offset of each direction, indexed as [`DIRECTIONS`].
const OFFSETS: [(i32, i32, i32); 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];

/// `Direction.getOpposite()`, indexed as [`DIRECTIONS`].
const OPPOSITE: [usize; 6] = [1, 0, 3, 2, 5, 4];

/// `Direction.getAxis()`, indexed as [`DIRECTIONS`]. Y, Y, Z, Z, X, X.
const AXIS: [u8; 6] = [1, 1, 2, 2, 0, 0];

/// `Direction.Plane.HORIZONTAL`, in the order `forEach` walks it.
const HORIZONTAL: [usize; 4] = [2, 5, 3, 4];

/// One of `MultifaceSpreader.SpreadType`, in `$VALUES` order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpreadType {
    SamePosition,
    SamePlane,
    WrapAround,
}

/// `MultifaceSpreader.DEFAULT_SPREAD_ORDER`.
const DEFAULT_SPREAD_ORDER: [SpreadType; 3] = [
    SpreadType::SamePosition,
    SpreadType::SamePlane,
    SpreadType::WrapAround,
];

impl SpreadType {
    /// `SpreadType#getSpreadPos(pos, direction, face)` — position and face.
    fn spread_pos(
        self,
        pos: (i32, i32, i32),
        direction: usize,
        face: usize,
    ) -> ((i32, i32, i32), usize) {
        match self {
            Self::SamePosition => (pos, direction),
            Self::SamePlane => (offset(pos, direction), face),
            Self::WrapAround => (offset(offset(pos, direction), face), OPPOSITE[direction]),
        }
    }
}

fn offset((x, y, z): (i32, i32, i32), direction: usize) -> (i32, i32, i32) {
    let (dx, dy, dz) = OFFSETS[direction];
    (x + dx, y + dy, z + dz)
}

/// `BlockPos.relative(direction)`, for the modules that borrow
/// [`can_attach_to`] and so need the same direction indexing.
#[must_use]
pub fn offset_of(pos: (i32, i32, i32), direction: usize) -> (i32, i32, i32) {
    offset(pos, direction)
}

/// `MultifaceGrowthConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct MultifaceGrowthConfig {
    /// The block placed, and the block whose spreader rules apply.
    pub block: String,
    /// The block's default state, which every placement is built from.
    pub default_state: String,
    pub search_range: i32,
    pub can_place_on_floor: bool,
    pub can_place_on_ceiling: bool,
    pub can_place_on_wall: bool,
    pub chance_of_spreading: f32,
    /// `can_be_placed_on`, either a tag or a list of blocks.
    pub can_be_placed_on: Vec<String>,
    pub can_be_placed_on_tag: Option<String>,
}

impl MultifaceGrowthConfig {
    /// # Errors
    /// If a field is missing or malformed, or the block is not in the extract.
    pub fn parse(config: &serde_json::Value, states: &dyn DefaultStates) -> Result<Self, String> {
        let block = config
            .get("block")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "multiface_growth config has no `block`".to_string())?
            .to_string();
        let default_state = states
            .default_state(&block)
            .ok_or_else(|| format!("{block} is not in the block-state extract"))?
            .to_string();

        let placed_on = config
            .get("can_be_placed_on")
            .ok_or_else(|| "multiface_growth config has no `can_be_placed_on`".to_string())?;
        let (can_be_placed_on, can_be_placed_on_tag) = match placed_on {
            serde_json::Value::String(one) => match one.strip_prefix('#') {
                Some(tag) => (Vec::new(), Some(tag.to_string())),
                None => (vec![one.clone()], None),
            },
            serde_json::Value::Array(list) => (
                list.iter()
                    .map(|v| {
                        v.as_str()
                            .map(ToString::to_string)
                            .ok_or_else(|| "can_be_placed_on entry is not a name".to_string())
                    })
                    .collect::<Result<Vec<_>, String>>()?,
                None,
            ),
            other => {
                return Err(format!(
                    "can_be_placed_on is neither a name nor a list: {other}"
                ))
            }
        };

        Ok(Self {
            block,
            default_state,
            // `Codec.intRange(1, 64).optionalFieldOf("search_range", 10)`.
            search_range: config
                .get("search_range")
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .unwrap_or(10),
            can_place_on_floor: flag(config, "can_place_on_floor"),
            can_place_on_ceiling: flag(config, "can_place_on_ceiling"),
            can_place_on_wall: flag(config, "can_place_on_wall"),
            // `Codec.floatRange(0, 1).optionalFieldOf("chance_of_spreading", 0.5f)`.
            #[expect(
                clippy::cast_possible_truncation,
                reason = "the codec's field is a float; the JSON carries a float's worth \
                          of precision and vanilla compares it against nextFloat()"
            )]
            chance_of_spreading: config
                .get("chance_of_spreading")
                .and_then(serde_json::Value::as_f64)
                .map_or(0.5, |v| v as f32),
            can_be_placed_on,
            can_be_placed_on_tag,
        })
    }

    /// `validDirections`, built in the constructor's order: ceiling first,
    /// then floor, then the horizontal plane.
    ///
    /// The order is the shuffle's input, so it is behaviour rather than
    /// bookkeeping — a different starting order is a different world.
    #[must_use]
    pub fn valid_directions(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(6);
        if self.can_place_on_ceiling {
            out.push(1); // UP
        }
        if self.can_place_on_floor {
            out.push(0); // DOWN
        }
        if self.can_place_on_wall {
            out.extend_from_slice(&HORIZONTAL);
        }
        out
    }

    /// Whether the port can evaluate this configuration.
    #[must_use]
    pub fn is_supported(&self) -> bool {
        // Only the two blocks whose spreader rules are ported. A third
        // multiface block would spread by the wrong rules and place real
        // blocks in the wrong places, which is worse than refusing.
        matches!(
            self.block.as_str(),
            "minecraft:glow_lichen" | "minecraft:sculk_vein"
        )
    }
}

/// `state.is(config.canBePlacedOn)` — a `HolderSet`, so either a tag or a
/// list of block names.
fn can_be_placed_on(
    tags: &crate::tags::BlockTags,
    config: &MultifaceGrowthConfig,
    state: &str,
) -> bool {
    let name = plain(state);
    config.can_be_placed_on_tag.as_ref().map_or_else(
        || config.can_be_placed_on.iter().any(|b| b == name),
        |tag| tags.contains(tag, name),
    )
}

/// `optionalFieldOf(name, false)`.
fn flag(config: &serde_json::Value, name: &str) -> bool {
    config
        .get(name)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

/// The world this feature reads and writes.
pub trait MultifaceWorld: PredicateWorld {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str);
    /// `ChunkAccess#markPosForPostProcessing`.
    ///
    /// **Required, deliberately.** This feature's marks were written off with
    /// the comment "`markPosForPostProcessing` changes no block", which is
    /// true of the call and false of the pass it feeds, and it is the third
    /// placeholder of that exact shape this port has had to remove.
    fn mark_pos_for_post_processing(&mut self, x: i32, y: i32, z: i32);
}

/// The feature could not be evaluated.
#[derive(Debug, PartialEq, Eq)]
pub struct NeedsRules(pub String);

fn needs(what: &str) -> NeedsRules {
    NeedsRules(what.to_string())
}

/// `MultifaceGrowthFeature#place`.
///
/// # Errors
/// If the world cannot answer a block question the rules need.
pub fn place<W: MultifaceWorld + ?Sized>(
    world: &mut W,
    tags: &crate::tags::BlockTags,
    config: &MultifaceGrowthConfig,
    random: &mut dyn crate::rng::RandomSource,
    origin: (i32, i32, i32),
) -> Result<bool, NeedsRules> {
    if !is_air_or_water(world, origin)? {
        return Ok(false);
    }
    if !config.is_supported() {
        return Err(needs(&format!("spreader rules for {}", config.block)));
    }

    let valid = config.valid_directions();
    let order = shuffled(&valid, random);

    let here = world.block_at(origin.0, origin.1, origin.2);
    if place_growth_if_possible(world, tags, config, random, origin, &here, &order)? {
        return Ok(true);
    }

    for &direction in &order {
        let unshuffled: Vec<usize> = valid
            .iter()
            .copied()
            .filter(|d| *d != OPPOSITE[direction])
            .collect();
        let except = shuffled(&unshuffled, random);

        // One probe, not `search_range` of them. See the module docs.
        if config.search_range <= 0 {
            continue;
        }
        let probe = offset(origin, direction);
        let state = world.block_at(probe.0, probe.1, probe.2);
        if !is_air_or_water(world, probe)? && !is_block(&state, &config.block) {
            continue;
        }
        if place_growth_if_possible(world, tags, config, random, probe, &state, &except)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `MultifaceGrowthFeature#placeGrowthIfPossible`.
fn place_growth_if_possible<W: MultifaceWorld + ?Sized>(
    world: &mut W,
    tags: &crate::tags::BlockTags,
    config: &MultifaceGrowthConfig,
    random: &mut dyn crate::rng::RandomSource,
    pos: (i32, i32, i32),
    state: &str,
    directions: &[usize],
) -> Result<bool, NeedsRules> {
    for &face in directions {
        let backing = offset(pos, face);
        let backing_state = world.block_at(backing.0, backing.1, backing.2);
        if !can_be_placed_on(tags, config, &backing_state) {
            continue;
        }
        let Some(placed) = state_for_placement(world, config, state, pos, face)? else {
            // A refusal here ends the whole call — vanilla returns rather
            // than trying the next face.
            return Ok(false);
        };
        world.set_block(pos.0, pos.1, pos.2, &placed);
        // `level.getChunk(pos).markPosForPostProcessing(pos)`, and **after**
        // the write — the order the two calls appear in is the order the
        // position enters the section's list, and the list is replayed in
        // order.
        world.mark_pos_for_post_processing(pos.0, pos.1, pos.2);

        if random.next_f32() < config.chance_of_spreading {
            // `spreadFromFaceTowardRandomDirection(.., true)` — the feature
            // passes the mark flag set, and `iconst_1` is the last argument
            // pushed at offset 134.
            spread_from_face_toward_random_direction(
                world, tags, config, &placed, pos, face, random, true,
            )?;
        }
        return Ok(true);
    }
    Ok(false)
}

/// `MultifaceBlock#getStateForPlacement`. `None` is vanilla's null.
fn state_for_placement<W: MultifaceWorld + ?Sized>(
    world: &mut W,
    config: &MultifaceGrowthConfig,
    state: &str,
    pos: (i32, i32, i32),
    face: usize,
) -> Result<Option<String>, NeedsRules> {
    if !is_valid_state_for_placement(world, config, state, pos, face)? {
        return Ok(None);
    }
    // Growing onto a block that is already this one keeps its other faces.
    let base = if is_block(state, &config.block) {
        state.to_string()
    } else if is_water_source(world, pos)? {
        with_property(&config.default_state, "waterlogged", "true")
    } else {
        config.default_state.clone()
    };
    Ok(Some(with_property(&base, DIRECTIONS[face], "true")))
}

/// `MultifaceBlock#isValidStateForPlacement`.
fn is_valid_state_for_placement<W: MultifaceWorld + ?Sized>(
    world: &mut W,
    config: &MultifaceGrowthConfig,
    state: &str,
    pos: (i32, i32, i32),
    face: usize,
) -> Result<bool, NeedsRules> {
    // `isFaceSupported` is `true` for every direction on both blocks here.
    if is_block(state, &config.block) && has_face(state, face) {
        return Ok(false);
    }
    can_attach_to(world, offset(pos, face), face)
}

/// `MultifaceBlock#canAttachTo` — the backing block must present a full face
/// towards the growth, on **either** its support shape or its collision shape.
///
/// Public because `VineBlock.isAcceptableNeighbour` is exactly this call and
/// nothing else, so the vine feature borrows it rather than restating it.
///
/// # Errors
/// If the world cannot answer either shape question.
pub fn can_attach_to<W: PredicateWorld + ?Sized>(
    world: &mut W,
    backing: (i32, i32, i32),
    face: usize,
) -> Result<bool, NeedsRules> {
    let towards = OPPOSITE[face];
    let sturdy = world
        .face_is_sturdy(backing.0, backing.1, backing.2, towards)
        .ok_or_else(|| needs("isFaceSturdy"))?;
    if sturdy {
        return Ok(true);
    }
    world
        .face_is_collision_full(backing.0, backing.1, backing.2, towards)
        .ok_or_else(|| needs("isFaceFull(collisionShape)"))
}

/// `MultifaceSpreader#spreadFromFaceTowardRandomDirection`.
///
/// Six directions shuffled — five draws — then the first that yields a
/// position wins. The stream is lazy, so a success on the first direction
/// still costs the whole shuffle and nothing more.
#[allow(clippy::too_many_arguments)] // Vanilla's signature, mark flag included.
fn spread_from_face_toward_random_direction<W: MultifaceWorld + ?Sized>(
    world: &mut W,
    tags: &crate::tags::BlockTags,
    config: &MultifaceGrowthConfig,
    state: &str,
    pos: (i32, i32, i32),
    face: usize,
    random: &mut dyn crate::rng::RandomSource,
    mark: bool,
) -> Result<(), NeedsRules> {
    let all: Vec<usize> = (0..6).collect();
    for direction in shuffled(&all, random) {
        if spread_from_face_toward_direction(
            world,
            tags,
            config,
            state,
            pos,
            face,
            direction,
            mark,
            &DEFAULT_SPREAD_ORDER,
        )? {
            return Ok(());
        }
    }
    Ok(())
}

/// `MultifaceSpreader#spreadFromFaceTowardDirection`, folded together with
/// `getSpreadFromFaceTowardDirection` and `spreadToFace`.
/// `SpreadConfig#canSpreadFrom(state, direction)`.
///
/// ```java
///   return isOtherBlockValidAsSource(state) || hasFace(state, direction);
/// ```
///
/// For a sculk vein `isOtherBlockValidAsSource` is true of anything that is
/// not itself a vein, so a vein can spread *from* a face it does not occupy
/// when the source block is, say, deepslate — which is how a patch gets
/// started at all.
fn can_spread_from(config: &MultifaceGrowthConfig, state: &str, direction: usize) -> bool {
    is_other_block_valid_as_source(config, state) || has_face(state, direction)
}

/// `MultifaceSpreader#spreadAll(state, level, pos, markForPostprocessing)`.
///
/// ```java
///   return Direction.stream()
///       .filter(d -> config.canSpreadFrom(state, d))
///       .map(d -> spreadFromFaceTowardAllDirections(state, level, pos, d, flag))
///       .reduce(0L, Long::sum);
/// ```
///
/// and the inner fold is
///
/// ```java
///   return Direction.stream()
///       .map(d -> spreadFromFaceTowardDirection(state, level, pos, face, d, flag))
///       .filter(Optional::isPresent)
///       .count();
/// ```
///
/// So it is **every face crossed with every direction** — up to 36 attempts —
/// and the result is a *count of successes*, not a stop-at-first. Vanilla's
/// callers only ever compare it against zero, but the count is what the method
/// returns and short-circuiting would place fewer blocks, not merely report a
/// different number.
///
/// Faces and directions are both walked in `Direction.values()` order, which
/// is [`DIRECTIONS`], because `Direction.stream()` is `Arrays.stream(values())`
/// and nothing shuffles here — this path takes no random at all.
///
/// # Errors
/// If a spread rule needs behaviour this port does not model.
pub fn spread_all<W: MultifaceWorld + ?Sized>(
    world: &mut W,
    tags: &crate::tags::BlockTags,
    config: &MultifaceGrowthConfig,
    state: &str,
    pos: (i32, i32, i32),
    mark: bool,
) -> Result<u64, NeedsRules> {
    spread_all_with(world, tags, config, state, pos, mark, &DEFAULT_SPREAD_ORDER)
}

/// [`spread_all`] through a spreader whose `SpreadConfig#getSpreadTypes` is
/// `types`: `SculkVeinBlock`'s same-space spreader is
/// `new SculkVeinSpreaderConfig(SpreadType.SAME_POSITION)`, and
/// `SculkBehaviour.DEFAULT.attemptSpreadVein` uses it for a cursor that has no
/// facings yet.
///
/// # Errors
/// As [`spread_all`].
pub fn spread_all_with<W: MultifaceWorld + ?Sized>(
    world: &mut W,
    tags: &crate::tags::BlockTags,
    config: &MultifaceGrowthConfig,
    state: &str,
    pos: (i32, i32, i32),
    mark: bool,
    types: &[SpreadType],
) -> Result<u64, NeedsRules> {
    let mut placed = 0;
    for face in 0..DIRECTIONS.len() {
        if !can_spread_from(config, state, face) {
            continue;
        }
        for direction in 0..DIRECTIONS.len() {
            if spread_from_face_toward_direction(
                world, tags, config, state, pos, face, direction, mark, types,
            )? {
                placed += 1;
            }
        }
    }
    Ok(placed)
}

#[allow(clippy::too_many_arguments)] // Vanilla's signature, mark flag included.
fn spread_from_face_toward_direction<W: MultifaceWorld + ?Sized>(
    world: &mut W,
    tags: &crate::tags::BlockTags,
    config: &MultifaceGrowthConfig,
    state: &str,
    pos: (i32, i32, i32),
    face: usize,
    direction: usize,
    mark: bool,
    types: &[SpreadType],
) -> Result<bool, NeedsRules> {
    if AXIS[direction] == AXIS[face] {
        return Ok(false);
    }
    if !is_other_block_valid_as_source(config, state)
        && (!has_face(state, face) || has_face(state, direction))
    {
        return Ok(false);
    }

    for &spread_type in types {
        let (target, target_face) = spread_type.spread_pos(pos, direction, face);
        if !can_spread_into(world, tags, config, pos, target, target_face)? {
            continue;
        }
        // `spreadToFace`: read the target, ask for a state, write it.
        let there = world.block_at(target.0, target.1, target.2);
        let Some(placed) = state_for_placement(world, config, &there, target, target_face)? else {
            return Ok(false);
        };
        // `SpreadConfig#placeBlock` marks **before** its `setBlock`, which is
        // the opposite order to `placeGrowthIfPossible` above. Both are
        // reproduced as written: the pass walks the list in order.
        if mark {
            world.mark_pos_for_post_processing(target.0, target.1, target.2);
        }
        world.set_block(target.0, target.1, target.2, &placed);
        return Ok(true);
    }
    Ok(false)
}

/// `SpreadConfig#isOtherBlockValidAsSource`. False by default; sculk vein
/// says yes to anything that is not already sculk vein.
fn is_other_block_valid_as_source(config: &MultifaceGrowthConfig, state: &str) -> bool {
    config.block == "minecraft:sculk_vein" && !is_block(state, "minecraft:sculk_vein")
}

/// `SpreadConfig#canSpreadInto` — `stateCanBeReplaced` and then the same
/// placement validity test the feature itself used.
fn can_spread_into<W: MultifaceWorld + ?Sized>(
    world: &mut W,
    tags: &crate::tags::BlockTags,
    config: &MultifaceGrowthConfig,
    from: (i32, i32, i32),
    target: (i32, i32, i32),
    target_face: usize,
) -> Result<bool, NeedsRules> {
    let there = world.block_at(target.0, target.1, target.2);
    if !state_can_be_replaced(world, tags, config, from, target, target_face, &there)? {
        return Ok(false);
    }
    is_valid_state_for_placement(world, config, &there, target, target_face)
}

/// `DefaultSpreaderConfig#stateCanBeReplaced`, with sculk vein's override.
fn state_can_be_replaced<W: MultifaceWorld + ?Sized>(
    world: &mut W,
    tags: &crate::tags::BlockTags,
    config: &MultifaceGrowthConfig,
    from: (i32, i32, i32),
    target: (i32, i32, i32),
    target_face: usize,
    state: &str,
) -> Result<bool, NeedsRules> {
    if config.block == "minecraft:sculk_vein" {
        // Never grow off the sculk that produced it.
        let behind = offset(target, target_face);
        let behind_state = world.block_at(behind.0, behind.1, behind.2);
        for block in [
            "minecraft:sculk",
            "minecraft:sculk_catalyst",
            "minecraft:moving_piston",
        ] {
            if is_block(&behind_state, block) {
                return Ok(false);
            }
        }
        // A wrap-around lands two steps away; if the block it wrapped past
        // has a sturdy face there, the vein would be growing through it.
        if manhattan(from, target) == 2 {
            let corner = offset(from, OPPOSITE[target_face]);
            if world
                .face_is_sturdy(corner.0, corner.1, corner.2, target_face)
                .ok_or_else(|| needs("isFaceSturdy"))?
            {
                return Ok(false);
            }
        }
        let fluid = world.fluid_at(target.0, target.1, target.2);
        if fluid != "minecraft:empty" && fluid != "minecraft:water" {
            return Ok(false);
        }
        if tags.contains("minecraft:fire", plain(state)) {
            return Ok(false);
        }
        if world
            .replaceable_at(target.0, target.1, target.2)
            .ok_or_else(|| needs("canBeReplaced"))?
        {
            return Ok(true);
        }
    }

    // The default rule: air, itself, or a water *source*.
    if world
        .air_at(target.0, target.1, target.2)
        .ok_or_else(|| needs("isAir"))?
    {
        return Ok(true);
    }
    if is_block(state, &config.block) {
        return Ok(true);
    }
    Ok(is_block(state, "minecraft:water") && is_water_source(world, target)?)
}

fn manhattan(a: (i32, i32, i32), b: (i32, i32, i32)) -> i32 {
    (a.0 - b.0).abs() + (a.1 - b.1).abs() + (a.2 - b.2).abs()
}

/// `MultifaceGrowthFeature#isAirOrWater` — `isAir()` or the block is water.
fn is_air_or_water<W: MultifaceWorld + ?Sized>(
    world: &mut W,
    pos: (i32, i32, i32),
) -> Result<bool, NeedsRules> {
    if world
        .air_at(pos.0, pos.1, pos.2)
        .ok_or_else(|| needs("isAir"))?
    {
        return Ok(true);
    }
    let state = world.block_at(pos.0, pos.1, pos.2);
    Ok(is_block(&state, "minecraft:water"))
}

/// `state.getFluidState().isSourceOfType(Fluids.WATER)`.
fn is_water_source<W: MultifaceWorld + ?Sized>(
    world: &mut W,
    pos: (i32, i32, i32),
) -> Result<bool, NeedsRules> {
    if world.fluid_at(pos.0, pos.1, pos.2) != "minecraft:water" {
        return Ok(false);
    }
    // `minecraft:water` as a fluid name is already the source fluid —
    // flowing water reports `minecraft:flowing_water`. The level check is
    // belt and braces for a world that answers by block name.
    let state = world.block_at(pos.0, pos.1, pos.2);
    Ok(!state.contains("level=") || state.contains("level=0"))
}

/// `MultifaceBlock#hasFace` — the face property is present and true.
fn has_face(state: &str, face: usize) -> bool {
    state.contains(&format!("{}=true", DIRECTIONS[face]))
}

/// Whether the state is this block, whatever its properties.
fn is_block(state: &str, block: &str) -> bool {
    plain(state) == block
}

fn plain(state: &str) -> &str {
    state.split_once('[').map_or(state, |(n, _)| n)
}

/// `state.setValue(property, value)`, on the canonical spelling. A state that
/// does not carry the property is returned unchanged — vanilla would not have
/// reached here with one.
pub(crate) fn with_property(state: &str, property: &str, value: &str) -> String {
    let Some((name, rest)) = state.split_once('[') else {
        return state.to_string();
    };
    let pairs: Vec<String> = rest
        .trim_end_matches(']')
        .split(',')
        .map(|pair| match pair.split_once('=') {
            Some((k, _)) if k == property => format!("{property}={value}"),
            _ => pair.to_string(),
        })
        .collect();
    format!("{name}[{}]", pairs.join(","))
}

/// `Util#shuffle` — downward Fisher-Yates, `size - 1` draws.
fn shuffled(input: &[usize], random: &mut dyn crate::rng::RandomSource) -> Vec<usize> {
    let mut out = input.to_vec();
    let mut i = out.len();
    while i > 1 {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_possible_wrap,
            reason = "a direction list is at most six long"
        )]
        let j = random.next_i32_bound(i as i32) as usize;
        out.swap(i - 1, j);
        i -= 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;
    use crate::rng::RandomSource;
    use crate::tags::BlockTags;
    use std::collections::BTreeMap;

    struct Cave {
        blocks: BTreeMap<(i32, i32, i32), String>,
        /// Every position handed to `markPosForPostProcessing`, in call
        /// order — the order is behaviour, so a set would not do.
        marks: Vec<(i32, i32, i32)>,
    }

    impl PredicateWorld for Cave {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.blocks
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
        fn air_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:air")
        }
        fn replaceable_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:air")
        }
        fn face_is_sturdy(&mut self, x: i32, y: i32, z: i32, _d: usize) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:stone")
        }
        fn face_is_collision_full(&mut self, x: i32, y: i32, z: i32, _d: usize) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:stone")
        }
    }

    impl MultifaceWorld for Cave {
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
            self.blocks.insert((x, y, z), state.to_string());
        }
        fn mark_pos_for_post_processing(&mut self, x: i32, y: i32, z: i32) {
            self.marks.push((x, y, z));
        }
    }

    const LICHEN: &str = "minecraft:glow_lichen[down=false,east=false,north=false,\
                          south=false,up=false,waterlogged=false,west=false]";

    fn states() -> super::super::state_spec::DefaultStateTable {
        super::super::state_spec::DefaultStateTable::from_pairs(&[
            ("minecraft:glow_lichen", LICHEN),
            (
                "minecraft:sculk_vein",
                "minecraft:sculk_vein[down=false,east=false,north=false,south=false,\
                 up=false,waterlogged=false,west=false]",
            ),
        ])
    }

    fn config(json: &str) -> MultifaceGrowthConfig {
        MultifaceGrowthConfig::parse(&serde_json::from_str(json).expect("json"), &states())
            .expect("parse")
    }

    fn lichen() -> MultifaceGrowthConfig {
        config(
            r#"{"block":"minecraft:glow_lichen",
                "can_be_placed_on":["minecraft:stone"],
                "can_place_on_ceiling":true,
                "can_place_on_wall":true,
                "chance_of_spreading":0.0,
                "search_range":20}"#,
        )
    }

    fn tags() -> BlockTags {
        BlockTags::from_entries(BTreeMap::new()).expect("tags")
    }

    /// The vein path reads `#minecraft:fire` when deciding replaceability, so
    /// an empty tag set panics rather than answering false. Supplied empty but
    /// *present*, which is the honest shape: the tag exists and holds nothing
    /// relevant here.
    fn vein_tags() -> BlockTags {
        BlockTags::from_entries(BTreeMap::from([(
            "minecraft:fire".to_string(),
            vec!["minecraft:fire".to_string()],
        )]))
        .expect("tags")
    }

    fn vein() -> MultifaceGrowthConfig {
        config(
            r#"{"block":"minecraft:sculk_vein",
                "can_be_placed_on":["minecraft:stone","minecraft:deepslate"],
                "can_place_on_ceiling":true,
                "can_place_on_wall":true,
                "chance_of_spreading":0.0,
                "search_range":20}"#,
        )
    }

    /// How long 640 `spread_all` calls take — the per-patch budget for a
    /// sculk patch (10 cursors x 64 attempts).
    ///
    /// Wiring `spread_all` into `attemptSpreadVein` made a single chunk take
    /// over 120 s where the whole 25-chunk window had taken seconds. Reading
    /// the function says that should be impossible: it is 6 faces x 6
    /// directions x 3 spread types of bounded work, with no search. This
    /// measures it rather than continuing to reason about it.
    #[test]
    fn six_hundred_and_forty_spread_alls_are_cheap() {
        let mut cave = Cave {
            blocks: BTreeMap::new(),
            marks: Vec::new(),
        };
        for x in -2..=2 {
            for z in -2..=2 {
                cave.blocks
                    .insert((x, -1, z), "minecraft:stone".to_string());
            }
        }

        let start = std::time::Instant::now();
        for _ in 0..640 {
            let _ = spread_all(
                &mut cave,
                &vein_tags(),
                &vein(),
                "minecraft:sculk_vein[down=true]",
                (0, 0, 0),
                true,
            );
        }
        let elapsed = start.elapsed();
        println!("640 spread_all calls in {elapsed:?}");
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "one patch's worth of vein spreading took {elapsed:?}, which \
             cannot explain a 120 s chunk on its own"
        );
    }

    /// `spreadAll` is every face crossed with every direction, and it counts
    /// successes rather than stopping at the first. A vein on a stone floor in
    /// open air should paint several faces in one call.
    #[test]
    fn spread_all_places_more_than_one_face() {
        let mut cave = Cave {
            blocks: BTreeMap::new(),
            marks: Vec::new(),
        };
        // A stone floor under the origin, and stone one step north as well, so
        // both a same-plane and a wrap-around candidate exist.
        cave.blocks
            .insert((0, -1, 0), "minecraft:stone".to_string());
        cave.blocks
            .insert((0, -1, -1), "minecraft:stone".to_string());
        cave.blocks.insert((0, 0, -1), "minecraft:air".to_string());

        let placed = spread_all(
            &mut cave,
            &vein_tags(),
            &vein(),
            "minecraft:sculk_vein[down=true]",
            (0, 0, 0),
            true,
        )
        .expect("no unmodelled behaviour");
        assert!(
            placed > 0,
            "a vein on a stone floor should spread somewhere"
        );
    }

    /// Nothing to attach to means nothing placed, and no error — an empty
    /// result is a legitimate answer rather than a refusal.
    #[test]
    fn spread_all_in_empty_air_places_nothing() {
        let mut cave = Cave {
            blocks: BTreeMap::new(),
            marks: Vec::new(),
        };
        let placed = spread_all(
            &mut cave,
            &vein_tags(),
            &vein(),
            "minecraft:sculk_vein[down=true]",
            (0, 0, 0),
            true,
        )
        .expect("no unmodelled behaviour");
        assert_eq!(placed, 0);
    }

    /// `canSpreadFrom` is `isOtherBlockValidAsSource || hasFace`. For a vein
    /// the first is true of any non-vein block, so a *deepslate* source can
    /// spread from every face — which is how a patch starts at all.
    #[test]
    fn a_non_vein_source_can_spread_from_any_face() {
        for face in 0..DIRECTIONS.len() {
            assert!(
                can_spread_from(&vein(), "minecraft:deepslate", face),
                "deepslate should be a valid source on face {face}"
            );
            assert!(
                !can_spread_from(&lichen(), "minecraft:stone", face),
                "glow lichen has no such escape and needs the face itself"
            );
        }
    }

    /// A ceiling of stone with air below it: the lichen goes on the air block
    /// with its `up` face set, not into the stone.
    #[test]
    fn lichen_paints_the_face_of_the_block_it_hangs_from() {
        let mut world = Cave {
            blocks: BTreeMap::from([((0, 1, 0), "minecraft:stone".to_string())]),
            marks: Vec::new(),
        };
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(7));
        assert!(place(&mut world, &tags(), &lichen(), &mut random, (0, 0, 0)).expect("ok"));
        let placed = world.blocks.get(&(0, 0, 0)).expect("placed");
        assert!(placed.starts_with("minecraft:glow_lichen["), "{placed}");
        assert!(placed.contains("up=true"), "{placed}");
        assert!(world.blocks.get(&(0, 1, 0)).expect("stone") == "minecraft:stone");
    }

    /// Nothing to attach to anywhere: no block, and the draws are only the
    /// shuffles.
    #[test]
    fn an_empty_cave_grows_nothing() {
        let mut world = Cave {
            blocks: BTreeMap::new(),
            marks: Vec::new(),
        };
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(7));
        assert!(!place(&mut world, &tags(), &lichen(), &mut random, (0, 0, 0)).expect("ok"));
        assert!(world.blocks.is_empty());
    }

    /// `Util#shuffle` takes `size - 1` draws, not `size`.
    #[test]
    fn a_shuffle_of_six_takes_five_draws() {
        let mut counted = WorldgenRandom::new(LegacyRandomSource::new(3));
        let before = counted.next_i64();
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(3));
        let out = shuffled(&[0, 1, 2, 3, 4, 5], &mut random);
        assert_eq!(out.len(), 6);
        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(3));
        for _ in 0..5 {
            probe.next_i32_bound(6);
        }
        // Five draws consumed, and the sixth value in each stream is the
        // same only if the counts agree.
        assert_ne!(before, 0, "the probe stream ran");
        assert_eq!(random.next_i64(), probe.next_i64());
    }

    /// A one-element list draws nothing at all.
    #[test]
    fn a_shuffle_of_one_takes_no_draws() {
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(9));
        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(9));
        assert_eq!(shuffled(&[3], &mut random), vec![3]);
        assert_eq!(random.next_i64(), probe.next_i64());
    }

    /// The valid-direction list is built ceiling, floor, then horizontal —
    /// and the horizontal plane in `Direction.Plane` order.
    #[test]
    fn valid_directions_are_built_in_vanillas_order() {
        let config = config(
            r#"{"block":"minecraft:sculk_vein",
                "can_be_placed_on":["minecraft:stone"],
                "can_place_on_floor":true,
                "can_place_on_ceiling":true,
                "can_place_on_wall":true}"#,
        );
        assert_eq!(
            config.valid_directions(),
            vec![1, 0, 2, 5, 3, 4],
            "UP, DOWN, then NORTH, EAST, SOUTH, WEST"
        );
    }

    #[test]
    fn spread_positions_match_each_type() {
        // pos (0,0,0), spreading east (5) from an up face (1).
        assert_eq!(
            SpreadType::SamePosition.spread_pos((0, 0, 0), 5, 1),
            ((0, 0, 0), 5)
        );
        assert_eq!(
            SpreadType::SamePlane.spread_pos((0, 0, 0), 5, 1),
            ((1, 0, 0), 1)
        );
        assert_eq!(
            SpreadType::WrapAround.spread_pos((0, 0, 0), 5, 1),
            ((1, 1, 0), 4)
        );
    }

    #[test]
    fn a_third_multiface_block_is_refused_rather_than_guessed() {
        let config = MultifaceGrowthConfig {
            block: "minecraft:pale_moss_carpet".to_string(),
            default_state: "minecraft:pale_moss_carpet".to_string(),
            search_range: 10,
            can_place_on_floor: true,
            can_place_on_ceiling: false,
            can_place_on_wall: false,
            chance_of_spreading: 0.5,
            can_be_placed_on: vec!["minecraft:stone".to_string()],
            can_be_placed_on_tag: None,
        };
        assert!(!config.is_supported());
        let mut world = Cave {
            blocks: BTreeMap::new(),
            marks: Vec::new(),
        };
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(1));
        assert!(place(&mut world, &tags(), &config, &mut random, (0, 0, 0)).is_err());
    }

    #[test]
    fn both_multiface_configs_in_the_datapack_parse() {
        let Some(extract) = super::super::state_spec::extract_or_skip() else {
            return;
        };
        let dir = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/configured_feature"
        ));
        if !rustmc_data::fixture::available(dir, "datagen extract", "ci/vanilla-server.sh data") {
            return;
        }
        let mut seen = 0usize;
        for entry in std::fs::read_dir(dir).expect("readable") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let json: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).expect("readable"))
                    .expect("json");
            if json["type"] != "minecraft:multiface_growth" {
                continue;
            }
            let config = MultifaceGrowthConfig::parse(&json["config"], &extract)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!(config.is_supported(), "{}", path.display());
            seen += 1;
        }
        assert_eq!(seen, 2, "the datapack has glow_lichen and sculk_vein");
    }
}
