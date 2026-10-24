//! Ocean ruins: the draws that choose one, and the height it settles at.
//!
//! # Why this module is only half a structure
//!
//! Choosing a ruin and *writing* one are separated here because they need
//! different things and fail in different ways. [`plan`] is pure arithmetic
//! over a `RandomSource` and can be pinned against a number vanilla already
//! committed to disk. [`settle_height`] needs the terrain, so it takes a
//! reader rather than a world, and is testable against a hand-built column.
//! Neither of them touches a chunk.
//!
//! # The draw order, from bytecode
//!
//! `OceanRuinStructure.generatePieces` is three statements and spends draws in
//! two of them:
//!
//! ```text
//!   pos      = BlockPos(chunk.getMinBlockX(), 90, chunk.getMinBlockZ())
//!   rotation = Rotation.getRandom(random)            <- draw 1
//!   OceanRuinPieces.addPieces(manager, pos, rotation, builder, random, this)
//! ```
//!
//! The y of 90 is a placeholder and never survives: `postProcess` overwrites
//! it twice, first from the `OCEAN_FLOOR_WG` heightmap and then from
//! [`settle_height`].
//!
//! `addPieces` continues:
//!
//! ```text
//!   large     = nextFloat() <= largeProbability      <- draw 2, unconditional
//!   integrity = large ? 0.9 : 0.8
//!   addPiece(..., large, integrity)                  <- draw 3
//!   if (large && nextFloat() <= clusterProbability)  <- draw 4, GUARDED
//!       addClusterRuins(...)
//! ```
//!
//! and the warm branch of `addPiece` is a single `Util.getRandom(array,
//! random)`, which javap confirms is exactly `array[random.nextInt(len)]` —
//! one draw, over `WARM_RUINS` when small and `BIG_WARM_RUINS` when large.
//!
//! Two things about draw 4 are easy to get wrong and both change every
//! subsequent structure in the chunk. It is spent **only** when `large`, so a
//! small ruin costs three draws and not four; and `addPiece` happens *before*
//! it, so the template index is drawn before the cluster coin, not after.
//!
//! # What a server has already told us
//!
//! At seed 31415926 a server saved
//!
//! ```text
//!   minecraft:ocean_ruin_warm start (67, 65) 1 pieces,
//!                                   union 1066,48,1040..1072,54,1045
//! ```
//!
//! Every warm template is 6x7x7 and every big one 16x16x16, so a 7x7x6 union
//! is a **small** ruin turned 90 or 270 degrees. With `large_probability` at
//! 0.3 that fixes draw 2 above 0.3, fixes the integrity at 0.8, and means draw
//! 4 was never spent. Those are predictions this module can be held to rather
//! than decorations on it.

use crate::rng::RandomSource;
use crate::structure::template::Rotation;

/// `WARM_RUINS`, in the order `<clinit>` builds the array — the order is the
/// whole point, since the choice is an index into it.
pub const WARM_RUINS: [&str; 8] = [
    "warm_1", "warm_2", "warm_3", "warm_4", "warm_5", "warm_6", "warm_7", "warm_8",
];

/// `BIG_WARM_RUINS`. Note it is 4 long and skips 1..3 — the names are
/// `big_warm_4` through `big_warm_7`, not `big_warm_1` through `4`.
pub const BIG_WARM_RUINS: [&str; 4] = ["big_warm_4", "big_warm_5", "big_warm_6", "big_warm_7"];

/// `Rotation.values()`, in declaration order, because `Rotation.getRandom` is
/// `Util.getRandom(values(), random)` and therefore indexes this order.
pub const ROTATIONS: [Rotation; 4] = [
    Rotation::None,
    Rotation::Clockwise90,
    Rotation::Clockwise180,
    Rotation::Counterclockwise90,
];

