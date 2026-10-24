//! The End's features.
//!
//! References: `net.minecraft.world.level.levelgen.feature.EndSpikeFeature`
//! (and its `EndSpike` / `SpikeCacheLoader` inner classes) and
//! `EndPlatformFeature`.
//!
//! # Why the spikes are the order-sensitive one
//!
//! Everything else in this dimension is a pure function of position. The ten
//! obsidian spikes are not: their radii, heights and which two are caged come
//! out of a **shuffled** list of `0..10`, drawn from a random source seeded
//! from the world seed by way of a 16-bit key. Get the shuffle's draw order
//! wrong and every spike is a plausible spike in the wrong place — the shape
//! of bug that survives inspection and only a diff catches.

use crate::feature::block_predicate::PredicateWorld;
use crate::rng::{LegacyRandomSource, RandomSource};

/// What an End feature needs from the world.
///
/// Deliberately not folded into `NetherWorld` even though the first three
/// members would be the same signatures: the two dimensions' features share
/// no code, and a trait named for one dimension carrying another's contract
/// is how the wrong `min_y` gets read.
pub trait EndWorld: PredicateWorld {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str);

    /// `LevelHeightAccessor#getMinY`. The bottom of `placeSpike`'s box.
    fn min_y(&self) -> i32;

    /// `WorldGenLevel#getSeed`. `getSpikesForLevel` reads it directly — the
    /// spike list is a property of the *world*, not of the feature's random
    /// stream.
    fn world_seed(&mut self) -> i64;

    /// `WorldGenLevel.addFreshEntity` — the spike's end crystal. A world
    /// that does not record entities drops it, which is what every world did
    /// before 2026-10-04.
    fn add_fresh_entity(&mut self, _entity: rustmc_world::nbt::Nbt) {}
}

/// One of the ten spikes. `EndSpikeFeature$EndSpike`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EndSpike {
    pub center_x: i32,
    pub center_z: i32,
    pub radius: i32,
    pub height: i32,
    pub guarded: bool,
}

impl EndSpike {
    /// `isCenterWithinChunk(pos)` — both coordinates' **section** coords
    /// match. Note it compares against the position's chunk, not against a
    /// distance, so exactly one chunk in the world places each spike.
    #[must_use]
    pub const fn center_within_chunk(&self, x: i32, z: i32) -> bool {
        (x >> 4) == (self.center_x >> 4) && (z >> 4) == (self.center_z >> 4)
    }
}

/// `Util.toShuffledList(IntStream.range(0, 10), random)`.
///
/// A Fisher-Yates from the top down. The bytecode is
/// `list.set(i - 1, list.set(j, list.getInt(i - 1)))`, and the argument
/// evaluation order — `getInt(i - 1)` is read before either `set` runs — makes
/// it a plain swap of `j` and `i - 1`:
///
/// ```text
///   for (i = size; i > 1; i--) { j = random.nextInt(i); swap(list[j], list[i - 1]); }
/// ```
///
/// **`nextInt(i)`, not `nextInt(i - 1)`**, and the loop runs `size - 1` times.
/// Both are the classic places this shuffle is transcribed wrong, and either
/// gives a differently-permuted list from the same seed.
fn to_shuffled_list<R: RandomSource>(size: usize, random: &mut R) -> Vec<i32> {
    let mut list: Vec<i32> = (0..size)
        .map(|v| i32::try_from(v).expect("a shuffled index fits in i32"))
        .collect();
    let mut i = size;
    while i > 1 {
        let j = random.next_i32_bound(i32::try_from(i).expect("the list length fits in i32"));
        let j = usize::try_from(j).expect("nextInt(bound) is non-negative");
        list.swap(j, i - 1);
        i -= 1;
    }
    list
}

/// `EndSpikeFeature.getSpikesForLevel(level)` and `SpikeCacheLoader.load`,
/// together.
///
/// Two random sources, and neither is the feature's:
///
/// ```text
///   random = RandomSource.createThreadLocalInstance(level.getSeed())
///   key    = random.nextLong() & 65535L
///   list   = Util.toShuffledList(IntStream.range(0, 10),
///                               RandomSource.createThreadLocalInstance(key))
/// ```
///
/// `createThreadLocalInstance(long)` is `new SingleThreadedRandomSource(seed)`
/// — the same LCG as `LegacyRandomSource`, differing only in that it is not
/// thread-safe, so the draw sequence is identical.
///
/// The 16-bit key is why two worlds whose seeds differ can share a spike
/// layout: there are only 65,536 of them. Vanilla caches on it for five
/// minutes; we recompute, because the whole thing is ~12 draws.
///
/// Then, for `i` in `0..10`:
///
/// ```text
///   x        = Mth.floor(42.0 * Math.cos(2.0 * (-PI + 0.3141592653589793 * i)))
///   z        = Mth.floor(42.0 * Math.sin(2.0 * (-PI + 0.3141592653589793 * i)))
///   idx      = list.get(i)
///   radius   = 2 + idx / 3          // Java int division
///   height   = 76 + idx * 3
///   guarded  = idx == 1 || idx == 2 // exactly two of the ten, always
/// ```
///
/// The angle constant is the `double` nearest `PI / 10` as it appears in the
/// class file, not `PI / 10.0` recomputed — the two differ in the last bit
/// and `Mth.floor` of a 42-block radius is close enough to an integer
/// boundary that it matters.
#[must_use]
pub fn spikes_for_seed(world_seed: i64) -> Vec<EndSpike> {
    const PI: f64 = std::f64::consts::PI;
    /// The literal in `SpikeCacheLoader.load`'s constant pool.
    const TENTH_PI: f64 = 0.314_159_265_358_979_3;

    let mut random = LegacyRandomSource::new(world_seed);
    let key = random.next_i64() & 65535;
    let mut key_random = LegacyRandomSource::new(key);
    let list = to_shuffled_list(10, &mut key_random);

    (0..10)
        .map(|i| {
            let angle = 2.0 * (-PI + TENTH_PI * f64::from(i));
            let center_x = crate::math::floor(42.0 * angle.cos());
            let center_z = crate::math::floor(42.0 * angle.sin());
            let idx = list[usize::try_from(i).expect("0..10 fits in usize")];
            EndSpike {
                center_x,
                center_z,
                radius: 2 + idx / 3,
                height: 76 + idx * 3,
                guarded: idx == 1 || idx == 2,
            }
        })
        .collect()
}

