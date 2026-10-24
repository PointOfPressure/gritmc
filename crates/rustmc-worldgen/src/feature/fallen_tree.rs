//! `FallenTreeFeature` — a stump, a gap, and a log lying on the ground.
//!
//! Read off `javap -p -c -constants` of `FallenTreeFeature`,
//! `FallenTreeConfiguration`, `AttachedToLogsDecorator`, `TrunkVineDecorator`,
//! `TreeDecorator$Context` and `Util.shuffle` (ORACLE.md rule 4).
//!
//! It is **not** a `TreeFeature`: it shares the decorators and the trunk
//! provider and nothing else. There is no trunk placer, no foliage, no
//! `minimum_size`, and no undo — `place` returns `true` unconditionally, and
//! the stump is written before anything is checked.
//!
//! # The shape, and where the draws go
//!
//! ```text
//!   placeStump(config, level, random, origin.mutable())      <- writes at the origin
//!   Direction d = HORIZONTAL.getRandomDirection(random)      <- 1 draw
//!   int len = config.logLength.sample(random) - 2            <- 1 draw
//!   MutableBlockPos p = origin.relative(d, 2 + nextInt(2))   <- 1 draw
//!   setGroundHeightForFallenLogStartPos(level, p)            <- reads only
//!   if (canPlaceEntireFallenLog(level, len, p, d))           <- reads only
//!       placeFallenLog(config, level, random, len, p, d)
//! ```
//!
//! The stump goes down **first**, so its decorators' draws come before the
//! direction is even chosen. That ordering is the whole reason this cannot be
//! folded into the tree feature: a tree decorates last.
//!
//! `logLength` is the sampled length **minus two** — the two blocks between
//! the stump and where the log starts — so `fallen_oak_tree`'s uniform 4..7
//! lays 2 to 5 log blocks, not 4 to 7.
//!
//! # What the ground test actually asks
//!
//! `mayPlaceOn` is `TreeFeature.validTreePos && isOverSolidGround`, and
//! `isOverSolidGround` is `getBlockState(pos.below()).isFaceSturdy(level, pos,
//! UP)` — the block **below**, its **up** face, `SupportType.FULL`. Note
//! vanilla passes the *unmoved* `pos` as the context argument to
//! `isFaceSturdy`; nothing in the vanilla tree of shapes reads it, and this
//! port's `face_is_sturdy` does not take one.
//!
//! `canPlaceEntireFallenLog` allows a run of at most **two** consecutive
//! unsupported blocks (`FALLEN_LOG_MAX_GROUND_GAP`), reset by any supported
//! one, and refuses outright at the first position a tree may not occupy. It
//! walks the mutable position forward and, on success only, walks it back —
//! which is why the same variable is then handed to `placeFallenLog`.

use super::block_predicate::PredicateWorld;
use super::disk::DiskWorld;
use super::int_provider::IntProvider;
use super::state_provider::StateProvider;
use super::state_spec::DefaultStates;
use super::tree::decorator::{self, Context, TreeDecorator};
use super::tree::{valid_tree_pos, NeedsRules, Placed, Pos, TreeWorld};
use crate::rng::RandomSource;
use crate::tags::BlockTags;

/// `Direction.Plane.HORIZONTAL.faces` — N, E, S, W, and the pick is
/// `nextInt(4)` into exactly that array.
const HORIZONTAL: [Pos; 4] = [(0, 0, -1), (1, 0, 0), (0, 0, 1), (-1, 0, 0)];

/// `Direction.values()` index of UP, which is what `isFaceSturdy` is asked.
const UP: usize = 1;

/// `FALLEN_LOG_MAX_GROUND_GAP`.
const MAX_GROUND_GAP: i32 = 2;

/// The downward search for the log's ground, `FALLEN_LOG_MAX_FALL_HEIGHT_TO_GROUND`
/// plus the one step up it starts with.
const MAX_FALL_HEIGHT: i32 = 6;

/// `FallenTreeConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct FallenTreeConfig {
    pub trunk_provider: StateProvider,
    pub log_length: IntProvider,
    pub stump_decorators: Vec<TreeDecorator>,
    pub log_decorators: Vec<TreeDecorator>,
}

impl FallenTreeConfig {
    /// # Errors
    /// If a field is missing or malformed.
    pub fn parse(config: &serde_json::Value, states: &dyn DefaultStates) -> Result<Self, String> {
        let decorators = |name: &str| -> Result<Vec<TreeDecorator>, String> {
            config
                .get(name)
                .and_then(serde_json::Value::as_array)
                .map(|list| {
                    list.iter()
                        .map(|d| {
                            TreeDecorator::parse(
                                d,
                                states,
                                &crate::feature::tree::decorator::no_registry,
                            )
                        })
                        .collect::<Result<Vec<_>, String>>()
                })
                .transpose()
                .map(Option::unwrap_or_default)
        };
        Ok(Self {
            trunk_provider: StateProvider::parse(
                config
                    .get("trunk_provider")
                    .ok_or_else(|| "fallen_tree has no `trunk_provider`".to_string())?,
                states,
            )?,
            log_length: IntProvider::parse(
                config
                    .get("log_length")
                    .ok_or_else(|| "fallen_tree has no `log_length`".to_string())?,
            )?,
            stump_decorators: decorators("stump_decorators")?,
            log_decorators: decorators("log_decorators")?,
        })
    }

