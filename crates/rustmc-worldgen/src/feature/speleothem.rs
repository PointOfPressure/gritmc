//! Speleothems — a single stalactite or stalagmite, with its root patch.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `SpeleothemFeature`, `SpeleothemUtils`, `SpeleothemConfiguration`,
//! `SpeleothemThickness`.
//!
//! # Which way it points is a draw only when it could go either way
//!
//! ```text
//! ceiling = isBase(above)
//! floor   = isBase(below)
//! if (ceiling && floor) return nextBoolean() ? DOWN : UP;
//! if (ceiling)          return DOWN;
//! if (floor)            return UP;
//! return empty;
//! ```
//!
//! The coin is spent **only** in a one-block gap with rock on both sides.
//! Hanging from a ceiling costs nothing and standing on a floor costs
//! nothing, so a port that always drew — the obvious way to write it — would
//! consume one extra value at almost every speleothem in the world.
//!
//! # The root patch's draws are nested, not parallel
//!
//! `createPatchOfBaseBlocks` walks the four horizontal directions and, per
//! direction, spends up to three draws in a chain: spread at all, then
//! radius 2, then radius 3. Each is gated on the one before, and radius 2 and
//! 3 each also draw a **random direction** to step in. So one direction can
//! cost anywhere from one draw to five, and how many depends on values drawn
//! inside the loop.
//!
//! # Thickness is a shape, not a size
//!
//! `buildBaseToTipColumn` emits BASE, then `height - 3` MIDDLEs, then
//! FRUSTUM, then TIP — but each segment is separately gated on the height, so
//! a two-block speleothem is FRUSTUM+TIP with no base at all. Writing it as
//! "base, fill, tip" gives the right length and the wrong blocks.

use super::block_predicate::PredicateWorld;
use super::state_spec::DefaultStates;
use crate::rng::RandomSource;
use crate::tags::BlockTags;

/// `SpeleothemThickness`, in the order the column is built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Thickness {
    Base,
    Middle,
    Frustum,
    Tip,
    TipMerge,
}

impl Thickness {
    const fn name(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Middle => "middle",
            Self::Frustum => "frustum",
            Self::Tip => "tip",
            Self::TipMerge => "tip_merge",
        }
    }
}

/// `SpeleothemConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct SpeleothemConfig {
    /// The block the speleothem grows *out of* — `sulfur`, `dripstone_block`.
    pub base_block: String,
    /// The pointed block, e.g. `minecraft:sulfur_spike`.
    pub pointed_block: String,
    pub chance_of_taller_generation: f32,
    pub chance_of_directional_spread: f32,
    pub chance_of_spread_radius2: f32,
    pub chance_of_spread_radius3: f32,
    /// `replaceable_blocks`, a `HolderSet` — a tag or a list.
    pub replaceable: Vec<String>,
    pub replaceable_tag: Option<String>,
}

impl SpeleothemConfig {
    /// # Errors
    /// If a field is missing or malformed.
    pub fn parse(config: &serde_json::Value, states: &dyn DefaultStates) -> Result<Self, String> {
        let state = |name: &str| -> Result<String, String> {
            super::state_spec::canonical(
                config
                    .get(name)
                    .ok_or_else(|| format!("speleothem config has no `{name}`"))?,
                states,
            )
        };
        // All four chances are `optionalFieldOf` with a default, and
        // `pointed_dripstone` omits every one of them — so a parser that
        // required them refused the only dripstone feature in the game.
        // The defaults are read off the codec, not remembered.
        let chance = |name: &str, default: f32| -> f32 {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "the codec's fields are floats, compared against nextFloat()"
            )]
            let v = config
                .get(name)
                .and_then(serde_json::Value::as_f64)
                .map_or(default, |v| v as f32);
            v
        };
        let replaceable_value = config
            .get("replaceable_blocks")
            .ok_or_else(|| "speleothem config has no `replaceable_blocks`".to_string())?;
        let (replaceable, replaceable_tag) = match replaceable_value {
            serde_json::Value::String(one) => match one.strip_prefix('#') {
                Some(tag) => (Vec::new(), Some(tag.to_string())),
                None => (vec![one.clone()], None),
            },
            serde_json::Value::Array(list) => (
                list.iter()
                    .map(|v| {
                        v.as_str()
                            .map(ToString::to_string)
                            .ok_or_else(|| "replaceable_blocks entry is not a name".to_string())
                    })
                    .collect::<Result<Vec<_>, String>>()?,
                None,
            ),
            other => {
                return Err(format!(
                    "replaceable_blocks is neither a name nor a list: {other}"
                ))
            }
        };
        Ok(Self {
            base_block: state("base_block")?,
            pointed_block: state("pointed_block")?,
            chance_of_taller_generation: chance("chance_of_taller_generation", 0.2),
            chance_of_directional_spread: chance("chance_of_directional_spread", 0.7),
            chance_of_spread_radius2: chance("chance_of_spread_radius2", 0.5),
            chance_of_spread_radius3: chance("chance_of_spread_radius3", 0.5),
            replaceable,
            replaceable_tag,
        })
    }

    #[must_use]
    pub fn is_supported(&self) -> bool {
        true
    }
}