/// `EndSpikeFeature.place` — the whole feature.
///
/// The placement chain is a bare `biome` filter, so this runs once per chunk
/// at the chunk origin and places every spike whose centre chunk this is.
/// Returns `true` unconditionally, as vanilla does.
///
/// # The one draw, and why it is not optional
///
/// `placeSpike` consumes exactly one value from the feature random —
/// `random.nextFloat()` for the end crystal's yaw — and nothing else. The
/// crystal itself goes out through [`EndWorld::add_fresh_entity`] since
/// 2026-10-04; before that the draw was taken and the entity dropped. It is
/// taken in the same place, after the cage and before the bedrock.
pub fn place_spike<W: EndWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    random: &mut R,
    origin: (i32, i32, i32),
) -> bool {
    let seed = world.world_seed();
    for spike in spikes_for_seed(seed) {
        if spike.center_within_chunk(origin.0, origin.2) {
            place_one_spike(world, random, &spike);
        }
    }
    true
}

/// `EndSpikeFeature.placeSpike`.
fn place_one_spike<W: EndWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    random: &mut R,
    spike: &EndSpike,
) {
    let radius = spike.radius;
    let min_y = world.min_y();
    // `BlockPos.betweenClosed((cx - r, minY, cz - r), (cx + r, height + 10,
    // cz + r))`. Every position is visited once, so the iteration order
    // cannot change the result; it is written x-then-y-then-z because that is
    // `betweenClosed`'s order and a reader should not have to wonder.
    for z in (spike.center_z - radius)..=(spike.center_z + radius) {
        for y in min_y..=(spike.height + 10) {
            for x in (spike.center_x - radius)..=(spike.center_x + radius) {
                let dx = f64::from(x - spike.center_x);
                let dz = f64::from(z - spike.center_z);
                // `distToLowCornerSqr(centerX, pos.getY(), centerZ)` — the y
                // term is zero by construction because the y passed in is the
                // position's own. `radius * radius + 1` is an int widened to
                // double, and the test is `<=`.
                let dist_sqr = dx * dx + dz * dz;
                if dist_sqr <= f64::from(radius * radius + 1) && y < spike.height {
                    world.set_block(x, y, z, "minecraft:obsidian");
                } else if y > 65 {
                    // Above y 65 the box is *cleared*, not left alone. This
                    // is a destructive write outside the cone and it is what
                    // gives the spikes their bare silhouette.
                    world.set_block(x, y, z, "minecraft:air");
                }
            }
        }
    }

    if spike.guarded {
        place_cage(world, spike);
    }

    // The crystal: `new EndCrystal`, `snapTo(cx + 0.5, height + 1, cz + 0.5,
    // random.nextFloat() * 360, 0)`, then `addFreshEntity`.
    let yaw = random.next_f32() * 360.0;
    let crystal_y = spike.height + 1;
    let block = (spike.center_x, crystal_y, spike.center_z);
    world.add_fresh_entity(crate::structure::spawned::end_crystal(
        block,
        yaw,
        crate::structure::spawned::uuid("minecraft:end_crystal", block, 0),
    ));

    // `crystal.blockPosition()` of an entity snapped to
    // `(cx + 0.5, height + 1, cz + 0.5)` is `(cx, height + 1, cz)`, so the
    // bedrock lands ON the spike's top block and the fire one above it.
    world.set_block(
        spike.center_x,
        crystal_y - 1,
        spike.center_z,
        "minecraft:bedrock",
    );
    // `FireBlock.getState(level, pos)`: soul fire if the block below is a
    // soul-fire base, otherwise `getStateForPlacement`, which returns the
    // **default** state unless the block below both cannot burn and is not
    // face-sturdy upward. The block below is the bedrock written on the line
    // above — not a soul-fire base, and sturdy — so both branches resolve to
    // the default and the connection properties are never computed.
    world.set_block(
        spike.center_x,
        crystal_y,
        spike.center_z,
        "minecraft:fire[age=0,east=false,north=false,south=false,up=false,west=false]",
    );
}

/// The iron-bars cage over a guarded spike.
///
/// A 5x5 column four blocks tall, of which only the walls and the lid are
/// written: `|dx| == 2 || |dz| == 2 || dy == 3`. That is 73 blocks a cage, so
/// the two caged spikes are 146 iron bars — which is exactly what a census of
/// the reference window counts, and is how we know the bars are worldgen and
/// not something the dragon fight adds later.
fn place_cage<W: EndWorld + ?Sized>(world: &mut W, spike: &EndSpike) {
    for dx in -2..=2 {
        for dz in -2..=2 {
            for dy in 0..=3 {
                let edge_x = i32::abs(dx) == 2;
                let edge_z = i32::abs(dz) == 2;
                let lid = dy == 3;
                if !edge_x && !edge_z && !lid {
                    continue;
                }
                // Two flags, and they are *crossed*: the north/south pair is
                // enabled by the x edges and the lid, the west/east pair by
                // the z edges and the lid. Then each individual side is
                // switched off on the face it would point out of.
                let ns = dx == -2 || dx == 2 || lid;
                let we = dz == -2 || dz == 2 || lid;
                let north = ns && dz != -2;
                let south = ns && dz != 2;
                let west = we && dx != -2;
                let east = we && dx != 2;
                let state = format!(
                    "minecraft:iron_bars[east={east},north={north},south={south},\
                     waterlogged=false,west={west}]"
                );
                world.set_block(
                    spike.center_x + dx,
                    spike.height + dy,
                    spike.center_z + dz,
                    &state,
                );
            }
        }
    }
}

/// `EndPlatformFeature.createEndPlatform(level, origin, false)`.
///
/// A 5x5 obsidian slab with three blocks of air above it, at a
/// `fixed_placement` of (100, 49, 0) — so the obsidian is at y 48. Draws
/// nothing.
///
/// The loop nest is dz outer, dx middle, dy inner, and dy runs `-1..3`
/// **exclusive** at the top. Since each position is written at most once the
/// nesting cannot change the result, but the `dy == -1` test is what selects
/// obsidian rather than air and reading the nest wrong inverts the slab.
pub fn place_platform<W: EndWorld + ?Sized>(world: &mut W, origin: (i32, i32, i32)) -> bool {
    for dz in -2..=2 {
        for dx in -2..=2 {
            for dy in -1..3 {
                let (x, y, z) = (origin.0 + dx, origin.1 + dy, origin.2 + dz);
                let want = if dy == -1 {
                    "minecraft:obsidian"
                } else {
                    "minecraft:air"
                };
                // `BlockState.is(Block)` compares the block, not the state,
                // so a stateful match is by name.
                let have = <W as PredicateWorld>::block_at(world, x, y, z);
                if have.split('[').next() != Some(want) {
                    world.set_block(x, y, z, want);
                }
            }
        }
    }
    true
}

