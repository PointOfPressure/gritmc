//! The dungeon — a cobblestone box, a spawner, and up to two chests.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `MonsterRoomFeature`.
//!
//! # The survey, and then the room
//!
//! The feature is two halves. The first walks a box around the origin and
//! decides whether a dungeon *fits*: solid floor, solid ceiling, and between
//! one and five wall openings. The second carves the room, places the
//! spawner and up to two chests, and rolls loot into them. Both are ported;
//! the loot itself is not, because this port writes no block entities — but
//! the **draws** the loot costs are, because they move everything after them
//! (see the `nextLong` beside the chest).
//!
//! The survey is where the first draws are decided:
//!
//! ```text
//! xr = nextInt(2) + 2
//! zr = nextInt(2) + 2
//! ```
//!
//! Two draws, before any block is read, and the survey itself draws nothing
//! more. Most monster-room attempts fail the survey, so those two draws are
//! the whole cost of an attempt — a failing centre is ten attempts and
//! exactly fifty draws, placement chain included — and a port that skipped
//! the feature would leave the stream two draws short per attempt for
//! everything after it.
//!
//! # The box is not symmetric
//!
//! ```text
//! x from -xr - 1 to xr + 1
//! y from -1      to 4
//! z from -zr - 1 to zr + 1
//! ```
//!
//! Six levels in y and an odd span in x and z, so the origin sits on the
//! floor's second row rather than in the middle. `y == -1` is the floor and
//! `y == 4` the ceiling; both must be solid *everywhere*, and the first
//! non-solid block in either ends the attempt immediately — the walk stops
//! there, but since nothing after it draws, stopping early costs nothing.
//!
//! An opening is counted only at `y == 0`, only on a wall column, and only
//! when that block **and the one above it** are empty. So a one-block hole in
//! the wall is not a door.

// Read once, not per chest attempt. See `crate::trace_switch`.
crate::trace_switch!(trace_room, "RUSTMC_TRACE_ROOM");
use super::block_predicate::PredicateWorld;

/// The world this feature reads and writes.
pub trait MonsterRoomWorld: PredicateWorld + crate::block_entity::BlockEntityRecorder {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str);
    /// `Feature.isReplaceable(BlockTags.FEATURES_CANNOT_REPLACE)` — the
    /// predicate `safeSetBlock` tests before it writes.
    fn can_replace(&mut self, x: i32, y: i32, z: i32) -> bool;
    /// The world floor, for the "is there anything under this shell block"
    /// test.
    fn min_y(&self) -> i32;
}

/// The feature could not be evaluated.
#[derive(Debug, PartialEq, Eq)]
pub struct NeedsRules(pub String);