/// The three block fields every speleothem-shaped feature carries.
///
/// `SpeleothemConfiguration` and `SpeleothemClusterConfiguration` are separate
/// records in the reference and share no supertype, but `SpeleothemUtils` —
/// `isBase`, `placeBaseBlockIfPossible`, `growSpeleothem`, `createPointedBlock`
/// — is written against these three fields alone and is called by both. A
/// trait rather than a second copy of the helpers: the cluster's tips and the
/// single speleothem's tips must be the same blocks, and two copies is how
/// they stop being.
pub trait SpeleothemBlocks {
    /// The block the speleothem grows *out of*.
    fn base_block(&self) -> &str;
    /// The pointed block, with its default properties.
    fn pointed_block(&self) -> &str;
    /// `replaceable_blocks` written as a list.
    fn replaceable(&self) -> &[String];
    /// `replaceable_blocks` written as a tag, without the `#`.
    fn replaceable_tag(&self) -> Option<&str>;

    /// The base block's plain name, which is what `isBase` compares.
    fn base_name(&self) -> &str {
        plain(self.base_block())
    }
}

impl SpeleothemBlocks for SpeleothemConfig {
    fn base_block(&self) -> &str {
        &self.base_block
    }
    fn pointed_block(&self) -> &str {
        &self.pointed_block
    }
    fn replaceable(&self) -> &[String] {
        &self.replaceable
    }
    fn replaceable_tag(&self) -> Option<&str> {
        self.replaceable_tag.as_deref()
    }
}

/// The world a speleothem reads and writes.
pub trait SpeleothemWorld: PredicateWorld {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str);
    /// `LevelAccessor#isWaterAt` — for the pointed block's `waterlogged`.
    fn is_water_at(&mut self, x: i32, y: i32, z: i32) -> bool;
}

/// The feature could not be evaluated.
#[derive(Debug, PartialEq, Eq)]
pub struct NeedsRules(pub String);

pub(crate) fn plain(state: &str) -> &str {
    state.split_once('[').map_or(state, |(n, _)| n)
}

/// `Direction.values()` offsets, in vanilla's order.
pub(crate) const DIRECTIONS: [(i32, i32, i32); 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];
/// `Direction.Plane.HORIZONTAL`, in `forEach` order.
const HORIZONTAL: [(i32, i32, i32); 4] = [(0, 0, -1), (1, 0, 0), (0, 0, 1), (-1, 0, 0)];

/// `SpeleothemUtils#isBase` — the base block itself, or anything the config
/// calls replaceable.
pub(crate) fn is_base<W: SpeleothemWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    blocks: &dyn SpeleothemBlocks,
    pos: (i32, i32, i32),
) -> bool {
    let state = world.block_at(pos.0, pos.1, pos.2);
    let name = plain(&state);
    name == blocks.base_name() || blocks_are_replaceable(tags, blocks, name)
}

pub(crate) fn is_replaceable(
    tags: &BlockTags,
    tag: Option<&str>,
    list: &[String],
    name: &str,
) -> bool {
    tag.map_or_else(
        || list.iter().any(|b| b == name),
        |tag| tags.contains(tag, name),
    )
}