/// What the draws decided, before any terrain is consulted.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// The template's name within `minecraft:structure/underwater_ruin`.
    pub template: &'static str,
    pub rotation: Rotation,
    /// 0.9 when large, 0.8 when not. Passed to `BlockRotProcessor`.
    pub integrity: f32,
    pub large: bool,
    /// `(minBlockX, 90, minBlockZ)`. The 90 is vanilla's placeholder and is
    /// replaced before anything is written; it is kept so that the value being
    /// replaced is visible rather than implied.
    pub origin: (i32, i32, i32),
    /// Whether the cluster coin was spent. False for a small ruin — recorded
    /// because "did not cluster" and "never asked" leave the stream in
    /// different places, and only one of them is what vanilla does.
    pub cluster_coin_spent: bool,
    /// True when the cluster coin was both spent and passed.
    pub clustered: bool,
}

/// The whole warm-ruin decision, in vanilla's draw order.
///
/// `random` must already be seeded the way `Structure.generate` seeds it —
/// `setLargeFeatureSeed(seed, chunkX, chunkZ)` — which is the same call the
/// mineshaft start uses.
pub fn plan<R: RandomSource + ?Sized>(
    random: &mut R,
    chunk_x: i32,
    chunk_z: i32,
    large_probability: f32,
    cluster_probability: f32,
) -> Plan {
    let origin = (chunk_x * 16, 90, chunk_z * 16);

    // Draw 1. Before addPieces, so a structure that got the rotation from the
    // wrong place would still land in the right chunk and be wrong everywhere
    // inside it.
    let rotation = ROTATIONS[random.next_i32_bound(ROTATIONS.len() as i32) as usize];

    // Draw 2. `fcmpg; ifgt` — large when the float is <= the probability, so
    // the comparison is inclusive on the wrong side to guess.
    let large = random.next_f32() <= large_probability;
    let integrity = if large { 0.9 } else { 0.8 };

    // Draw 3, inside addPiece.
    let template = if large {
        BIG_WARM_RUINS[random.next_i32_bound(BIG_WARM_RUINS.len() as i32) as usize]
    } else {
        WARM_RUINS[random.next_i32_bound(WARM_RUINS.len() as i32) as usize]
    };

    // Draw 4, guarded. Short-circuit is load-bearing: `large` false must not
    // consume the float.
    let mut cluster_coin_spent = false;
    let mut clustered = false;
    if large {
        cluster_coin_spent = true;
        clustered = random.next_f32() <= cluster_probability;
    }

    Plan {
        template,
        rotation,
        integrity,
        large,
        origin,
        cluster_coin_spent,
        clustered,
    }
}

/// What [`settle_height`] needs to know about the world, and nothing more.
///
/// Deliberately not a chunk or a level: the routine is a column walk, and
/// giving it a world would make it untestable without generating one.
pub trait ColumnReader {
    /// The block state at a position, as a full state string.
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String;
    /// The floor of the world, `level.getMinY()`.
    fn min_y(&self) -> i32;
}

/// Whether a state stops the descent in `getHeight`.
///
/// Vanilla's test is `state.isAir() || fluid.is(WATER) || state.is(ICE)`, so
/// it passes through air, water *and* ice. Water is checked as a **fluid**,
/// which is why waterlogged blocks do not count: their fluid is water but the
/// block is not, and `getFluidState` of a waterlogged stair is water — so a
/// waterlogged block *does* pass the test. That is vanilla's behaviour, not a
/// simplification, and it matters on a ruin because ruins are full of
/// waterlogged stairs.
fn passes_through(state: &str) -> bool {
    let name = state.split('[').next().unwrap_or(state);
    let bare = name.strip_prefix("minecraft:").unwrap_or(name);
    if matches!(
        bare,
        "air" | "cave_air" | "void_air" | "water" | "flowing_water"
    ) {
        return true;
    }
    // BlockTags.ICE
    if matches!(bare, "ice" | "packed_ice" | "blue_ice" | "frosted_ice") {
        return true;
    }
    // A waterlogged block's fluid state is water.
    state.contains("waterlogged=true")
}