/// `Mth.floor(float)` — `(int) value`, then one down if the truncation went
/// the wrong way. **Not** `f32::floor`: the intermediate is an `int`, so the
/// result saturates at `i32` rather than wrapping, and every step stays in
/// `f32`.
fn floor_f32(value: f32) -> i32 {
    let i = value as i32;
    if value < i as f32 {
        i - 1
    } else {
        i
    }
}

/// `Mth.ceil(float)` — the mirror of [`floor_f32`].
fn ceil_f32(value: f32) -> i32 {
    let i = value as i32;
    if value > i as f32 {
        i + 1
    } else {
        i
    }
}

/// `EndIslandFeature.place`.
///
/// A stack of shrinking end-stone discs going **down** from the origin, one
/// per loop turn, until the radius falls to 0.5 or below. Placed by
/// `end_island_decorated` in `small_end_islands` — `rarity_filter 14`, then a
/// weighted count of 1 or 2, `in_square`, and a uniform height in 55..70 — so
/// it is the outer End's floating rubble, not the islands the density function
/// makes.
///
/// # Every number here is an `f32`, and that is the whole feature
///
/// ```text
///   float radius = random.nextInt(3) + 4.0f;
///   for (int dy = 0; radius > 0.5f; dy--) {
///       for (int dx = Mth.floor(-radius); dx <= Mth.ceil(radius); dx++)
///         for (int dz = Mth.floor(-radius); dz <= Mth.ceil(radius); dz++)
///           if ((float)(dx*dx + dz*dz) <= (radius + 1.0f) * (radius + 1.0f))
///               setBlock(origin.offset(dx, dy, dz), END_STONE);
///       radius -= random.nextInt(2) + 0.5f;
///   }
/// ```
///
/// The radius test compares an `int` sum widened to `f32` against an `f32`
/// product, and the loop bounds are `Mth.floor`/`Mth.ceil` of an `f32`.
/// Promoting any of it to `f64` changes which cells are on the rim.
///
/// # The draws
///
/// One `nextInt(3)` for the starting radius, then one `nextInt(2)` **per
/// layer** — so the draw count depends on the radii this instance walks
/// through and is not a fixed cost.
pub fn place_island<W: EndWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    random: &mut R,
    origin: (i32, i32, i32),
) -> bool {
    let mut radius = random.next_i32_bound(3) as f32 + 4.0;
    let mut dy = 0;
    while radius > 0.5 {
        let lo = floor_f32(-radius);
        let hi = ceil_f32(radius);
        for dx in lo..=hi {
            for dz in lo..=hi {
                let sum = (dx * dx + dz * dz) as f32;
                if sum <= (radius + 1.0) * (radius + 1.0) {
                    world.set_block(
                        origin.0 + dx,
                        origin.1 + dy,
                        origin.2 + dz,
                        "minecraft:end_stone",
                    );
                }
            }
        }
        radius -= random.next_i32_bound(2) as f32 + 0.5;
        dy -= 1;
    }
    true
}

/// The registry name of a state string, without its properties.
fn plain(state: &str) -> &str {
    state.split_once('[').map_or(state, |(name, _)| name)
}