/// The same question asked of a config that carries both spellings.
pub(crate) fn blocks_are_replaceable(
    tags: &BlockTags,
    blocks: &dyn SpeleothemBlocks,
    name: &str,
) -> bool {
    is_replaceable(tags, blocks.replaceable_tag(), blocks.replaceable(), name)
}

/// `SpeleothemUtils#isEmptyOrWater` — air, or the water *block*.
pub(crate) fn is_empty_or_water<W: SpeleothemWorld + ?Sized>(
    world: &mut W,
    pos: (i32, i32, i32),
) -> Result<bool, NeedsRules> {
    if world
        .air_at(pos.0, pos.1, pos.2)
        .ok_or_else(|| NeedsRules("isAir".to_string()))?
    {
        return Ok(true);
    }
    let state = world.block_at(pos.0, pos.1, pos.2);
    Ok(plain(&state) == "minecraft:water")
}

/// `FluidState#is(FluidTags.WATER)`.
///
/// `#minecraft:fluid/water` holds **two** fluids — `water` and
/// `flowing_water` — read out of `data/minecraft/tags/fluid/water.json` in
/// the pinned jar, and `LevelReader#isWaterAt` is
/// `getFluidState(pos).is(FluidTags.WATER)`. A predicate that tests only
/// `minecraft:water` therefore answers *false* on a flowing source, where
/// vanilla answers true.
///
/// **This lives here, once, because it was previously written twice and the
/// two copies disagreed.** `speleothem_cluster`'s `canPlacePool` path had it
/// right and the `SpeleothemWorld::is_water_at` impl in `stage/decorate.rs`
/// had it narrow, so a speleothem crossing *flowing* water was left
/// un-waterlogged while a pool two blocks away was refused correctly. A
/// correction that lives only in the file it corrected does not travel.
#[must_use]
pub(crate) fn is_water_fluid(fluid: &str) -> bool {
    fluid == "minecraft:water" || fluid == "minecraft:flowing_water"
}

/// `SpeleothemUtils#isEmptyOrWaterOrLava` — air, the water *block*, or the
/// lava *block*.
///
/// One block name wider than [`is_empty_or_water`], and the difference is the
/// whole reason `large_dripstone` can grow a column down into a lava lake's
/// ceiling and stop: the embedding test treats lava as open, the base test
/// treats it as fatal.
pub(crate) fn is_empty_or_water_or_lava<W: SpeleothemWorld + ?Sized>(
    world: &mut W,
    pos: (i32, i32, i32),
) -> Result<bool, NeedsRules> {
    if world
        .air_at(pos.0, pos.1, pos.2)
        .ok_or_else(|| NeedsRules("isAir".to_string()))?
    {
        return Ok(true);
    }
    let name = world.block_at(pos.0, pos.1, pos.2);
    let name = plain(&name);
    Ok(name == "minecraft:water" || name == "minecraft:lava")
}

/// `SpeleothemUtils#isCircleMostlyEmbeddedInStone`.
///
/// Samples a ring of `2 * PI * radius / 6` positions — the step is
/// `6.0 / radius` radians, so the *count* of samples is fixed at about
/// `2 * PI / (6 / radius)` and grows with the radius while the arc between
/// them stays six blocks. Not a full circle test: it is `false` as soon as
/// any one sample is open, and the centre is tested first.
///
/// **Named "mostly" and implemented as "entirely", on a coarse ring.** The
/// slack is in the sampling, not in a threshold, so a column can be accepted
/// with a gap narrower than the arc — reproducing the ring exactly is the
/// only way to reproduce which columns are accepted.
///
/// The angle accumulates in `f32` by repeated addition. Recomputing it as
/// `i as f32 * step` would be a different sequence of roundings and a
/// different last sample.
pub(crate) fn is_circle_mostly_embedded_in_stone<W: SpeleothemWorld + ?Sized>(
    world: &mut W,
    pos: (i32, i32, i32),
    radius: i32,
) -> Result<bool, NeedsRules> {
    if is_empty_or_water_or_lava(world, pos)? {
        return Ok(false);
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "`(float) i` — the reference widens the radius here"
    )]
    let step = 6.0_f32 / radius as f32;
    let mut angle = 0.0_f32;
    while angle < std::f32::consts::TAU {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_precision_loss,
            reason = "`(int) (Mth.cos(f) * (float) i)` — the reference's own widths"
        )]
        let (dx, dz) = (
            (crate::math::cos(f64::from(angle)) * radius as f32) as i32,
            (crate::math::sin(f64::from(angle)) * radius as f32) as i32,
        );
        if is_empty_or_water_or_lava(world, (pos.0 + dx, pos.1, pos.2 + dz))? {
            return Ok(false);
        }
        angle += step;
    }
    Ok(true)
}

