//! `FireBlock` and `BaseFireBlock` — the flammability table, the placement
//! walk, and the scheduled tick that burns and spreads.
//!
//! Every line is read from `javap -p -c -constants` over the pinned
//! `server-26.2.jar`. The offsets quoted are that disassembly's.
//!
//! # 26.2 moved two things this mechanic has always been described by, and
//! both descriptions are now wrong
//!
//! * **There is no `doFireTick` game rule.** `FireBlock.tick`'s first
//!   conditional is `ServerLevel.canSpreadFireAround(pos)`, whose body is
//!
//!   ```text
//!     r = gameRules.get(FIRE_SPREAD_RADIUS_AROUND_PLAYER)      // Integer
//!     return r == -1 || chunkSource.chunkMap.anyPlayerCloseEnoughTo(pos, r)
//!   ```
//!
//!   and the rule is `registerInteger("fire_spread_radius_around_player",
//!   UPDATES, 128, -1)` — **default 128, minimum -1**, where -1 means "no
//!   radius, always spread". `anyPlayerCloseEnoughTo` is a linear scan of
//!   every player, skipping spectators, comparing
//!   `player.position().distanceTo(new Vec3(blockPos)) < r` — a `Vec3` built
//!   from the block's integer corner, not its centre. So the gate is a
//!   *distance to a player*, not a boolean, and a port that reads a boolean
//!   game rule is right only by accident on a world with one player standing
//!   next to the fire.
//! * **There is no 15-block humidity check.** What gates the burn-out and
//!   halves the spread odds is
//!   `level.environmentAttributes().getValue(EnvironmentAttributes
//!   .INCREASED_FIRE_BURNOUT, pos)`, a `Boolean` off the environment-attribute
//!   system — offsets 274-291. It contributes `-50` to each of the six
//!   neighbour chances and an integer halving to the spread odds. That system
//!   is not on this tree, so [`Env::increased_burnout`] is a parameter with a
//!   named default rather than a guess; see its docs.
//!
//! # What is ported and what is refused
//!
//! Ported: [`FireOdds`] and its two lookups, [`can_burn`],
//! [`is_valid_fire_location`], [`ignite_odds_at`], [`state_for_placement`],
//! [`fire_tick_delay`], and [`tick`] — the whole of `FireBlock.tick`
//! including the six-face burn-out pass and the 3x3x6 spread walk, in
//! vanilla's draw order.
//!
//! Refused, each because the mechanism it needs is another lane's or is
//! absent from this tree, and each named so nobody re-derives it:
//!
//! * **`BaseFireBlock.entityInside` and everything downstream of it.** Its
//!   whole body is three `InsideBlockEffectApplier` calls, and the one that
//!   does damage is `runAfter(FIRE_IGNITE, e -> e.hurt(level.damageSources()
//!   .inFire(), fireDamage))`. `Entity.hurt` and the `hurtServer` that
//!   `Entity.baseTick`'s fire arm calls belong to `origin/player-systems-2`,
//!   which owns `hurt_server`; the join is `Entity.baseTick` offsets 125-187,
//!   whose non-damage half is transcribed in [`FireTicks`] and whose damage
//!   half is a single `hurtServer(level, damageSources().onFire(), 1.0f)`
//!   call at offset 173. Nothing here calls it.
//! * **Mob burning.** `mob-combat`'s.
//! * **The nether's fire specialisation.** `dimensions`'. Concretely:
//!   `tick`'s `infiniburn` flag is
//!   `belowState.is(level.dimensionType().infiniburn())`, a `HolderSet` off
//!   the dimension type, and there is no dimension type on this tree. It is a
//!   parameter with a named default — see [`Env::infiniburn_below`].
//! * **`BaseFireBlock.onPlace`'s portal branch.** `inPortalDimension` then
//!   `PortalShape.findEmptyPortalShape` then `createPortalBlocks`. The
//!   *non*-portal half of `onPlace` is ported by [`on_placed`].
//! * **`checkBurnOut`'s TNT arm.** Offsets 100-120,
//!   `if (block instanceof TntBlock) TntBlock.prime(level, pos)` — `explosions`
//!   owns that. [`BurnOut::primed_tnt`] records that it would have fired, so
//!   the refusal is visible at the call site rather than silent.
//! * **The rain arms.** `isNearRain` is transcribed
//!   ([`Env::raining`] drives it, through [`raining_at`]) but this server has
//!   no weather, so every rain test is false — which is the *substrate*, not
//!   an approximation: `ci/t2-gameplay.sh` sets `advance_weather false` on both
//!   servers.
//!
//! # The one thing about this mechanism that can never be compared
//!
//! `FireBlock.tick`'s `RandomSource` is `ServerLevel`'s, and `Level`'s
//! constructor sets it from `RandomSource.create()` — unseeded. Two vanilla
//! servers of this version, on the same world, at the same tick, make
//! different fire decisions and always will. `crate::randomtick`'s module
//! records the same fact about `Level.randValue`.
//!
//! So **no check can compare this server's fire spread against a live
//! vanilla's**, and none is written. What is comparable is: the odds table
//! (`ci/fire-odds-golden.sh`, against vanilla's own maps), the draw *order*
//! and the arithmetic (transcription, defended by the offsets quoted at each
//! site and by a seeded golden of the one expression vanilla will evaluate
//! for us — see `ci/fire-odds-golden.sh`'s `delay` rows), and the rate.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use rustmc_worldgen::rng::RandomSource;

use crate::edits::Blocks;
use crate::interact::{BlockPos, Face};
use crate::shapes::{Effects, ShapeTable};

/// The three level reads `FireBlock`'s **placement** half makes.
///
/// It exists because that half has two callers whose worlds are shaped
/// differently and neither may get its own copy of the walk:
/// [`crate::ticks::run`] holds a `&dyn Blocks`, and
/// [`crate::itemuse::use_on`] holds an [`crate::itemuse::At`] of three
/// closures. A second transcription of `getStateForPlacement` for the second
/// caller is exactly how two answers to one question drift apart — which this
/// tree has already paid for once, in `ci/gen-gameplay-tables.sh`'s "no
/// second copy of the grep" note.
///
/// The **tick** half is not behind this: it writes as well as reads, so it
/// takes a `&dyn Blocks` directly.
pub trait FireLevel {
    /// `LevelReader.getBlockState(pos)`, as a full state string.
    fn state_at(&self, pos: BlockPos) -> String;
    /// `BlockState.isFaceSturdy(level, pos, face)`.
    fn face_sturdy(&self, pos: BlockPos, face: Face) -> bool;
    /// `Block.defaultBlockState()` by block name.
    fn default_state(&self, block: &str) -> Option<String>;
}

impl FireLevel for dyn Blocks + '_ {
    fn state_at(&self, pos: BlockPos) -> String {
        Blocks::state_at(self, pos)
    }
    fn face_sturdy(&self, pos: BlockPos, face: Face) -> bool {
        Blocks::face_sturdy(self, pos, face)
    }
    fn default_state(&self, block: &str) -> Option<String> {
        Blocks::default_state(self, block)
    }
}

/// `Direction.values()` — the order every `for (Direction d : values())` in
/// `FireBlock` and `BaseFireBlock` walks.
///
/// `Face`'s discriminants are already `Direction.get3DDataValue`'s, so this is
/// the enum's declaration order and not a re-ordering.
pub const DIRECTIONS: [Face; 6] = [
    Face::Down,
    Face::Up,
    Face::North,
    Face::South,
    Face::West,
    Face::East,
];

/// `FireBlock.getFireTickDelay(random)` — `bipush 30; nextInt(10); iadd`.
///
/// The whole method, offsets 0-11. It is called from **two** places and both
/// pass `level.getRandom()` rather than the `random` a rule was handed:
/// `FireBlock.onPlace:14` and `FireBlock.tick:4`. On this server they are the
/// same source, but the distinction is vanilla's and is kept at the call
/// sites.
pub fn fire_tick_delay(rng: &mut dyn RandomSource) -> i64 {
    i64::from(30 + rng.next_i32_bound(10))
}

/// The six neighbour chances of `FireBlock.tick`'s burn-out pass, **in the
/// order vanilla calls them**, with the `BlockPos` accessor each uses.
///
/// Offsets 306-417. The order is `east, west, below, above, north, south` and
/// it is not `Direction.values()` — this is the one walk in the class that
/// uses its own. It matters because every call draws from the shared
/// generator whether or not it burns anything, so a reordering shifts every
/// later draw in the tick.
///
/// The chance is `300` for the four horizontals and `250` for up and down,
/// each plus `k`, where `k` is `-50` when `INCREASED_FIRE_BURNOUT` holds at
/// the fire's own position and `0` otherwise (offsets 293-304).
pub const BURN_OUT_FACES: [(Face, i32); 6] = [
    (Face::East, 300),
    (Face::West, 300),
    (Face::Down, 250),
    (Face::Up, 250),
    (Face::North, 300),
    (Face::South, 300),
];

/// `FireBlock.MAX_AGE`.
pub const MAX_AGE: i32 = 15;

/// The extract this module reads, relative to the repo root.
///
/// A `&str` const rather than a `join` at each call site, and the literal is
/// spelled out here because `ci/gen-gameplay-tables.sh` derives the set of
/// tables a tree must generate by grepping `crates tools plugins` for exactly
/// `"data/vanilla/<name>.json"`. A path assembled from pieces, or one written
/// `"../../data/vanilla/..."` in a test, matches nothing — which is how
/// `block_entities.json` got a correct generator arm that no gate ever ran.
/// `ci/check-generator-arms.sh`'s header records what that cost.
pub const TABLE_PATH: &str = "data/vanilla/fire_odds.json";