/// `BlockState#isAir` — three blocks, not one.
fn is_air_name(name: &str) -> bool {
    matches!(
        name,
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}

/// `BlockGetter#isEmptyBlock`.
fn empty<W: EndWorld + ?Sized>(world: &mut W, x: i32, y: i32, z: i32) -> bool {
    let state = <W as PredicateWorld>::block_at(world, x, y, z);
    is_air_name(plain(&state))
}

/// `Direction.Plane.HORIZONTAL.faces`, **in order**: NORTH, EAST, SOUTH,
/// WEST.
///
/// `getRandomDirection` is `Util.getRandom(faces, random)`, i.e.
/// `faces[random.nextInt(faces.length)]`, so this order is a draw-to-outcome
/// mapping and not a formatting choice. Rotating it by one gives a plant that
/// branches the right number of times in the wrong compass directions — a
/// diff, not a crash.
const HORIZONTAL: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

/// `ChorusFlowerBlock.allNeighborsEmpty(level, pos, except)`.
///
/// **Horizontal only** — the four sides, never up or down. `except` is skipped
/// by identity, and is `None` for a vertical step and the *opposite* of the
/// branch direction for a sideways one, so a branch is allowed to have the
/// stem it grew out of beside it and nothing else.
fn all_neighbors_empty<W: EndWorld + ?Sized>(
    world: &mut W,
    (x, y, z): (i32, i32, i32),
    except: Option<usize>,
) -> bool {
    for (idx, (dx, dz)) in HORIZONTAL.iter().enumerate() {
        if Some(idx) == except {
            continue;
        }
        if !empty(world, x + dx, y, z + dz) {
            return false;
        }
    }
    true
}

/// The index of the opposite horizontal direction in [`HORIZONTAL`].
///
/// NORTH<->SOUTH is 0<->2 and EAST<->WEST is 1<->3, so it is `idx ^ 2` — but
/// only because of the order above, which is why it is written as a match.
const fn opposite(idx: usize) -> usize {
    match idx {
        0 => 2,
        1 => 3,
        2 => 0,
        _ => 1,
    }
}

/// `ChorusPlantBlock.getStateWithConnections(level, pos, state)`, for the
/// default `chorus_plant` state.
///
/// Six booleans read out of the world, and **`down` is not like the other
/// five**:
///
/// ```text
///   down  = below.is(CHORUS_PLANT) || below.is(CHORUS_FLOWER)
///                                  || below.is(BlockTags.SUPPORTS_CHORUS_PLANT)
///   up    = above.is(CHORUS_PLANT) || above.is(CHORUS_FLOWER)
///   north/east/south/west  — same two-way test as `up`
/// ```
///
/// so a plant standing on end stone has `down=true` while its own top has
/// `down=true` too (the block beneath is plant) and `up=false` until the
/// flower or the next segment lands. Every write in `growTreeRecursive` goes
/// through here, and several positions are written **twice** for exactly that
/// reason: the second write is what turns a neighbour's connection on.
///
/// Writing the bare block name instead would be wrong on every block this
/// feature places — the same mistake `basalt[axis=y]` made in the nether.
fn state_with_connections<W: EndWorld + ?Sized>(
    world: &mut W,
    tags: &crate::tags::BlockTags,
    (x, y, z): (i32, i32, i32),
) -> String {
    // `BlockState#is(Block)` compares the block, so a stateful neighbour
    // matches by name.
    let linked = |world: &mut W, bx: i32, by: i32, bz: i32, supports: bool| {
        let state = <W as PredicateWorld>::block_at(world, bx, by, bz);
        let name = plain(&state);
        name == "minecraft:chorus_plant"
            || name == "minecraft:chorus_flower"
            || (supports && tags.contains("minecraft:supports_chorus_plant", name))
    };
    let down = linked(world, x, y - 1, z, true);
    let up = linked(world, x, y + 1, z, false);
    let north = linked(world, x, y, z - 1, false);
    let east = linked(world, x + 1, y, z, false);
    let south = linked(world, x, y, z + 1, false);
    let west = linked(world, x - 1, y, z, false);
    // Alphabetical, which is the order a state string carries everywhere else
    // in this crate.
    format!(
        "minecraft:chorus_plant[down={down},east={east},north={north},\
south={south},up={up},west={west}]"
    )
}

/// The `chorus_flower` a finished branch is capped with.
///
/// `age=5` is the **mature** flower and comes from the growth code, not from
/// the feature: `growTreeRecursive` writes
/// `CHORUS_FLOWER.defaultBlockState().setValue(AGE, 5)`, and the default is
/// `age=0`. A worldgen chorus plant never has a growing tip.
const GROWN_FLOWER: &str = "minecraft:chorus_flower[age=5]";

/// `ChorusFlowerBlock.generatePlant(level, pos, random, maxSpread)`.
///
/// One block at the origin, then the recursion. `maxSpread` is 8, passed by
/// `ChorusPlantFeature.place` and never by anything else in worldgen.
fn generate_plant<W: EndWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    tags: &crate::tags::BlockTags,
    random: &mut R,
    origin: (i32, i32, i32),
    max_spread: i32,
) {
    let state = state_with_connections(world, tags, origin);
    <W as EndWorld>::set_block(world, origin.0, origin.1, origin.2, &state);
    grow_tree_recursive(world, tags, random, origin, origin, max_spread, 0);
}

/// `ChorusFlowerBlock.growTreeRecursive`.
///
/// ```text
///   int height = random.nextInt(4) + 1;
///   if (depth == 0) height++;
///   for (int i = 0; i < height; i++) {
///       BlockPos up = pos.above(i + 1);
///       if (!allNeighborsEmpty(level, up, null)) return;   // <- gives up entirely
///       set(up); set(up.below());
///   }
///   boolean branched = false;
///   if (depth < 4) {
///       int branches = random.nextInt(4);
///       if (depth == 0) branches++;
///       for (int j = 0; j < branches; j++) {
///           Direction dir = HORIZONTAL.getRandomDirection(random);
///           BlockPos side = pos.above(height).relative(dir);
///           if (abs(side.x - origin.x) < maxSpread && abs(side.z - origin.z) < maxSpread
///               && isEmpty(side) && isEmpty(side.below())
///               && allNeighborsEmpty(level, side, dir.getOpposite())) {
///               branched = true;
///               set(side); set(side.relative(dir.getOpposite()));
///               growTreeRecursive(level, side, random, origin, maxSpread, depth + 1);
///           }
///       }
///   }
///   if (!branched) set(pos.above(height), CHORUS_FLOWER age=5);
/// ```
///
/// Four traps, all of them silent:
///
/// * The early `return` on a blocked column skips the **branch loop and the
///   flower**, so a plant that runs into something is left with no tip. It is
///   not a `break`.
/// * `nextInt(4)` for the branch count sits **inside** the `depth < 4` guard,
///   so a fifth-generation branch spends no draws at all beyond its own
///   height. Hoisting it shifts every later draw in the chunk.
/// * `Direction` is drawn once per loop turn **whether or not the branch is
///   taken**, and the four rejection tests short-circuit in the order written
///   — but none of them draw, so only the direction costs.
/// * `branched` is set by *any* successful branch, so a plant with two
///   branches has no flower on its stem, only on its tips.
#[allow(clippy::too_many_arguments)]
fn grow_tree_recursive<W: EndWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    tags: &crate::tags::BlockTags,
    random: &mut R,
    pos: (i32, i32, i32),
    root: (i32, i32, i32),
    max_spread: i32,
    depth: i32,
) {
    let mut height = random.next_i32_bound(4) + 1;
    if depth == 0 {
        height += 1;
    }
    for i in 0..height {
        let up = (pos.0, pos.1 + i + 1, pos.2);
        if !all_neighbors_empty(world, up, None) {
            return;
        }
        let state = state_with_connections(world, tags, up);
        <W as EndWorld>::set_block(world, up.0, up.1, up.2, &state);
        let below = (up.0, up.1 - 1, up.2);
        let state = state_with_connections(world, tags, below);
        <W as EndWorld>::set_block(world, below.0, below.1, below.2, &state);
    }
    let mut branched = false;
    if depth < 4 {
        let mut branches = random.next_i32_bound(4);
        if depth == 0 {
            branches += 1;
        }
        for _ in 0..branches {
            let dir = usize::try_from(random.next_i32_bound(4))
                .expect("nextInt(4) is non-negative and small");
            let (dx, dz) = HORIZONTAL[dir];
            let side = (pos.0 + dx, pos.1 + height, pos.2 + dz);
            if (side.0 - root.0).abs() < max_spread
                && (side.2 - root.2).abs() < max_spread
                && empty(world, side.0, side.1, side.2)
                && empty(world, side.0, side.1 - 1, side.2)
                && all_neighbors_empty(world, side, Some(opposite(dir)))
            {
                branched = true;
                let state = state_with_connections(world, tags, side);
                <W as EndWorld>::set_block(world, side.0, side.1, side.2, &state);
                let (bx, bz) = HORIZONTAL[opposite(dir)];
                let back = (side.0 + bx, side.1, side.2 + bz);
                let state = state_with_connections(world, tags, back);
                <W as EndWorld>::set_block(world, back.0, back.1, back.2, &state);
                grow_tree_recursive(world, tags, random, side, root, max_spread, depth + 1);
            }
        }
    }
    if !branched {
        let tip = (pos.0, pos.1 + height, pos.2);
        <W as EndWorld>::set_block(world, tip.0, tip.1, tip.2, GROWN_FLOWER);
    }
}

