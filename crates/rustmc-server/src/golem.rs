//! `CarvedPumpkinBlock.onPlace` → `trySpawnGolem`: a pumpkin put on top of
//! the right blocks turns them into a golem.
//!
//! Transcribed from the pinned 26.2 jar, `javap -p -c -constants` on
//! `CarvedPumpkinBlock`, `BlockPattern`, `BlockPatternBuilder`,
//! `CopperChestBlock` and `ChestBlock`.
//!
//! # What runs, in order
//!
//! ```text
//! onPlace(state, level, pos, old, moved)
//!    0-12   if (old.is(state.getBlock())) return
//!   14-16   trySpawnGolem(level, pos)
//!
//! trySpawnGolem
//!    1-51   snow full match   -> SNOW_GOLEM,  spawnGolemInWorld(m, golem, m.getBlock(0,2,0))
//!   53-113  iron full match   -> IRON_GOLEM,  setPlayerCreated(true) FIRST (88-91),
//!                                              spawnGolemInWorld(m, golem, m.getBlock(1,2,0))
//!  115-187  copper full match -> COPPER_GOLEM, spawnGolemInWorld(m, golem, m.getBlock(0,0,0)),
//!                                              replaceCopperBlockWithChest(m),
//!                                              golem.spawn(getWeatherStateFromPattern(m))
//!
//! spawnGolemInWorld(level, m, entity, pos)
//!    0-2    clearPatternBlocks: for i < width, for j < height:
//!             setBlock(getBlock(i,j,0).pos, AIR, 2)
//!             levelEvent(2001, pos, Block.getId(getBlock(i,j,0).getState()))
//!    5-35   entity.snapTo(pos.x + 0.5, pos.y + 0.05, pos.z + 0.5, 0.0F, 0.0F)
//!   38-43   level.addFreshEntity(entity)
//!   45-98   SUMMONED_ENTITY for every ServerPlayer in bb.inflate(5)
//!  102-103  updatePatternBlocks: updateNeighborsAt(getBlock(i,j,0).pos, AIR), same loop
//! ```
//!
//! The level-event data is the state **the pattern cached when it matched**,
//! not a fresh read: `BlockInWorld.getState()` loads once and keeps it, and
//! every cell was read by `matches` before the clear. So the pumpkin's own
//! id rides the event at its position, and an air cell's is `0`.
//!
//! # Where this sits in the write
//!
//! `onPlace` is reached from `LevelChunk.setBlockState`, inside the write
//! that put the pumpkin down — so by the time `BlockItem.place` reads the
//! state back for its sound, the pumpkin is already air and the sound is
//! **stone's** (`Blocks.AIR`'s sound type), which is what vanilla recorded
//! for `788` and `789` and this server did not.
//!
//! # What is not ported, by name
//!
//! * `CriteriaTriggers.SUMMONED_ENTITY` (offsets 45-98): no advancement
//!   trigger of that kind exists here.
//! * `BlockInWorld`'s `loadChunks = false`: a cell in an unloaded chunk
//!   reads `null` and fails its predicate; here every chunk the world can
//!   answer is loaded, so the arm is unreachable rather than modelled.

use crate::edits::Blocks;
use crate::interact::BlockPos;

/// `Direction`, in `Direction.values()` order — which is the order
/// `BlockPattern.find` tries orientations in, and so decides which of
/// several equivalent matches wins and therefore the order of the level
/// events a clear sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Dir {
    pub const VALUES: [Self; 6] = [
        Self::Down,
        Self::Up,
        Self::North,
        Self::South,
        Self::West,
        Self::East,
    ];

    #[must_use]
    pub const fn step(self) -> (i32, i32, i32) {
        match self {
            Self::Down => (0, -1, 0),
            Self::Up => (0, 1, 0),
            Self::North => (0, 0, -1),
            Self::South => (0, 0, 1),
            Self::West => (-1, 0, 0),
            Self::East => (1, 0, 0),
        }
    }

    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Down => Self::Up,
            Self::Up => Self::Down,
            Self::North => Self::South,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::East => Self::West,
        }
    }

    /// `Direction.getClockWise()` about the Y axis — defined for the four
    /// horizontals, which is all a `FACING` can be.
    #[must_use]
    pub const fn clockwise(self) -> Self {
        match self {
            Self::North => Self::East,
            Self::East => Self::South,
            Self::South => Self::West,
            Self::West => Self::North,
            other => other,
        }
    }

    #[must_use]
    pub const fn counter_clockwise(self) -> Self {
        match self {
            Self::North => Self::West,
            Self::West => Self::South,
            Self::South => Self::East,
            Self::East => Self::North,
            other => other,
        }
    }

    #[must_use]
    pub fn from_name(s: &str) -> Option<Self> {
        Some(match s {
            "down" => Self::Down,
            "up" => Self::Up,
            "north" => Self::North,
            "south" => Self::South,
            "west" => Self::West,
            "east" => Self::East,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Down => "down",
            Self::Up => "up",
            Self::North => "north",
            Self::South => "south",
            Self::West => "west",
            Self::East => "east",
        }
    }

    fn relative(self, p: BlockPos) -> BlockPos {
        let s = self.step();
        (p.0 + s.0, p.1 + s.1, p.2 + s.2)
    }
}