    /// Whether the port can lay this log.
    #[must_use]
    pub fn is_supported(&self) -> bool {
        self.trunk_provider.is_supported()
            && self
                .stump_decorators
                .iter()
                .chain(self.log_decorators.iter())
                .all(TreeDecorator::is_supported)
    }
}

/// `FallenTreeFeature#place`. Always true, like vanilla's.
///
/// # Errors
/// If the world cannot answer `isAir` or `isFaceSturdy`, or a decorator is
/// one the port does not have.
pub fn place<R: RandomSource>(
    world: &mut (dyn crate::feature::configured::FeatureWorld + '_),
    tags: &BlockTags,
    config: &FallenTreeConfig,
    random: &mut R,
    origin: Pos,
) -> Result<bool, NeedsRules> {
    place_stump(config, world, tags, random, origin)?;

    #[expect(clippy::cast_sign_loss, reason = "nextInt(4) is never negative")]
    let direction = HORIZONTAL[(random.next_i32_bound(4) as usize) & 3];
    let log_length = config.log_length.sample(random) - 2;
    let away = 2 + random.next_i32_bound(2);
    let mut pos = (
        origin.0 + direction.0 * away,
        origin.1,
        origin.2 + direction.2 * away,
    );

    set_ground_height_for_log_start(world, tags, &mut pos)?;
    if can_place_entire_log(world, tags, log_length, &mut pos, direction)? {
        place_fallen_log(config, world, tags, random, log_length, &mut pos, direction)?;
    }
    Ok(true)
}

/// `FallenTreeFeature#placeStump` — one log at the origin, then the stump
/// decorators over a set of exactly that one position.
fn place_stump<R: RandomSource>(
    config: &FallenTreeConfig,
    world: &mut (dyn crate::feature::configured::FeatureWorld + '_),
    tags: &BlockTags,
    random: &mut R,
    origin: Pos,
) -> Result<(), NeedsRules> {
    let placed_at = place_log_block(config, world, tags, random, origin, None)?;
    let mut stump = Placed::default();
    stump
        .logs
        .insert(placed_at)
        .map_err(|_| NeedsRules("a HashSet bucket reached eight entries".to_string()))?;
    decorate(world, tags, random, &stump, &config.stump_decorators)
}

/// `FallenTreeFeature#setGroundHeightForFallenLogStartPos` — up one, then
/// down at most six looking for somewhere a log may lie.
fn set_ground_height_for_log_start<W: TreeWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    pos: &mut Pos,
) -> Result<(), NeedsRules> {
    pos.1 += 1;
    for _ in 0..MAX_FALL_HEIGHT {
        if may_place_on(world, tags, *pos)? {
            return Ok(());
        }
        pos.1 -= 1;
    }
    // Vanilla leaves the position where the search ended and carries on; the
    // length test below is what refuses it.
    Ok(())
}

/// `FallenTreeFeature#canPlaceEntireFallenLog`. Leaves `pos` where it started
/// when it answers true, and somewhere along the log when it answers false.
fn can_place_entire_log<W: TreeWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    log_length: i32,
    pos: &mut Pos,
    direction: Pos,
) -> Result<bool, NeedsRules> {
    let mut gap = 0;
    for _ in 0..log_length {
        if !valid_tree_pos(world, tags, *pos)? {
            return Ok(false);
        }
        if is_over_solid_ground(world, *pos)? {
            gap = 0;
        } else {
            gap += 1;
            if gap > MAX_GROUND_GAP {
                return Ok(false);
            }
        }
        pos.0 += direction.0;
        pos.2 += direction.2;
    }
    pos.0 -= direction.0 * log_length;
    pos.2 -= direction.2 * log_length;
    Ok(true)
}

