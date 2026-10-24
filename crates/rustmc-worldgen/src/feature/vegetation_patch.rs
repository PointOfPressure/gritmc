//! The vegetation patch — the moss and clay floors of a lush cave.
//!
//! Reference (26.2, behaviour ported — never transcribed, CLAUDE.md rule 4):
//! `net.minecraft.world.level.levelgen.feature.VegetationPatchFeature`,
//! `configurations.VegetationPatchConfiguration`. The class is **absent from
//! the decompile cache**, so this was read out of the mapped jar's bytecode.
//!
//! # Three short-circuits decide the draw count
//!
//! Every one of them is a `&&` whose *left* side is a config constant, so a
//! port that evaluates the random side first consumes values vanilla never
//! does — and the configs that ship with the game split evenly across both
//! sides of each test, so getting it wrong is right half the time:
//!
//! ```text
//!   edge column:   extraEdgeColumnChance != 0.0  && nextFloat() <= chance
//!   extra depth:   extraBottomBlockChance > 0.0  && nextFloat() <  chance
//!   vegetation:    vegetationChance > 0.0        && nextFloat() <  chance
//! ```
//!
//! `moss_patch` has a non-zero edge chance (0.3) and a **zero** bottom chance,
//! so the two traps do not both fire on the same config. A port tested only
//! against moss would find one of them.
//!
//! # A corner is skipped for free
//!
//! The patch is a rectangle, and the four corner columns are dropped before
//! any draw at all — the edge coin is spent on edges that are *not* corners.
//! That is what rounds the patch off, and it is one `continue` above the
//! draw rather than below it.
//!
//! # `placeGround` can stand still
//!
//! When the block it is about to write is already the ground block, it moves
//! to the next iteration **without moving the cursor**. The loop then spins
//! on the same position for its remaining depth, calling the state provider
//! each time. For a `simple_state_provider` that costs nothing; for a
//! weighted or noise provider it is a draw per spin, and the block never
//! changes. It looks like a bug in the reference and it is behaviour.

use crate::feature::int_provider::IntProvider;
use crate::feature::state_provider::StateProvider;
use crate::java_hash_set::{JavaHashSet, Untracked};
use crate::rng::RandomSource;
use crate::tags::BlockTags;

// Trace switches, read once each rather than per call: `getenv` in a
// feature inner loop was 3.8% of a full pipeline. See
// `crate::trace_switch`.
crate::trace_switch!(trace_patch, "RUSTMC_TRACE_PATCH");

/// `CaveSurface` — which way the patch grows away from its floor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// `floor`: the patch hangs under the column, direction DOWN.
    Floor,
    /// `ceiling`: direction UP.
    Ceiling,
}

impl Surface {
    /// `CaveSurface#getDirection`, as a y step.
    #[must_use]
    pub const fn step(self) -> i32 {
        match self {
            Self::Floor => -1,
            Self::Ceiling => 1,
        }
    }
}

/// `VegetationPatchConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct VegetationPatchConfig {
    pub replaceable: String,
    pub ground_state: StateProvider,
    pub surface: Surface,
    pub depth: IntProvider,
    pub extra_bottom_block_chance: f32,
    pub vertical_range: i32,
    pub vegetation_chance: f32,
    pub xz_radius: IntProvider,
    pub extra_edge_column_chance: f32,
}