/// One predicate of a pattern: which states a cell accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    /// `PUMPKINS_PREDICATE` — `CARVED_PUMPKIN` or `JACK_O_LANTERN`, any state.
    Pumpkin,
    /// `BlockStatePredicate.forBlock(Blocks.SNOW_BLOCK)`.
    Snow,
    /// `BlockStatePredicate.forBlock(Blocks.IRON_BLOCK)`.
    Iron,
    /// `BlockBehaviour$BlockStateBase::isAir` — the iron pattern's `~`.
    Air,
    /// `state.is(BlockTags.COPPER)`.
    Copper,
}

/// The four copper blocks and their four waxed twins that `BlockTags.COPPER`
/// names — `tags_block.json`'s `copper` entry, and the same eight
/// `WeatheringCopperCollection`s `COPPER_TO_COPPER_CHEST_MAPPING` zips.
pub const COPPER_TAG: [&str; 8] = [
    "minecraft:copper_block",
    "minecraft:exposed_copper",
    "minecraft:weathered_copper",
    "minecraft:oxidized_copper",
    "minecraft:waxed_copper_block",
    "minecraft:waxed_exposed_copper",
    "minecraft:waxed_weathered_copper",
    "minecraft:waxed_oxidized_copper",
];

fn block_of(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

fn is_copper(block: &str) -> bool {
    // The tag is read from the extract when it is there, so a jar that
    // widened `#copper` would widen this with it; the constant is the
    // fallback and the unit tests' fixture.
    crate::fallable::tags().map_or_else(
        || COPPER_TAG.contains(&block),
        |t| t.contains("minecraft:copper", block),
    )
}

impl Cell {
    #[must_use]
    pub fn test(self, state: &str) -> bool {
        let block = block_of(state);
        match self {
            Self::Pumpkin => {
                block == "minecraft:carved_pumpkin" || block == "minecraft:jack_o_lantern"
            }
            Self::Snow => block == "minecraft:snow_block",
            Self::Iron => block == "minecraft:iron_block",
            Self::Air => crate::shapes::is_air(state),
            Self::Copper => is_copper(block),
        }
    }
}

/// `BlockPatternBuilder.aisle(rows...).where(...)`, one aisle deep — every
/// golem pattern is. `rows[j][i]`, top row first, `None` for the builder's
/// implicit `' '`, which matches anything.
#[derive(Debug)]
pub struct Pattern {
    pub rows: &'static [&'static [Option<Cell>]],
}

impl Pattern {
    #[must_use]
    pub fn width(&self) -> i32 {
        i32::try_from(self.rows[0].len()).unwrap_or(0)
    }

    #[must_use]
    pub fn height(&self) -> i32 {
        i32::try_from(self.rows.len()).unwrap_or(0)
    }

    fn cell(&self, i: i32, j: i32) -> Option<Cell> {
        let (i, j) = (usize::try_from(i).ok()?, usize::try_from(j).ok()?);
        self.rows[j][i]
    }
}

use Cell::{Air, Copper, Iron, Pumpkin, Snow};

/// `getOrCreateSnowGolemFull`: `aisle("^", "#", "#")`.
pub static SNOW_GOLEM_FULL: Pattern = Pattern {
    rows: &[&[Some(Pumpkin)], &[Some(Snow)], &[Some(Snow)]],
};

/// `getOrCreateIronGolemFull`: `aisle("~^~", "###", "~#~")`.
pub static IRON_GOLEM_FULL: Pattern = Pattern {
    rows: &[
        &[Some(Air), Some(Pumpkin), Some(Air)],
        &[Some(Iron), Some(Iron), Some(Iron)],
        &[Some(Air), Some(Iron), Some(Air)],
    ],
};