/// The world facts `FireBlock.tick` reads that this tree has no subsystem
/// for.
///
/// **Each field is a refusal with a name, not a simplification.** They are
/// parameters so that the day the owning lane lands, the wiring is one
/// struct literal and the rule body does not move; and they are grouped here
/// so a reader can see the whole of what this port is standing in for
/// instead of finding four `false`s scattered through the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Env {
    /// `ServerLevel.canSpreadFireAround(pos)` — see the module header. `true`
    /// is what a world whose rule is at its `-1` minimum answers, and also
    /// what a world at the 128 default answers for any fire within 128 blocks
    /// of a non-spectating player. The corpus's fire is always a few blocks
    /// from its one player, so both readings agree there.
    pub can_spread_fire_around: bool,
    /// `belowState.is(level.dimensionType().infiniburn())`. `dimensions` owns
    /// the dimension type; on the overworld the tag is
    /// `#minecraft:infiniburn_overworld`, whose only member is
    /// `netherrack`, and this server generates none under a fire.
    pub infiniburn_below: bool,
    /// `level.isRaining()`. No weather here, and `ci/t2-gameplay.sh` sets
    /// `advance_weather false` on **both** servers, so this is the substrate
    /// rather than an approximation.
    pub raining: bool,
    /// `EnvironmentAttributes.INCREASED_FIRE_BURNOUT` at the fire's position.
    /// 26.2's replacement for the humidity check; no environment-attribute
    /// system here.
    pub increased_burnout: bool,
    /// `level.getDifficulty().getId()` — `PEACEFUL` 0, `EASY` 1, `NORMAL` 2,
    /// `HARD` 3. Read into the spread odds as `+ difficulty * 7`.
    pub difficulty: i32,
}

impl Default for Env {
    /// What this server's world is: fire spreads, nothing is infiniburn,
    /// nothing rains, no environment attribute holds, and the difficulty is
    /// `EASY` — `server.properties`' own default, which
    /// `ci/vanilla-server.sh` does not override.
    fn default() -> Self {
        Self {
            can_spread_fire_around: true,
            infiniburn_below: false,
            raining: false,
            increased_burnout: false,
            difficulty: 1,
        }
    }
}

/// `FireBlock.igniteOdds` and `FireBlock.burnOdds`, from
/// `ci/fire-odds-golden.sh`.
///
/// Two `Object2IntMap<Block>` with **identical key sets** — `setFlammable`
/// writes both in one body, and the generator refuses a table where they
/// differ — so one map of pairs is faithful.
#[derive(Debug, Default)]
pub struct FireOdds {
    by_block: HashMap<String, (i32, i32)>,
}

impl FireOdds {
    /// Load the extract.
    ///
    /// # Errors
    /// If the file is unreadable, is not JSON, or is not the shape
    /// `FireOddsGolden` writes.
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let doc: serde_json::Value = serde_json::from_str(&text).map_err(io::Error::other)?;
        let odds = doc
            .get("odds")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| io::Error::other("fire_odds.json has no `odds` object"))?;
        let mut by_block = HashMap::new();
        for (block, cell) in odds {
            let ignite = cell.get("ignite").and_then(serde_json::Value::as_i64);
            let burn = cell.get("burn").and_then(serde_json::Value::as_i64);
            let (Some(ignite), Some(burn)) = (ignite, burn) else {
                return Err(io::Error::other(format!(
                    "fire_odds.json: {block} has no ignite/burn pair"
                )));
            };
            #[expect(
                clippy::cast_possible_truncation,
                reason = "both are `bipush`/`iconst` operands in bootStrap; the largest is 100"
            )]
            by_block.insert(block.clone(), (ignite as i32, burn as i32));
        }
        // **An empty table is refused rather than defaulted, and the
        // asymmetry against every other loader in this crate is the point.**
        // A missing odds column makes every block non-flammable at once, and
        // the output of that is a fire that sits still and goes out — which
        // is also the output of a correct server on stone. No gate can
        // separate those, so the stale table has to refuse. Same argument as
        // `block_props.json`'s two random-tick columns.
        if by_block.is_empty() {
            return Err(io::Error::other("fire_odds.json carries no blocks"));
        }
        Ok(Self { by_block })
    }

    /// Load once for the process.
    ///
    /// # Errors
    /// Whatever [`FireOdds::load`] returned on the first call.
    pub fn shared(path: &Path) -> Result<&'static Self, String> {
        static TABLE: std::sync::OnceLock<Result<FireOdds, String>> = std::sync::OnceLock::new();
        TABLE
            .get_or_init(|| Self::load(path).map_err(|e| e.to_string()))
            .as_ref()
            .map_err(Clone::clone)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.by_block.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_block.is_empty()
    }

    /// `FireBlock.getIgniteOdds(BlockState)` — offsets 0-41.
    ///
    /// ```text
    ///    0-27  if (state.hasProperty(WATERLOGGED) && state.getValue(WATERLOGGED)) return 0
    ///   28-41  return igniteOdds.getInt(state.getBlock())
    /// ```
    ///
    /// The `Object2IntMap` default return value is `0`, so an absent block is
    /// zero rather than an error.
    #[must_use]
    pub fn ignite_odds(&self, state: &str) -> i32 {
        if waterlogged(state) {
            return 0;
        }
        self.by_block
            .get(crate::blockentity::block_of(state))
            .map_or(0, |(i, _)| *i)
    }

    /// `FireBlock.getBurnOdds(BlockState)` — offsets 0-41, the same shape
    /// against the other map.
    #[must_use]
    pub fn burn_odds(&self, state: &str) -> i32 {
        if waterlogged(state) {
            return 0;
        }
        self.by_block
            .get(crate::blockentity::block_of(state))
            .map_or(0, |(_, b)| *b)
    }

    /// `FireBlock.canBurn(BlockState)` — `getIgniteOdds(state) > 0`.
    #[must_use]
    pub fn can_burn(&self, state: &str) -> bool {
        self.ignite_odds(state) > 0
    }

    /// `FireBlock.isValidFireLocation(BlockGetter, BlockPos)` — offsets 0-52.
    ///
    /// Any of `Direction.values()`'s six neighbours `canBurn`. Note it is the
    /// *neighbours* of the fire, the fire's own position not included.
    #[must_use]
    pub fn is_valid_fire_location<L: FireLevel + ?Sized>(&self, level: &L, pos: BlockPos) -> bool {
        DIRECTIONS
            .iter()
            .any(|d| self.can_burn(&level.state_at(d.relative(pos))))
    }

    /// `FireBlock.getIgniteOdds(LevelReader, BlockPos)` — offsets 0-73, the
    /// *position* overload, which is a different method from the state one
    /// above and is what the spread walk calls.
    ///
    /// ```text
    ///    0-11  if (!level.isEmptyBlock(pos)) return 0
    ///   12-72  i = 0; for (Direction d : values())
    ///              i = Math.max(getIgniteOdds(level.getBlockState(pos.relative(d))), i)
    ///      73  return i
    /// ```
    ///
    /// **`isEmptyBlock` is `getBlockState(pos).isAir()`**, so a position
    /// holding anything at all — including another fire — scores 0 and the
    /// spread walk skips it.
    #[must_use]
    pub fn ignite_odds_at(&self, blocks: &dyn Blocks, pos: BlockPos) -> i32 {
        if crate::blockentity::block_of(&blocks.state_at(pos)) != "minecraft:air" {
            return 0;
        }
        DIRECTIONS.iter().fold(0, |acc, d| {
            self.ignite_odds(&blocks.state_at(d.relative(pos))).max(acc)
        })
    }
}

/// `state.hasProperty(WATERLOGGED) && state.getValue(WATERLOGGED)`.
///
/// A state with no `waterlogged` property has no such key in its string, so
/// "absent" and "false" collapse — which is what the two-step vanilla test
/// computes too.
fn waterlogged(state: &str) -> bool {
    crate::placement::property_of(state, "waterlogged") == Some("true")
}

/// `FireBlock.getStateForPlacement(BlockGetter, BlockPos)` — offsets 0-133,
/// the whole method.
///
/// ```text
///    0-33   below = pos.below(); belowState = getter.getBlockState(below)
///           if (canBurn(belowState) || belowState.isFaceSturdy(getter, below, UP))
///               return defaultBlockState()
///   41-131  state = defaultBlockState()
///           for (Direction d : values()):
///               p = PROPERTY_BY_DIRECTION.get(d)
///               if (p != null)
///                   state = state.setValue(p, canBurn(getter.getBlockState(
///                                                        pos.relative(d))))
///      133  return state
/// ```
///
/// **`PROPERTY_BY_DIRECTION` has no `DOWN`.** It is built in `<clinit>` by
/// filtering `PipeBlock.PROPERTY_BY_DIRECTION` through `lambda$static$0`,
/// which drops the `DOWN` entry, so the `p != null` guard skips exactly one
/// of the six and a fire has no `down` property at all.
///
/// Returns `None` only when the block table does not know `minecraft:fire`,
/// which is a broken table rather than a placement answer.
#[must_use]
pub fn state_for_placement<L: FireLevel + ?Sized>(
    odds: &FireOdds,
    level: &L,
    pos: BlockPos,
) -> Option<String> {
    let below = (pos.0, pos.1 - 1, pos.2);
    let below_state = level.state_at(below);
    let default = level.default_state("minecraft:fire")?;
    if odds.can_burn(&below_state) || level.face_sturdy(below, Face::Up) {
        return Some(default);
    }
    let mut state = default;
    for d in DIRECTIONS {
        let Some(name) = property_for_direction(d) else {
            continue;
        };
        let burns = odds.can_burn(&level.state_at(d.relative(pos)));
        state = crate::placement::with_property(&state, name, if burns { "true" } else { "false" });
    }
    Some(state)
}