/// `OceanRuinPiece.getHeight` — the second and final y adjustment.
///
/// Walks every column of the piece's footprint from one below the current
/// template position down to the first block that is neither air, water nor
/// ice, and then decides whether the ruin should sink to meet them.
///
/// ```text
///   result  = pos.getY()
///   minSeen = 512
///   floor   = result - 1
///   count   = 0
///   for each (x, z) in betweenClosed(pos, corner):
///       y = pos.getY() - 1
///       while passes_through(x, y, z) && y > minY + 1:  y -= 1
///       minSeen = min(minSeen, y)
///       if y < floor - 2: count += 1
///   span = abs(pos.getX() - corner.getX())
///   if floor - minSeen > 2 && count > span - 2:  result = minSeen + 1
///   return result
/// ```
///
/// The two guards are what stop a ruin from dropping into a trench it only
/// clips the edge of: the drop has to be more than two blocks deep *and* be
/// under most of the footprint.
pub fn settle_height<C: ColumnReader + ?Sized>(
    reader: &mut C,
    pos: (i32, i32, i32),
    corner: (i32, i32, i32),
) -> i32 {
    let mut result = pos.1;
    let mut min_seen = 512;
    let floor = result - 1;
    let mut count = 0;

    let min_y = reader.min_y();
    let (x0, x1) = (pos.0.min(corner.0), pos.0.max(corner.0));
    let (z0, z1) = (pos.2.min(corner.2), pos.2.max(corner.2));

    for x in x0..=x1 {
        for z in z0..=z1 {
            let mut y = pos.1 - 1;
            while passes_through(&reader.block_at(x, y, z)) && y > min_y + 1 {
                y -= 1;
            }
            min_seen = min_seen.min(y);
            if y < floor - 2 {
                count += 1;
            }
        }
    }

    let span = (pos.0 - corner.0).abs();
    if floor - min_seen > 2 && count > span - 2 {
        result = min_seen + 1;
    }
    result
}

/// The blocks a warm ruin writes, in vanilla's processing order.
///
/// `StructureTemplate.processBlockInfos` is two passes and this mirrors both:
///
/// ```java
///   for each template block b:
///       worldPos = calculateRelativePosition(settings, b.pos).offset(offset)
///       info = StructureBlockInfo(worldPos, b.state, b.nbt)
///       for processor in settings.getProcessors():        // per-block pass
///           if (info == null) break
///           info = processor.processBlock(..., info, settings)
///       if (info != null) { out.add(info); original.add(b) }
///   for processor in settings.getProcessors():            // finalize pass
///       out = processor.finalizeProcessing(..., original, out, settings)
/// ```
///
/// The split matters more than it looks. `BlockRotProcessor` and
/// `BlockIgnoreProcessor` run per block and may return null, so by the time
/// the capped processor's `finalizeProcessing` runs, the list it shuffles is
/// the **survivors** — not the template's 294 entries. Shuffling the wrong
/// length would pick different indices from the same seed and put the
/// suspicious sand somewhere else entirely.
///
/// `y` is passed in rather than computed because `settle_height` is a separate
/// question with its own evidence; fixing y here keeps a failure in the
/// processor chain from being blamed on the height walk, and vice versa.
pub fn place_blocks(
    template: &crate::structure::template::Template,
    plan: &Plan,
    y: i32,
    world_seed: i64,
) -> Vec<((i32, i32, i32), String)> {
    place_blocks_for(template, plan, y, world_seed, false)
}