/// `FallenTreeFeature#placeFallenLog` — the logs, sideways, then the log
/// decorators over the set they formed.
fn place_fallen_log<R: RandomSource>(
    config: &FallenTreeConfig,
    world: &mut (dyn crate::feature::configured::FeatureWorld + '_),
    tags: &BlockTags,
    random: &mut R,
    log_length: i32,
    pos: &mut Pos,
    direction: Pos,
) -> Result<(), NeedsRules> {
    // `getSidewaysStateModifier`: `trySetValue(RotatedPillarBlock.AXIS, ...)`,
    // so a trunk block without an axis is written unchanged.
    let axis = if direction.0 == 0 { "z" } else { "x" };
    let mut logs = Placed::default();
    for _ in 0..log_length {
        let at = place_log_block(config, world, tags, random, *pos, Some(axis))?;
        logs.logs
            .insert(at)
            .map_err(|_| NeedsRules("a HashSet bucket reached eight entries".to_string()))?;
        pos.0 += direction.0;
        pos.2 += direction.2;
    }
    decorate(world, tags, random, &logs, &config.log_decorators)
}

/// `FallenTreeFeature#placeLogBlock` — the provider draws, the modifier is
/// applied, the block goes down with flag 3, and the column above is marked.
fn place_log_block<R: RandomSource>(
    config: &FallenTreeConfig,
    world: &mut (dyn crate::feature::configured::FeatureWorld + '_),
    tags: &BlockTags,
    random: &mut R,
    pos: Pos,
    axis: Option<&str>,
) -> Result<Pos, NeedsRules> {
    let state = config
        .trunk_provider
        .state(world, tags, random, pos)
        .map_err(|e| NeedsRules(e.0))?;
    let state = match axis {
        Some(axis) => try_set_axis(&state, axis),
        None => state,
    };
    <dyn crate::feature::configured::FeatureWorld as TreeWorld>::set_block(
        world, pos.0, pos.1, pos.2, &state,
    );
    <dyn crate::feature::configured::FeatureWorld as DiskWorld>::mark_above_for_post_processing(
        world, pos.0, pos.1, pos.2,
    );
    <dyn crate::feature::configured::FeatureWorld as TreeWorld>::set_block(
        world, pos.0, pos.1, pos.2, &state,
    );
    <dyn crate::feature::configured::FeatureWorld as DiskWorld>::mark_above_for_post_processing(
        world, pos.0, pos.1, pos.2,
    );
    Ok(pos)
}

/// `decorateLogs` — nothing at all when the list is empty, which is what
/// keeps a birch stump from paying for a context it never builds.
fn decorate<R: RandomSource>(
    world: &mut (dyn crate::feature::configured::FeatureWorld + '_),
    tags: &BlockTags,
    random: &mut R,
    logs: &Placed,
    decorators: &[TreeDecorator],
) -> Result<(), NeedsRules> {
    if decorators.is_empty() {
        return Ok(());
    }
    // `new TreeDecorator.Context(level, setter, random, logs, Set.of(),
    // Set.of())` — the leaves and roots are empty and all three lists are
    // then stably sorted by y, which `Context::new` does.
    let context = Context::new(logs);
    // Vanilla throws the decorators' own writes away: the setter writes
    // straight to the level and nothing collects them, because a fallen tree
    // has no second pass to run over them.
    let mut written = Placed::default();
    for decorator in decorators {
        decorator::place(world, tags, decorator, random, &context, &mut written)?;
    }
    Ok(())
}

/// `mayPlaceOn` — a tree may occupy it and something holds it up.
fn may_place_on<W: TreeWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    pos: Pos,
) -> Result<bool, NeedsRules> {
    if !valid_tree_pos(world, tags, pos)? {
        return Ok(false);
    }
    is_over_solid_ground(world, pos)
}

/// `isOverSolidGround` — the block below, up face, `SupportType.FULL`.
fn is_over_solid_ground<W: TreeWorld + ?Sized>(
    world: &mut W,
    pos: Pos,
) -> Result<bool, NeedsRules> {
    <W as PredicateWorld>::face_is_sturdy(world, pos.0, pos.1 - 1, pos.2, UP)
        .ok_or_else(|| NeedsRules("isFaceSturdy".to_string()))
}

/// `BlockState#trySetValue(RotatedPillarBlock.AXIS, axis)` — set it if the
/// state has it, leave the state alone if it does not.
fn try_set_axis(state: &str, axis: &str) -> String {
    let Some((name, rest)) = state.split_once('[') else {
        return state.to_string();
    };
    let pairs: Vec<String> = rest
        .trim_end_matches(']')
        .split(',')
        .map(|pair| match pair.split_once('=') {
            Some(("axis", _)) => format!("axis={axis}"),
            _ => pair.to_string(),
        })
        .collect();
    format!("{name}[{}]", pairs.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_axis_is_set_only_when_the_block_has_one() {
        assert_eq!(
            try_set_axis("minecraft:oak_log[axis=y]", "x"),
            "minecraft:oak_log[axis=x]"
        );
        // `trySetValue` on a state without the property is a no-op, not a
        // crash and not an invented property.
        assert_eq!(try_set_axis("minecraft:dirt", "x"), "minecraft:dirt");
        assert_eq!(
            try_set_axis("minecraft:mushroom_stem[down=true]", "z"),
            "minecraft:mushroom_stem[down=true]"
        );
    }

    #[test]
    fn the_horizontal_plane_is_north_east_south_west() {
        // `Direction.Plane.HORIZONTAL` is built from that array in that
        // order and `getRandomDirection` indexes it directly, so a rotation
        // of this list is a different world.
        assert_eq!(HORIZONTAL[0], (0, 0, -1), "NORTH");
        assert_eq!(HORIZONTAL[1], (1, 0, 0), "EAST");
        assert_eq!(HORIZONTAL[2], (0, 0, 1), "SOUTH");
        assert_eq!(HORIZONTAL[3], (-1, 0, 0), "WEST");
    }
}