/// `FireBlock.PROPERTY_BY_DIRECTION.get(d)` — the property name, or `None`
/// for `DOWN`, which the map does not carry.
#[must_use]
pub const fn property_for_direction(d: Face) -> Option<&'static str> {
    match d {
        Face::Down => None,
        Face::Up => Some("up"),
        Face::North => Some("north"),
        Face::South => Some("south"),
        Face::West => Some("west"),
        Face::East => Some("east"),
    }
}

/// `BaseFireBlock.getState(BlockGetter, BlockPos)` — offsets 0-38.
///
/// ```text
///    0-26  if (SoulFireBlock.canSurviveOnBlock(getBlockState(pos.below())))
///              return SOUL_FIRE.defaultBlockState()
///   27-38  return ((FireBlock) FIRE).getStateForPlacement(getter, pos)
/// ```
///
/// `SoulFireBlock.canSurviveOnBlock` is
/// `state.is(BlockTags.SOUL_FIRE_BASE_BLOCKS)`, whose two members are read
/// out of the pinned jar's own datapack and inlined in
/// [`crate::itemuse`] — the only tag either module needs.
#[must_use]
pub fn base_fire_state<L: FireLevel + ?Sized>(
    odds: &FireOdds,
    level: &L,
    pos: BlockPos,
) -> Option<String> {
    let below = (pos.0, pos.1 - 1, pos.2);
    if crate::itemuse::is_soul_fire_base(&level.state_at(below)) {
        return level.default_state("minecraft:soul_fire");
    }
    state_for_placement(odds, level, pos)
}

/// `FireBlock.canSurvive(state, LevelReader, BlockPos)` — offsets 0-40.
///
/// `below.isFaceSturdy(level, belowPos, UP) || isValidFireLocation(level, pos)`.
#[must_use]
pub fn can_survive<L: FireLevel + ?Sized>(odds: &FireOdds, level: &L, pos: BlockPos) -> bool {
    let below = (pos.0, pos.1 - 1, pos.2);
    level.face_sturdy(below, Face::Up) || odds.is_valid_fire_location(level, pos)
}

/// What one `FireBlock.checkBurnOut` call did, so the TNT refusal is visible
/// to its caller rather than silent.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BurnOut {
    /// The `nextInt(chance) < burnOdds` test passed and the block was
    /// replaced or removed.
    pub burned: bool,
    /// The burned block was a `TntBlock` and vanilla would have called
    /// `TntBlock.prime`. **Not done here** — `explosions` owns it. Recorded
    /// so a caller can count what it is not doing.
    pub primed_tnt: bool,
}

/// `FireBlock.checkBurnOut(Level, BlockPos, int chance, RandomSource, int age)`
/// — offsets 0-121, the whole method.
///
/// ```text
///    0-21   if (random.nextInt(chance) >= getBurnOdds(getBlockState(pos))) return
///   24-29   was = level.getBlockState(pos)            // re-read, before the write
///   31-52   if (random.nextInt(age + 10) < 5 && !level.isRainingAt(pos)):
///   55-89       n = Math.min(age + random.nextInt(5) / 4, 15)
///               level.setBlock(pos, getStateWithAge(level, pos, n), 3)
///   93-99   else: level.removeBlock(pos, false)
///  100-120  if (was.getBlock() instanceof TntBlock) TntBlock.prime(level, pos)
/// ```
///
/// **Two things about the draws.** The `nextInt(chance)` is taken
/// unconditionally — before the odds are even compared — so a face whose
/// block cannot burn still costs one draw. And the `nextInt(age + 10)` and
/// the `nextInt(5)` are taken only inside the successful branch, in that
/// order, with the rain test **between** them: `isRainingAt` short-circuits
/// after the first and before the second.
///
/// **`Math.min(a, b)` with the age first**, not `min(15, ...)` — the same
/// value, transcribed in vanilla's argument order because the two differ
/// nowhere and a reader comparing against the bytecode should not have to
/// prove that.
#[expect(
    clippy::too_many_arguments,
    reason = "vanilla's own five plus this tree's four substitutes for a Level"
)]
pub fn check_burn_out(
    odds: &FireOdds,
    blocks: &dyn Blocks,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    chance: i32,
    rng: &mut dyn RandomSource,
    age: i32,
    env: Env,
    updates: &mut Effects,
) -> BurnOut {
    let mut out = BurnOut::default();
    let here = blocks.state_at(pos);
    if rng.next_i32_bound(chance) >= odds.burn_odds(&here) {
        return out;
    }
    out.burned = true;
    out.primed_tnt = crate::blockentity::block_of(&here) == "minecraft:tnt";
    if rng.next_i32_bound(age + 10) < 5 && !raining_at(env, pos) {
        let n = (age + rng.next_i32_bound(5) / 4).min(MAX_AGE);
        if let Some(state) = state_with_age(odds, blocks, pos, n) {
            crate::shapes::set_block(
                blocks,
                shapes,
                pos,
                &state,
                crate::shapes::UPDATE_ALL,
                crate::shapes::RECURSION_LIMIT,
                updates,
            );
        }
    } else {
        remove_block(blocks, shapes, pos, updates);
    }
    out
}

/// `FireBlock.getStateWithAge(LevelReader, BlockPos, int)` — offsets 0-36.
///
/// ```text
///    0-15  state = BaseFireBlock.getState(level, pos)
///          if (!state.is(Blocks.FIRE)) return state
///   18-33  return state.setValue(AGE, age)
/// ```
///
/// So a position over soul-fire base takes **soul fire with no age**, which
/// is why the age is set after the `is(FIRE)` test rather than before.
#[must_use]
pub fn state_with_age<L: FireLevel + ?Sized>(
    odds: &FireOdds,
    level: &L,
    pos: BlockPos,
    age: i32,
) -> Option<String> {
    let state = base_fire_state(odds, level, pos)?;
    if crate::blockentity::block_of(&state) != "minecraft:fire" {
        return Some(state);
    }
    Some(crate::placement::with_property(
        &state,
        "age",
        &age.to_string(),
    ))
}

/// `FireBlock.isNearRain(Level, BlockPos)` — offsets 0-57.
///
/// `level.isRainingAt(pos) || isRainingAt(pos.west()) || …east() || …north()
/// || …south()`, in that order, short-circuiting on the first true. Five
/// positions, not four: the fire's own is tested first.
///
/// Both this and [`raining_at`] bottom out in `Level.isRainingAt`, whose own
/// first instruction is `if (!isRaining()) return false` — so with no weather
/// on this tree the per-position half is never reached. See [`raining_at`].
#[must_use]
pub fn is_near_rain(env: Env, pos: BlockPos) -> bool {
    raining_at(env, pos)
        || raining_at(env, Face::West.relative(pos))
        || raining_at(env, Face::East.relative(pos))
        || raining_at(env, Face::North.relative(pos))
        || raining_at(env, Face::South.relative(pos))
}

/// `Level.isRainingAt(BlockPos)`.
///
/// Vanilla is `isRaining() && canSeeSky(pos) && getHeightmapPos(MOTION_BLOCKING,
/// pos).getY() <= pos.getY() && getBiome(pos).value().hasPrecipitation()`.
///
/// **Only the first conjunct is here, and that is exact for every world this
/// server can be in rather than an approximation**: nothing sets
/// [`Env::raining`] true — there is no weather on this tree, and
/// `ci/t2-gameplay.sh` sets `advance_weather false` on both servers — so the
/// remaining three are unreachable. If weather ever lands, this function
/// **over**-reports and the four fire arms that read it get wetter than
/// vanilla's; it is written as one conjunct rather than as a literal `false`
/// so that the day it matters the diff is here and not spread over four call
/// sites, and it is named in the module header as a refusal.
const fn raining_at(env: Env, _pos: BlockPos) -> bool {
    env.raining
}

/// `Level.removeBlock(pos, false)` — `setBlock(pos, getFluidState(pos)
/// .createLegacyBlock(), UPDATE_ALL | (movedByPiston ? … : 0))`, which for a
/// fire is always air: fire is never waterlogged and holds no fluid state.
fn remove_block(
    blocks: &dyn Blocks,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    updates: &mut Effects,
) {
    crate::shapes::set_block(
        blocks,
        shapes,
        pos,
        "minecraft:air",
        crate::shapes::UPDATE_ALL,
        crate::shapes::RECURSION_LIMIT,
        updates,
    );
}