/// `SpeleothemUtils#getSpeleothemHeight`.
///
/// The shape of a large speleothem: `radius` is how far out from its axis the
/// question is asked, `max_radius` its widest, `scale` and `bluntness` the two
/// sampled parameters. Returns the height of the column at that radius.
///
/// `bluntness` is a *floor on the radius*, not a term — a blunt speleothem is
/// one whose tip is evaluated as though it were already `bluntness` blocks
/// wide, so it stops narrowing and ends flat. That is why the first line
/// raises `radius` rather than subtracting anything.
///
/// `Math.pow` and `Math.log` are the JDK's, not a table, so this is the one
/// place in the family where the answer depends on libm being IEEE-correct.
/// Rust's `f64::powf`/`ln` are the same StrictMath-equivalent operations.
#[must_use]
pub(crate) fn get_speleothem_height(
    radius: f64,
    max_radius: f64,
    scale: f64,
    bluntness: f64,
) -> f64 {
    let radius = if radius < bluntness {
        bluntness
    } else {
        radius
    };
    let normalised = radius / max_radius * 0.384;
    let a = 0.75 * normalised.powf(1.333_333_333_333_333_3);
    let b = normalised.powf(0.666_666_666_666_666_6);
    let c = 0.333_333_333_333_333_3 * normalised.ln();
    let height = crate::math::java_max(scale * (a - b - c), 0.0);
    height / 0.384 * max_radius
}

/// `net.minecraft.world.level.levelgen.Column`, as the two values it carries.
///
/// The reference has three subclasses — `Range`, `Ray`, `Line` — and which one
/// `Column.create` returns is decided entirely by which of the two ends is
/// present. Collapsing them to a pair of `Option`s keeps that and drops the
/// dispatch; the only behaviour that depended on the subclass was `getHeight`,
/// which `Range` answers and the other two decline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Column {
    pub(crate) floor: Option<i32>,
    pub(crate) ceiling: Option<i32>,
}

impl Column {
    /// `Range#height` — the number of *empty* blocks between the two ends, so
    /// a floor at 10 and a ceiling at 14 is a height of three.
    ///
    /// Only a `Range` has one. `Ray` and `Line` return empty, which is why a
    /// one-ended column can never have merging tips — and why
    /// `large_dripstone`, which needs a `Range` and nothing else, can test
    /// for one by asking for a height.
    pub(crate) const fn height(self) -> Option<i32> {
        match (self.floor, self.ceiling) {
            (Some(floor), Some(ceiling)) => Some(ceiling - floor - 1),
            _ => None,
        }
    }
}

/// The end predicate `Column#scan` is given.
///
/// A named type rather than the signature written out twice: `clippy` calls
/// the inline form "very complex", and the two functions that take it must
/// agree exactly or a caller silently gets the wrong one.
pub(crate) type ColumnTip<'a, W> =
    dyn FnMut(&mut W, (i32, i32, i32)) -> Result<bool, NeedsRules> + 'a;