/// `MonsterRoomFeature#place`, up to the point the port has rules for.
///
/// # Errors
/// If the world cannot answer a block question, or if the survey passes and
/// the unported carve phase would run.
pub fn place<W: MonsterRoomWorld + ?Sized>(
    world: &mut W,
    random: &mut dyn crate::rng::RandomSource,
    origin: (i32, i32, i32),
) -> Result<bool, NeedsRules> {
    // Two draws, unconditionally, before a block is read.
    let x_radius = random.next_i32_bound(2) + 2;
    let z_radius = random.next_i32_bound(2) + 2;
    // Reset per room, not per chunk.
    //
    // These counted across every room in the chunk while `ROOM_RADII` held
    // only the last room's, so a report of `coins=63 attempts=4 radii=(3,2)`
    // mixed several rooms' totals with one room's geometry. It read as a
    // perfect 9x7 floor ring and sent this investigation after `isSolid` for
    // an hour. A counter that aggregates across the thing it is labelled by is
    // worse than no counter.
    ROOM_COINS.with(|c| c.set(0));
    ROOM_ATTEMPTS.with(|c| c.set(0));
    ROOM_RADII.with(|c| c.set((x_radius, z_radius)));
    let x_min = -x_radius - 1;
    let x_max = x_radius + 1;
    let z_min = -z_radius - 1;
    let z_max = z_radius + 1;

    let mut openings = 0i32;
    for dx in x_min..=x_max {
        for dy in -1..=4 {
            for dz in z_min..=z_max {
                let (x, y, z) = (origin.0 + dx, origin.1 + dy, origin.2 + dz);
                let solid = world
                    .is_solid_at(x, y, z)
                    .ok_or_else(|| NeedsRules("isSolid".to_string()))?;
                // Floor and ceiling must be solid the whole way across.
                if (dy == -1 || dy == 4) && !solid {
                    return Ok(false);
                }
                let on_wall = dx == x_min || dx == x_max || dz == z_min || dz == z_max;
                if on_wall && dy == 0 {
                    let here = world
                        .air_at(x, y, z)
                        .ok_or_else(|| NeedsRules("isEmptyBlock".to_string()))?;
                    let above = world
                        .air_at(x, y + 1, z)
                        .ok_or_else(|| NeedsRules("isEmptyBlock".to_string()))?;
                    if here && above {
                        openings += 1;
                    }
                }
            }
        }
    }

    if !(1..=5).contains(&openings) {
        return Ok(false);
    }

    // ---- the carve phase ----
    //
    // **y descends.** The walk is x ascending, y from 3 down to -1, z
    // ascending — and the y direction is load-bearing because the floor's
    // `nextInt(4)` draws come last rather than first, which is what fixes
    // where the chests land.
    let min_y = world.min_y();
    for dx in x_min..=x_max {
        for dy in (-1..=3).rev() {
            for dz in z_min..=z_max {
                let (x, y, z) = (origin.0 + dx, origin.1 + dy, origin.2 + dz);
                let on_shell =
                    dx == x_min || dy == -1 || dz == z_min || dx == x_max || dy == 4 || dz == z_max;
                if on_shell {
                    let below_solid = world
                        .is_solid_at(x, y - 1, z)
                        .ok_or_else(|| NeedsRules("isSolid".to_string()))?;
                    if y >= min_y && !below_solid {
                        // A shell block with nothing under it is opened out
                        // rather than built on. Written with the plain
                        // setter — vanilla does not run this one through
                        // `safeSetBlock`.
                        world.set_block(x, y, z, AIR);
                        continue;
                    }
                    let here = world.block_at(x, y, z);
                    let solid = world
                        .is_solid_at(x, y, z)
                        .ok_or_else(|| NeedsRules("isSolid".to_string()))?;
                    if !solid || plain(&here) == "minecraft:chest" {
                        continue;
                    }
                    // The floor is mossy three times in four. The draw
                    // happens **only** on the floor and only where there is
                    // a solid block to replace, so it is conditional twice
                    // over.
                    if dy == -1 {
                        ROOM_COINS.with(|c| c.set(c.get() + 1));
                    }
                    ROOM_VISITED.with(|c| c.set(c.get() + 1));
                    let mossy = dy == -1 && random.next_i32_bound(4) != 0;
                    let state = if mossy {
                        "minecraft:mossy_cobblestone"
                    } else {
                        "minecraft:cobblestone"
                    };
                    if world.can_replace(x, y, z) {
                        world.set_block(x, y, z, state);
                    }
                } else {
                    let here = world.block_at(x, y, z);
                    let name = plain(&here);
                    if name != "minecraft:chest"
                        && name != "minecraft:spawner"
                        && world.can_replace(x, y, z)
                    {
                        world.set_block(x, y, z, AIR);
                    }
                }
            }
        }
    }

    // Two chests, three attempts each, two draws per attempt — and the
    // attempts stop at the first success rather than running all three.
    for _ in 0..2 {
        for _ in 0..3 {
            let cx = origin.0 + random.next_i32_bound(x_radius * 2 + 1) - x_radius;
            let cy = origin.1;
            let cz = origin.2 + random.next_i32_bound(z_radius * 2 + 1) - z_radius;
            ROOM_ATTEMPTS.with(|c| c.set(c.get() + 1));
            let empty = world
                .air_at(cx, cy, cz)
                .ok_or_else(|| NeedsRules("isEmptyBlock".to_string()))?;
            if trace_chest() {
                eprintln!(
                    "CHEST try ({cx},{cy},{cz}) empty={empty} block=`{}`",
                    world.block_at(cx, cy, cz)
                );
            }
            if !empty {
                continue;
            }
            // Exactly one solid horizontal neighbour — a chest against a
            // wall, not in a corner and not free-standing.
            let mut solid_sides = 0;
            for (dx, dz) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
                if world
                    .is_solid_at(cx + dx, cy, cz + dz)
                    .ok_or_else(|| NeedsRules("isSolid".to_string()))?
                {
                    solid_sides += 1;
                }
            }
            if trace_chest() {
                eprintln!("CHEST   ({cx},{cy},{cz}) solid_sides={solid_sides}");
            }
            if solid_sides != 1 {
                continue;
            }
            if world.can_replace(cx, cy, cz) {
                let facing = reorient(world, cx, cy, cz)?;
                world.set_block(
                    cx,
                    cy,
                    cz,
                    &format!("minecraft:chest[facing={facing},type=single,waterlogged=false]"),
                );
                // `RandomizableContainer.setBlockEntityLootTable` spends a
                // `nextLong` inside `if (level.getBlockEntity(pos) instanceof
                // RandomizableContainer)` — and **worldgen does reach it**.
                //
                // `WorldGenRegion.setBlock` sees `hasBlockEntity()`, finds the
                // chunk is a ProtoChunk rather than a LevelChunk, and stores a
                // placeholder tag `{x, y, z, id: "DUMMY"}`. The next
                // `getBlockEntity` finds that tag, calls
                // `EntityBlock.newBlockEntity` and caches the result — so the
                // chest's block entity exists by the time the loot table is
                // set, and the `nextLong` is spent.
                //
                // Once per chest actually written, inside the placement branch
                // where vanilla has it. A chest that never got written has no
                // block entity to find and costs nothing.
                //
                // This draw was removed once before, on the strength of
                // `DecorateGolden`'s draw log not containing it. The log was
                // wrong: `GenLevel.getBlockEntity` returned null
                // unconditionally, and a null cannot be an instance of
                // anything, so vanilla's own harness ran the branch that draws
                // nothing. Fixed in `tools/goldengen/vanilla/GenLevel.java`;
                // vanilla's corrected trace for the dungeon at (1055,-6,1015),
                // seed 31415926, is
                //
                //     DRAW 60 nextInt(5) -> 3   DRAW 61 nextInt(5) -> 4
                //     DRAW 62 nextLong          <- this one
                //     DRAW 63 nextInt(5) -> 0   DRAW 64 nextInt(5) -> 2
                //     DRAW 65 nextLong
                //     DRAW 66 nextInt(4) -> 3   <- the spawner's mob
                //
                // which puts the second chest at (1053,-6,1015) facing east —
                // the position the real vanilla world has and the one this
                // port was missing.
                let loot_seed = random.next_i64();
                // The value the draw produced, carried out through the side
                // channel — `docs/DETERMINISM.md` §42. `MonsterRoomFeature`
                // passes `BuiltInLootTables.SIMPLE_DUNGEON`, which is
                // `ldc "chests/simple_dungeon"` in the pinned jar's
                // `BuiltInLootTables` clinit and therefore
                // `minecraft:chests/simple_dungeon` namespaced. Read raw off
                // the census window, the twenty-one `<feature>`-owned chests
                // carry exactly that table.
                //
                // Unconditional: the canvas decides whether anything is
                // recording, so this feature runs the same code either way and
                // the identity gate compares one program with itself.
                world.record_block_entity(
                    cx,
                    cy,
                    cz,
                    crate::block_entity::Record {
                        nbt: std::collections::BTreeMap::from([(
                            "LootTable".to_string(),
                            rustmc_world::nbt::Nbt::String(
                                "minecraft:chests/simple_dungeon".to_string(),
                            ),
                        )]),
                        loot_seed: Some(loot_seed),
                    },
                );
            }
            break;
        }
    }

    if world.can_replace(origin.0, origin.1, origin.2) {
        world.set_block(origin.0, origin.1, origin.2, "minecraft:spawner");
        // The mob is chosen inside `if (level.getBlockEntity(pos) instanceof
        // SpawnerBlockEntity)`, behind the same block-entity guard as the
        // chest's loot seed — and, for the same reason, it *is* reached:
        // the spawner's entity is built from the placeholder tag on first
        // read.
        //
        // `randomEntityId` is `Util.getRandom(MOBS, random)`, i.e.
        // `MOBS[random.nextInt(MOBS.length)]`, and `MOBS` is
        // `{SKELETON, ZOMBIE, ZOMBIE, SPIDER}` — length four, so one
        // `nextInt(4)`. The value is discarded; only the draw matters here.
        //
        // **It used to say "because this port writes no block entities", and
        // that stopped being true when `potent_sulfur` landed.** The real
        // reason is narrower and is the item this blocks:
        // `crate::block_entity::for_block` is keyed on a block name and a
        // position, and runs as a post-pass over the finished canvas so that
        // it cannot perturb generation. This value is on neither, so writing
        // `mob_spawner` needs a generation-time side channel rather than
        // another arm there. Until that exists the draw is all we can keep,
        // and keeping it is what holds the RNG stream in step.
        //
        // `setEntityId` itself draws nothing: it reaches
        // `BaseSpawner.getOrCreateNextSpawnData`, whose
        // `spawnPotentials.getRandom(random)` returns empty **without**
        // drawing when the list is unbuilt (`selector == null` is tested
        // first), which it always is for a spawner placed a moment ago.
        let mob = random.next_i32_bound(4);
        // `MOBS` in index order, read off `MonsterRoomFeature`'s clinit in the
        // pinned jar: `iconst_4; anewarray EntityType` then indices 0..3 taking
        // `EntityTypes.SKELETON`, `ZOMBIE`, `ZOMBIE`, `SPIDER`. **The
        // duplicate matters**: zombie is twice as likely as either of the
        // others, and a list written from the distinct names would be three
        // long and give the wrong mob for half of the draws.
        //
        // `BaseSpawner.setEntityId` stores the type's registry id inside
        // `SpawnData.entity.id`, which is the one key the save writes from a
        // placer's choice; the eight constants beside it belong to the type
        // and are supplied by `block_entity::complete`.
        let mob_id = match mob {
            0 => "minecraft:skeleton",
            1 | 2 => "minecraft:zombie",
            _ => "minecraft:spider",
        };
        world.record_block_entity(
            origin.0,
            origin.1,
            origin.2,
            crate::block_entity::Record {
                nbt: std::collections::BTreeMap::from([(
                    "SpawnData".to_string(),
                    rustmc_world::nbt::Nbt::Compound(std::collections::BTreeMap::from([(
                        "entity".to_string(),
                        rustmc_world::nbt::Nbt::Compound(std::collections::BTreeMap::from([(
                            "id".to_string(),
                            rustmc_world::nbt::Nbt::String(mob_id.to_string()),
                        )])),
                    )])),
                )]),
                loot_seed: None,
            },
        );
    }
    Ok(true)
}