/// `FireBlock.onPlace` — offsets 0-24.
///
/// `super.onPlace(...)` then
/// `level.scheduleTick(pos, this, getFireTickDelay(level.getRandom()))`.
///
/// `BaseFireBlock.onPlace`'s own body, offsets 0-67, is: return if the state
/// replaced was already this block; **the portal branch** (refused, see the
/// module header); otherwise remove the fire if it cannot survive. The
/// early-return is what stops a fire that re-writes itself from rescheduling
/// for ever.
#[expect(
    clippy::too_many_arguments,
    reason = "vanilla's own five plus the odds table, the shape table and the level RandomSource"
)]
pub fn on_placed(
    odds: &FireOdds,
    blocks: &dyn Blocks,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    old_state: &str,
    new_state: &str,
    rng: &mut dyn RandomSource,
    updates: &mut Effects,
) {
    if crate::blockentity::block_of(old_state) == crate::blockentity::block_of(new_state) {
        return;
    }
    if !can_survive(odds, blocks, pos) {
        remove_block(blocks, shapes, pos, updates);
        return;
    }
    updates.ticks.push(crate::shapes::TickRequest {
        pos,
        block: crate::blockentity::block_of(new_state).to_string(),
        delay: fire_tick_delay(rng),
        priority: crate::ticks::PRIORITY_NORMAL,
    });
}

/// The odds table, loaded once for the process from [`TABLE_PATH`].
///
/// Same shape as [`crate::power::shared`], and for the same reason: the two
/// call sites inside `crate::shapes::set_block`'s `onPlace` dispatch have no
/// root to join against, because `Level.setBlockState` does not have one
/// either. The cwd-then-`CARGO_MANIFEST_DIR` fallback is what lets a unit
/// test in this crate see the same table a server booted from the repo root
/// sees.
///
/// `None` — no extract — is a working server in which no fire ever ticks and
/// a flint-and-steel lights only on a sturdy top. Said on the console once.
#[must_use]
pub fn shared_odds() -> Option<&'static FireOdds> {
    static TABLE: std::sync::OnceLock<Option<FireOdds>> = std::sync::OnceLock::new();
    TABLE
        .get_or_init(|| {
            let cwd_relative = std::path::Path::new(TABLE_PATH);
            let path: std::path::PathBuf = if cwd_relative.exists() {
                cwd_relative.to_path_buf()
            } else {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .join(TABLE_PATH)
            };
            match FireOdds::load(&path) {
                Ok(t) => {
                    eprintln!("fire odds table: {} flammable blocks", t.len());
                    Some(t)
                }
                Err(e) => {
                    eprintln!(
                        "no fire odds table ({e}) — nothing burns, nothing spreads, and a \
                         flint-and-steel lights only on a block with a sturdy top"
                    );
                    None
                }
            }
        })
        .as_ref()
}

/// `Level.random` — the one `RandomSource` every fire decision draws from.
///
/// # This is deliberately seeded where vanilla's is not, and nothing is lost
///
/// `Level`'s constructor sets `this.random = RandomSource.create()`, i.e.
/// `new LegacyRandomSource(RandomSupport.generateUniqueSeed())` — a time-and-
/// counter seed. So **two vanilla servers of this version disagree about
/// every fire from their first tick**, and no comparison against a live
/// vanilla was ever available for anything downstream of this stream.
///
/// Given that, a fixed seed costs no parity and buys a property vanilla does
/// not have: two runs of *this* server over the same inputs make the same
/// fire. That matters to `ci/t2-gameplay.sh`, whose verdicts are per-scenario
/// and whose one fire scenario sits inside a settle window. An unseeded
/// source here would make that check flap for a reason no gate could
/// attribute, which is the failure `ci/checks/gameplay-floor.toml` exists to
/// count.
///
/// The algorithm is vanilla's own (`crate::rustmc_worldgen::rng::legacy` is
/// `java.util.Random`), so the *shape* of the stream — the draw order, the
/// bounds, how many draws a tick costs — is faithful; only the seed is ours.
/// `crate::randomtick` records the same fact about `Level.randValue` and took
/// the other way out of it, `DEFAULT_SPEED = 0`, because there the stream is
/// the whole observable and here it is not.
pub fn level_random() -> &'static std::sync::Mutex<rustmc_worldgen::rng::legacy::LegacyRandomSource>
{
    static RNG: std::sync::OnceLock<
        std::sync::Mutex<rustmc_worldgen::rng::legacy::LegacyRandomSource>,
    > = std::sync::OnceLock::new();
    RNG.get_or_init(|| {
        std::sync::Mutex::new(rustmc_worldgen::rng::legacy::LegacyRandomSource::new(
            LEVEL_RANDOM_SEED,
        ))
    })
}

/// [`level_random`], locked for **one draw at a time** — what every caller
/// that can reach `crate::shapes::set_block` must draw through.
///
/// **A guard held across a tick is a deadlock, and it shipped as one.**
/// [`tick`] ages a fire with `set_block` (the flag-260 write above), that
/// write reaches `set_block`'s `onPlace` dispatch, and the dispatch's fire
/// arm, [`on_place`], draws `getFireTickDelay` from this same stream — which
/// is vanilla's order: `FireBlock.onPlace` runs inside the tick's own
/// `setBlock` and reads `level.random`. With `crate::ticks::run` holding a
/// `MutexGuard` across [`tick`], that nested lock never returned: the clock
/// thread hung on the first tick whose `nextInt(3) / 2` aged the fire, every
/// scheduled tick in the world stopped, and `ci/t2-gameplay.sh` saw it as
/// a player the server would no longer let walk
/// (`321-a-fire-being-lit-is-heard-by-everyone-else`, 10.5 blocks short of its
/// workspace, then every later scenario through the reconnect). Taking the
/// lock per draw keeps the draw order exactly what it was — one thread, the
/// same stream, the nested draw landing between the tick's own — and holds
/// nothing across a write.
#[derive(Debug, Default, Clone, Copy)]
pub struct LevelRandom;

impl LevelRandom {
    fn with<T>(f: impl FnOnce(&mut rustmc_worldgen::rng::legacy::LegacyRandomSource) -> T) -> T {
        let mut g = level_random().lock().unwrap_or_else(|e| e.into_inner());
        f(&mut g)
    }
}

impl RandomSource for LevelRandom {
    fn set_seed(&mut self, seed: i64) {
        Self::with(|r| r.set_seed(seed));
    }
    fn next_i32(&mut self) -> i32 {
        Self::with(RandomSource::next_i32)
    }
    fn next_i32_bound(&mut self, bound: i32) -> i32 {
        Self::with(|r| r.next_i32_bound(bound))
    }
    fn next_i64(&mut self) -> i64 {
        Self::with(RandomSource::next_i64)
    }
    fn next_bool(&mut self) -> bool {
        Self::with(RandomSource::next_bool)
    }
    fn next_f32(&mut self) -> f32 {
        Self::with(RandomSource::next_f32)
    }
    fn next_f64(&mut self) -> f64 {
        Self::with(RandomSource::next_f64)
    }
    fn next_gaussian(&mut self) -> f64 {
        Self::with(RandomSource::next_gaussian)
    }
}

/// The seed [`level_random`] uses. Arbitrary and fixed; see that function.
pub const LEVEL_RANDOM_SEED: i64 = 0x1F1E_0000_0000_0001;

/// `FireBlock.onPlace` from inside `Level.setBlockState`'s `onPlace`
/// dispatch, drawing its delay from [`level_random`].
///
/// Called by [`crate::shapes::set_block`] beside the rail and redstone arms.
/// Every arm there is mutually exclusive by block, so the `is_fire` guard is
/// the whole of the dispatch: `FireBlock` is the only class with this
/// override and `SoulFireBlock` does not inherit it — `BaseFireBlock.onPlace`
/// is the one soul fire runs, and it schedules nothing.
pub fn on_place(
    blocks: &dyn Blocks,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    old_state: &str,
    updates: &mut Effects,
) {
    if crate::blockentity::block_of(state) != "minecraft:fire" {
        return;
    }
    let Some(odds) = shared_odds() else {
        return;
    };
    // Per draw, not a guard: `on_placed` can remove the fire through
    // `set_block`, and see [`LevelRandom`] for what a held guard cost.
    on_placed(
        odds,
        blocks,
        shapes,
        pos,
        old_state,
        state,
        &mut LevelRandom,
        updates,
    );
}

/// What one [`tick`] did, beyond the block writes it put in `Effects`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Ticked {
    /// The fire rescheduled itself. Always true on a tick that ran at all —
    /// the reschedule is `tick`'s **first** statement, before the game-rule
    /// gate — and false only when the block at the position was not fire.
    pub rescheduled: bool,
    /// Faces the burn-out pass burned.
    pub burned: u32,
    /// Positions the spread pass lit.
    pub spread: u32,
    /// The fire removed itself.
    pub extinguished: bool,
    /// A `checkBurnOut` burned a TNT block and `TntBlock.prime` was **not**
    /// called. See [`BurnOut::primed_tnt`].
    pub unprimed_tnt: u32,
}