/// `ChorusPlantFeature.place`.
///
/// Two conditions and then the whole plant:
///
/// ```text
///   if (level.isEmptyBlock(origin)
///       && level.getBlockState(origin.below()).is(BlockTags.SUPPORTS_CHORUS_PLANT))
///       ChorusFlowerBlock.generatePlant(level, origin, random, 8);
/// ```
///
/// **The feature draws nothing when it declines**, so a rejected origin costs
/// zero — unlike most features, whose config is read before the position test.
/// `SUPPORTS_CHORUS_PLANT` is a one-block tag (`end_stone`), but it is read
/// from the tag table rather than hard-coded, because
/// `SUPPORTS_CHORUS_FLOWER` is a *different* tag with the same single member
/// today and the two are not interchangeable.
pub fn place_chorus_plant<W: EndWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    tags: &crate::tags::BlockTags,
    random: &mut R,
    origin: (i32, i32, i32),
) -> bool {
    if !empty(world, origin.0, origin.1, origin.2) {
        return false;
    }
    let below = <W as PredicateWorld>::block_at(world, origin.0, origin.1 - 1, origin.2);
    if !tags.contains("minecraft:supports_chorus_plant", plain(&below)) {
        return false;
    }
    generate_plant(world, tags, random, origin, 8);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `HORIZONTAL` is a draw-to-outcome mapping, and `opposite` has to agree
    /// with it. Both are `const`, so nothing else can catch a rotation.
    ///
    /// The offsets are Minecraft's, not a maths convention: NORTH is **-z**.
    #[test]
    fn the_horizontal_order_is_north_east_south_west() {
        assert_eq!(HORIZONTAL, [(0, -1), (1, 0), (0, 1), (-1, 0)]);
        for idx in 0..4 {
            let (dx, dz) = HORIZONTAL[idx];
            let (bx, bz) = HORIZONTAL[opposite(idx)];
            assert_eq!((dx + bx, dz + bz), (0, 0), "direction {idx}");
            assert_eq!(opposite(opposite(idx)), idx, "direction {idx}");
        }
    }

    /// The ten centres are a fixed ring, independent of the seed: only the
    /// radius, the height and the caging come from the shuffle.
    #[test]
    fn the_ring_is_seed_independent() {
        let a = spikes_for_seed(20260722);
        let b = spikes_for_seed(1);
        let centres_a: Vec<_> = a.iter().map(|s| (s.center_x, s.center_z)).collect();
        let centres_b: Vec<_> = b.iter().map(|s| (s.center_x, s.center_z)).collect();
        assert_eq!(centres_a, centres_b);
        // `i = 0` gives the angle `-2 PI`, whose cosine is exactly 1.0 and
        // whose sine is `+2.449e-16` — positive, so `Mth.floor(42 * sin)` is
        // 0 and not -1. Which side of zero that lands on is decided in the
        // sixteenth digit, so it is a fact about the constant rather than
        // about the arithmetic, and it is asserted here because a recomputed
        // `PI / 10.0` can move it.
        assert_eq!(centres_a[0], (42, 0));
        // Every centre is on the 42-block ring — but `Mth.floor` rounds each
        // coordinate **down**, never to nearest, so the measured radius is not
        // 42. It drops to `42 - sqrt(2)` near 45 degrees, where both axes lose
        // most of a block, and *rises* past 42 where one axis is already
        // integral and the other floors away from zero: (-42,-1) measures
        // 42.0119. Both directions, and neither is a defect. The real gate on
        // this ring is the reference window, which is 100.000000% exact.
        for (x, z) in centres_a {
            let r = (f64::from(x) * f64::from(x) + f64::from(z) * f64::from(z)).sqrt();
            assert!((40.5..=43.0).contains(&r), "({x},{z}) is at radius {r}");
        }
    }

    /// `guarded = idx == 1 || idx == 2` over a permutation of `0..10`, so
    /// **exactly two** spikes are caged in every world. 73 blocks a cage
    /// gives the 146 iron bars the reference window contains.
    #[test]
    fn exactly_two_spikes_are_caged() {
        for seed in [0_i64, 1, -1, 20260722, i64::MAX] {
            let spikes = spikes_for_seed(seed);
            assert_eq!(
                spikes.iter().filter(|s| s.guarded).count(),
                2,
                "seed {seed}"
            );
        }
    }

    /// The shuffle is a permutation, so the ten radii and heights are always
    /// the same multiset — `2 + idx / 3` and `76 + idx * 3` over `0..10`.
    #[test]
    fn the_shuffle_is_a_permutation() {
        let spikes = spikes_for_seed(20260722);
        let mut heights: Vec<_> = spikes.iter().map(|s| s.height).collect();
        heights.sort_unstable();
        assert_eq!(
            heights,
            (0..10).map(|i| 76 + i * 3).collect::<Vec<i32>>(),
            "every idx in 0..10 must appear exactly once"
        );
        let mut radii: Vec<_> = spikes.iter().map(|s| s.radius).collect();
        radii.sort_unstable();
        assert_eq!(radii, vec![2, 2, 2, 3, 3, 3, 4, 4, 4, 5]);
    }

    /// One spike per chunk: no two centres share a chunk, so
    /// `isCenterWithinChunk` selects exactly one placer for each.
    #[test]
    fn each_spike_has_its_own_chunk() {
        let spikes = spikes_for_seed(20260722);
        let mut chunks: Vec<_> = spikes
            .iter()
            .map(|s| (s.center_x >> 4, s.center_z >> 4))
            .collect();
        chunks.sort_unstable();
        let before = chunks.len();
        chunks.dedup();
        assert_eq!(chunks.len(), before, "two spikes share a chunk");
    }
}