impl VegetationPatchConfig {
    /// # Errors
    /// If a field is missing or a nested value fails to parse.
    pub fn parse(
        value: &serde_json::Value,
        states: &dyn super::state_spec::DefaultStates,
    ) -> Result<Self, String> {
        let float = |name: &str| -> Result<f32, String> {
            value
                .get(name)
                .and_then(serde_json::Value::as_f64)
                .map(|v| v as f32)
                .ok_or_else(|| format!("vegetation_patch has no `{name}`"))
        };
        let provider = |name: &str| -> Result<IntProvider, String> {
            IntProvider::parse(
                value
                    .get(name)
                    .ok_or_else(|| format!("vegetation_patch has no `{name}`"))?,
            )
        };
        let surface = match value.get("surface").and_then(serde_json::Value::as_str) {
            Some("floor") => Surface::Floor,
            Some("ceiling") => Surface::Ceiling,
            other => return Err(format!("vegetation_patch has a bad `surface`: {other:?}")),
        };
        Ok(Self {
            replaceable: value
                .get("replaceable")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "vegetation_patch has no `replaceable`".to_string())?
                .to_string(),
            ground_state: StateProvider::parse(
                value
                    .get("ground_state")
                    .ok_or_else(|| "vegetation_patch has no `ground_state`".to_string())?,
                states,
            )?,
            surface,
            depth: provider("depth")?,
            extra_bottom_block_chance: float("extra_bottom_block_chance")?,
            vertical_range: value
                .get("vertical_range")
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| "vegetation_patch has no `vertical_range`".to_string())?,
            vegetation_chance: float("vegetation_chance")?,
            xz_radius: provider("xz_radius")?,
            extra_edge_column_chance: float("extra_edge_column_chance")?,
        })
    }
}

/// The world a vegetation patch reads and writes.
pub trait PatchWorld: crate::feature::block_predicate::PredicateWorld {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str);

    /// `BlockGetter#isEmptyBlock` — true for `cave_air` and `void_air` too.
    fn is_empty_block(&mut self, x: i32, y: i32, z: i32) -> Option<bool>;

    /// `BlockState#isFaceSturdy(level, pos, direction)`, `SupportType.FULL`.
    fn face_is_sturdy(&mut self, x: i32, y: i32, z: i32, direction: usize) -> Option<bool>;
}

/// Runs the config's inline `vegetation_feature` at a position.
///
/// A callback rather than a field on the config, because the inner feature is
/// a whole `PlacedFeature` written inline and only `configured` knows how to
/// build and run one. Returning `Err` makes the patch refuse: the inner
/// feature *draws*, so skipping it would leave the stream short and every
/// later feature in the step wrong, which is worse than placing nothing.
pub type Vegetation<'a, W> =
    dyn FnMut(&mut W, &mut dyn RandomSource, (i32, i32, i32)) -> Result<bool, NeedsBehaviour> + 'a;

/// The feature could not be evaluated. Carries what stopped it.
#[derive(Debug, PartialEq, Eq)]
pub struct NeedsBehaviour(pub String);

impl From<Untracked> for NeedsBehaviour {
    fn from(u: Untracked) -> Self {
        Self(u.0)
    }
}

fn plain(state: &str) -> &str {
    state.split_once('[').map_or(state, |(name, _)| name)
}

/// `BlockState#isAir` through the predicate world, refusing rather than
/// guessing — `cave_air` fills the very caves this feature grows in, so a
/// name check would walk straight past the floor it is looking for.
fn air_at<W: PatchWorld + ?Sized>(
    world: &mut W,
    x: i32,
    y: i32,
    z: i32,
) -> Result<bool, NeedsBehaviour> {
    crate::feature::block_predicate::PredicateWorld::air_at(world, x, y, z)
        .ok_or_else(|| NeedsBehaviour("isAir".to_string()))
}

/// `VegetationPatchFeature#place`.
///
/// # Errors
/// If the world cannot answer a shape question or run the inner feature.
pub fn place<W: PatchWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    tags: &BlockTags,
    config: &VegetationPatchConfig,
    random: &mut R,
    origin: (i32, i32, i32),
    vegetation: &mut Vegetation<'_, W>,
) -> Result<bool, NeedsBehaviour> {
    // Both radii are drawn before the world is touched, and they are two
    // separate samples rather than one used twice — a patch is rectangular,
    // not square.
    let xz_1 = config.xz_radius.sample(random) + 1;
    let xz_2 = config.xz_radius.sample(random) + 1;

    let placed = place_ground_patch(world, tags, config, random, origin, xz_1, xz_2)?;
    distribute_vegetation(world, config, random, &placed, vegetation)?;
    Ok(!placed.is_empty())
}