/// `RUINS_BRICK`, `RUINS_CRACKED`, `RUINS_MOSSY` and their `BIG_` twins, in
/// `<clinit>` order. A cold `addPiece` draws **one** index into the brick
/// array and uses the same index into the other two, so a cold ruin is three
/// pieces at one position and one rotation, at integrities `integrity`, 0.7
/// and 0.5 — and its draw count is exactly the warm one's.
pub const RUINS_BRICK: [&str; 8] = [
    "brick_1", "brick_2", "brick_3", "brick_4", "brick_5", "brick_6", "brick_7", "brick_8",
];
pub const RUINS_CRACKED: [&str; 8] = [
    "cracked_1",
    "cracked_2",
    "cracked_3",
    "cracked_4",
    "cracked_5",
    "cracked_6",
    "cracked_7",
    "cracked_8",
];
pub const RUINS_MOSSY: [&str; 8] = [
    "mossy_1", "mossy_2", "mossy_3", "mossy_4", "mossy_5", "mossy_6", "mossy_7", "mossy_8",
];
pub const BIG_RUINS_BRICK: [&str; 4] = ["big_brick_1", "big_brick_2", "big_brick_3", "big_brick_8"];
pub const BIG_RUINS_CRACKED: [&str; 4] = [
    "big_cracked_1",
    "big_cracked_2",
    "big_cracked_3",
    "big_cracked_8",
];
pub const BIG_RUINS_MOSSY: [&str; 4] = ["big_mossy_1", "big_mossy_2", "big_mossy_3", "big_mossy_8"];