/// `EndGatewayConfiguration`: the exit the gateway's block entity is given,
/// and whether a teleport lands on it exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatewayConfig {
    pub exit: Option<(i32, i32, i32)>,
    pub exact: bool,
}

impl GatewayConfig {
    /// # Errors
    /// If `exit` is present and not three integers.
    pub fn parse(config: &serde_json::Value) -> Result<Self, String> {
        let exit = match config.get("exit") {
            None => None,
            Some(v) => {
                let a = v
                    .as_array()
                    .filter(|a| a.len() == 3)
                    .ok_or_else(|| format!("end_gateway `exit` is not three ints: {v}"))?;
                let n = |i: usize| {
                    a[i].as_i64()
                        .and_then(|x| i32::try_from(x).ok())
                        .ok_or_else(|| format!("end_gateway `exit` is not three ints: {v}"))
                };
                Some((n(0)?, n(1)?, n(2)?))
            }
        };
        Ok(Self {
            exit,
            exact: config
                .get("exact")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        })
    }
}

/// `EndGatewayFeature.place` — from `javap -p -c -constants` on the pinned jar.
///
/// `betweenClosed(origin + (-1,-2,-1), origin + (1,2,1))`, 3x5x3, and per
/// position, in this order of tests:
///
/// ```text
///   x, y and z all equal origin's         -> END_GATEWAY, then its exit
///   y equal origin's (the rest of that layer) -> AIR
///   |dy| == 2 and x and z equal origin's  -> BEDROCK   (the caps)
///   x or z equal origin's, and |dy| != 2  -> BEDROCK   (the cross)
///   anything else                         -> AIR
/// ```
///
/// The last two read the bytecode's offsets 255..296: `sx || sz` falls to
/// the `cap` test, a cap there jumps to the AIR write at 270 and anything
/// else lands on BEDROCK at 286; neither `sx` nor `sz` jumps straight to
/// AIR. This said the reverse for one commit and put 34 of the 45 blocks
/// wrong at both gateways vanilla saved.
///
/// No draw at all, and every write is `Feature.setBlock` (flags 3) with no
/// read first, so the walk order cannot change the result. The block entity's
/// exit is `setExitPosition(config.exit, config.exact)`; the save writes
/// `exit_portal` (`BlockPos.CODEC`, an int array) and `ExactTeleport`, and
/// `Age`, which the emitter supplies.
pub fn place_gateway<W>(world: &mut W, config: GatewayConfig, origin: (i32, i32, i32)) -> bool
where
    W: EndWorld + crate::block_entity::BlockEntityRecorder + ?Sized,
{
    for ((x, y, z), state) in gateway_writes(origin) {
        world.set_block(x, y, z, state);
        if (x, y, z) == origin {
            if let Some((ex, ey, ez)) = config.exit {
                use rustmc_world::nbt::Nbt;
                world.record_block_entity(
                    x,
                    y,
                    z,
                    crate::block_entity::Record {
                        nbt: std::collections::BTreeMap::from([
                            ("exit_portal".to_string(), Nbt::IntArray(vec![ex, ey, ez])),
                            (
                                "ExactTeleport".to_string(),
                                Nbt::Byte(i8::from(config.exact)),
                            ),
                        ]),
                        loot_seed: None,
                    },
                );
            }
        }
    }
    true
}

/// The 45 writes of [`place_gateway`], in its walk order (z, then y, then
/// x), without the block entity. Pure, so the dragon fight's
/// `spawnNewGateway` can be built and checked off a canvas.
#[must_use]
pub fn gateway_writes(origin: (i32, i32, i32)) -> Vec<((i32, i32, i32), &'static str)> {
    let (ox, oy, oz) = origin;
    let mut out = Vec::with_capacity(45);
    for z in oz - 1..=oz + 1 {
        for y in oy - 2..=oy + 2 {
            for x in ox - 1..=ox + 1 {
                let (sx, sy, sz) = (x == ox, y == oy, z == oz);
                let cap = (y - oy).abs() == 2;
                let state = if sx && sy && sz {
                    "minecraft:end_gateway"
                } else if sy {
                    "minecraft:air"
                } else if (cap && sx && sz) || ((sx || sz) && !cap) {
                    "minecraft:bedrock"
                } else {
                    "minecraft:air"
                };
                out.push(((x, y, z), state));
            }
        }
    }
    out
}

/// `EndPodiumFeature.place` — the exit portal the dragon fight builds, from
/// `javap -p -c -constants` on the pinned jar. Not a decoration feature: it is
/// placed by `EnderDragonFight.spawnExitPortal(active)`, which runs only once a
/// player is in the arena (`scanState` is behind `dragonEvent.getPlayers()`),
/// so no harvest without a player holds one.
///
/// Over `betweenClosed(origin + (-4,-1,-4), origin + (4,32,4))`, with
/// `inner = closerThan(origin, 2.5)` (3-D `distSqr < 6.25`):
///
/// ```text
///   not inner and not closerThan(origin, 3.5)  -> untouched
///   y < origin.y:  inner -> BEDROCK, else END_STONE
///   y > origin.y:  AIR
///   y = origin.y:  not inner -> BEDROCK, else END_PORTAL if active, AIR if not
/// ```
///
/// then BEDROCK at origin.above(0..3) and a WALL_TORCH facing each horizontal
/// direction (`Direction.Plane.HORIZONTAL`: north, east, south, west) around
/// origin.above(2). `active` places through `dropPreviousAndSetBlock`, which
/// only drops items first; the block written is the same.
///
/// Returns the writes in vanilla's order, so a caller can apply them and a
/// golden can compare them one for one.
/// The vanilla vectors for [`podium_writes`], from `ci/end-podium-golden.sh`.
pub const PODIUM_TABLE: &str = "data/vanilla/end_podium.json";

/// The return-gateway ring the jar computes (`ci/end-gateway-ring-golden.sh`),
/// read by `golden_end_podium`. Spelled as one literal so
/// `ci/gen-gameplay-tables.sh` derives its generator arm from it.
pub const GATEWAY_RING_TABLE: &str = "data/vanilla/end_gateway_ring.json";