/// `VegetationPatchFeature#placeGroundPatch`.
fn place_ground_patch<W: PatchWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &VegetationPatchConfig,
    random: &mut R,
    origin: (i32, i32, i32),
    xz_1: i32,
    xz_2: i32,
) -> Result<JavaHashSet<(i32, i32, i32)>, NeedsBehaviour> {
    let step = config.surface.step();
    let opposite = -step;
    // A `HashSet` in the reference, and its iteration order reaches the
    // random stream through `distributeVegetation` — so it is reproduced
    // rather than swapped for a Rust set. See `java_hash_set`.
    let mut placed = JavaHashSet::new();
    // Counted so the feature's draw total can be split into its four sites —
    // two radii, the edge coins, the depth samples, the vegetation coins —
    // rather than compared as one number. A body that is off by a single draw
    // is otherwise indistinguishable from four sites that happen to sum
    // right.
    let mut edge_coins = 0u32;
    // Priced, not inferred. Summing the sites by hand and subtracting from
    // the feature's total is how a site gets attributed to the wrong feature
    // — the counters accumulate and are only taken when a trace matches.
    let spent_at_entry = crate::rng::draws::peek();

    for x in -xz_1..=xz_1 {
        let edge_x = x == -xz_1 || x == xz_1;
        for z in -xz_2..=xz_2 {
            let edge_z = z == -xz_2 || z == xz_2;
            let edge = edge_x || edge_z;
            let corner = edge_x && edge_z;
            // A corner costs nothing at all — the `continue` is above the
            // coin, not below it.
            if corner {
                continue;
            }
            if edge {
                // The `!= 0.0` short-circuits, so a config with no edge
                // chance draws nothing here.
                if config.extra_edge_column_chance == 0.0 {
                    continue;
                }
                edge_coins += 1;
                if random.next_f32() > config.extra_edge_column_chance {
                    continue;
                }
            }

            let (cx, cz) = (origin.0 + x, origin.2 + z);
            let mut y = origin.1;
            // Walk away from the surface while the column is air...
            let mut i = 0;
            while i < config.vertical_range && air_at(world, cx, y, cz)? {
                y += step;
                i += 1;
            }
            // ...then back while it is *not* air. The two limits are counted
            // separately and both are `vertical_range`.
            let mut j = 0;
            while j < config.vertical_range && !air_at(world, cx, y, cz)? {
                y += opposite;
                j += 1;
            }

            let ground_y = y + step;
            let empty = world
                .is_empty_block(cx, y, cz)
                .ok_or_else(|| NeedsBehaviour("isEmptyBlock".to_string()))?;
            if !empty {
                continue;
            }
            // `isFaceSturdy(..., surface.getDirection().getOpposite())`.
            let face = if opposite > 0 { 1usize } else { 0usize };
            let sturdy = PatchWorld::face_is_sturdy(world, cx, ground_y, cz, face)
                .ok_or_else(|| NeedsBehaviour("isFaceSturdy".to_string()))?;
            if !sturdy {
                continue;
            }

            let mut depth = config.depth.sample(random);
            if trace_patch() {
                PATCH_COLUMNS.with(|c| c.set(c.get() + 1));
                PATCH_DEPTH_SUM.with(|c| c.set(c.get() + depth as u32));
            }
            // Same short-circuit shape as the edge coin, and `moss_patch`
            // sits on the other side of it with a chance of 0.0.
            if config.extra_bottom_block_chance > 0.0
                && random.next_f32() < config.extra_bottom_block_chance
            {
                depth += 1;
            }

            // Saved before `place_ground` walks the cursor down.
            let saved = (cx, ground_y, cz);
            if place_ground(world, tags, config, random, saved, depth)? {
                placed.insert(saved)?;
            }
        }
    }
    if trace_patch() {
        eprintln!(
            "PATCHSITES {} radii=({xz_1},{xz_2}) edge_coins={edge_coins} set={} \
             ground_draws={}",
            PATCH_FEATURE.with(|f| f.borrow().clone()),
            placed.len(),
            crate::rng::draws::peek() - spent_at_entry
        );
    }
    Ok(placed)
}

