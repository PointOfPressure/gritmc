//! `MossyCarpetBlock#placeAt` — the pale moss carpet, and the one draw it can
//! want.
//!
//! Reference (26.2, `javap -p -c -constants` on the pinned
//! `server-26.2.jar`): `net.minecraft.world.level.block.MossyCarpetBlock`
//! `placeAt`, `createTopperWithSideChance`, `getUpdatedState`, `hasFaces`,
//! `canSupportAtFace`.
//!
//! # Why this is not a `set_block`
//!
//! `SimpleBlockFeature` special-cases two blocks and this is one of them:
//!
//! ```text
//!   else if (block instanceof MossyCarpetBlock)
//!       MossyCarpetBlock.placeAt(level, pos, level.getRandom(), 2)
//! ```
//!
//! `level.getRandom()` is not the feature's random. Inside worldgen it is
//! `WorldGenRegion.getRandom()`, which `simple_block`'s header called "a
//! different stream, positional per chunk" and then refused the whole block
//! over. The refusal was right to exist and the reasoning was half right: it
//! is positional per chunk, and that is exactly what makes it reproducible.
//!
//! # Where the random comes from, which is the whole of it
//!
//! The random reaches exactly one place — a `BooleanSupplier` handed to
//! `createTopperWithSideChance`, consulted once per horizontal side that
//! already attaches — and `WorldGenRegion`'s is not the unmodellable shared
//! server stream it looks like:
//!
//! ```text
//!   this.random = level.getChunkSource().randomState()
//!       .getOrCreateRandomFactory(minecraft:worldgen_region_random)
//!       .at(center.getPos().getWorldPosition())
//! ```
//!
//! A **positional** factory, at the generating chunk's origin, one stream per
//! chunk shared by every feature in it. That is machinery this port already
//! has, so the block is placed rather than refused, and
//! `SimpleBlockWorld::level_random_next_boolean` is how a world lends it.
//!
//! A world that has no such stream answers `None`, and that is a refusal
//! rather than a branch. It has to be: both branches write plausible carpet
//! and only a census would ever disagree.
//!
//! # The order is vanilla's, and it is load-bearing
//!
//! The base is written **before** the topper is computed, because the
//! topper's `getUpdatedState` runs at `pos.above()` with `BASE=false`, and
//! that state's `!base` branch reads the block below it — `pos`, which vanilla
//! has just written. Then the base is written a second time, because the
//! topper standing above it can turn one of its sides from `low` to `tall`.
//! Vanilla's own window A holds twelve `bottom=true,...=tall` carpets, so
//! that second write is not a formality.
//!
//! # The refusal must stay inside the feature
//!
//! `pale_moss_patch` is placed by `PaleMossDecorator` from inside a tree, and
//! a decorator refusal rolls the whole tree back. So a refusal here is not
//! "one carpet missing", it is "one pale oak missing": in window A the block's
//! survival rule alone was hiding 169 of them. See `docs/PARITY-ROADMAP.md`
//! §89.4.

#[cfg(test)]
use super::block_predicate::PredicateWorld;
use super::multiface_growth::{can_attach_to, offset_of};
use super::simple_block::{SimpleBlockWorld, Unplaceable};

/// `Direction.Plane.HORIZONTAL`, in the order `forEach` walks it, indexed as
/// `Direction.values()`. North, east, south, west.
///
/// The order is not load-bearing for the ported branch — it takes no draws —
/// but it is written in vanilla's order anyway, because the branch that *is*
/// refused consults the supplier once per direction and would be.
const HORIZONTAL: [usize; 4] = [2, 5, 3, 4];

/// `MossyCarpetBlock.PROPERTY_BY_DIRECTION`, as the property name in a state
/// string.
fn property_for_face(direction: usize) -> &'static str {
    match direction {
        2 => "north",
        3 => "south",
        4 => "west",
        5 => "east",
        _ => unreachable!("MossyCarpetBlock has no property for a vertical face"),
    }
}

/// `WallSide`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WallSide {
    None,
    Low,
    Tall,
}

impl WallSide {
    fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Low => "low",
            Self::Tall => "tall",
        }
    }
}

/// `state.getValue(property)` for one of the four side properties.
///
/// An unreadable or absent property answers `NONE` rather than panicking:
/// every state reaching here comes from the datapack extract, and a shape
/// change there should surface as a census disagreement in the block that
/// changed, not as a panic in the middle of a chunk.
fn side_of(state: &str, property: &str) -> WallSide {
    match property_value(state, property) {
        Some("low") => WallSide::Low,
        Some("tall") => WallSide::Tall,
        _ => WallSide::None,
    }
}