/// `FireBlock.tick(state, ServerLevel, BlockPos, RandomSource)` — offsets
/// 0-655, the whole method.
///
/// The body in the bytecode's own order, with the draw at each step:
///
/// ```text
///     0-10   scheduleTick(pos, this, getFireTickDelay(level.getRandom()))  [1 draw]
///    13-21   if (!level.canSpreadFireAround(pos)) return
///    22-37   if (!state.canSurvive(level, pos)) removeBlock(pos, false)   <- NO return
///    38-46   belowState = getBlockState(pos.below())
///    48-60   infiniburn = belowState.is(dimensionType().infiniburn())
///    62-75   age = state.getValue(AGE)
///    77-127  if (!infiniburn && isRaining() && isNearRain(pos)
///                && random.nextFloat() < 0.2f + age * 0.03f):            [1 draw]
///                removeBlock(pos, false); return
///   128-146  n = Math.min(15, age + random.nextInt(3) / 2)               [1 draw]
///   148-180  if (age != n) setBlock(pos, state.setValue(AGE, n), 260)
///   181-232  if (!infiniburn && !isValidFireLocation(pos)):
///                below = pos.below()
///                if (!getBlockState(below).isFaceSturdy(level, below, UP) || age > 3)
///                    removeBlock(pos, false)
///                return
///   233-273  if (!infiniburn && age == 15 && random.nextInt(4) == 0       [1 draw]
///                && !canBurn(getBlockState(pos.below()))):
///                removeBlock(pos, false); return
///   274-304  k = environmentAttributes().getValue(INCREASED_FIRE_BURNOUT, pos)
///                ? -50 : 0
///   306-417  six checkBurnOut calls: east 300+k, west 300+k, below 250+k,
///                above 250+k, north 300+k, south 300+k                   [>= 6 draws]
///   420-655  the spread walk
/// ```
///
/// **Three things a transcription gets wrong and this one does not.**
///
/// 1. **The `canSurvive` arm at 22-37 does not return.** It removes the
///    block and falls through into the rest of the tick, which then reads
///    `state`'s age off the state it was *handed* rather than off the world.
///    Every later `setBlock` in the tick therefore writes fire back over a
///    position the same tick just cleared, unless a later arm returns first.
///    That is vanilla's behaviour and the missing `return` is visible in the
///    bytecode as the fall-through at offset 38.
/// 2. **The age write at 171-180 uses flag `sipush 260`**, which is
///    `UPDATE_INVISIBLE | UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS` — `Block
///    .UPDATE_NONE` plus the skip. Nothing goes on the wire. A check that
///    counts `block_update` packets sees the *spread*, never the ageing, and
///    one that expected sixteen packets from a fire ageing to 15 would read
///    a correct server as broken. `crate::randomtick` records the same trap
///    for sugar cane's flag 260.
/// 3. **The spread walk's loops are x, then z, then y** — offsets 429-450 —
///    and the y range is `-1..=4` while x and z are `-1..=1`.
///    `MutableBlockPos.setWithOffset(pos, dx, dy, dz)` is called with
///    `(var12, var14, var13)`, so the *middle* loop variable is `dz` and the
///    *inner* one is `dy`. The chance bound is `100` and, for `dy > 1`,
///    `100 + (dy - 1) * 100` — offsets 474-494. That is not a `+ 30` of
///    anything, and it is per-position rather than per-fire.
///
/// The spread body, offsets 456-636:
///
/// ```text
///   456-471  if (dx == 0 && dy == 0 && dz == 0) continue
///   474-494  bound = 100; if (dy > 1) bound += (dy - 1) * 100
///   496-508  mutable.setWithOffset(pos, dx, dy, dz)
///   509-523  o = getIgniteOdds(level, mutable); if (o <= 0) continue
///   526-548  m = (o + 40 + level.getDifficulty().getId() * 7) / (age + 30)
///   550-559  if (increasedBurnout) m /= 2
///   561-577  if (m > 0 && random.nextInt(bound) <= m):                  [1 draw]
///   580-597      if (isRaining() && isNearRain(mutable)) continue
///   600-618      n2 = Math.min(15, age + random.nextInt(5) / 4)         [1 draw]
///   620-636      setBlock(mutable, getStateWithAge(level, mutable, n2), 3)
/// ```
///
/// **`nextInt(bound) <= m`, not `<`** — `if_icmpgt` at 577 skips when the
/// draw is *greater*, so a draw equal to `m` lights. And the `m > 0` guard is
/// checked before the draw, so a position whose odds round to zero costs no
/// draw at all, while a position that draws and then hits rain costs one and
/// lights nothing.
///
/// `state` is the state the tick was handed; a caller that re-read the world
/// would be running vanilla's `LevelTicks` for it, which
/// [`crate::ticks::run`] already does.
#[expect(
    clippy::too_many_arguments,
    reason = "vanilla's own four arguments plus this tree's four substitutes for a ServerLevel"
)]
pub fn tick(
    odds: &FireOdds,
    blocks: &dyn Blocks,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    rng: &mut dyn RandomSource,
    env: Env,
    updates: &mut Effects,
) -> Ticked {
    let mut out = Ticked::default();

    // 0-10. Unconditional and first: a fire reschedules even where the game
    // rule then stops it doing anything, so a fire outside the spread radius
    // is a live tick that does nothing rather than a dead one.
    updates.ticks.push(crate::shapes::TickRequest {
        pos,
        block: crate::blockentity::block_of(state).to_string(),
        delay: fire_tick_delay(rng),
        priority: crate::ticks::PRIORITY_NORMAL,
    });
    out.rescheduled = true;

    // 13-21.
    if !env.can_spread_fire_around {
        return out;
    }

    // 22-37. No return; see the doc comment.
    if !can_survive(odds, blocks, pos) {
        remove_block(blocks, shapes, pos, updates);
        out.extinguished = true;
    }

    let below = (pos.0, pos.1 - 1, pos.2);
    let infiniburn = env.infiniburn_below;
    let age = crate::placement::property_of(state, "age")
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or(0);

    // 77-127.
    if !infiniburn && env.raining && is_near_rain(env, pos) {
        #[expect(
            clippy::cast_precision_loss,
            reason = "vanilla's own `i2f` at offset 110; age is 0..15"
        )]
        let threshold = 0.2f32 + age as f32 * 0.03f32;
        if rng.next_f32() < threshold {
            remove_block(blocks, shapes, pos, updates);
            out.extinguished = true;
            return out;
        }
    }

    // 128-180. `nextInt(3) / 2` is 0, 0, 1 — so two ticks in three do not age.
    let n = MAX_AGE.min(age + rng.next_i32_bound(3) / 2);
    if age != n {
        let aged = crate::placement::with_property(state, "age", &n.to_string());
        // Flag 260. Deliberately not `UPDATE_ALL`: see the doc comment.
        crate::shapes::set_block(
            blocks,
            shapes,
            pos,
            &aged,
            UPDATE_NONE_SKIP_BLOCK_ENTITY,
            crate::shapes::RECURSION_LIMIT,
            updates,
        );
    }

    if !infiniburn {
        // 186-232.
        if !odds.is_valid_fire_location(blocks, pos) {
            if !blocks.face_sturdy(below, Face::Up) || age > 3 {
                remove_block(blocks, shapes, pos, updates);
                out.extinguished = true;
            }
            return out;
        }
        // 233-273.
        if age == MAX_AGE && rng.next_i32_bound(4) == 0 && !odds.can_burn(&blocks.state_at(below)) {
            remove_block(blocks, shapes, pos, updates);
            out.extinguished = true;
            return out;
        }
    }

    // 274-304.
    let k = if env.increased_burnout { -50 } else { 0 };

    // 306-417.
    for (face, chance) in BURN_OUT_FACES {
        let b = check_burn_out(
            odds,
            blocks,
            shapes,
            face.relative(pos),
            chance + k,
            rng,
            age,
            env,
            updates,
        );
        if b.burned {
            out.burned += 1;
        }
        if b.primed_tnt {
            out.unprimed_tnt += 1;
        }
    }

    // 420-655.
    for dx in -1..=1 {
        for dz in -1..=1 {
            for dy in -1..=4 {
                if dx == 0 && dy == 0 && dz == 0 {
                    continue;
                }
                let mut bound = 100;
                if dy > 1 {
                    bound += (dy - 1) * 100;
                }
                let at = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
                let o = odds.ignite_odds_at(blocks, at);
                if o <= 0 {
                    continue;
                }
                let mut m = (o + 40 + env.difficulty * 7) / (age + 30);
                if env.increased_burnout {
                    m /= 2;
                }
                if m > 0 && rng.next_i32_bound(bound) <= m {
                    if env.raining && is_near_rain(env, at) {
                        continue;
                    }
                    let n2 = MAX_AGE.min(age + rng.next_i32_bound(5) / 4);
                    if let Some(s) = state_with_age(odds, blocks, at, n2) {
                        crate::shapes::set_block(
                            blocks,
                            shapes,
                            at,
                            &s,
                            crate::shapes::UPDATE_ALL,
                            crate::shapes::RECURSION_LIMIT,
                            updates,
                        );
                        out.spread += 1;
                    }
                }
            }
        }
    }

    out
}

/// `sipush 260` at `FireBlock.tick:174` — `Block.UPDATE_INVISIBLE` (4) plus
/// `Block.UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS` (256).
///
/// `UPDATE_NEIGHBORS` and `UPDATE_CLIENTS` are both clear, so the ageing
/// write neither cascades nor reaches a client.
pub const UPDATE_NONE_SKIP_BLOCK_ENTITY: i32 = 260;