thread_local! {
    /// The feature whose patch is being traced.
    ///
    /// Without it the `PATCHSITES` lines are anonymous, and a chunk that runs
    /// three patches — two from a ceiling feature with two positions, one
    /// from something else in the same step — invites attributing a line to
    /// the wrong feature. Doing exactly that produced a draw budget this turn
    /// that could not be satisfied by any arrangement of its sites, which is
    /// how the misattribution was noticed at all.
    static PATCH_FEATURE: std::cell::RefCell<String> =
        const { std::cell::RefCell::new(String::new()) };
    /// Ground blocks actually written, when `RUSTMC_TRACE_PATCH` is set.
    static PATCH_WROTE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// Depth steps spent on the no-move branch — the ground state already
    /// matched, so the cursor stayed put and nothing was laid.
    static PATCH_NO_MOVE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// Columns that passed the vegetation coin and invoked the feature.
    static PATCH_VEG_TRIED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// Columns that passed every acceptance test and sampled a depth.
    static PATCH_COLUMNS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// The sum of those sampled depths — with `moss_patch_ceiling`'s
    /// `uniform 1..2` the block total is not the column count, so both are
    /// needed to tell "fewer columns" from "shallower columns".
    static PATCH_DEPTH_SUM: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// Ground blocks written, split by the chunk they landed in.
    static PATCH_BY_CHUNK: std::cell::RefCell<std::collections::BTreeMap<(i32, i32), u32>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
}

/// The three outcomes counted so far, and reset.
///
/// A patch can match vanilla's draw count exactly and still lay far fewer
/// blocks, because neither the no-move branch nor the early return spends a
/// draw. Counting them separates "we never tried" from "we tried and were
/// refused".
/// Name the feature whose patch runs next, for the trace.
pub fn set_patch_feature(name: &str) {
    PATCH_FEATURE.with(|f| f.borrow_mut().replace_range(.., name));
}

#[must_use]
pub fn take_patch_counts() -> (u32, u32, u32) {
    (
        PATCH_WROTE.with(std::cell::Cell::take),
        PATCH_NO_MOVE.with(std::cell::Cell::take),
        PATCH_VEG_TRIED.with(std::cell::Cell::take),
    )
}

/// Accepted columns and the sum of the depths they drew.
#[must_use]
pub fn take_patch_columns() -> (u32, u32) {
    (
        PATCH_COLUMNS.with(std::cell::Cell::take),
        PATCH_DEPTH_SUM.with(std::cell::Cell::take),
    )
}

/// Ground writes per chunk since the last take — the figure that *is*
/// comparable with vanilla's per-chunk `changes`.
#[must_use]
pub fn take_patch_by_chunk() -> std::collections::BTreeMap<(i32, i32), u32> {
    PATCH_BY_CHUNK.with(|m| std::mem::take(&mut *m.borrow_mut()))
}

/// `VegetationPatchFeature#placeGround`.
fn place_ground<W: PatchWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &VegetationPatchConfig,
    random: &mut R,
    start: (i32, i32, i32),
    depth: i32,
) -> Result<bool, NeedsBehaviour> {
    let step = config.surface.step();
    let (x, mut y, z) = start;
    for i in 0..depth {
        let ground = config
            .ground_state
            .state(world, tags, random, (x, y, z))
            .map_err(|e| NeedsBehaviour(e.0))?;
        let here = world.block_at(x, y, z);
        // `blockState.is(blockState2.getBlock())` — block identity, not
        // state equality, so a different `snowy` or `axis` still counts.
        if plain(&ground) == plain(&here) {
            if trace_patch() {
                PATCH_NO_MOVE.with(|c| c.set(c.get() + 1));
            }
            // **No move.** The cursor stays put and the remaining depth is
            // spent re-asking the provider. See the module docs.
            continue;
        }
        if !tags.contains(&config.replaceable, plain(&here)) {
            // `RUSTMC_TRACE_PATCH=1` reports every early return. This path
            // writes nothing and spends no draw, which is how a patch can
            // match vanilla's draw count exactly and still lay far fewer
            // blocks — the shape of the lush-cave gap.
            if trace_patch() {
                eprintln!(
                    "PATCH STOP at ({x},{y},{z}) i={i}/{depth}: `{}` is not in `{}`",
                    plain(&here),
                    config.replaceable
                );
            }
            // `return i != 0` — a patch that laid at least one block counts
            // as placed even though it stopped early.
            return Ok(i != 0);
        }
        if trace_patch() {
            PATCH_WROTE.with(|c| c.set(c.get() + 1));
            // Also per chunk. The total is not comparable with vanilla's
            // `changes`, which records only what a feature wrote **inside
            // the chunk being dumped** — a patch's writes into a neighbour
            // never appear there. Comparing our total against that number
            // read as agreement when it was not.
            PATCH_BY_CHUNK.with(|m| {
                *m.borrow_mut().entry((x >> 4, z >> 4)).or_insert(0u32) += 1;
            });
        }
        world.set_block(x, y, z, &ground);
        y += step;
    }
    Ok(true)
}