/// `getOrCreateCopperGolemFull`: `aisle("^", "#")`.
pub static COPPER_GOLEM_FULL: Pattern = Pattern {
    rows: &[&[Some(Pumpkin)], &[Some(Copper)]],
};

/// `BlockPattern$BlockPatternMatch` with its cache: the cells' positions
/// and the states they held when the pattern matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub front_top_left: BlockPos,
    pub forwards: Dir,
    pub up: Dir,
    pub width: i32,
    pub height: i32,
    /// `(i, j) -> state` as `matches` read it — `BlockInWorld`'s cache.
    cached: Vec<((i32, i32), String)>,
}

impl Match {
    /// `getBlock(i, j, 0).getPos()`.
    #[must_use]
    pub fn pos(&self, i: i32, j: i32) -> BlockPos {
        translate_and_rotate(self.front_top_left, self.forwards, self.up, i, j, 0)
    }

    /// `getBlock(i, j, 0).getState()` — the cached state.
    #[must_use]
    pub fn state(&self, i: i32, j: i32) -> &str {
        self.cached
            .iter()
            .find(|(k, _)| *k == (i, j))
            .map_or("minecraft:air", |(_, s)| s.as_str())
    }
}

const fn cross(a: (i32, i32, i32), b: (i32, i32, i32)) -> (i32, i32, i32) {
    (
        a.1 * b.2 - a.2 * b.1,
        a.2 * b.0 - a.0 * b.2,
        a.0 * b.1 - a.1 * b.0,
    )
}

/// `BlockPattern.translateAndRotate(pos, forwards, up, x, y, z)`:
/// `pos + up * -y + (forwards × up) * x + forwards * z`.
#[must_use]
pub fn translate_and_rotate(
    pos: BlockPos,
    forwards: Dir,
    up: Dir,
    x: i32,
    y: i32,
    z: i32,
) -> BlockPos {
    let f = forwards.step();
    let u = up.step();
    let c = cross(f, u);
    (
        pos.0 + u.0 * -y + c.0 * x + f.0 * z,
        pos.1 + u.1 * -y + c.1 * x + f.1 * z,
        pos.2 + u.2 * -y + c.2 * x + f.2 * z,
    )
}

/// `BlockPattern.matches(pos, forwards, up, cache)`: every cell, `i` outer,
/// `j` inner, the first failure returning null.
fn matches(
    pattern: &Pattern,
    at: &dyn Fn(BlockPos) -> String,
    pos: BlockPos,
    forwards: Dir,
    up: Dir,
) -> Option<Match> {
    let mut cached = Vec::new();
    for i in 0..pattern.width() {
        for j in 0..pattern.height() {
            let p = translate_and_rotate(pos, forwards, up, i, j, 0);
            let state = at(p);
            if let Some(cell) = pattern.cell(i, j) {
                if !cell.test(&state) {
                    return None;
                }
            }
            cached.push(((i, j), state));
        }
    }
    Some(Match {
        front_top_left: pos,
        forwards,
        up,
        width: pattern.width(),
        height: pattern.height(),
        cached,
    })
}