/// `state.getValue(BASE)`. The state string spells it `bottom`.
fn is_base(state: &str) -> bool {
    property_value(state, "bottom") == Some("true")
}

/// `state.is(Blocks.PALE_MOSS_CARPET)`.
fn is_carpet(state: &str) -> bool {
    state.split_once('[').map_or(state, |(name, _)| name) == "minecraft:pale_moss_carpet"
}

/// One property's value out of the canonical `name[a=b,c=d]` spelling.
///
/// Matched as a whole comma-separated field, for the reason
/// `rustmc_behavior::survival` gives for the same helper: `bottom=true` is a
/// substring of a hypothetical `not_bottom=true`, and a property test a prefix
/// can fool is a silent wrong answer.
fn property_value<'a>(state: &'a str, property: &str) -> Option<&'a str> {
    let body = state.split_once('[')?.1;
    let body = body.strip_suffix(']').unwrap_or(body);
    body.split(',').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == property).then_some(value)
    })
}

/// `BlockState#setValue` on a state string, for one of this block's
/// properties.
fn with_property(state: &str, property: &str, value: &str) -> String {
    let Some((name, rest)) = state.split_once('[') else {
        return state.to_string();
    };
    let pairs: Vec<String> = rest
        .trim_end_matches(']')
        .split(',')
        .map(|pair| match pair.split_once('=') {
            Some((key, _)) if key == property => format!("{property}={value}"),
            _ => pair.to_string(),
        })
        .collect();
    format!("{name}[{}]", pairs.join(","))
}

/// `MossyCarpetBlock#getUpdatedState`.
///
/// ```text
///   ignoreExisting |= state.getValue(BASE)
///   for (dir : HORIZONTAL)
///       side = canSupportAtFace(level, pos, dir)
///                ? (ignoreExisting ? LOW : state.getValue(prop))
///                : NONE
///       if (side == LOW) {
///           if (above.is(CARPET) && above.getValue(prop) != NONE
///               && !above.getValue(BASE))                     side = TALL
///           if (!state.getValue(BASE)
///               && below.is(CARPET) && below.getValue(prop) == NONE) side = NONE
///       }
///       state = state.setValue(prop, side)
/// ```
///
/// `above` and `below` are read **lazily and at most once each**, which this
/// keeps: they are only read on a `LOW` side, and a carpet on flat ground has
/// none, so the ordinary case touches neither. That is not an optimisation
/// here — this port's world reads are free of side effects — but it is the
/// shape of the original and the next reader should not have to check.
fn get_updated_state<W: SimpleBlockWorld + ?Sized>(
    world: &mut W,
    state: &str,
    pos: (i32, i32, i32),
    ignore_existing: bool,
) -> Result<String, Unplaceable> {
    let (x, y, z) = pos;
    let base = is_base(state);
    let ignore_existing = ignore_existing || base;
    let mut above: Option<String> = None;
    let mut below: Option<String> = None;
    let mut out = state.to_string();

    for direction in HORIZONTAL {
        let property = property_for_face(direction);
        let attaches = can_attach_to(world, offset_of(pos, direction), direction)
            .map_err(|needs| Unplaceable(needs.0))?;
        let mut side = if attaches {
            if ignore_existing {
                WallSide::Low
            } else {
                side_of(&out, property)
            }
        } else {
            WallSide::None
        };

        if side == WallSide::Low {
            if above.is_none() {
                above = Some(world.block_at(x, y + 1, z));
            }
            let above_state = above.as_deref().unwrap_or_default();
            if is_carpet(above_state)
                && side_of(above_state, property) != WallSide::None
                && !is_base(above_state)
            {
                side = WallSide::Tall;
            }
            if !base {
                if below.is_none() {
                    below = Some(world.block_at(x, y - 1, z));
                }
                let below_state = below.as_deref().unwrap_or_default();
                if is_carpet(below_state) && side_of(below_state, property) == WallSide::None {
                    side = WallSide::None;
                }
            }
        }

        out = with_property(&out, property, side.as_str());
    }
    Ok(out)
}