/// `Column#scan`.
///
/// The column predicate is `isEmptyOrWater` for every caller in the game, so
/// it is fixed here; the **end** predicate is not — the cluster asks for
/// `isNeitherEmptyNorWater` and `large_dripstone` asks whether the block is in
/// its config's `replaceable_blocks`. That difference is the whole reason this
/// takes a callback instead of being written twice: a stalactite may hang off
/// dripstone or stone, and a cluster may hang off anything solid at all.
///
/// The scan starts *at* the origin and moves outwards at most
/// `range - 1` blocks in each direction, so a `range` of 1 finds nothing at
/// all. An end is only recorded if the block the walk stopped on satisfies
/// `tip` — running out of range in open air yields no end rather than the last
/// empty block, which is what makes a column in a large cavern one-ended.
///
/// UP is scanned before DOWN, which is the order the reference passes to
/// `Column.create(floor, ceiling)` after evaluating both.
pub(crate) fn scan<W: SpeleothemWorld + ?Sized>(
    world: &mut W,
    pos: (i32, i32, i32),
    range: i32,
    tip: &mut ColumnTip<'_, W>,
) -> Result<Option<Column>, NeedsRules> {
    if !is_empty_or_water(world, pos)? {
        return Ok(None);
    }
    let ceiling = scan_direction(world, pos, range, 1, tip)?;
    let floor = scan_direction(world, pos, range, -1, tip)?;
    Ok(Some(Column { floor, ceiling }))
}

/// `Column#scanDirection`. `step` is `+1` for `Direction.UP`, `-1` for `DOWN`.
fn scan_direction<W: SpeleothemWorld + ?Sized>(
    world: &mut W,
    pos: (i32, i32, i32),
    range: i32,
    step: i32,
    tip: &mut ColumnTip<'_, W>,
) -> Result<Option<i32>, NeedsRules> {
    let mut y = pos.1;
    let mut i = 1;
    while i < range && is_empty_or_water(world, (pos.0, y, pos.2))? {
        y += step;
        i += 1;
    }
    if tip(world, (pos.0, y, pos.2))? {
        Ok(Some(y))
    } else {
        Ok(None)
    }
}

/// `SpeleothemUtils::isNeitherEmptyNorWater`, as a `Column#scan` end
/// predicate — the cluster's.
pub(crate) fn neither_empty_nor_water<W: SpeleothemWorld + ?Sized>(
    world: &mut W,
    pos: (i32, i32, i32),
) -> Result<bool, NeedsRules> {
    is_empty_or_water(world, pos).map(|open| !open)
}

/// `SpeleothemFeature#place`.
///
/// # Errors
/// If the world cannot answer a block question.
pub fn place<W: SpeleothemWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &SpeleothemConfig,
    random: &mut dyn RandomSource,
    origin: (i32, i32, i32),
) -> Result<bool, NeedsRules> {
    // `getTipDirection`. `dir` is the direction the *tip* points.
    let ceiling = is_base(world, tags, config, (origin.0, origin.1 + 1, origin.2));
    let floor = is_base(world, tags, config, (origin.0, origin.1 - 1, origin.2));
    // DOWN is index 0, UP is index 1 in `Direction.values()`.
    let tip = if ceiling && floor {
        // The only case that draws. See the module docs.
        if random.next_bool() {
            0
        } else {
            1
        }
    } else if ceiling {
        0
    } else if floor {
        1
    } else {
        return Ok(false);
    };

    // The root sits *behind* the tip.
    let opposite = if tip == 0 { 1 } else { 0 };
    let root = offset(origin, opposite);
    create_patch_of_base_blocks(world, tags, config, random, root);

    // Two blocks tall only if the coin says so *and* there is room. The `&&`
    // is short-circuiting in Java too, so a failed coin means the room is
    // never checked — and checking it first would read a block vanilla did
    // not read, which matters to nothing here and would matter to a proxy.
    let taller = random.next_f32() < config.chance_of_taller_generation
        && is_empty_or_water(world, offset(origin, tip))?;
    let height = if taller { 2 } else { 1 };

    grow_speleothem(world, tags, config, origin, tip, height, false);
    Ok(true)
}

pub(crate) fn offset((x, y, z): (i32, i32, i32), direction: usize) -> (i32, i32, i32) {
    let (dx, dy, dz) = DIRECTIONS[direction];
    (x + dx, y + dy, z + dz)
}