#[must_use]
pub fn podium_writes(
    origin: (i32, i32, i32),
    active: bool,
) -> Vec<((i32, i32, i32), &'static str)> {
    let (ox, oy, oz) = origin;
    let mut out = Vec::new();
    for z in oz - 4..=oz + 4 {
        for y in oy - 1..=oy + 32 {
            for x in ox - 4..=ox + 4 {
                let d2 = f64::from((x - ox).pow(2) + (y - oy).pow(2) + (z - oz).pow(2));
                let inner = d2 < 2.5 * 2.5;
                if !inner && d2 >= 3.5 * 3.5 {
                    continue;
                }
                let state = if y < oy {
                    if inner {
                        "minecraft:bedrock"
                    } else {
                        "minecraft:end_stone"
                    }
                } else if y > oy {
                    "minecraft:air"
                } else if !inner {
                    "minecraft:bedrock"
                } else if active {
                    "minecraft:end_portal"
                } else {
                    "minecraft:air"
                };
                out.push(((x, y, z), state));
            }
        }
    }
    for dy in 0..4 {
        out.push(((ox, oy + dy, oz), "minecraft:bedrock"));
    }
    for (facing, dx, dz) in [
        ("north", 0, -1),
        ("east", 1, 0),
        ("south", 0, 1),
        ("west", -1, 0),
    ] {
        let state = match facing {
            "north" => "minecraft:wall_torch[facing=north]",
            "east" => "minecraft:wall_torch[facing=east]",
            "south" => "minecraft:wall_torch[facing=south]",
            _ => "minecraft:wall_torch[facing=west]",
        };
        out.push(((ox + dx, oy + 2, oz + dz), state));
    }
    out
}

/// Where `EnderDragonFight.spawnExitPortal` puts the podium the first time:
///
/// ```text
///   p = level.getHeightmapPos(MOTION_BLOCKING_NO_LEAVES, EndPodiumFeature.getLocation(origin)).below()
///   while level.getBlockState(p).is(BEDROCK) && p.y > 63: p = p.below()
///   p = p.atY(max(level.getMinY() + 1, p.y))
/// ```
///
/// `getLocation(origin)` is `END_PODIUM_LOCATION.offset(origin)`, and both
/// are `BlockPos.ZERO` for a fresh fight, so the column is (0, 0).
/// `height_at_origin` is the `MOTION_BLOCKING_NO_LEAVES` height there (one
/// above the top blocking block), read off the world as the fight finds it.
#[must_use]
pub fn exit_portal_origin(
    height_at_origin: i32,
    mut block_at: impl FnMut(i32, i32, i32) -> String,
    min_y: i32,
) -> (i32, i32, i32) {
    let mut y = height_at_origin - 1;
    while block_at(0, y, 0).split('[').next() == Some("minecraft:bedrock") && y > 63 {
        y -= 1;
    }
    (0, y.max(min_y + 1), 0)
}

/// The return-gateway ring `EnderDragonFight` builds, one gateway per dragon
/// kill, in the order the kills build them.
///
/// `init(level, seed, origin)`, when the saved list is empty, fills it with
/// `0..20` and runs `Util.shuffle(list, RandomSource.createThreadLocalInstance
/// (seed))` (`for i = size; i > 1; i--: swap(i - 1, nextInt(i))`), where
/// `seed` is `WorldOptions.seed()`. `spawnNewGateway()` then takes
/// `list.remove(list.size() - 1)`, so the first kill builds the LAST index.
/// Gated against the jar by [`GATEWAY_RING_TABLE`].
#[must_use]
pub fn gateway_spawn_order(world_seed: i64) -> Vec<i32> {
    let mut random = LegacyRandomSource::new(world_seed);
    let mut list = to_shuffled_list(20, &mut random);
    list.reverse();
    list
}

/// `spawnNewGateway()`'s position for ring index `i`:
///
/// ```text
///   x = floor(96 * cos(2 * (-PI + 0.15707963267948966 * i)))
///   z = floor(96 * sin(2 * (-PI + 0.15707963267948966 * i)))
///   y = 75
/// ```
///
/// The block there is `END_GATEWAY_DELAYED` (`EndGatewayConfiguration
/// .delayedExitSearch()`: no exit, not exact), i.e. [`place_gateway`] with
/// `GatewayConfig { exit: None, exact: false }`.
#[must_use]
pub fn gateway_ring_position(i: i32) -> (i32, i32, i32) {
    const PI: f64 = std::f64::consts::PI;
    /// The literal in `spawnNewGateway`'s constant pool (PI / 20).
    const TWENTIETH_PI: f64 = 0.157_079_632_679_489_66;
    let angle = 2.0 * (-PI + TWENTIETH_PI * f64::from(i));
    (
        crate::math::floor(96.0 * angle.cos()),
        75,
        crate::math::floor(96.0 * angle.sin()),
    )
}

/// What `TheEndGatewayBlockEntity`'s exit search reads from the level.
///
/// Every chunk it names is one vanilla forces to FULL (`Level.getChunk(x,
/// z)`), so an implementation answers off decorated chunks.
pub trait GatewayExitWorld {
    /// `LevelChunk.getHighestFilledSectionIndex`, as the section's Y
    /// (`getSectionYFromSectionIndex`), or `None` when every section of the
    /// chunk holds only air.
    fn highest_filled_section_y(&mut self, chunk_x: i32, chunk_z: i32) -> Option<i32>;
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String;
    /// `BlockState.isCollisionShapeFullBlock`.
    fn is_collision_full_block(&mut self, state: &str) -> bool;
    /// `getMinY` / `getMaxY` (inclusive) of the End.
    fn min_y(&self) -> i32;
    fn max_y(&self) -> i32;
}

/// The result of a first entry into a return gateway that has no exit yet.
#[derive(Clone, Debug, PartialEq)]
pub struct GatewayExit {
    /// `findExitPortalXZPosTentative` (y is always 0).
    pub tentative: (f64, f64),
    /// `findValidSpawnInChunk`, or the island centre when there was none.
    /// Not the winning block: the iterator's last position (see the body).
    pub spawn: (i32, i32, i32),
    /// `Some(pos)` when no spawn was found and vanilla placed
    /// `EndFeatures.END_ISLAND` at `pos` with `RandomSource.create()` (an
    /// unseeded random: its shape is not reproducible, so a caller checks
    /// nothing that depends on it).
    pub island: Option<(i32, i32, i32)>,
    /// `findTallestBlock(spawn, 16, true).above(10)`: where the exit gateway
    /// is built (`EndGatewayConfiguration.knownExit(gateway, false)`) and the
    /// position the entered gateway saves as `exit_portal`.
    pub exit: (i32, i32, i32),
}