/// `Entity.baseTick`'s fire arm — offsets 125-187 — **transcribed and not
/// wired**, because the one thing it does is call a method this branch does
/// not own.
///
/// ```text
///   125-131  if (remainingFireTicks <= 0) skip the whole arm
///   132-143  if (fireImmune()) { clearFire(); }
///   146-176  else if (remainingFireTicks % 20 == 0 && !isInLava())
///                hurtServer(serverLevel, damageSources().onFire(), 1.0f)
///   177-187  setRemainingFireTicks(remainingFireTicks - 1)
///       190  on a client level: clearFire()
/// ```
///
/// and the ignition side, `BaseFireBlock.fireIgnite(Entity)`, offsets 0-72:
///
/// ```text
///    0-4    if (fireImmune()) return
///    7-24   if (getRemainingFireTicks() < 0)
///               setRemainingFireTicks(getRemainingFireTicks() + 1)
///   27-56   else if (entity instanceof ServerPlayer)
///               setRemainingFireTicks(getRemainingFireTicks()
///                                     + level.getRandom().nextInt(1, 3))
///   59-71   if (getRemainingFireTicks() >= 0) igniteForSeconds(8.0f)
/// ```
///
/// with `igniteForSeconds(f)` = `igniteForTicks(Mth.floor(f * 20.0f))` and
/// `igniteForTicks(n)` = `if (remainingFireTicks < n) setRemainingFireTicks(n);
/// clearFreeze()` — so 8 seconds is **160 ticks** and the ignite never
/// *shortens* an existing burn. `clearFire()` is
/// `setRemainingFireTicks(Math.min(0, getRemainingFireTicks()))`, which
/// preserves a negative value rather than zeroing it. `nextInt(1, 3)` is
/// `nextInt(3 - 1 + 1) + 1`, i.e. 1..=3, and it is drawn **only for a
/// `ServerPlayer`**.
///
/// [`FireTicks`] is the state and the two transitions. **No damage.** The
/// `hurtServer` at offset 173 and the `hurt` in
/// `BaseFireBlock.lambda$entityInside$0` are `origin/player-systems-2`'s
/// `hurt_server`; this type is the join, and wiring it is that lane's one
/// call.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FireTicks {
    /// `Entity.remainingFireTicks`. Signed: a negative value is an entity
    /// that has been given fire *resistance* time, and `fireIgnite` counts it
    /// up towards zero one tick at a time rather than igniting.
    pub remaining: i32,
}

impl FireTicks {
    /// `BaseFireBlock.SECONDS_ON_FIRE` — `ldc 8.0f` at offset 67.
    pub const SECONDS_ON_FIRE: f32 = 8.0;