/// `BlockPattern.find(level, pos)`: every corner in the cube
/// `pos .. pos + (max(w, h, d) - 1)`, walked x fastest then y then z
/// (`BlockPos.betweenClosed`), and at each every `(forwards, up)` pair that
/// is not collinear, in `Direction.values()` order. The first match wins.
#[must_use]
pub fn find(pattern: &Pattern, at: &dyn Fn(BlockPos) -> String, pos: BlockPos) -> Option<Match> {
    let n = pattern.width().max(pattern.height()).max(1);
    for dz in 0..n {
        for dy in 0..n {
            for dx in 0..n {
                let corner = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
                for forwards in Dir::VALUES {
                    for up in Dir::VALUES {
                        if up == forwards || up == forwards.opposite() {
                            continue;
                        }
                        if let Some(m) = matches(pattern, at, corner, forwards, up) {
                            return Some(m);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Which golem a pumpkin made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Golem {
    Snow,
    Iron,
    Copper,
}

impl Golem {
    #[must_use]
    pub const fn entity_type(self) -> &'static str {
        match self {
            Self::Snow => "minecraft:snow_golem",
            Self::Iron => "minecraft:iron_golem",
            Self::Copper => "minecraft:copper_golem",
        }
    }
}

/// `WeatheringCopper$WeatherState`, by ordinal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Weather {
    Unaffected = 0,
    Exposed = 1,
    Weathered = 2,
    Oxidized = 3,
}

impl Weather {
    /// `WeatherState.next()` — `values()[min(ordinal + 1, 3)]`.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Unaffected => Self::Exposed,
            Self::Exposed => Self::Weathered,
            Self::Weathered | Self::Oxidized => Self::Oxidized,
        }
    }
}

/// `getWeatherStateFromPattern`: the copper cell's block's age, reading a
/// waxed block through `HoneycombItem.WAX_OFF_BY_BLOCK` — so a waxed block
/// gives the golem its unwaxed twin's age — and `COPPER_BLOCK`'s unaffected
/// age when neither applies.
#[must_use]
pub fn weather_of(copper_block: &str) -> Weather {
    let unwaxed =
        copper_block
            .strip_prefix("minecraft:waxed_")
            .map_or(copper_block.to_string(), |rest| {
                if rest == "copper_block" {
                    "minecraft:copper_block".to_string()
                } else {
                    format!("minecraft:{rest}")
                }
            });
    match unwaxed.as_str() {
        "minecraft:exposed_copper" => Weather::Exposed,
        "minecraft:weathered_copper" => Weather::Weathered,
        "minecraft:oxidized_copper" => Weather::Oxidized,
        _ => Weather::Unaffected,
    }
}

/// `COPPER_TO_COPPER_CHEST_MAPPING` via `getOrDefault(block, COPPER_CHEST
/// unaffected)`: `WeatheringCopperCollection.zipApply` of the copper-block
/// and copper-chest collections, eight pairs.
#[must_use]
pub fn chest_for_copper(copper_block: &str) -> &'static str {
    match copper_block {
        "minecraft:exposed_copper" => "minecraft:exposed_copper_chest",
        "minecraft:weathered_copper" => "minecraft:weathered_copper_chest",
        "minecraft:oxidized_copper" => "minecraft:oxidized_copper_chest",
        "minecraft:waxed_copper_block" => "minecraft:waxed_copper_chest",
        "minecraft:waxed_exposed_copper" => "minecraft:waxed_exposed_copper_chest",
        "minecraft:waxed_weathered_copper" => "minecraft:waxed_weathered_copper_chest",
        "minecraft:waxed_oxidized_copper" => "minecraft:waxed_oxidized_copper_chest",
        _ => "minecraft:copper_chest",
    }
}

fn prop<'a>(state: &'a str, key: &str) -> Option<&'a str> {
    let inner = state.split_once('[')?.1.strip_suffix(']')?;
    inner.split(',').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then_some(v)
    })
}

/// The copper-chest weather ordinal of a chest block, `None` for anything
/// that is not a copper chest.
fn chest_weather(block: &str) -> Option<(Weather, bool)> {
    let rest = block.strip_prefix("minecraft:")?;
    let (waxed, rest) = rest
        .strip_prefix("waxed_")
        .map_or((false, rest), |r| (true, r));
    let w = match rest {
        "copper_chest" => Weather::Unaffected,
        "exposed_copper_chest" => Weather::Exposed,
        "weathered_copper_chest" => Weather::Weathered,
        "oxidized_copper_chest" => Weather::Oxidized,
        _ => return None,
    };
    Some((w, waxed))
}

/// `ChestBlock.candidatePartnerFacing` with `CopperChestBlock.chestCanConnectTo`
/// (`is(COPPER_CHESTS) && hasProperty(TYPE)`): the neighbour's facing when
/// it is a single copper chest.
fn candidate_partner_facing(
    at: &dyn Fn(BlockPos) -> String,
    pos: BlockPos,
    dir: Dir,
) -> Option<Dir> {
    let s = at(dir.relative(pos));
    chest_weather(block_of(&s))?;
    if prop(&s, "type")? != "single" {
        return None;
    }
    Dir::from_name(prop(&s, "facing")?)
}

/// `ChestBlock.getChestType(level, pos, facing)`.
fn chest_type(at: &dyn Fn(BlockPos) -> String, pos: BlockPos, facing: Dir) -> &'static str {
    if candidate_partner_facing(at, pos, facing.clockwise()) == Some(facing) {
        "left"
    } else if candidate_partner_facing(at, pos, facing.counter_clockwise()) == Some(facing) {
        "right"
    } else {
        "single"
    }
}