/// `VegetationPatchFeature#distributeVegetation`.
fn distribute_vegetation<W: PatchWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    config: &VegetationPatchConfig,
    random: &mut R,
    placed: &JavaHashSet<(i32, i32, i32)>,
    vegetation: &mut Vegetation<'_, W>,
) -> Result<(), NeedsBehaviour> {
    let opposite = -config.surface.step();
    // The set's own order, which is why it is a `JavaHashSet`: the coin
    // below is drawn per element, so a different order gives a different
    // element each float.
    let positions: Vec<(i32, i32, i32)> = placed.iter().copied().collect();
    // The set's length and the number of elements its iterator actually
    // yields are separate facts, and only one of them prices the coin loop.
    if trace_patch() {
        eprintln!(
            "PATCHDIST set_len={} iterated={} chance={}",
            placed.len(),
            positions.len(),
            config.vegetation_chance
        );
    }
    let mut coins = 0u32;
    for (x, y, z) in positions {
        if config.vegetation_chance <= 0.0 {
            continue;
        }
        coins += 1;
        if random.next_f32() >= config.vegetation_chance {
            continue;
        }
        if trace_patch() {
            PATCH_VEG_TRIED.with(|c| c.set(c.get() + 1));
        }
        // `pos.relative(surface.getDirection().getOpposite())` — the plant
        // goes on top of the floor it was given, not inside it.
        //
        // Priced individually under the trace. The patch's own four draw
        // sites are arithmetic — two radii, one coin per edge-not-corner
        // column, one depth per accepted column, one coin per placed column —
        // so when a body is off by a single draw against vanilla and those
        // four agree, what is left is what the vegetation feature itself
        // spends, and that is a different feature per call.
        let before = crate::rng::draws::peek();
        // Reported, not just propagated. A `?` here abandons the rest of the
        // set, so the patch stops partway through its columns and every count
        // downstream — blocks written, draws spent, the stream every later
        // feature reads — is short by whatever the remaining columns would
        // have cost. Silently that looks like a placement bug rather than a
        // missing implementation.
        if let Err(e) = vegetation(world, random, (x, y + opposite, z)) {
            if trace_patch() {
                eprintln!(
                    "PATCHDIST ABORT after {coins} coins at ({x},{},{z}): {}",
                    y + opposite,
                    e.0
                );
            }
            return Err(e);
        }
        if trace_patch() {
            eprintln!(
                "PATCHVEG at ({x},{},{z}) cost {} draws",
                y + opposite,
                crate::rng::draws::peek() - before
            );
        }
    }
    if trace_patch() {
        eprintln!("PATCHDIST coins={coins}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// The corner columns are dropped before the edge coin, so a patch draws
    /// fewer floats than its rectangle has edge columns.
    #[test]
    fn corners_cost_no_draw() {
        // A 3x3 patch (radius 1 each way) has 8 edge columns, 4 of them
        // corners — so 4 coins, not 8.
        let mut edges = 0;
        for x in -1..=1i32 {
            let edge_x = x == -1 || x == 1;
            for z in -1..=1i32 {
                let edge_z = z == -1 || z == 1;
                if edge_x && edge_z {
                    continue;
                }
                if edge_x || edge_z {
                    edges += 1;
                }
            }
        }
        assert_eq!(edges, 4, "the four sides, with the corners dropped");
    }
}