/// The three cold pieces for the warm template the shared draw picked:
/// `(template, integrity)` for brick, cracked and mossy. The warm planner's
/// one `nextInt` over an 8- or 4-long array is the cold one's too, so the
/// index is recovered from the warm name rather than drawn again.
#[must_use]
pub fn cold_pieces(warm: &str, integrity: f32) -> Option<[(&'static str, f32); 3]> {
    if let Some(i) = WARM_RUINS.iter().position(|n| *n == warm) {
        return Some([
            (RUINS_BRICK[i], integrity),
            (RUINS_CRACKED[i], 0.7),
            (RUINS_MOSSY[i], 0.5),
        ]);
    }
    let i = BIG_WARM_RUINS.iter().position(|n| *n == warm)?;
    Some([
        (BIG_RUINS_BRICK[i], integrity),
        (BIG_RUINS_CRACKED[i], 0.7),
        (BIG_RUINS_MOSSY[i], 0.5),
    ])
}

/// [`place_blocks`] for either temperature. The two `archyRuleProcessor`s
/// differ only in what they match and what they write: `SAND` to
/// `SUSPICIOUS_SAND` warm, `GRAVEL` to `SUSPICIOUS_GRAVEL` cold, both capped
/// at `ConstantInt.of(5)` (`OceanRuinPieces.<clinit>` offsets 0..27).
pub fn place_blocks_for(
    template: &crate::structure::template::Template,
    plan: &Plan,
    y: i32,
    world_seed: i64,
    cold: bool,
) -> Vec<((i32, i32, i32), String)> {
    let (target, replacement) = if cold {
        ("minecraft:gravel", "minecraft:suspicious_gravel[dusted=0]")
    } else {
        ("minecraft:sand", "minecraft:suspicious_sand[dusted=0]")
    };
    use crate::structure::template::{ignored_by_structure_and_air, survives_rot, Mirror};

    let origin = (plan.origin.0, y, plan.origin.2);

    // Per-block pass, in `makeSettings` order: rot, then ignore. The capped
    // processor's own `processBlock` is a pass-through; it acts in finalize.
    let mut survivors: Vec<((i32, i32, i32), String)> = Vec::new();
    for (pos, state) in template.blocks_rotated(origin, Mirror::None, plan.rotation, (0, 0)) {
        // BlockRotProcessor: position-derived, spends nothing from any stream.
        if !survives_rot(pos, plan.integrity) {
            continue;
        }
        // BlockIgnoreProcessor.STRUCTURE_AND_AIR.
        if ignored_by_structure_and_air(&state) {
            continue;
        }
        survivors.push((pos, state));
    }

    // Finalize pass: the capped sand rule. The origin it forks at is the
    // piece's world position — `aload_2` in `finalizeProcessing`.
    let is_sand = |i: usize| {
        let state: &str = &survivors[i].1;
        state.split('[').next().unwrap_or(state) == target
    };
    let chosen = crate::structure::processor::capped_selection(
        world_seed,
        origin,
        survivors.len(),
        SUSPICIOUS_CAP,
        is_sand,
    );
    for index in chosen {
        // The rule's replacement is `SUSPICIOUS_SAND.defaultBlockState()`, and
        // that default carries `dusted=0`. Writing the bare name reads as the
        // same block to a human and as a different state to a diff: the
        // suspicious-sand gate compared with `starts_with` and passed, while a
        // full block-for-block comparison against the server found all five
        // wrong. Canonical states, always.
        survivors[index].1 = replacement.to_string();
    }

    survivors
}

/// `ConstantInt.of(5)` in `WARM_SUSPICIOUS_BLOCK_PROCESSOR` — `iconst_5`.
pub const SUSPICIOUS_CAP: i32 = 5;

/// `Mth.nextInt(random, lo, hi)` — inclusive both ends.
///
/// `random.nextInt(hi - lo + 1) + lo`, one draw. Spelled out because the
/// cluster code calls it eighteen times and an off-by-one in the span would
/// shift every satellite without changing the draw count.
fn mth_next_int<R: RandomSource + ?Sized>(random: &mut R, lo: i32, hi: i32) -> i32 {
    random.next_i32_bound(hi - lo + 1) + lo
}

/// `OceanRuinPieces.allPositions` — the eight candidate satellite positions.
///
/// A 3x3 grid of chunk-sized cells around the anchor with the centre left out,
/// each cell jittered by its own pair of draws. The spans are **not** uniform:
///
/// ```text
///   (-16 + n(1,8), 0,  16 + n(1,7))
///   (-16 + n(1,8), 0,       n(1,7))
///   (-16 + n(1,8), 0, -16 + n(4,8))
///   (      n(1,7), 0,  16 + n(1,7))
///   (      n(1,7), 0, -16 + n(4,6))
///   ( 16 + n(1,7), 0,  16 + n(3,8))
///   ( 16 + n(1,7), 0,       n(1,7))
///   ( 16 + n(1,7), 0, -16 + n(4,8))
/// ```
///
/// Note `n(1,8)` on the west column against `n(1,7)` elsewhere, and `n(4,6)`,
/// `n(3,8)`, `n(4,8)` scattered through the south and north rows. They look
/// like they should all be the same and they are not; each is read off its own
/// `bipush` pair. Sixteen draws in total, in exactly this order.
pub fn all_positions<R: RandomSource + ?Sized>(
    random: &mut R,
    anchor: (i32, i32, i32),
) -> Vec<(i32, i32, i32)> {
    let (ax, ay, az) = anchor;
    let mut out = Vec::with_capacity(8);
    let mut push = |random: &mut R, xb: i32, xl: i32, xh: i32, zb: i32, zl: i32, zh: i32| {
        let dx = xb + mth_next_int(random, xl, xh);
        let dz = zb + mth_next_int(random, zl, zh);
        out.push((ax + dx, ay, az + dz));
    };
    push(random, -16, 1, 8, 16, 1, 7);
    push(random, -16, 1, 8, 0, 1, 7);
    push(random, -16, 1, 8, -16, 4, 8);
    push(random, 0, 1, 7, 16, 1, 7);
    push(random, 0, 1, 7, -16, 4, 6);
    push(random, 16, 1, 7, 16, 3, 8);
    push(random, 16, 1, 7, 0, 1, 7);
    push(random, 16, 1, 7, -16, 4, 8);
    out
}

/// One satellite of a cluster.
#[derive(Debug, Clone, PartialEq)]
pub struct Satellite {
    pub plan: Plan,
    /// False when the candidate overlapped the main ruin's box and was
    /// skipped. Kept rather than filtered because a skip still consumed the
    /// position and the rotation draw, just not the template draw.
    pub placed: bool,
}

/// `OceanRuinPieces.addClusterRuins`, in draw order.
///
/// Called from `addPieces` only when the ruin is large **and** the second coin
/// passes at `clusterProbability` 0.9 — so on a large ruin it is the usual
/// outcome, not a rarity.
///
/// ```java
///   base   = new BlockPos(pos.getX(), 90, pos.getZ());
///   corner = transform(new BlockPos(15, 0, 15), NONE, rotation, ZERO).offset(base);
///   mainBox = BoundingBox.fromCorners(base, corner);
///   anchor  = new BlockPos(min(base.x, corner.x), base.y, min(base.z, corner.z));
///   positions = allPositions(random, anchor);          // 16 draws
///   count = Mth.nextInt(random, 4, 8);                 // 1 draw
///   for (i = 0; i < count; i++) {
///       if (positions.isEmpty()) continue;
///       p = positions.remove(random.nextInt(positions.size()));   // 1 draw
///       r = Rotation.getRandom(random);                           // 1 draw
///       c2 = transform(new BlockPos(5, 0, 6), NONE, r, ZERO).offset(p);
///       if (!BoundingBox.fromCorners(p, c2).intersects(mainBox))
///           addPiece(..., false, 0.8f);                           // 1 draw
///   }
/// ```
///
/// Two details decide the stream and both are easy to miss:
///
///   * the loop runs `count` times whether or not a position is placed, and
///     the **template draw is inside the intersection test** — a skipped
///     satellite costs two draws, a placed one costs three;
///   * the satellite corner uses `BlockPos(5, 0, 6)`, not the template's real
///     size. It is a fixed probe box for the overlap test only.
///
/// Every satellite is small with integrity 0.8, so none of them recurses.
pub fn cluster<R: RandomSource + ?Sized>(random: &mut R, main: &Plan) -> Vec<Satellite> {
    use crate::structure::template::{transform, Mirror};

    let base = (main.origin.0, 90, main.origin.2);
    let far = transform((15, 0, 15), Mirror::None, main.rotation, (0, 0));
    let corner = (base.0 + far.0, base.1 + far.1, base.2 + far.2);
    let main_box = (
        base.0.min(corner.0),
        base.2.min(corner.2),
        base.0.max(corner.0),
        base.2.max(corner.2),
    );
    let anchor = (base.0.min(corner.0), base.1, base.2.min(corner.2));

    let mut positions = all_positions(random, anchor);
    let count = mth_next_int(random, 4, 8);

    let mut out = Vec::new();
    for _ in 0..count {
        if positions.is_empty() {
            continue;
        }
        let index = random.next_i32_bound(positions.len() as i32) as usize;
        let pos = positions.remove(index);
        let rotation = ROTATIONS[random.next_i32_bound(ROTATIONS.len() as i32) as usize];

        let probe = transform((5, 0, 6), Mirror::None, rotation, (0, 0));
        let other = (pos.0 + probe.0, pos.2 + probe.2);
        let box2 = (
            pos.0.min(other.0),
            pos.2.min(other.1),
            pos.0.max(other.0),
            pos.2.max(other.1),
        );
        let intersects = box2.0 <= main_box.2
            && box2.2 >= main_box.0
            && box2.1 <= main_box.3
            && box2.3 >= main_box.1;

        if intersects {
            out.push(Satellite {
                plan: Plan {
                    template: "",
                    rotation,
                    integrity: 0.8,
                    large: false,
                    origin: pos,
                    cluster_coin_spent: false,
                    clustered: false,
                },
                placed: false,
            });
            continue;
        }

        let template = WARM_RUINS[random.next_i32_bound(WARM_RUINS.len() as i32) as usize];
        out.push(Satellite {
            plan: Plan {
                template,
                rotation,
                integrity: 0.8,
                large: false,
                origin: pos,
                cluster_coin_spent: false,
                clustered: false,
            },
            placed: true,
        });
    }
    out
}

/// The blocks `handleDataMarker` writes, which the template itself does not.
///
/// `TemplateStructurePiece.postProcess` scans the placed template for
/// `structure_block` entries with metadata and dispatches on the string.
/// `OceanRuinPiece` understands two:
///
/// ```java
///   "chest"   -> setBlock(pos, Blocks.CHEST.defaultBlockState()
///                                 .setValue(WATERLOGGED, fluid.is(WATER)), 2)
///                if (getBlockEntity(pos) instanceof ChestBlockEntity chest)
///                    chest.setLootTable(OCEAN_RUIN_WARM_ARCHAEOLOGY,
///                                       random.nextLong());
///   "drowned" -> EntityType.DROWNED.create(...), an entity, not a block
/// ```
///
/// Three things here are not guesses and each would be wrong the obvious way:
///
///   * **The chest is not rotated.** It is `defaultBlockState()`, so `facing`
///     stays north however the piece is turned. A server's chest at this ruin
///     reads `minecraft:chest[facing=north,type=single,waterlogged=true]` while
///     the piece is rotated 90 degrees, which settles it by observation rather
///     than by reading.
///   * **The marker's own block never appears.**
///     `BlockIgnoreProcessor.STRUCTURE_AND_AIR` strips `structure_block`, so
///     the marker steers this and writes nothing itself.
///   * **The `nextLong` is guarded by `getBlockEntity(pos) instanceof
///     ChestBlockEntity`** — and the guard **passes**, so vanilla does spend
///     it. This comment used to say the opposite, by analogy with the two
///     draws removed from `monster_room` on the grounds that a worldgen
///     ProtoChunk has no block entity. That was wrong in both places:
///     `WorldGenRegion.setBlock` stores a `{x,y,z,id:"DUMMY"}` tag for any
///     state with `hasBlockEntity()`, and `WorldGenRegion.getBlockEntity`
///     rebuilds the entity from it. `monster_room` now spends its two.
///
///     This one is **not** spent here, and that is a decision rather than an
///     omission: the draw comes off the per-start `RandomSource` that
///     `StructureStart.placeInChunk` hands each piece, and `handleDataMarker`
///     is the **only** call on that random anywhere in `OceanRuinPiece` —
///     `BlockRotProcessor` and the capped sand rule both fork positionally,
///     which is why this port models them without a stream at all. So the
///     draw's only consequence is the loot seed of a *later* chest in the
///     same start, and this port writes no loot. Nothing in blocks moves
///     either way. Spend it the moment a piece type that reads that stream
///     for a block decision is ported.
///
/// `water_at` answers whether the fluid at a world position is water, which is
/// the only thing the chest's state depends on.
pub fn marker_blocks<W>(
    template: &crate::structure::template::Template,
    plan: &Plan,
    y: i32,
    mut water_at: W,
) -> Vec<((i32, i32, i32), String)>
where
    // `FnMut`, not `Fn`: the caller in the pipeline answers this by reading
    // the generating world, which it holds mutably.
    W: FnMut((i32, i32, i32)) -> bool,
{
    use crate::structure::template::{transform, Mirror};

    let origin = (plan.origin.0, y, plan.origin.2);
    let mut out = Vec::new();
    for block in &template.blocks {
        let Some(marker) = block.marker.as_deref() else {
            continue;
        };
        let (dx, dy, dz) = transform(block.pos, Mirror::None, plan.rotation, (0, 0));
        let pos = (origin.0 + dx, origin.1 + dy, origin.2 + dz);
        match marker {
            "chest" => {
                let waterlogged = water_at(pos);
                out.push((
                    pos,
                    format!("minecraft:chest[facing=north,type=single,waterlogged={waterlogged}]"),
                ));
            }
            // An entity, spawned by the caller
            // (`worldgen_assembly::generate::place_ocean_ruin_in_chunk`),
            // which also writes the marker's water or air.
            "drowned" => {}
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;

    /// A flat world at a known floor, so `settle_height` has one right answer.
    struct Flat {
        floor: i32,
    }
    impl ColumnReader for Flat {
        fn block_at(&mut self, _x: i32, y: i32, _z: i32) -> String {
            if y > self.floor {
                "minecraft:water".to_string()
            } else {
                "minecraft:stone".to_string()
            }
        }
        fn min_y(&self) -> i32 {
            -64
        }
    }

    #[test]
    fn a_small_ruin_never_spends_the_cluster_coin() {
        // Large probability 0.0 makes `large` false for any float in [0,1),
        // since the test is `<=` and nextFloat never returns exactly 0 for
        // every seed — checked below by asserting on the plan, not the float.
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
        random.set_large_feature_seed(31_415_926, 67, 65);
        let plan = plan(&mut random, 67, 65, 0.0, 0.9);
        assert!(!plan.large);
        assert!(
            !plan.cluster_coin_spent,
            "the cluster nextFloat is guarded by `large` and must not be spent"
        );
        assert_eq!(plan.integrity, 0.8);
        assert!(WARM_RUINS.contains(&plan.template));
    }

    /// The guard is the whole point: forcing `large` must cost exactly one
    /// more draw than not forcing it.
    ///
    /// Counted with the same `draws` counter the decoration trace uses, rather
    /// than inferred by comparing streams — an inferred count is exactly the
    /// kind of thing that passes for the wrong reason.
    #[test]
    fn a_large_ruin_spends_one_more_draw_than_a_small_one() {
        let count = |large_probability: f32| {
            let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
            random.set_large_feature_seed(31_415_926, 67, 65);
            crate::rng::draws::arm();
            let plan = plan(&mut random, 67, 65, large_probability, 0.9);
            (plan.large, crate::rng::draws::take())
        };

        let (small_large, small) = count(0.0);
        let (big_large, big) = count(1.0);

        assert!(
            !small_large && big_large,
            "the coin was not actually forced: small={small_large} large={big_large}"
        );
        assert_eq!(small, 3, "rotation, the large coin, the template index");
        assert_eq!(big, 4, "and the cluster coin on top");
    }

    /// Flat ground under the whole footprint leaves the ruin where it is: the
    /// drop guard `floor - minSeen > 2` is not met.
    #[test]
    fn flat_ground_does_not_move_the_ruin() {
        let mut world = Flat { floor: 47 };
        let y = settle_height(&mut world, (1066, 48, 1040), (1072, 48, 1045));
        assert_eq!(y, 48, "nothing to sink into");
    }

    /// A deep flat floor well below the piece pulls it down to one above the
    /// solid surface — both guards are met.
    #[test]
    fn a_deep_floor_pulls_the_ruin_down_to_it() {
        let mut world = Flat { floor: 40 };
        let y = settle_height(&mut world, (1066, 48, 1040), (1072, 48, 1045));
        assert_eq!(y, 41, "minSeen + 1, one above the first solid block");
    }

    /// Water passes through and ice passes through, but a waterlogged block
    /// also passes through — its fluid state is water. Getting this wrong
    /// would stop the descent on the ruin's own stairs.
    #[test]
    fn the_descent_passes_through_water_ice_and_waterlogged_blocks() {
        assert!(passes_through("minecraft:water"));
        assert!(passes_through("minecraft:air"));
        assert!(passes_through("minecraft:blue_ice"));
        assert!(passes_through(
            "minecraft:stone_brick_stairs[waterlogged=true,facing=north]"
        ));
        assert!(!passes_through(
            "minecraft:stone_brick_stairs[waterlogged=false,facing=north]"
        ));
        assert!(!passes_through("minecraft:sand"));
    }
}