/// `CopperChestBlock.getFromCopperBlock(block, facing, level, pos)`: the
/// chest state, run through `getLeastOxidizedChestOfConnectedBlocks`.
#[must_use]
pub fn chest_from_copper(
    at: &dyn Fn(BlockPos) -> String,
    copper_block: &str,
    facing: Dir,
    pos: BlockPos,
) -> String {
    let chest = chest_for_copper(copper_block);
    let ty = chest_type(at, pos, facing);
    let state = format!(
        "{chest}[facing={},type={ty},waterlogged=false]",
        facing.name()
    );
    least_oxidized_of_connected(at, &state, pos)
}

/// `CopperChestBlock.getLeastOxidizedChestOfConnectedBlocks(state, level,
/// pos)`: a half of a double copper chest takes the less-oxidised block of
/// the pair, reading waxed halves through `unwaxBlock` when exactly one of
/// the two is waxed.
fn least_oxidized_of_connected(
    at: &dyn Fn(BlockPos) -> String,
    state: &str,
    pos: BlockPos,
) -> String {
    let (Some(ty), Some(facing)) = (
        prop(state, "type"),
        prop(state, "facing").and_then(Dir::from_name),
    ) else {
        return state.to_string();
    };
    if ty == "single" {
        return state.to_string();
    }
    // `ChestBlock.getConnectedDirection`: LEFT → clockwise, else counter.
    let connected = if ty == "left" {
        facing.clockwise()
    } else {
        facing.counter_clockwise()
    };
    let other = at(connected.relative(pos));
    let (Some((mine_w, mine_waxed)), Some((other_w, other_waxed))) = (
        chest_weather(block_of(state)),
        chest_weather(block_of(&other)),
    ) else {
        return state.to_string();
    };
    let unwax = |b: &str| -> String {
        b.strip_prefix("minecraft:waxed_")
            .map_or(b.to_string(), |r| format!("minecraft:{r}"))
    };
    let (mine_block, other_block) = if mine_waxed == other_waxed {
        (block_of(state).to_string(), block_of(&other).to_string())
    } else {
        (unwax(block_of(state)), unwax(block_of(&other)))
    };
    let chosen = if mine_w <= other_w {
        mine_block
    } else {
        other_block
    };
    // `withPropertiesOf(state)` — every chest in the family shares the
    // property set, so the property tail carries over whole.
    let tail = state.split_once('[').map_or("", |(_, t)| t);
    format!("{chosen}[{tail}")
}

/// A golem `trySpawnGolem` asked for, held until the handler drains it — the
/// level events it caused have to leave first, and they wait in
/// [`crate::shapes::Effects`] too.
#[derive(Debug, Clone, PartialEq)]
pub struct GolemSpawn {
    pub golem: Golem,
    /// `snapTo`'s position: the spawn cell's `x + 0.5, y + 0.05, z + 0.5`.
    pub at: (f64, f64, f64),
    /// `CopperGolem.spawn(WeatherState)`, run after the chest is written.
    pub weather: Option<Weather>,
}

/// `CarvedPumpkinBlock.onPlace`. Called from `shapes::set_block` inside the
/// write, with the state being written and the one it replaced.
pub fn on_place(
    blocks: &dyn Blocks,
    shapes: Option<&crate::shapes::ShapeTable>,
    pos: BlockPos,
    state: &str,
    old: &str,
    updates: &mut crate::shapes::Effects,
) {
    let block = block_of(state);
    if block != "minecraft:carved_pumpkin" && block != "minecraft:jack_o_lantern" {
        return;
    }
    // 0-12: `old.is(state.getBlock())` — re-placing the same block is not a
    // new pumpkin.
    if block_of(old) == block {
        return;
    }
    try_spawn_golem(blocks, shapes, pos, updates);
}