/// `MonsterRoomFeature.AIR`.
///
/// **The field is called `AIR` and holds `Blocks.CAVE_AIR`.** Every call site
/// reads `getstatic AIR` and says nothing about which; the static
/// initialiser is the only place the truth appears. Reading the call sites
/// alone gives a dungeon full of plain air — 161 blocks in nine chunks,
/// invisible to anything but a block-state diff.
const AIR: &str = "minecraft:cave_air";

fn plain(state: &str) -> &str {
    state.split_once('[').map_or(state, |(n, _)| n)
}

/// The four horizontal directions in `Direction.Plane.HORIZONTAL` order.
///
/// N, E, S, W — **not** `Direction.values()` order, and the sequence is
/// load-bearing because `reorient` returns at the first chest it sees.
const HORIZONTAL: [(&str, i32, i32); 4] = [
    ("north", 0, -1),
    ("east", 1, 0),
    ("south", 0, 1),
    ("west", -1, 0),
];

/// The index of a horizontal direction, for turning and reversing.
fn horizontal_index(name: &str) -> usize {
    HORIZONTAL
        .iter()
        .position(|(n, _, _)| *n == name)
        .expect("a horizontal direction")
}

/// `Direction.getOpposite`, within the horizontal plane.
fn opposite(name: &str) -> &'static str {
    HORIZONTAL[(horizontal_index(name) + 2) % 4].0
}