/// `MossyCarpetBlock#placeAt`.
///
/// Returns the state left at `origin`.
///
/// **The order is vanilla's and it is load-bearing.** The base is written
/// first and the topper is computed second, because the topper's own
/// `getUpdatedState` runs at `pos.above()` with `BASE=false`, and that state's
/// `!base` branch reads the block *below* it — which is `pos`, which vanilla
/// has just written. Computing the topper first, against the world as it was,
/// gives a different answer for any side whose base side came out `NONE`.
///
/// A refusal therefore leaves the base carpet behind, and that is the right
/// trade: it is a block vanilla also writes at that position. Inside a tree
/// the `UndoWorld` rolls it back with everything else.
///
/// # Errors
/// [`Unplaceable`] when a side chance is needed and this world has no level
/// random, or when a read the rule makes cannot be answered.
pub fn place_at<W: SimpleBlockWorld + ?Sized>(
    world: &mut W,
    state: &str,
    origin: (i32, i32, i32),
) -> Result<String, Unplaceable> {
    let (x, y, z) = origin;
    // `PALE_MOSS_CARPET.defaultBlockState()`, which `placeAt` uses in place of
    // whatever the caller had. Derived from the state the provider produced
    // rather than spelled out, so it stays correct if the extract's property
    // order moves: the default is `BASE` true with four `NONE` sides.
    if !state.contains('[') {
        return Err(Unplaceable(format!(
            "MossyCarpetBlock.placeAt({state}) — no properties on the state"
        )));
    }
    let mut default = with_property(state, "bottom", "true");
    for direction in HORIZONTAL {
        default = with_property(&default, property_for_face(direction), "none");
    }

    let placed = get_updated_state(world, &default, origin, true)?;
    world.set_block(x, y, z, &placed);

    let Some(topper) = create_topper(world, origin, &default)? else {
        return Ok(placed);
    };
    world.set_block(x, y + 1, z, &topper);
    // `getUpdatedState(updatedBase, level, pos, true)` again, with the topper
    // now standing above: this is the only thing that can turn a base side
    // from `low` to `tall`, and vanilla's own census of window A holds
    // twelve `bottom=true,...,south=tall` among its 512.
    let replaced = get_updated_state(world, &placed, origin, true)?;
    world.set_block(x, y, z, &replaced);
    Ok(replaced)
}