    /// `Entity.igniteForSeconds(float)` then `igniteForTicks(int)`.
    pub fn ignite_for_seconds(&mut self, seconds: f32) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "Mth.floor(f) is `(int) Math.floor(f)`; the argument here is 8.0 * 20"
        )]
        let ticks = (seconds * 20.0f32).floor() as i32;
        self.ignite_for_ticks(ticks);
    }

    /// `Entity.igniteForTicks(int)` — a floor, never a shortening.
    pub const fn ignite_for_ticks(&mut self, ticks: i32) {
        if self.remaining < ticks {
            self.remaining = ticks;
        }
    }

    /// `Entity.clearFire()` — `setRemainingFireTicks(Math.min(0, remaining))`.
    pub const fn clear_fire(&mut self) {
        if self.remaining > 0 {
            self.remaining = 0;
        }
    }

    /// `BaseFireBlock.fireIgnite(Entity)`, offsets 0-72.
    ///
    /// `is_server_player` selects the `instanceof ServerPlayer` branch, which
    /// is the only one that draws.
    pub fn fire_ignite(
        &mut self,
        fire_immune: bool,
        is_server_player: bool,
        rng: &mut dyn RandomSource,
    ) {
        if fire_immune {
            return;
        }
        if self.remaining < 0 {
            self.remaining += 1;
        } else if is_server_player {
            self.remaining += rng.next_i32_between_inclusive(1, 3);
        }
        if self.remaining >= 0 {
            self.ignite_for_seconds(Self::SECONDS_ON_FIRE);
        }
    }

    /// `Entity.baseTick`'s fire arm on a server level, **minus the damage**.
    ///
    /// Returns whether vanilla would have called
    /// `hurtServer(level, damageSources().onFire(), 1.0f)` this tick. Nothing
    /// in this tree acts on that; see the type's docs.
    pub const fn base_tick(&mut self, fire_immune: bool, in_lava: bool) -> bool {
        if self.remaining <= 0 {
            return false;
        }
        if fire_immune {
            self.clear_fire();
            return false;
        }
        let hurts = self.remaining % 20 == 0 && !in_lava;
        self.remaining -= 1;
        hurts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmc_worldgen::rng::legacy::LegacyRandomSource;
    use std::collections::BTreeMap as Map;
    use std::sync::Mutex;

    /// The odds this module's tests use, hand-built to a shape the real table
    /// has rather than loaded from it: **the loader is tested against the real
    /// file separately** ([`the_shipped_table_is_the_one_the_bytecode_counts`])
    /// and a rule test that also depended on the extract would fail for two
    /// unrelated reasons and say only one of them.
    ///
    /// The three values are real rows: `oak_planks` (5, 20), `oak_leaves`
    /// (30, 60) and `white_wool` (30, 60) — the last through the
    /// `Blocks.WOOL` sweep, so a table that lost the sweep loses this row.
    fn odds() -> FireOdds {
        let mut by_block = HashMap::new();
        by_block.insert("minecraft:oak_planks".to_string(), (5, 20));
        by_block.insert("minecraft:oak_leaves".to_string(), (30, 60));
        by_block.insert("minecraft:white_wool".to_string(), (30, 60));
        by_block.insert("minecraft:tnt".to_string(), (15, 100));
        FireOdds { by_block }
    }

    #[derive(Default)]
    struct Fake {
        blocks: Mutex<Map<BlockPos, String>>,
        /// Positions whose upward face is sturdy. Explicit rather than
        /// "anything that is not air", because **this module's whole
        /// placement branch turns on exactly that distinction**: a sturdy
        /// block below short-circuits both `canSurvive` and the connection
        /// walk, so a double that called every solid block sturdy could not
        /// reach the walk at all and every test of it would pass vacuously.
        sturdy: Vec<BlockPos>,
    }

    impl Fake {
        fn with(states: &[(BlockPos, &str)], sturdy: &[BlockPos]) -> Self {
            let f = Self {
                sturdy: sturdy.to_vec(),
                ..Self::default()
            };
            for (pos, state) in states {
                f.blocks
                    .lock()
                    .expect("fake")
                    .insert(*pos, (*state).to_string());
            }
            f
        }
    }

    impl Blocks for Fake {
        fn state_at(&self, pos: BlockPos) -> String {
            self.blocks
                .lock()
                .expect("fake")
                .get(&pos)
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
        fn set_block(&self, pos: BlockPos, state: &str) -> Option<i32> {
            if self.state_at(pos) == state {
                return None;
            }
            self.blocks
                .lock()
                .expect("fake")
                .insert(pos, state.to_string());
            Some(7)
        }
        fn default_state(&self, block: &str) -> Option<String> {
            Some(match block {
                // The real default state, so a test cannot pass on a state
                // string vanilla never emits. `down` is absent: see
                // `property_for_direction`.
                "minecraft:fire" => {
                    "minecraft:fire[age=0,east=false,north=false,south=false,up=false,west=false]"
                        .to_string()
                }
                other => other.to_string(),
            })
        }
        fn has_state(&self, _: &str) -> bool {
            true
        }
        fn face_sturdy(&self, pos: BlockPos, face: Face) -> bool {
            face == Face::Up && self.sturdy.contains(&pos)
        }
        fn redstone_conductor(&self, _: BlockPos) -> bool {
            false
        }
        fn signal_source(&self, _: BlockPos) -> bool {
            false
        }
        fn state_id(&self, _: &str) -> Option<i32> {
            Some(1)
        }
        fn replaceable(&self, _: BlockPos, _: &crate::placement::Replacing<'_>) -> bool {
            true
        }
        fn legacy_fluid_at(&self, _: BlockPos) -> String {
            "minecraft:air".to_string()
        }
        fn water_at(&self, pos: BlockPos) -> bool {
            self.state_at(pos).starts_with("minecraft:water")
        }
    }

    // -----------------------------------------------------------------------
    // The table
    // -----------------------------------------------------------------------

    /// **The positive control for the whole file.** Everything below asserts
    /// that some rule reads the odds; this asserts the odds are what vanilla's
    /// own maps say, against the shipped extract rather than against the
    /// hand-built [`odds`].
    ///
    /// The three figures are the generator's own cross-check, restated on this
    /// side of the wall: a table whose `flammableBlocks` stopped agreeing with
    /// `directSetFlammableSites + colorCollectionSweeps * colorsPerSweep`
    /// could only get here through a generator whose refusal had been removed.
    #[test]
    fn the_shipped_table_is_the_one_the_bytecode_counts() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../")
            .join(TABLE_PATH);
        if !rustmc_data::fixture::available(&path, "fire odds extract", "ci/fire-odds-golden.sh") {
            return;
        }
        let table = FireOdds::load(&path).expect("load fire_odds.json");
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("parse");
        let n = |k: &str| doc.get(k).and_then(serde_json::Value::as_i64).expect(k);
        assert_eq!(
            n("flammableBlocks"),
            n("directSetFlammableSites") + n("colorCollectionSweeps") * n("colorsPerSweep"),
            "the table disagrees with the bytecode count its own generator refuses on"
        );
        assert_eq!(
            table.len(),
            usize::try_from(n("flammableBlocks")).expect("count"),
            "the loader dropped rows the generator wrote"
        );
        // Four rows read off `bootStrap()`'s disassembly by hand, one from
        // each shape the method has: a `bipush`/`bipush` pair, a
        // `bipush`/`iconst` pair, and one from each `ColorCollection.forEach`
        // sweep. The wool and the carpet are the load-bearing pair — they are
        // the thirty-two rows a reader who counted only the literal call
        // sites would not have.
        for (block, ignite, burn) in [
            ("minecraft:oak_planks", 5, 20),
            ("minecraft:oak_log", 5, 5),
            ("minecraft:white_wool", 30, 60),
            ("minecraft:black_carpet", 60, 20),
        ] {
            assert_eq!(
                (table.ignite_odds(block), table.burn_odds(block)),
                (ignite, burn),
                "{block}"
            );
        }
        assert_eq!(
            table.ignite_odds("minecraft:stone"),
            0,
            "stone is not in it"
        );
    }

    /// The `WATERLOGGED` short-circuit at offsets 0-27 of **both** lookups.
    #[test]
    fn a_waterlogged_state_has_no_odds_at_all() {
        let t = odds();
        assert_eq!(t.ignite_odds("minecraft:oak_leaves[waterlogged=false]"), 30);
        assert_eq!(t.burn_odds("minecraft:oak_leaves[waterlogged=false]"), 60);
        assert_eq!(t.ignite_odds("minecraft:oak_leaves[waterlogged=true]"), 0);
        assert_eq!(t.burn_odds("minecraft:oak_leaves[waterlogged=true]"), 0);
        assert!(!t.can_burn("minecraft:oak_leaves[waterlogged=true]"));
    }

    /// An empty table refuses rather than loading. See [`FireOdds::load`].
    #[test]
    fn an_empty_table_is_refused_rather_than_read_as_nothing_being_flammable() {
        let dir = std::env::temp_dir().join(format!("fire-odds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let p = dir.join("empty.json");
        std::fs::write(&p, br#"{"odds":{}}"#).expect("write");
        let e = FireOdds::load(&p).expect_err("an empty table must not load");
        assert!(e.to_string().contains("carries no blocks"), "{e}");
        std::fs::remove_file(&p).ok();
    }

    // -----------------------------------------------------------------------
    // The placement walk
    // -----------------------------------------------------------------------

    /// `getStateForPlacement` offsets 0-40: a sturdy block below returns the
    /// **default** state without walking anything, so the connection booleans
    /// stay false even with a burnable neighbour beside it.
    #[test]
    fn a_sturdy_block_below_takes_the_default_state_without_the_walk() {
        let w = Fake::with(
            &[
                ((0, -1, 0), "minecraft:stone"),
                ((0, 0, 1), "minecraft:oak_planks"),
            ],
            &[(0, -1, 0)],
        );
        let s = state_for_placement(&odds(), &w as &dyn Blocks, (0, 0, 0)).expect("fire");
        assert_eq!(
            s, "minecraft:fire[age=0,east=false,north=false,south=false,up=false,west=false]",
            "the south neighbour burns, and the walk did not run"
        );
    }

    /// A burnable block below also short-circuits — the guard is
    /// `canBurn(below) || sturdy(below)`, and this arm is the `canBurn` one.
    #[test]
    fn a_burnable_block_below_also_takes_the_default_state() {
        let w = Fake::with(
            &[
                ((0, -1, 0), "minecraft:oak_leaves"),
                ((0, 0, 1), "minecraft:oak_planks"),
            ],
            &[],
        );
        let s = state_for_placement(&odds(), &w as &dyn Blocks, (0, 0, 0)).expect("fire");
        assert!(s.contains("south=false"), "{s}");
    }

    /// The walk itself, and the `DOWN` hole in `PROPERTY_BY_DIRECTION`.
    ///
    /// Below is neither burnable nor sturdy, so offsets 41-131 run. A
    /// burnable block below-but-unreachable proves the `down` case: the
    /// `p != null` guard skips it, so **no `down` property is written at
    /// all** — and if one were, the state string would carry a key the block
    /// does not have and nothing would render it.
    #[test]
    fn the_connection_walk_sets_five_faces_and_never_a_sixth() {
        let w = Fake::with(
            &[
                // Below: not burnable, not sturdy — so the walk runs, and it
                // is also the DOWN neighbour the walk must skip.
                ((0, -1, 0), "minecraft:oak_fence"),
                ((0, 0, -1), "minecraft:oak_planks"), // north
                ((0, 1, 0), "minecraft:white_wool"),  // up
                ((-1, 0, 0), "minecraft:oak_leaves"), // west
            ],
            &[],
        );
        let s = state_for_placement(&odds(), &w as &dyn Blocks, (0, 0, 0)).expect("fire");
        assert!(s.contains("north=true"), "{s}");
        assert!(s.contains("up=true"), "{s}");
        assert!(s.contains("west=true"), "{s}");
        assert!(s.contains("east=false"), "{s}");
        assert!(s.contains("south=false"), "{s}");
        assert!(!s.contains("down"), "fire has no `down` property: {s}");
    }

    /// `getIgniteOdds(LevelReader, BlockPos)` — the max over six, and the
    /// `isEmptyBlock` guard in front of it.
    #[test]
    fn the_positional_ignite_odds_are_the_max_of_six_and_zero_on_a_full_block() {
        let t = odds();
        let w = Fake::with(
            &[
                ((0, 0, 1), "minecraft:oak_planks"), // 5
                ((0, 1, 0), "minecraft:oak_leaves"), // 30
                ((5, 0, 0), "minecraft:stone"),
                ((5, 0, 1), "minecraft:oak_leaves"),
            ],
            &[],
        );
        assert_eq!(
            t.ignite_odds_at(&w as &dyn Blocks, (0, 0, 0)),
            30,
            "the max, not the sum"
        );
        assert_eq!(
            t.ignite_odds_at(&w as &dyn Blocks, (5, 0, 0)),
            0,
            "a position that is not air scores 0 whatever is beside it"
        );
    }

    // -----------------------------------------------------------------------
    // The tick
    // -----------------------------------------------------------------------

    const FIRE0: &str =
        "minecraft:fire[age=0,east=false,north=false,south=false,up=false,west=false]";

    /// A world with a fire on stone and one plank wall beside it.
    fn burning_world() -> Fake {
        Fake::with(
            &[
                ((0, -1, 0), "minecraft:stone"),
                ((0, 0, 0), FIRE0),
                ((1, 0, 0), "minecraft:oak_planks"),
            ],
            &[(0, -1, 0), (1, -1, 0)],
        )
    }

    /// **The reschedule is unconditional and first.** Offsets 0-10 run before
    /// the game-rule gate at 13, so a fire the rule has switched off still
    /// asks to be woken — and a port that put the gate first would leave a
    /// fire that never ticks again once the rule flips back.
    #[test]
    fn a_fire_reschedules_before_the_game_rule_gate_and_does_nothing_after_it() {
        let w = burning_world();
        let mut rng = LegacyRandomSource::new(1);
        let mut fx = Effects::new();
        let env = Env {
            can_spread_fire_around: false,
            ..Env::default()
        };
        let out = tick(&odds(), &w, None, (0, 0, 0), FIRE0, &mut rng, env, &mut fx);
        assert!(out.rescheduled);
        assert_eq!(fx.ticks.len(), 1);
        assert_eq!(fx.ticks[0].block, "minecraft:fire");
        assert!(
            (30..40).contains(&fx.ticks[0].delay),
            "getFireTickDelay is 30 + nextInt(10), got {}",
            fx.ticks[0].delay
        );
        assert_eq!(out.burned, 0, "the gate stopped everything after it");
        assert_eq!(out.spread, 0);
        assert!(fx.blocks.is_empty(), "and it wrote nothing");
    }

    /// `getFireTickDelay` is `30 + nextInt(10)` and nothing else. Both bounds
    /// asserted, because a port writing `nextInt(30) + 10` has the same mean.
    #[test]
    fn the_fire_tick_delay_is_thirty_plus_a_draw_under_ten() {
        let mut rng = LegacyRandomSource::new(20_260_906);
        let mut lo = i64::MAX;
        let mut hi = i64::MIN;
        for _ in 0..10_000 {
            let d = fire_tick_delay(&mut rng);
            lo = lo.min(d);
            hi = hi.max(d);
        }
        assert_eq!((lo, hi), (30, 39));
    }

    /// **The `canSurvive` arm at 22-37 does not return**, and this is the
    /// assertion that says so: a fire with nothing under it and nothing
    /// burnable beside it is removed *and the tick keeps going*, so the
    /// unconditional reschedule at the top still stands and the age draw is
    /// still taken.
    ///
    /// A port that added the `return` a reader expects passes every other
    /// test in this file.
    #[test]
    fn a_fire_that_cannot_survive_is_removed_and_the_tick_continues() {
        let w = Fake::with(&[((0, 0, 0), FIRE0)], &[]);
        let mut rng = LegacyRandomSource::new(3);
        let mut fx = Effects::new();
        let out = tick(
            &odds(),
            &w,
            None,
            (0, 0, 0),
            FIRE0,
            &mut rng,
            Env::default(),
            &mut fx,
        );
        assert!(out.extinguished);
        assert_eq!(w.state_at((0, 0, 0)), "minecraft:air");
        assert!(
            out.rescheduled,
            "the reschedule happened before the removal"
        );
    }

    /// The six-face burn-out pass draws once per face **whatever is there**,
    /// so its cost in the stream is fixed. Measured by the arithmetic rather
    /// than by counting calls: two runs from the same seed over worlds that
    /// differ only in what the six neighbours are must leave the generator at
    /// the same place if — and only if — nothing burned.
    ///
    /// This is the property a reordering of [`BURN_OUT_FACES`] breaks and no
    /// block assertion can see.
    #[test]
    fn the_burn_out_pass_costs_one_draw_a_face_whatever_is_there() {
        let empty = Fake::with(
            &[((0, -1, 0), "minecraft:stone"), ((0, 0, 0), FIRE0)],
            &[(0, -1, 0)],
        );
        // Nothing burnable anywhere: `is_valid_fire_location` is false and
        // the tick returns at offset 232, so this is the control for the
        // draws *before* the pass.
        let mut a = LegacyRandomSource::new(99);
        let mut fx = Effects::new();
        let out = tick(
            &odds(),
            &empty,
            None,
            (0, 0, 0),
            FIRE0,
            &mut a,
            Env::default(),
            &mut fx,
        );
        assert_eq!(out.burned, 0);
        assert_eq!(out.spread, 0);
        // Now the same seed with one plank beside it, which reaches the pass.
        let world = burning_world();
        let mut b = LegacyRandomSource::new(99);
        let mut fx = Effects::new();
        let out = tick(
            &odds(),
            &world,
            None,
            (0, 0, 0),
            FIRE0,
            &mut b,
            Env::default(),
            &mut fx,
        );
        assert!(out.rescheduled, "the second world reached the pass at all");
        assert_ne!(
            a.next_i32(),
            b.next_i32(),
            "the two runs consumed the same number of draws, so the six-face \
             pass took none — the pass is not running"
        );
    }

    /// The ageing write uses flag 260 and **nothing goes on the wire**. A
    /// check counting `block_update` packets sees the spread and never the
    /// ageing; this is the assertion that pins the number rather than the
    /// consequence.
    #[test]
    fn the_ageing_write_is_flag_two_hundred_and_sixty() {
        assert_eq!(UPDATE_NONE_SKIP_BLOCK_ENTITY, 260);
        assert_eq!(
            UPDATE_NONE_SKIP_BLOCK_ENTITY & crate::shapes::UPDATE_CLIENTS,
            0
        );
        assert_eq!(
            UPDATE_NONE_SKIP_BLOCK_ENTITY & crate::shapes::UPDATE_NEIGHBORS,
            0
        );
    }

    /// `checkBurnOut`'s TNT arm is refused, and the refusal is **counted**
    /// rather than silent — which is the only way a caller can tell "no TNT
    /// burned" from "TNT burned and nothing happened".
    #[test]
    fn burning_tnt_is_recorded_as_unprimed_rather_than_dropped() {
        let t = odds();
        let w = Fake::with(&[((0, 0, 0), "minecraft:tnt")], &[]);
        let mut fx = Effects::new();
        // `burnOdds` for TNT is 100 and the chance is 1, so `nextInt(1)` is 0
        // and the burn is certain: the draw is pinned, not sampled.
        let mut rng = LegacyRandomSource::new(5);
        let b = check_burn_out(
            &t,
            &w,
            None,
            (0, 0, 0),
            1,
            &mut rng,
            0,
            Env::default(),
            &mut fx,
        );
        assert!(b.burned);
        assert!(b.primed_tnt, "the refusal is visible to the caller");
    }

    /// The spread walk's chance bound: `100`, and `100 + (dy - 1) * 100` for
    /// `dy > 1`. Six values, from the loop's own y range.
    #[test]
    fn the_spread_bound_rises_only_above_the_fire_and_by_a_hundred_a_level() {
        let bound = |dy: i32| if dy > 1 { 100 + (dy - 1) * 100 } else { 100 };
        assert_eq!(
            (-1..=4).map(bound).collect::<Vec<_>>(),
            vec![100, 100, 100, 200, 300, 400],
            "offsets 474-494: a `+ 30` or a per-fire bound gets this wrong"
        );
    }

    /// The spread test is `nextInt(bound) <= m`, **not `<`** — `if_icmpgt` at
    /// offset 577. One off, and it changes the spread rate by `1/bound`.
    #[test]
    fn a_spread_draw_equal_to_the_odds_lights() {
        // `m = (o + 40 + difficulty * 7) / (age + 30)`; with `o = 30`,
        // difficulty 1 and age 0 that is `77 / 30 = 2`, integer division.
        let m = |o: i32, difficulty: i32, age: i32| (o + 40 + difficulty * 7) / (age + 30);
        assert_eq!(m(30, 1, 0), 2);
        // The two draws either side of the boundary, as `tick` tests them.
        assert!(2 <= m(30, 1, 0), "a draw equal to m lights");
        assert!(3 > m(30, 1, 0), "one more does not");
        // And the age divisor really divides: an old fire spreads less.
        assert_eq!(m(60, 1, 15), 2);
        assert_eq!(m(60, 1, 0), 3);
    }

    /// A fire on stone with planks beside it spreads, and every position it
    /// lights is inside the walk's own box.
    #[test]
    fn a_fire_spreads_only_inside_its_own_box() {
        let mut lit = 0;
        for seed in 0..40i64 {
            let w = Fake::with(
                &[
                    ((0, -1, 0), "minecraft:stone"),
                    ((0, 0, 0), FIRE0),
                    ((1, 0, 0), "minecraft:oak_leaves"),
                    ((1, 1, 0), "minecraft:oak_leaves"),
                    ((1, 2, 0), "minecraft:oak_leaves"),
                ],
                &[(0, -1, 0)],
            );
            let mut rng = LegacyRandomSource::new(seed);
            let mut fx = Effects::new();
            let out = tick(
                &odds(),
                &w,
                None,
                (0, 0, 0),
                FIRE0,
                &mut rng,
                Env::default(),
                &mut fx,
            );
            lit += out.spread;
            for (p, _) in &fx.blocks {
                assert!(
                    (-1..=1).contains(&p.0) && (-1..=4).contains(&p.1) && (-1..=1).contains(&p.2),
                    "wrote outside the 3x6x3 box at {p:?}"
                );
            }
        }
        assert!(lit > 0, "40 ticks over a leaf wall lit nothing at all");
    }

    // -----------------------------------------------------------------------
    // Entity fire ticks — transcribed, damage refused
    // -----------------------------------------------------------------------

    /// `igniteForSeconds(8.0)` is **160 ticks**, and `igniteForTicks` is a
    /// floor rather than an assignment.
    #[test]
    fn igniting_for_eight_seconds_is_a_hundred_and_sixty_ticks_and_never_shortens() {
        let mut f = FireTicks::default();
        f.ignite_for_seconds(FireTicks::SECONDS_ON_FIRE);
        assert_eq!(f.remaining, 160);
        f.remaining = 300;
        f.ignite_for_seconds(FireTicks::SECONDS_ON_FIRE);
        assert_eq!(
            f.remaining, 300,
            "an ignite never shortens an existing burn"
        );
    }

    /// `clearFire()` is `min(0, remaining)` and **keeps a negative value**. A
    /// port that zeroed it would give an entity with fire resistance time a
    /// fresh ignition on its next tick.
    #[test]
    fn clearing_fire_keeps_a_negative_remaining() {
        let mut f = FireTicks { remaining: 40 };
        f.clear_fire();
        assert_eq!(f.remaining, 0);
        let mut f = FireTicks { remaining: -7 };
        f.clear_fire();
        assert_eq!(f.remaining, -7);
    }

    /// `fireIgnite` draws **only** for a `ServerPlayer`, and a negative
    /// remaining counts up by one instead of igniting.
    #[test]
    fn fire_ignite_draws_only_for_a_server_player() {
        let mut rng = LegacyRandomSource::new(11);
        let before = rng;
        let mut f = FireTicks { remaining: 0 };
        f.fire_ignite(false, false, &mut rng);
        let mut after = rng;
        let mut b = before;
        assert_eq!(
            after.next_i32(),
            b.next_i32(),
            "a non-player took a draw it should not have"
        );
        assert_eq!(f.remaining, 160);

        let mut f = FireTicks { remaining: -5 };
        f.fire_ignite(false, true, &mut rng);
        assert_eq!(f.remaining, -4, "counted up, did not ignite");

        let mut f = FireTicks { remaining: 0 };
        f.fire_ignite(false, true, &mut rng);
        assert_eq!(f.remaining, 160, "1..=3 then floored to 160");

        let mut f = FireTicks { remaining: 40 };
        f.fire_ignite(true, true, &mut rng);
        assert_eq!(f.remaining, 40, "fire-immune returns at the first line");
    }

    /// `baseTick`'s fire arm: it decrements every tick and reports the damage
    /// on every twentieth, and **`hurtServer` is not called here**. The
    /// return value is the join to `origin/player-systems-2`.
    #[test]
    fn the_base_tick_arm_decrements_and_reports_the_damage_it_does_not_do() {
        let mut f = FireTicks { remaining: 40 };
        assert!(f.base_tick(false, false), "40 % 20 == 0");
        assert_eq!(f.remaining, 39);
        assert!(!f.base_tick(false, false));
        assert_eq!(f.remaining, 38);

        let mut f = FireTicks { remaining: 40 };
        assert!(
            !f.base_tick(false, true),
            "in lava the fire arm does no damage of its own"
        );
        assert_eq!(f.remaining, 39, "and still decrements");

        let mut f = FireTicks { remaining: 40 };
        assert!(!f.base_tick(true, false));
        assert_eq!(f.remaining, 0, "fire-immune clears rather than burning");
    }
}