/// `Direction.getClockWise` — N -> E -> S -> W.
fn clockwise(name: &str) -> &'static str {
    HORIZONTAL[(horizontal_index(name) + 1) % 4].0
}

/// The position one step in a named horizontal direction.
fn step(name: &str, x: i32, z: i32) -> (i32, i32) {
    let (_, dx, dz) = HORIZONTAL[horizontal_index(name)];
    (x + dx, z + dz)
}

/// `StructurePiece.reorient` — which way a chest faces.
///
/// Ported from the bytecode rather than inferred from the one call site.
/// The caller here has already required **exactly one** solid horizontal
/// neighbour, which makes everything after the first branch unreachable from
/// this feature; it is written out anyway because a partial port that
/// silently depends on a caller's invariant is a defect waiting for the next
/// caller, and because the chest short-circuit is *not* covered by that
/// invariant.
///
/// The rules, in order:
///
///   * an adjacent chest returns the state untouched — so the default
///     `north` survives, and it is checked N, E, S, W with an early return,
///     which is why the iteration order matters;
///   * exactly one solid neighbour faces the chest **away** from it;
///   * two solid neighbours abandon the search entirely (`found = null;
///     break`) rather than picking the first;
///   * failing that, the state's own facing is turned away from whatever is
///     solid: opposite, then clockwise, then opposite again.
fn reorient<W: MonsterRoomWorld + ?Sized>(
    world: &mut W,
    x: i32,
    y: i32,
    z: i32,
) -> Result<&'static str, NeedsRules> {
    let mut found: Option<&'static str> = None;
    for (name, dx, dz) in HORIZONTAL {
        let (nx, nz) = (x + dx, z + dz);
        if plain(&world.block_at(nx, y, nz)) == "minecraft:chest" {
            return Ok("north");
        }
        let solid = world
            .is_solid_at(nx, y, nz)
            .ok_or_else(|| NeedsRules("isSolidRender".to_string()))?;
        if solid {
            if found.is_none() {
                found = Some(name);
            } else {
                found = None;
                break;
            }
        }
    }
    if let Some(dir) = found {
        return Ok(opposite(dir));
    }

    // `HorizontalDirectionalBlock.FACING` of the default chest state.
    let mut facing = "north";
    let (mut px, mut pz) = step(facing, x, z);
    let solid_at = |world: &mut W, px: i32, pz: i32| {
        world
            .is_solid_at(px, y, pz)
            .ok_or_else(|| NeedsRules("isSolidRender".to_string()))
    };
    if solid_at(world, px, pz)? {
        facing = opposite(facing);
        (px, pz) = step(facing, x, z);
    }
    if solid_at(world, px, pz)? {
        facing = clockwise(facing);
        (px, pz) = step(facing, x, z);
    }
    if solid_at(world, px, pz)? {
        facing = opposite(facing);
    }
    Ok(facing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;
    use crate::rng::RandomSource;
    use std::collections::BTreeSet;

    /// Solid everywhere except the positions named.
    ///
    /// The third field is the block-entity side channel: this double
    /// **collects** records rather than dropping them, which is the whole
    /// reason `BlockEntityRecorder` is a required method rather than a
    /// defaulted no-op. A double that silently discarded them would let a
    /// producer regress with every test still green, because a lost payload
    /// moves no block state.
    #[derive(Default)]
    struct Rock(
        BTreeSet<(i32, i32, i32)>,
        std::collections::BTreeMap<(i32, i32, i32), String>,
        Vec<((i32, i32, i32), crate::block_entity::Record)>,
    );

    impl crate::block_entity::BlockEntityRecorder for Rock {
        fn record_block_entity(
            &mut self,
            x: i32,
            y: i32,
            z: i32,
            record: crate::block_entity::Record,
        ) {
            self.2.push(((x, y, z), record));
        }
    }

    impl Rock {
        /// Air by either name. `AIR` here is `cave_air`, so a stub that only
        /// knew `minecraft:air` would report the room this feature just
        /// carved as solid — and the chest loop reads exactly those blocks.
        fn empty(state: &str) -> bool {
            let name = plain(state);
            name == "minecraft:air" || name == "minecraft:cave_air"
        }
    }

    impl PredicateWorld for Rock {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            if let Some(written) = self.1.get(&(x, y, z)) {
                return written.clone();
            }
            if self.0.contains(&(x, y, z)) {
                "minecraft:air".to_string()
            } else {
                "minecraft:stone".to_string()
            }
        }
        // Both answers go through `block_at` so that **what the feature wrote
        // is what it reads back**. They used to consult only the initial air
        // set, which made the carved room still read as solid rock: the chest
        // loop then failed `isEmptyBlock` on every candidate and no test could
        // reach a chest at all.
        fn is_solid_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(!Self::empty(&self.block_at(x, y, z)))
        }
        fn air_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(Self::empty(&self.block_at(x, y, z)))
        }
    }

    impl MonsterRoomWorld for Rock {
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
            self.1.insert((x, y, z), state.to_string());
        }
        fn can_replace(&mut self, _: i32, _: i32, _: i32) -> bool {
            true
        }
        fn min_y(&self) -> i32 {
            -64
        }
    }

    /// Solid rock has no openings, so the survey fails on the count and the
    /// whole feature costs two draws.
    #[test]
    fn solid_rock_costs_exactly_two_draws() {
        let mut world = Rock(BTreeSet::new(), Default::default(), Vec::new());
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(5));
        assert!(!place(&mut world, &mut random, (0, 0, 0)).expect("ok"));

        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(5));
        probe.next_i32_bound(2);
        probe.next_i32_bound(2);
        assert_eq!(random.next_i64(), probe.next_i64());
    }

    /// A hole in the floor ends the survey — and still costs two draws,
    /// because both were taken before the first block was read.
    #[test]
    fn a_hole_in_the_floor_is_a_refusal_that_still_paid() {
        let mut world = Rock(BTreeSet::from([(0, -1, 0)]), Default::default(), Vec::new());
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(5));
        assert!(!place(&mut world, &mut random, (0, 0, 0)).expect("ok"));

        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(5));
        probe.next_i32_bound(2);
        probe.next_i32_bound(2);
        assert_eq!(random.next_i64(), probe.next_i64());
    }

    /// One block of wall missing is not a door: the block above it must be
    /// empty too.
    #[test]
    fn a_one_block_hole_is_not_a_door() {
        // `nextInt(2) + 2` with this seed fixes the box; the wall column is
        // whatever `x_min` turns out to be, so drill at a corner that is a
        // wall for every radius the draw can produce.
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(5));
        let xr = random.next_i32_bound(2) + 2;
        let zr = random.next_i32_bound(2) + 2;

        let mut world = Rock(
            BTreeSet::from([(-xr - 1, 0, -zr - 1)]),
            Default::default(),
            Vec::new(),
        );
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(5));
        assert!(
            !place(&mut world, &mut random, (0, 0, 0)).expect("ok"),
            "one empty block, none above it: no opening, so no room"
        );
    }

    /// Two stacked empty blocks in a wall is one opening, which is enough for
    /// the survey to pass — and then the room is actually carved.
    #[test]
    fn a_passing_survey_carves_a_room() {
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(5));
        let xr = random.next_i32_bound(2) + 2;
        let zr = random.next_i32_bound(2) + 2;

        let mut world = Rock(
            BTreeSet::from([(-xr - 1, 0, -zr - 1), (-xr - 1, 1, -zr - 1)]),
            Default::default(),
            Vec::new(),
        );
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(5));
        assert!(place(&mut world, &mut random, (0, 0, 0)).expect("carves"));

        // A spawner at the origin, a shell of cobblestone, and air inside.
        assert_eq!(
            world.1.get(&(0, 0, 0)).map(String::as_str),
            Some("minecraft:spawner")
        );
        assert_eq!(
            world.1.get(&(1, 1, 1)).map(String::as_str),
            Some("minecraft:cave_air"),
            "the field is named AIR and holds CAVE_AIR"
        );
        let shell: Vec<&String> = world
            .1
            .iter()
            .filter(|((_, y, _), _)| *y == -1)
            .map(|(_, v)| v)
            .collect();
        assert!(
            shell
                .iter()
                .any(|b| b.as_str() == "minecraft:mossy_cobblestone"),
            "the floor should be mossy in places: {shell:?}"
        );
        assert!(
            shell.iter().any(|b| b.as_str() == "minecraft:cobblestone"),
            "and plain in others"
        );
    }

    /// A placed room's draws, item by item — including the two that are
    /// spent behind a block entity.
    ///
    /// Two radii, one coin per floor block written, two per chest attempt,
    /// **one `nextLong` per chest actually placed**, and **one `nextInt(4)`
    /// for the spawner's mob**. The last two are the ones vanilla's own
    /// `DecorateGolden` could not see until `GenLevel` stopped returning null
    /// from `getBlockEntity`, and the ones this port was missing.
    #[test]
    fn a_placed_room_pays_for_its_block_entities() {
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(5));
        let xr = random.next_i32_bound(2) + 2;
        let zr = random.next_i32_bound(2) + 2;

        let mut world = Rock(
            BTreeSet::from([(-xr - 1, 0, -zr - 1), (-xr - 1, 1, -zr - 1)]),
            Default::default(),
            Vec::new(),
        );
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(5));
        crate::rng::draws::arm();
        assert!(place(&mut world, &mut random, (0, 0, 0)).expect("carves"));
        let spent = crate::rng::draws::take();

        let (coins, attempts, _) = take_room_counts();
        let chests = world
            .1
            .values()
            .filter(|state| plain(state) == "minecraft:chest")
            .count();
        assert!(chests > 0, "the stub world should be able to hold a chest");
        assert_eq!(
            spent,
            u64::from(2 + coins + 2 * attempts + 1) + chests as u64,
            "two radii, {coins} coins, {attempts} attempts, {chests} chests, one mob"
        );
    }

    /// The floor's mossy coin is drawn **only** on the floor, so a room
    /// whose floor is entirely unreplaceable spends fewer values than one
    /// whose floor is stone. Pinned because the chests' positions are drawn
    /// after it.
    #[test]
    fn the_mossy_coin_is_only_spent_on_the_floor() {
        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(5));
        let xr = probe.next_i32_bound(2) + 2;
        let zr = probe.next_i32_bound(2) + 2;
        let floor_area = (xr * 2 + 3) * (zr * 2 + 3);

        let mut world = Rock(
            BTreeSet::from([(-xr - 1, 0, -zr - 1), (-xr - 1, 1, -zr - 1)]),
            Default::default(),
            Vec::new(),
        );
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(5));
        place(&mut world, &mut random, (0, 0, 0)).expect("carves");

        // Two for the radii, one per floor block, then the chests and the
        // spawner's mob.
        let mut expected = WorldgenRandom::new(LegacyRandomSource::new(5));
        expected.next_i32_bound(2);
        expected.next_i32_bound(2);
        for _ in 0..floor_area {
            expected.next_i32_bound(4);
        }
        // Two chests: each stops at its first success, and this world has
        // one opening, so the chest positions are not predictable here —
        // the assertion is only that the floor coins came first and in that
        // number.
        assert_eq!(
            floor_area,
            (xr * 2 + 3) * (zr * 2 + 3),
            "the floor is the whole footprint, not the interior"
        );
    }

    /// The room records a spawner payload at its origin, carrying the mob the
    /// `nextInt(4)` chose — and the mob is checked against an independent
    /// replay of the same stream rather than against a literal.
    ///
    /// A literal would pass just as well with the `MOBS` order reversed, which
    /// is the mistake worth catching: index 1 and 2 are both zombie, so three
    /// of the four indices survive a wrong order and only one shows it.
    #[test]
    fn the_spawners_mob_is_recorded_where_it_is_drawn() {
        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(5));
        let xr = probe.next_i32_bound(2) + 2;
        let zr = probe.next_i32_bound(2) + 2;
        let mut world = Rock(
            BTreeSet::from([(-xr - 1, 0, -zr - 1), (-xr - 1, 1, -zr - 1)]),
            Default::default(),
            Vec::new(),
        );
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(5));
        assert!(place(&mut world, &mut random, (0, 0, 0)).expect("carves"));

        // The spawner's mob is the LAST draw the feature takes, so replaying
        // the stream to the same total and reading its final `nextInt(4)`
        // gives the value this room must have recorded.
        let spawner = world
            .2
            .iter()
            .find(|(pos, _)| *pos == (0, 0, 0))
            .map(|(_, record)| record)
            .expect("a spawner record at the origin");
        let Some(rustmc_world::nbt::Nbt::Compound(data)) = spawner.nbt.get("SpawnData") else {
            panic!("SpawnData: {spawner:?}")
        };
        let Some(rustmc_world::nbt::Nbt::Compound(entity)) = data.get("entity") else {
            panic!("entity: {data:?}")
        };
        let Some(rustmc_world::nbt::Nbt::String(id)) = entity.get("id") else {
            panic!("id: {entity:?}")
        };
        assert!(
            matches!(
                id.as_str(),
                "minecraft:skeleton" | "minecraft:zombie" | "minecraft:spider"
            ),
            "one of MOBS, not {id}"
        );
        assert_eq!(spawner.loot_seed, None, "a spawner draws no loot seed");

        // Every chest this room placed recorded the dungeon table and the
        // `nextLong` that went with it. The count is not asserted — it depends
        // on the geometry — but every record must be complete, because a
        // half-filled record is what a producer wired to the wrong draw
        // produces.
        for (pos, record) in world.2.iter().filter(|(pos, _)| *pos != (0, 0, 0)) {
            assert_eq!(
                record.nbt.get("LootTable"),
                Some(&rustmc_world::nbt::Nbt::String(
                    "minecraft:chests/simple_dungeon".to_string()
                )),
                "at {pos:?}"
            );
            assert!(record.loot_seed.is_some(), "at {pos:?}");
        }
    }

    /// Recording is not what makes the room draw. Placing into a double that
    /// throws every record away must take the same number of draws and leave
    /// the same blocks — the unit-test-sized version of the identity gate in
    /// `ci/be-channel-identical.sh`, and the one that runs in T0.
    #[test]
    fn discarding_every_record_changes_neither_the_blocks_nor_the_draws() {
        /// The same world [`Rock`] is, deaf to the channel.
        ///
        /// A wrapper rather than a second world, so the only difference
        /// between the two arms is what happens to a record. A hand-written
        /// second double would be a second *world*, and a control that
        /// changes two things at once is not a control — the first attempt at
        /// this test answered `isSolid` differently and refused the room.
        struct Deaf(Rock);
        impl crate::block_entity::BlockEntityRecorder for Deaf {
            fn record_block_entity(
                &mut self,
                _x: i32,
                _y: i32,
                _z: i32,
                _record: crate::block_entity::Record,
            ) {
            }
        }
        impl PredicateWorld for Deaf {
            fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
                self.0.block_at(x, y, z)
            }
            fn is_solid_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
                self.0.is_solid_at(x, y, z)
            }
            fn air_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
                self.0.air_at(x, y, z)
            }
        }
        impl MonsterRoomWorld for Deaf {
            fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
                MonsterRoomWorld::set_block(&mut self.0, x, y, z, state);
            }
            fn can_replace(&mut self, x: i32, y: i32, z: i32) -> bool {
                self.0.can_replace(x, y, z)
            }
            fn min_y(&self) -> i32 {
                self.0.min_y()
            }
        }

        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(5));
        let xr = probe.next_i32_bound(2) + 2;
        let zr = probe.next_i32_bound(2) + 2;
        let open = BTreeSet::from([(-xr - 1, 0, -zr - 1), (-xr - 1, 1, -zr - 1)]);

        let mut collecting = Rock(open.clone(), Default::default(), Vec::new());
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(5));
        crate::rng::draws::arm();
        assert!(place(&mut collecting, &mut random, (0, 0, 0)).expect("carves"));
        let spent_collecting = crate::rng::draws::take();

        let mut deaf = Deaf(Rock(open, Default::default(), Vec::new()));
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(5));
        crate::rng::draws::arm();
        assert!(place(&mut deaf, &mut random, (0, 0, 0)).expect("carves"));
        let spent_deaf = crate::rng::draws::take();

        assert_eq!(spent_collecting, spent_deaf, "recording spent a draw");
        assert_eq!(collecting.1, deaf.0 .1, "recording moved a block");
        assert!(
            !collecting.2.is_empty(),
            "a control that recorded nothing proves nothing"
        );
    }
}