/// `SpeleothemFeature#createPatchOfBaseBlocks`.
///
/// The centre first, then a nested chain per horizontal direction. See the
/// module docs on why the draw count is not four.
fn create_patch_of_base_blocks<W: SpeleothemWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &SpeleothemConfig,
    random: &mut dyn RandomSource,
    root: (i32, i32, i32),
) {
    place_base_block_if_possible(world, tags, config, root);
    for (dx, dy, dz) in HORIZONTAL {
        if random.next_f32() > config.chance_of_directional_spread {
            continue;
        }
        let first = (root.0 + dx, root.1 + dy, root.2 + dz);
        place_base_block_if_possible(world, tags, config, first);

        if random.next_f32() > config.chance_of_spread_radius2 {
            continue;
        }
        // `Direction.getRandom(random)` — one draw over all six.
        let second = offset(first, random_direction(random));
        place_base_block_if_possible(world, tags, config, second);

        if random.next_f32() > config.chance_of_spread_radius3 {
            continue;
        }
        let third = offset(second, random_direction(random));
        place_base_block_if_possible(world, tags, config, third);
    }
}

/// `Direction.getRandom` — `values()[nextInt(6)]`.
fn random_direction(random: &mut dyn RandomSource) -> usize {
    usize::try_from(random.next_i32_bound(6)).unwrap_or(0)
}

/// `SpeleothemUtils#placeBaseBlockIfPossible` — only over a *replaceable*
/// block, and the base block itself does not qualify.
pub(crate) fn place_base_block_if_possible<W: SpeleothemWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    blocks: &dyn SpeleothemBlocks,
    pos: (i32, i32, i32),
) -> bool {
    let state = world.block_at(pos.0, pos.1, pos.2);
    if !blocks_are_replaceable(tags, blocks, plain(&state)) {
        return false;
    }
    let base = blocks.base_block().to_string();
    world.set_block(pos.0, pos.1, pos.2, &base);
    true
}

/// `SpeleothemUtils#growSpeleothem`.
pub(crate) fn grow_speleothem<W: SpeleothemWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    blocks: &dyn SpeleothemBlocks,
    origin: (i32, i32, i32),
    tip: usize,
    height: i32,
    merge_tip: bool,
) {
    // The block behind the tip must still be base *after* the patch ran — a
    // patch that replaced nothing there means nothing to grow from.
    let opposite = if tip == 0 { 1 } else { 0 };
    if !is_base(world, tags, blocks, offset(origin, opposite)) {
        return;
    }
    let mut cursor = origin;
    for thickness in base_to_tip_column(height, merge_tip) {
        let state = pointed_block(blocks, tip, thickness);
        // The tip's `waterlogged` is decided per block, from the block it is
        // replacing — a speleothem crossing a waterline is half wet.
        let state = if world.is_water_at(cursor.0, cursor.1, cursor.2) {
            with_property(&state, "waterlogged", "true")
        } else {
            state
        };
        world.set_block(cursor.0, cursor.1, cursor.2, &state);
        cursor = offset(cursor, tip);
    }
}

/// `SpeleothemUtils#buildBaseToTipColumn`, as the sequence it emits.
///
/// Each segment is separately gated on the height, so a two-block speleothem
/// is FRUSTUM+TIP and has no base at all.
fn base_to_tip_column(height: i32, merge_tip: bool) -> Vec<Thickness> {
    let mut out = Vec::new();
    if height >= 3 {
        out.push(Thickness::Base);
        for _ in 0..height - 3 {
            out.push(Thickness::Middle);
        }
    }
    if height >= 2 {
        out.push(Thickness::Frustum);
    }
    if height >= 1 {
        out.push(if merge_tip {
            Thickness::TipMerge
        } else {
            Thickness::Tip
        });
    }
    out
}

/// `SpeleothemUtils#createPointedBlock`.
fn pointed_block(blocks: &dyn SpeleothemBlocks, tip: usize, thickness: Thickness) -> String {
    let direction = if tip == 0 { "down" } else { "up" };
    let state = with_property(blocks.pointed_block(), "vertical_direction", direction);
    with_property(&state, "thickness", thickness.name())
}