fn try_spawn_golem(
    blocks: &dyn Blocks,
    shapes: Option<&crate::shapes::ShapeTable>,
    pos: BlockPos,
    updates: &mut crate::shapes::Effects,
) {
    let at = |p: BlockPos| blocks.state_at(p);
    if let Some(m) = find(&SNOW_GOLEM_FULL, &at, pos) {
        let spawn = m.pos(0, 2);
        spawn_in_world(blocks, shapes, &m, Golem::Snow, spawn, None, updates);
        return;
    }
    if let Some(m) = find(&IRON_GOLEM_FULL, &at, pos) {
        let spawn = m.pos(1, 2);
        spawn_in_world(blocks, shapes, &m, Golem::Iron, spawn, None, updates);
        return;
    }
    if let Some(m) = find(&COPPER_GOLEM_FULL, &at, pos) {
        let spawn = m.pos(0, 0);
        // `getWeatherStateFromPattern` is evaluated at offset 181, after the
        // chest write, but it reads the *cached* state, so the order is not
        // observable; the request carries it.
        let weather = weather_of(block_of(m.state(0, 1)));
        spawn_in_world(
            blocks,
            shapes,
            &m,
            Golem::Copper,
            spawn,
            Some(weather),
            updates,
        );
        replace_copper_block_with_chest(blocks, shapes, &m, updates);
    }
}

/// `spawnGolemInWorld`, with `addFreshEntity` turned into a request.
fn spawn_in_world(
    blocks: &dyn Blocks,
    shapes: Option<&crate::shapes::ShapeTable>,
    m: &Match,
    golem: Golem,
    spawn: BlockPos,
    weather: Option<Weather>,
    updates: &mut crate::shapes::Effects,
) {
    // clearPatternBlocks.
    for i in 0..m.width {
        for j in 0..m.height {
            let p = m.pos(i, j);
            crate::shapes::set_block(
                blocks,
                shapes,
                p,
                "minecraft:air",
                crate::shapes::UPDATE_CLIENTS,
                crate::shapes::RECURSION_LIMIT,
                updates,
            );
            let data = blocks.state_id(m.state(i, j)).unwrap_or(0);
            updates.level_events.push((2001, p, data));
        }
    }
    updates.golems.push(GolemSpawn {
        golem,
        at: (
            f64::from(spawn.0) + 0.5,
            f64::from(spawn.1) + 0.05,
            f64::from(spawn.2) + 0.5,
        ),
        weather,
    });
    // updatePatternBlocks.
    for i in 0..m.width {
        for j in 0..m.height {
            crate::power::update_neighbors_at(
                blocks,
                crate::power::shared(),
                shapes,
                m.pos(i, j),
                "minecraft:air",
                updates,
            );
        }
    }
}