/// `MossyCarpetBlock#createTopperWithSideChance`.
///
/// `Ok(None)` is vanilla's `AIR` return — no topper is written.
///
/// The `BooleanSupplier` is `random::nextBoolean` over the **level's** random,
/// consulted once per direction whose side is already non-`NONE`, in
/// `HORIZONTAL` order. A `false` clears that side. So the draw count is the
/// number of attaching sides and not four, and it is not one either: a world
/// that answered a single boolean and reused it would agree with vanilla
/// whenever exactly one side attaches, which is most of them.
fn create_topper<W: SimpleBlockWorld + ?Sized>(
    world: &mut W,
    origin: (i32, i32, i32),
    default_state: &str,
) -> Result<Option<String>, Unplaceable> {
    let (x, y, z) = origin;
    let above_pos = (x, y + 1, z);
    let above = world.block_at(x, y + 1, z);

    if is_carpet(&above) {
        if is_base(&above) {
            return Ok(None);
        }
    } else {
        let replaceable = world
            .replaceable_at(x, y + 1, z)
            .ok_or_else(|| Unplaceable("canBeReplaced above a pale_moss_carpet".to_string()))?;
        if !replaceable {
            return Ok(None);
        }
    }

    let topper = with_property(default_state, "bottom", "false");
    let mut topper = get_updated_state(world, &topper, above_pos, true)?;

    for direction in HORIZONTAL {
        let property = property_for_face(direction);
        if side_of(&topper, property) == WallSide::None {
            continue;
        }
        let keep = world.level_random_next_boolean().ok_or_else(|| {
            Unplaceable(
                "MossyCarpetBlock.createTopperWithSideChance needs level.getRandom()".to_string(),
            )
        })?;
        if !keep {
            topper = with_property(&topper, property, WallSide::None.as_str());
        }
    }

    // `hasFaces` short-circuits on `BASE`, which is false here, so it is
    // exactly "some side is non-NONE".
    let has_faces = HORIZONTAL
        .iter()
        .any(|&d| side_of(&topper, property_for_face(d)) != WallSide::None);
    if has_faces && topper != above {
        Ok(Some(topper))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A world of named blocks, a set of positions whose faces attach, and a
    /// **scripted** level random.
    ///
    /// Scripted rather than seeded, for the reason §85.7 recorded after a
    /// hanger test passed its own falsifier: a real stream makes whichever
    /// branch that seed happens to take look like coverage. A script that runs
    /// out panics, so a change in the draw accounting is a failure rather than
    /// a different answer.
    struct Blocks {
        blocks: BTreeMap<(i32, i32, i32), String>,
        sturdy: Vec<(i32, i32, i32)>,
        written: Vec<((i32, i32, i32), String)>,
        coins: std::collections::VecDeque<bool>,
        /// `None` is a world with no level random at all, which must refuse.
        has_random: bool,
        drawn: usize,
    }

    impl Blocks {
        fn flat() -> Self {
            let mut blocks = BTreeMap::new();
            blocks.insert((0, -1, 0), "minecraft:grass_block".to_string());
            Self {
                blocks,
                sturdy: Vec::new(),
                written: Vec::new(),
                coins: std::collections::VecDeque::new(),
                has_random: false,
                drawn: 0,
            }
        }

        fn with_coins(mut self, coins: &[bool]) -> Self {
            self.coins = coins.iter().copied().collect();
            self.has_random = true;
            self
        }

        /// A sturdy block at a position, so a carpet beside it attaches.
        fn wall(mut self, x: i32, y: i32, z: i32) -> Self {
            self.blocks.insert((x, y, z), "minecraft:stone".to_string());
            self.sturdy.push((x, y, z));
            self
        }
    }

    impl PredicateWorld for Blocks {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.blocks
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
        fn face_is_sturdy(&mut self, x: i32, y: i32, z: i32, _direction: usize) -> Option<bool> {
            Some(self.sturdy.contains(&(x, y, z)))
        }
        fn face_is_collision_full(
            &mut self,
            x: i32,
            y: i32,
            z: i32,
            _direction: usize,
        ) -> Option<bool> {
            Some(self.sturdy.contains(&(x, y, z)))
        }
        fn replaceable_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:air")
        }
    }

    impl SimpleBlockWorld for Blocks {
        fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
            self.blocks.insert((x, y, z), name.to_string());
            self.written.push(((x, y, z), name.to_string()));
        }
        fn is_empty_block(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:air")
        }
        fn level_random_next_boolean(&mut self) -> Option<bool> {
            if !self.has_random {
                return None;
            }
            self.drawn += 1;
            Some(
                self.coins
                    .pop_front()
                    .expect("the draw accounting moved: more coins were asked for than scripted"),
            )
        }
    }

    const DEFAULT: &str =
        "minecraft:pale_moss_carpet[bottom=true,east=none,north=none,south=none,west=none]";

    /// The ordinary case: open ground, nothing to attach to.
    ///
    /// **One block written and no draw taken.** The write list is asserted
    /// rather than the return value alone, because the failure that matters is
    /// a second write above — vanilla's topper — where vanilla writes nothing.
    #[test]
    fn a_carpet_on_open_ground_is_one_block_and_takes_no_draw() {
        let mut world = Blocks::flat().with_coins(&[]);
        let placed = place_at(&mut world, DEFAULT, (0, 0, 0)).expect("no draw is needed here");
        assert_eq!(placed, DEFAULT);
        assert_eq!(world.written, vec![((0, 0, 0), DEFAULT.to_string())]);
        assert_eq!(
            world.drawn, 0,
            "an open-ground carpet must not spend a draw"
        );
    }

    /// A wall two blocks tall: the base attaches, the topper attaches, the
    /// coin keeps it, and the base is rewritten as `tall`.
    ///
    /// The `tall` is the assertion that matters. It can only come from the
    /// **second** `getUpdatedState` at the origin, run after the topper is
    /// standing, so a port that wrote the base once would pass every other
    /// assertion here and fail this one.
    #[test]
    fn a_kept_topper_rewrites_the_base_as_tall() {
        let mut world = Blocks::flat()
            .wall(0, 0, -1)
            .wall(0, 1, -1)
            .with_coins(&[true]);
        let placed = place_at(&mut world, DEFAULT, (0, 0, 0)).expect("the coin keeps the side");

        assert_eq!(world.drawn, 1, "one attaching side is one draw");
        assert_eq!(
            placed,
            "minecraft:pale_moss_carpet[bottom=true,east=none,north=tall,south=none,west=none]"
        );
        assert_eq!(
            world.written,
            vec![
                (
                    (0, 0, 0),
                    "minecraft:pale_moss_carpet[bottom=true,east=none,north=low,south=none,west=none]"
                        .to_string()
                ),
                (
                    (0, 1, 0),
                    "minecraft:pale_moss_carpet[bottom=false,east=none,north=low,south=none,west=none]"
                        .to_string()
                ),
                (
                    (0, 0, 0),
                    "minecraft:pale_moss_carpet[bottom=true,east=none,north=tall,south=none,west=none]"
                        .to_string()
                ),
            ],
            "vanilla writes base, topper, base"
        );
    }

    /// The same wall, and the coin clears the side.
    ///
    /// The topper then has no faces, `hasFaces` is false, vanilla returns AIR
    /// and nothing goes above. The base stays `low` and is **not** rewritten.
    #[test]
    fn a_cleared_topper_leaves_the_base_low_and_writes_nothing_above() {
        let mut world = Blocks::flat()
            .wall(0, 0, -1)
            .wall(0, 1, -1)
            .with_coins(&[false]);
        let placed = place_at(&mut world, DEFAULT, (0, 0, 0)).expect("the coin clears the side");

        assert_eq!(world.drawn, 1);
        assert_eq!(
            placed,
            "minecraft:pale_moss_carpet[bottom=true,east=none,north=low,south=none,west=none]"
        );
        assert_eq!(
            world.written.len(),
            1,
            "no topper, and no second base write"
        );
    }

    /// Two attaching sides are two draws, and they are consulted in
    /// `HORIZONTAL` order — north, east, south, west.
    ///
    /// Scripted `[false, true]`, so the north side is cleared and the east
    /// kept. A port that drew once and reused the answer, or that walked the
    /// directions in `Direction.values()` order, gives a different state.
    #[test]
    fn one_draw_per_attaching_side_in_horizontal_order() {
        let mut world = Blocks::flat()
            .wall(0, 0, -1)
            .wall(0, 1, -1)
            .wall(1, 0, 0)
            .wall(1, 1, 0)
            .with_coins(&[false, true]);
        let placed = place_at(&mut world, DEFAULT, (0, 0, 0)).expect("two sides, two coins");

        assert_eq!(world.drawn, 2, "two attaching sides are two draws");
        assert_eq!(
            placed,
            "minecraft:pale_moss_carpet[bottom=true,east=tall,north=low,south=none,west=none]",
            "north was cleared by the first coin and east kept by the second"
        );
    }

    /// A world with no level random refuses, and names the stream.
    ///
    /// It refuses **after** writing the base, which is vanilla's order and is
    /// stated rather than hidden: the base is a block vanilla writes at that
    /// position too, and inside a tree the `UndoWorld` takes it back with
    /// everything else.
    #[test]
    fn a_world_without_a_level_random_refuses_and_names_it() {
        let mut world = Blocks::flat().wall(0, 0, -1).wall(0, 1, -1);
        let error = place_at(&mut world, DEFAULT, (0, 0, 0)).expect_err("a side attaches above");
        assert!(
            error.0.contains("level.getRandom()"),
            "the refusal must name the stream: {}",
            error.0
        );
        assert_eq!(
            world.written.len(),
            1,
            "the base is written and the topper is not: {:?}",
            world.written
        );
    }

    /// A one-block wall attaches the base and the topper attaches nothing, so
    /// no draw is taken even though a side is `low`.
    ///
    /// The distinction the refusal test cannot make on its own: a wall beside
    /// the *carpet* is not what costs the draw — a wall beside the block
    /// *above* it is.
    #[test]
    fn a_one_block_wall_attaches_the_base_and_still_needs_no_draw() {
        let mut world = Blocks::flat().wall(0, 0, -1);
        let placed = place_at(&mut world, DEFAULT, (0, 0, 0)).expect("the topper attaches nothing");
        assert_eq!(
            placed,
            "minecraft:pale_moss_carpet[bottom=true,east=none,north=low,south=none,west=none]"
        );
        assert_eq!(world.written.len(), 1);
        assert_eq!(world.drawn, 0);
    }

    /// A world that cannot answer `isFaceSturdy` refuses, rather than reading
    /// the `None` as "no wall".
    #[test]
    fn an_unanswerable_face_is_a_refusal_and_not_a_no() {
        struct Silent;
        impl PredicateWorld for Silent {
            fn block_at(&mut self, _x: i32, _y: i32, _z: i32) -> String {
                "minecraft:air".to_string()
            }
        }
        impl SimpleBlockWorld for Silent {
            fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _name: &str) {
                panic!("nothing may be written when a read cannot be answered");
            }
            fn is_empty_block(&mut self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
                Some(true)
            }
        }
        let error = place_at(&mut Silent, DEFAULT, (0, 0, 0)).expect_err("no world to read");
        assert!(error.0.contains("isFaceSturdy"), "{}", error.0);
    }

    /// `placeAt` uses the block's default state and not the caller's.
    #[test]
    fn the_callers_state_is_replaced_by_the_default_one() {
        let mut world = Blocks::flat().with_coins(&[]);
        let placed = place_at(
            &mut world,
            "minecraft:pale_moss_carpet[bottom=false,east=tall,north=low,south=none,west=none]",
            (0, 0, 0),
        )
        .expect("open ground");
        assert_eq!(placed, DEFAULT);
    }
}