/// `TheEndGatewayBlockEntity.getPortalPosition` for a gateway with no exit,
/// in the End: `findOrCreateValidTeleportPos(level, gateway).above(10)`.
///
/// ```text
///   dir = Vec3(gateway.x, 0, gateway.z).normalize()       (ZERO if |v| < 1e-5f)
///   p = dir * 1024
///   16 times at most, while chunk(p) is not empty: p = p + dir * -16
///   16 times at most, while chunk(p) is empty:     p = p + dir * 16
///   s = findValidSpawnInChunk(chunk(p))
///       over betweenClosed((minX, 30, minZ), (maxX, highestSectionPos + 15, maxZ)),
///       x fastest, then y, then z: END_STONE with the two blocks above
///       not collision-full; if any: the cursor's final value, (maxX, top, maxZ)
///   none -> s = containing(p.x + 0.5, 75, p.z + 0.5), END_ISLAND placed at s
///   exit = findTallestBlock(s, 16, allowBedrock = true).above(10)
/// ```
///
/// `findTallestBlock(c, r, bedrock)`: for dx in -r..=r, dz in -r..=r (dz
/// inner; (0,0) skipped only when `!bedrock`), scan y from `maxY` down while
/// `y > best.y` (`minY` before any best) and take the first collision-full
/// block (not bedrock unless allowed). Falls back to `c`.
/// `chunk(p)` is `(floor(p.x / 16), floor(p.z / 16))`; empty is
/// `getHighestFilledSectionIndex() == -1`, and a chunk's
/// `getHighestSectionPosition` is that section's block Y, or `minY` when empty.
pub fn gateway_exit<W: GatewayExitWorld + ?Sized>(
    world: &mut W,
    gateway: (i32, i32, i32),
) -> GatewayExit {
    let chunk_of = |p: (f64, f64)| -> (i32, i32) {
        (
            crate::math::floor(p.0 / 16.0),
            crate::math::floor(p.1 / 16.0),
        )
    };
    // Vec3(x, 0, z).normalize(); the float literal is 1.0E-5f widened.
    let (vx, vz) = (f64::from(gateway.0), f64::from(gateway.2));
    let len = (vx * vx + vz * vz).sqrt();
    let dir = if len < 9.999_999_747_378_752e-6 {
        (0.0, 0.0)
    } else {
        (vx / len, vz / len)
    };
    let mut p = (dir.0 * 1024.0, dir.1 * 1024.0);
    let empty = |world: &mut W, p: (f64, f64)| {
        let (cx, cz) = chunk_of(p);
        world.highest_filled_section_y(cx, cz).is_none()
    };
    let mut budget = 16;
    while !empty(world, p) {
        let go = budget > 0;
        budget -= 1;
        if !go {
            break;
        }
        p = (p.0 + dir.0 * -16.0, p.1 + dir.1 * -16.0);
    }
    let mut budget = 16;
    while empty(world, p) {
        let go = budget > 0;
        budget -= 1;
        if !go {
            break;
        }
        p = (p.0 + dir.0 * 16.0, p.1 + dir.1 * 16.0);
    }
    let tentative = p;

    let (cx, cz) = chunk_of(p);
    let highest = world
        .highest_filled_section_y(cx, cz)
        .map_or(world.min_y(), |s| s * 16);
    let (y_lo, y_hi) = (30.min(highest + 15), 30.max(highest + 15));
    let mut best: Option<((i32, i32, i32), f64)> = None;
    for z in cz * 16..=cz * 16 + 15 {
        for y in y_lo..=y_hi {
            for x in cx * 16..=cx * 16 + 15 {
                let here = world.block_at(x, y, z);
                if here.split('[').next() != Some("minecraft:end_stone") {
                    continue;
                }
                let up1 = world.block_at(x, y + 1, z);
                if world.is_collision_full_block(&up1) {
                    continue;
                }
                let up2 = world.block_at(x, y + 2, z);
                if world.is_collision_full_block(&up2) {
                    continue;
                }
                let d = (f64::from(x) + 0.5).powi(2)
                    + (f64::from(y) + 0.5).powi(2)
                    + (f64::from(z) + 0.5).powi(2);
                if best.is_none_or(|(_, b)| d < b) {
                    best = Some(((x, y, z), d));
                }
            }
        }
    }
    // `best = pos` stores the `betweenClosed` iterator's MutableBlockPos, not
    // a copy, and the cursor keeps moving: once the walk ends it holds the
    // box's LAST position. So any hit returns (maxX, top, maxZ), whichever
    // block won. Vanilla's own debug line says so: "Found suitable block to
    // teleport to: MutableBlockPos{x=-913, y=47, z=-289}" at seed 12345,
    // where the nearest candidate was (-927, 36, -303).
    let (spawn, island) = match best {
        Some(_) => ((cx * 16 + 15, y_hi, cz * 16 + 15), None),
        None => {
            let pos = (
                crate::math::floor(p.0 + 0.5),
                75,
                crate::math::floor(p.1 + 0.5),
            );
            (pos, Some(pos))
        }
    };
    let top = find_tallest_block(world, spawn, 16, true);
    GatewayExit {
        tentative,
        spawn,
        island,
        exit: (top.0, top.1 + 10, top.2),
    }
}

/// `TheEndGatewayBlockEntity.findTallestBlock`; see [`gateway_exit`].
pub fn find_tallest_block<W: GatewayExitWorld + ?Sized>(
    world: &mut W,
    centre: (i32, i32, i32),
    radius: i32,
    allow_bedrock: bool,
) -> (i32, i32, i32) {
    let mut best: Option<(i32, i32, i32)> = None;
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            if dx == 0 && dz == 0 && !allow_bedrock {
                continue;
            }
            let mut y = world.max_y();
            while y > best.map_or(world.min_y(), |b| b.1) {
                let pos = (centre.0 + dx, y, centre.2 + dz);
                let state = world.block_at(pos.0, pos.1, pos.2);
                if world.is_collision_full_block(&state)
                    && (allow_bedrock || state.split('[').next() != Some("minecraft:bedrock"))
                {
                    best = Some(pos);
                    break;
                }
                y -= 1;
            }
        }
    }
    best.unwrap_or(centre)
}