/// `replaceCopperBlockWithChest(level, match)`: the copper cell becomes the
/// matching copper chest, facing the way the pumpkin faced, written with
/// flag 2.
fn replace_copper_block_with_chest(
    blocks: &dyn Blocks,
    shapes: Option<&crate::shapes::ShapeTable>,
    m: &Match,
    updates: &mut crate::shapes::Effects,
) {
    let copper_pos = m.pos(0, 1);
    let copper = block_of(m.state(0, 1)).to_string();
    let facing = prop(m.state(0, 0), "facing")
        .and_then(Dir::from_name)
        .unwrap_or(Dir::North);
    let at = |p: BlockPos| blocks.state_at(p);
    let chest = chest_from_copper(&at, &copper, facing, copper_pos);
    crate::shapes::set_block(
        blocks,
        shapes,
        copper_pos,
        &chest,
        crate::shapes::UPDATE_CLIENTS,
        crate::shapes::RECURSION_LIMIT,
        updates,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn world(cells: &[(BlockPos, &str)]) -> impl Fn(BlockPos) -> String {
        let m: BTreeMap<BlockPos, String> =
            cells.iter().map(|(p, s)| (*p, (*s).to_string())).collect();
        move |p| {
            m.get(&p)
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
    }

    const PUMPKIN: &str = "minecraft:carved_pumpkin[facing=north]";

    /// `789`'s rig, relative to the anchor: iron at (2,0,-2), (2,1,-2),
    /// arms at (1,1,-2) and (3,1,-2), pumpkin at (2,2,-2). Vanilla's
    /// recorded level events came in the order x = 3, 2, 1 and, inside each
    /// column, y = 2, 1, 0 — which only one orientation produces.
    #[test]
    fn the_iron_pattern_matches_789s_rig_in_vanillas_recorded_order() {
        let at = world(&[
            ((2, 0, -2), "minecraft:iron_block"),
            ((2, 1, -2), "minecraft:iron_block"),
            ((1, 1, -2), "minecraft:iron_block"),
            ((3, 1, -2), "minecraft:iron_block"),
            ((2, 2, -2), PUMPKIN),
        ]);
        assert!(find(&SNOW_GOLEM_FULL, &at, (2, 2, -2)).is_none());
        let m = find(&IRON_GOLEM_FULL, &at, (2, 2, -2)).expect("iron golem");
        let order: Vec<BlockPos> = (0..3)
            .flat_map(|i| (0..3).map(move |j| (i, j)))
            .map(|(i, j)| m.pos(i, j))
            .collect();
        assert_eq!(
            order,
            vec![
                (3, 2, -2),
                (3, 1, -2),
                (3, 0, -2),
                (2, 2, -2),
                (2, 1, -2),
                (2, 0, -2),
                (1, 2, -2),
                (1, 1, -2),
                (1, 0, -2)
            ]
        );
        // The golem stands on the leg's cell, not the pumpkin's.
        assert_eq!(m.pos(1, 2), (2, 0, -2));
        assert_eq!(m.state(1, 0), PUMPKIN);
        assert_eq!(m.state(0, 0), "minecraft:air");
    }

    #[test]
    fn an_arm_stacked_on_the_ground_fails_the_air_cells() {
        let at = world(&[
            ((2, 0, -2), "minecraft:iron_block"),
            ((2, 1, -2), "minecraft:iron_block"),
            ((1, 1, -2), "minecraft:iron_block"),
            ((3, 1, -2), "minecraft:iron_block"),
            ((1, 0, -2), "minecraft:iron_block"),
            ((2, 2, -2), PUMPKIN),
        ]);
        assert!(find(&IRON_GOLEM_FULL, &at, (2, 2, -2)).is_none());
    }

    /// `788`'s rig: copper at (1,0,-1), pumpkin at (1,1,-1). Vanilla's
    /// events came pumpkin first, then copper, and the golem stood at the
    /// pumpkin's cell.
    #[test]
    fn the_copper_pattern_matches_788s_rig_top_first() {
        let at = world(&[
            ((1, 0, -1), "minecraft:copper_block"),
            ((1, 1, -1), PUMPKIN),
        ]);
        assert!(find(&SNOW_GOLEM_FULL, &at, (1, 1, -1)).is_none());
        assert!(find(&IRON_GOLEM_FULL, &at, (1, 1, -1)).is_none());
        let m = find(&COPPER_GOLEM_FULL, &at, (1, 1, -1)).expect("copper golem");
        assert_eq!(m.pos(0, 0), (1, 1, -1));
        assert_eq!(m.pos(0, 1), (1, 0, -1));
        let chest = chest_from_copper(&at, "minecraft:copper_block", Dir::North, (1, 0, -1));
        assert_eq!(
            chest,
            "minecraft:copper_chest[facing=north,type=single,waterlogged=false]"
        );
    }

    #[test]
    fn a_snow_golem_wins_over_nothing_else_and_stands_on_the_bottom() {
        let at = world(&[
            ((0, 0, 0), "minecraft:snow_block"),
            ((0, 1, 0), "minecraft:snow_block"),
            ((0, 2, 0), "minecraft:jack_o_lantern[facing=east]"),
        ]);
        let m = find(&SNOW_GOLEM_FULL, &at, (0, 2, 0)).expect("snow golem");
        assert_eq!(m.pos(0, 2), (0, 0, 0));
    }

    #[test]
    fn waxed_copper_gives_its_unwaxed_twins_age_and_its_own_chest() {
        assert_eq!(
            weather_of("minecraft:waxed_weathered_copper"),
            Weather::Weathered
        );
        assert_eq!(
            weather_of("minecraft:waxed_copper_block"),
            Weather::Unaffected
        );
        assert_eq!(weather_of("minecraft:oxidized_copper"), Weather::Oxidized);
        assert_eq!(
            chest_for_copper("minecraft:waxed_exposed_copper"),
            "minecraft:waxed_exposed_copper_chest"
        );
    }

    #[test]
    fn a_single_copper_chest_beside_joins_as_a_double_and_takes_the_younger_block() {
        // Facing north, clockwise is east: a single north-facing chest to the
        // east makes this one LEFT.
        let at = world(&[(
            (1, 0, 0),
            "minecraft:copper_chest[facing=north,type=single,waterlogged=false]",
        )]);
        let s = chest_from_copper(&at, "minecraft:oxidized_copper", Dir::North, (0, 0, 0));
        assert_eq!(
            s,
            "minecraft:copper_chest[facing=north,type=left,waterlogged=false]"
        );
    }
}