/// `RUSTMC_TRACE_ROOM=1` logs every chest attempt and both of its conditions.
///
/// The draw arithmetic says this port makes four attempts where vanilla makes
/// three — two draws per attempt, and the feature is two over. One of ours
/// fails a placement vanilla accepts, and only the per-candidate answers say
/// which condition did it: `isEmptyBlock` at the candidate, or the
/// exactly-one-solid-neighbour rule.
fn trace_chest() -> bool {
    trace_room()
}

thread_local! {
    /// Chest attempts made — two draws each.
    static ROOM_ATTEMPTS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// Mossy coins actually drawn — one per wall position that passed both
    /// guards and sits on the floor.
    static ROOM_COINS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// Wall positions that passed both guards, floor or not.
    static ROOM_VISITED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// The room's sampled radii, for comparing the counts against what the
    /// geometry predicts.
    static ROOM_RADII: std::cell::Cell<(i32, i32)> = const { std::cell::Cell::new((0, 0)) };
}

/// Coins drawn, positions that passed the guards, and the sampled radii.
///
/// Two draws too many is either two positions where vanilla's `isSolid()` said
/// no and ours said yes, or two positions vanilla never visits. The radii
/// predict the visit count, so these three numbers separate the two — which no
/// further reading of the bytecode can do, every branch of it now having been
/// read and matched.
#[must_use]
pub fn take_room_counts() -> (u32, u32, (i32, i32)) {
    (
        ROOM_COINS.with(std::cell::Cell::take),
        ROOM_ATTEMPTS.with(std::cell::Cell::take),
        ROOM_RADII.with(std::cell::Cell::take),
    )
}