/// `state.setValue(property, value)` on the canonical spelling.
fn with_property(state: &str, property: &str, value: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;
    use std::collections::BTreeMap;

    /// `#minecraft:fluid/water` has two members, and the second one is the
    /// whole point: the narrow predicate this replaced tested only
    /// `minecraft:water`, so it answered *false* on a flowing source where
    /// `LevelReader#isWaterAt` answers true. Both members are asserted, so
    /// a regression to the narrow form fails here rather than in a region
    /// diff a week later.
    #[test]
    fn water_fluid_tag_holds_flowing_water_too() {
        assert!(is_water_fluid("minecraft:water"), "the still source");
        assert!(
            is_water_fluid("minecraft:flowing_water"),
            "the flowing source — this is the member that was missing"
        );
        assert!(!is_water_fluid("minecraft:lava"));
        assert!(!is_water_fluid("minecraft:flowing_lava"));
        assert!(!is_water_fluid("minecraft:empty"));
        // Not a prefix match: `water` must not be inferred from a name that
        // merely contains it.
        assert!(!is_water_fluid("minecraft:waterlogged"));
    }

    struct Cave {
        blocks: BTreeMap<(i32, i32, i32), String>,
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
    }

    impl SpeleothemWorld for Cave {
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
            self.blocks.insert((x, y, z), state.to_string());
        }
        fn is_water_at(&mut self, x: i32, y: i32, z: i32) -> bool {
            self.block_at(x, y, z).starts_with("minecraft:water")
        }
    }

    fn tags() -> BlockTags {
        BlockTags::from_entries(BTreeMap::from([(
            "minecraft:sulfur_spike_replaceable_blocks".to_string(),
            vec![
                "minecraft:stone".to_string(),
                "minecraft:deepslate".to_string(),
            ],
        )]))
        .expect("tags")
    }

    fn states() -> crate::feature::state_spec::DefaultStateTable {
        crate::feature::state_spec::DefaultStateTable::from_pairs(&[
            ("minecraft:sulfur", "minecraft:sulfur"),
            (
                "minecraft:sulfur_spike",
                "minecraft:sulfur_spike[thickness=tip,vertical_direction=up,waterlogged=false]",
            ),
        ])
    }

    fn config() -> SpeleothemConfig {
        SpeleothemConfig::parse(
            // `r##` rather than `r#`: the JSON contains `"#minecraft:...` for
            // the tag, and `"#` would close an `r#` string in the middle of a
            // tag name.
            &serde_json::from_str(
                r##"{"base_block":{"Name":"minecraft:sulfur"},
                    "pointed_block":{"Name":"minecraft:sulfur_spike"},
                    "chance_of_taller_generation":0.2,
                    "chance_of_directional_spread":0.7,
                    "chance_of_spread_radius2":0.5,
                    "chance_of_spread_radius3":0.5,
                    "replaceable_blocks":"#minecraft:sulfur_spike_replaceable_blocks"}"##,
            )
            .expect("json"),
            &states(),
        )
        .expect("parse")
    }

    /// Nothing above and nothing below: no direction, and the feature refuses
    /// without drawing at all.
    #[test]
    fn open_air_refuses_for_free() {
        let mut world = Cave {
            blocks: BTreeMap::new(),
        };
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(4));
        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(4));
        assert!(!place(&mut world, &tags(), &config(), &mut random, (0, 0, 0)).expect("ok"));
        assert_eq!(random.next_i64(), probe.next_i64());
    }

    /// A ceiling alone costs no coin — the direction is forced.
    #[test]
    fn a_ceiling_does_not_spend_the_coin() {
        let mut world = Cave {
            blocks: BTreeMap::new(),
        };
        world
            .blocks
            .insert((0, 1, 0), "minecraft:stone".to_string());
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(4));
        assert!(place(&mut world, &tags(), &config(), &mut random, (0, 0, 0)).expect("ok"));

        // The first draw is the patch's, not a direction coin: reproduce the
        // sequence without a `nextBoolean` and the streams agree.
        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(4));
        let mut expected = Cave {
            blocks: BTreeMap::from([((0, 1, 0), "minecraft:stone".to_string())]),
        };
        create_patch_of_base_blocks(&mut expected, &tags(), &config(), &mut probe, (0, 1, 0));
        probe.next_f32();
        assert_eq!(random.next_i64(), probe.next_i64());
    }

    /// Rock above *and* below is the only case that spends the coin.
    #[test]
    fn a_one_block_gap_spends_the_coin() {
        let mut with_coin = Cave {
            blocks: BTreeMap::from([
                ((0, 1, 0), "minecraft:stone".to_string()),
                ((0, -1, 0), "minecraft:stone".to_string()),
            ]),
        };
        let mut a = WorldgenRandom::new(LegacyRandomSource::new(4));
        place(&mut with_coin, &tags(), &config(), &mut a, (0, 0, 0)).expect("ok");

        let mut ceiling_only = Cave {
            blocks: BTreeMap::from([((0, 1, 0), "minecraft:stone".to_string())]),
        };
        let mut b = WorldgenRandom::new(LegacyRandomSource::new(4));
        place(&mut ceiling_only, &tags(), &config(), &mut b, (0, 0, 0)).expect("ok");

        assert_ne!(
            a.next_i64(),
            b.next_i64(),
            "the gap spends one draw the ceiling does not"
        );
    }

    /// A one-block speleothem is a bare TIP; two is FRUSTUM then TIP; four is
    /// BASE, MIDDLE, FRUSTUM, TIP. Each segment is gated separately, which is
    /// why the short ones have no base.
    #[test]
    fn the_column_shape_is_gated_segment_by_segment() {
        assert_eq!(base_to_tip_column(1, false), vec![Thickness::Tip]);
        assert_eq!(
            base_to_tip_column(2, false),
            vec![Thickness::Frustum, Thickness::Tip]
        );
        assert_eq!(
            base_to_tip_column(3, false),
            vec![Thickness::Base, Thickness::Frustum, Thickness::Tip]
        );
        assert_eq!(
            base_to_tip_column(4, false),
            vec![
                Thickness::Base,
                Thickness::Middle,
                Thickness::Frustum,
                Thickness::Tip
            ]
        );
        assert_eq!(base_to_tip_column(1, true), vec![Thickness::TipMerge]);
    }

    /// The spike points away from what it grew out of, and it is waterlogged
    /// per block rather than per speleothem.
    #[test]
    fn a_spike_points_away_and_is_waterlogged_per_block() {
        let mut world = Cave {
            blocks: BTreeMap::from([
                ((0, 1, 0), "minecraft:stone".to_string()),
                ((0, 0, 0), "minecraft:water[level=0]".to_string()),
            ]),
        };
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(9));
        place(&mut world, &tags(), &config(), &mut random, (0, 0, 0)).expect("ok");
        let placed = world.blocks.get(&(0, 0, 0)).expect("a spike");
        assert!(placed.starts_with("minecraft:sulfur_spike"), "{placed}");
        assert!(
            placed.contains("vertical_direction=down"),
            "hanging from the ceiling: {placed}"
        );
        assert!(placed.contains("waterlogged=true"), "{placed}");
    }

    #[test]
    fn every_speleothem_config_in_the_datapack_parses() {
        let Some(extract) = crate::feature::state_spec::extract_or_skip() else {
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
            // Speleothems are never a top-level file: every one is written
            // *inline* inside a `simple_random_selector`, the same way every
            // lake but one is inline inside a `sequence`. A scan that only
            // looked at `json["type"]` found none and concluded there were
            // none.
            walk(&json, &extract, &mut seen, &path);
        }
        assert!(seen >= 2, "only {seen} speleothem configs parsed");
        println!("SPELEOTHEM CONFIGS: {seen} parsed, all inline");
    }

    /// Every `minecraft:speleothem` anywhere in a document, however nested.
    fn walk(
        value: &serde_json::Value,
        extract: &crate::feature::state_spec::DefaultStateTable,
        seen: &mut usize,
        path: &std::path::Path,
    ) {
        match value {
            serde_json::Value::Object(map) => {
                if map.get("type").and_then(serde_json::Value::as_str)
                    == Some("minecraft:speleothem")
                {
                    let config = map.get("config").unwrap_or(&serde_json::Value::Null);
                    SpeleothemConfig::parse(config, extract)
                        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                    *seen += 1;
                }
                for v in map.values() {
                    walk(v, extract, seen, path);
                }
            }
            serde_json::Value::Array(list) => {
                for v in list {
                    walk(v, extract, seen, path);
                }
            }
            _ => {}
        }
    }
}
