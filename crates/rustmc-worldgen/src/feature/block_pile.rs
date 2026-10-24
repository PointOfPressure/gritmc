//! The block-pile feature — a squat heap of one block, thrown over the
//! ground in an ellipse that frays at the edge.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `net.minecraft.world.level.levelgen.feature.BlockPileFeature`,
//! `configurations.BlockPileConfiguration`.
//!
//! Five configured features are `block_pile`: `pile_hay`, `pile_melon`,
//! `pile_pumpkin`, `pile_snow`, `pile_ice`. This port exists for the first
//! two, which is how a savanna village plants its hay and its melons — they
//! are reached through `feature_pool_element`, not through biome decoration,
//! so nothing in the decoration step ever asked for them and the type sat
//! unimplemented while the village that needs it was already being placed.
//!
//! # The draw sequence is the whole of it
//!
//! ```text
//! if (origin.y < level.getMinY() + 5) return false        // no draw at all
//! rx = 2 + nextInt(2)
//! rz = 2 + nextInt(2)
//! for each pos in betweenClosed(origin-(rx,0,rz), origin+(rx,1,rz)):
//!     dx = origin.x - pos.x;  dz = origin.z - pos.z
//!     if (dx*dx + dz*dz <= nextFloat()*10 - nextFloat()*6)   tryPlace(pos)
//!     else if (nextFloat() < 0.031)                          tryPlace(pos)
//! ```
//!
//! **Both `nextFloat`s in the comparison are always taken, in that order**,
//! and the `0.031` arm is only reached when the first test fails — so the
//! per-cell cost is two draws or three, never one. The comparison is
//! evaluated in `f32` on the left and `f32` on the right: the squared
//! distance is an `int` cast to `float`, which is the cast vanilla writes and
//! not a widening to `f64`.
//!
//! `0.031` is a **double** literal compared against a widened `float`. Keeping
//! it that way matters at the boundary and costs nothing.
//!
//! # The iteration order is X fastest, then Y, then Z
//!
//! `BlockPos.betweenClosed` walks `index % width` for x, `(index / width) %
//! height` for y and `index / width / height` for z. Getting that order wrong
//! does not change *which* cells are visited, it changes which cell each draw
//! belongs to — so the pile would be the right size, in the right place, and
//! wrong block by block. There is no cheaper way to catch that than to write
//! the order down.
//!
//! # `tryPlaceBlock` reads the world before it writes
//!
//! ```text
//! if (!level.isEmptyBlock(pos)) return                       // read
//! below = level.getBlockState(pos.below())                   // read
//! if (below is dirt_path) { if (!nextBoolean()) return }      // A DRAW
//! else if (!below.isFaceSturdy(level, below, UP)) return      // read, no draw
//! level.setBlock(pos, stateProvider.getState(level, random, pos), UPDATE_NONE)
//! ```
//!
//! The `dirt_path` arm spends a draw and the sturdy arm does not, so **what
//! is under the pile changes the stream**, not merely the blocks. A village
//! plants its hay beside its streets and `dirt_path` is what a village street
//! is made of, which makes this the arm that actually fires here rather than
//! an exotic one.

use crate::rng::RandomSource;

use super::block_predicate::{NeedsBehaviour, PredicateWorld};
use super::state_provider::StateProvider;
use crate::tags::BlockTags;

/// `BlockPileConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockPileConfig {
    pub state_provider: StateProvider,
}

impl BlockPileConfig {
    /// # Errors
    /// If `state_provider` is missing or names an unimplemented provider.
    pub fn parse(
        config: &serde_json::Value,
        states: &dyn super::state_spec::DefaultStates,
    ) -> Result<Self, String> {
        Ok(Self {
            state_provider: StateProvider::parse(
                config
                    .get("state_provider")
                    .ok_or_else(|| "block_pile config has no `state_provider`".to_string())?,
                states,
            )?,
        })
    }

    #[must_use]
    pub fn is_supported(&self) -> bool {
        self.state_provider.is_supported()
    }
}

/// The world this feature reads and writes.
pub trait BlockPileWorld: PredicateWorld {
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str);

    /// `BlockGetter#isEmptyBlock` — `getBlockState(pos).isAir()`, true for
    /// `cave_air` and `void_air` as well as `air`.
    fn is_empty_block(&mut self, x: i32, y: i32, z: i32) -> Option<bool>;

    /// `LevelHeightAccessor#getMinY`.
    fn min_y(&self) -> i32;
}

/// `BlockPileFeature#place`.
///
/// Returns whether the feature ran at all — vanilla answers `false` only for
/// the height guard and `true` otherwise, **including when every cell was
/// rejected**, so a pile that placed nothing still counts as placed.
///
/// # Errors
/// If the world cannot answer whether a face is sturdy, or the provider needs
/// block behaviour the port does not have. Refusing is the point: guessing
/// `false` there would place a pile with holes in it that looked like a
/// vanilla pile with holes in it.
pub fn place<W: BlockPileWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &BlockPileConfig,
    random: &mut R,
    origin: (i32, i32, i32),
) -> Result<bool, NeedsBehaviour> {
    // Before any draw. A pile that failed this after drawing would leave the
    // stream two ahead of vanilla's for everything after it in the chunk.
    if origin.1 < world.min_y() + 5 {
        return Ok(false);
    }
    let rx = 2 + random.next_i32_bound(2);
    let rz = 2 + random.next_i32_bound(2);

    // `BlockPos.betweenClosed(origin.offset(-rx, 0, -rz), origin.offset(rx, 1,
    // rz))`, in that iterator's own order: x fastest, then y, then z.
    for z in origin.2 - rz..=origin.2 + rz {
        for y in origin.1..=origin.1 + 1 {
            for x in origin.0 - rx..=origin.0 + rx {
                let dx = origin.0 - x;
                let dz = origin.2 - z;
                #[allow(clippy::cast_precision_loss, reason = "vanilla's own (float) cast")]
                let d2 = (dx * dx + dz * dz) as f32;
                // Both draws, always, left to right.
                let a = random.next_f32();
                let b = random.next_f32();
                // Written as one condition rather than two arms calling the
                // same thing, but the short-circuit is the behaviour and not
                // a style: the third `nextFloat` is spent **only** where the
                // ellipse test failed, so a `||` that evaluated both sides
                // would shift every later draw in the pile.
                let hit = if d2 <= a * 10.0 - b * 6.0 {
                    true
                } else {
                    f64::from(random.next_f32()) < 0.031
                };
                if hit {
                    try_place_block(world, tags, config, random, (x, y, z))?;
                }
            }
        }
    }
    Ok(true)
}

/// `BlockPileFeature#tryPlaceBlock`, with `mayPlaceOn` folded in — they are
/// one branch and splitting them would hide that only one arm draws.
fn try_place_block<W: BlockPileWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &BlockPileConfig,
    random: &mut R,
    pos: (i32, i32, i32),
) -> Result<(), NeedsBehaviour> {
    let (x, y, z) = pos;
    if !world
        .is_empty_block(x, y, z)
        .ok_or_else(|| NeedsBehaviour("block_pile: isEmptyBlock".to_string()))?
    {
        return Ok(());
    }
    let below = world.block_at(x, y - 1, z);
    let below_block = below.split('[').next().unwrap_or(&below);
    if below_block == "minecraft:dirt_path" {
        // The draw is spent whichever way it goes.
        if !random.next_bool() {
            return Ok(());
        }
    } else if !world
        // `Direction.UP` is ordinal 1 — `Direction.values()` groups the axes
        // Y, Z, X, so UP is the second entry and not the fifth.
        .face_is_sturdy(x, y - 1, z, 1)
        .ok_or_else(|| NeedsBehaviour("block_pile: isFaceSturdy(UP)".to_string()))?
    {
        return Ok(());
    }
    let state = config.state_provider.state(world, tags, random, pos)?;
    world.set_block(x, y, z, &state);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;
    use std::collections::BTreeMap;

    /// A floor of one block kind at `y = 63`, air above it, and a counter on
    /// every draw. The counter is the point of these tests: the blocks a pile
    /// lands on are checkable by eye, and the draw count is not.
    struct Ground {
        blocks: BTreeMap<(i32, i32, i32), String>,
        floor: String,
    }

    /// The origin every test that needs a floor places its pile at.
    const ORIGIN: (i32, i32, i32) = (0, 70, 0);

    /// The floor sits one below the origin every test places at. It was 63
    /// while the tests placed at 70, so `tryPlaceBlock` read air below every
    /// cell and returned before the arm that distinguishes a `dirt_path`
    /// floor from a sturdy one — which is exactly the branch two of these
    /// tests exist to pin, and both of them failed on it.
    const FLOOR_Y: i32 = ORIGIN.1 - 1;

    impl PredicateWorld for Ground {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            if y == FLOOR_Y {
                return self.floor.clone();
            }
            self.blocks
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
        fn face_is_sturdy(&mut self, _: i32, y: i32, _: i32, d: usize) -> Option<bool> {
            Some(y == FLOOR_Y && d == 1 && self.floor != "minecraft:air")
        }
    }

    impl BlockPileWorld for Ground {
        fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
            self.blocks.insert((x, y, z), name.to_string());
        }
        fn is_empty_block(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:air")
        }
        fn min_y(&self) -> i32 {
            -64
        }
    }

    /// Wraps a random and counts every draw that reaches it, and separately
    /// the `nextBoolean`s — because **the total is not comparable between two
    /// runs whose arms differ**. The moment the `dirt_path` arm spends its
    /// extra boolean the two streams diverge, so every later `nextFloat` in
    /// the pile differs, so a different set of cells takes the three-draw
    /// branch. Seed 7 lands both runs on 290 draws with 2 blocks placed
    /// against 9: the totals agree by coincidence and say nothing.
    struct Counting<R> {
        inner: R,
        draws: usize,
        bools: usize,
    }

    impl<R: RandomSource> RandomSource for Counting<R> {
        fn set_seed(&mut self, seed: i64) {
            self.inner.set_seed(seed);
        }
        fn next_i32(&mut self) -> i32 {
            self.draws += 1;
            self.inner.next_i32()
        }
        fn next_i32_bound(&mut self, bound: i32) -> i32 {
            self.draws += 1;
            self.inner.next_i32_bound(bound)
        }
        fn next_i64(&mut self) -> i64 {
            self.draws += 1;
            self.inner.next_i64()
        }
        fn next_bool(&mut self) -> bool {
            self.draws += 1;
            self.bools += 1;
            self.inner.next_bool()
        }
        fn next_f32(&mut self) -> f32 {
            self.draws += 1;
            self.inner.next_f32()
        }
        fn next_f64(&mut self) -> f64 {
            self.draws += 1;
            self.inner.next_f64()
        }
        fn next_gaussian(&mut self) -> f64 {
            self.draws += 1;
            self.inner.next_gaussian()
        }
    }

    fn random(seed: i64) -> Counting<WorldgenRandom<LegacyRandomSource>> {
        Counting {
            inner: WorldgenRandom::new(LegacyRandomSource::new(seed)),
            draws: 0,
            bools: 0,
        }
    }

    fn tags() -> BlockTags {
        BlockTags::from_entries(BTreeMap::new()).expect("tags")
    }

    fn states() -> super::super::state_spec::DefaultStateTable {
        super::super::state_spec::DefaultStateTable::from_pairs(&[(
            "minecraft:hay_block",
            "minecraft:hay_block[axis=y]",
        )])
    }

    fn hay() -> BlockPileConfig {
        BlockPileConfig::parse(
            &serde_json::from_str(
                r#"{"state_provider":{"type":"minecraft:rotated_block_provider",
                    "state":{"Name":"minecraft:hay_block",
                             "Properties":{"axis":"y"}}}}"#,
            )
            .expect("json"),
            &states(),
        )
        .expect("parse")
    }

    fn world(floor: &str) -> Ground {
        Ground {
            blocks: BTreeMap::new(),
            floor: floor.to_string(),
        }
    }

    /// **The height guard runs before the first draw.** A pile that drew and
    /// then bailed would leave the shared structure stream two ahead of
    /// vanilla's for every piece after it in the chunk — which is invisible
    /// in this feature's own blocks and fatal to the next one's.
    #[test]
    fn the_height_guard_spends_nothing() {
        let mut w = world("minecraft:grass_block");
        let mut r = random(1);
        // `min_y` is -64, so -60 is the last y that is still too low.
        assert_eq!(
            place(&mut w, &tags(), &hay(), &mut r, (0, -60, 0)),
            Ok(false)
        );
        assert_eq!(r.draws, 0, "the guard is before the draws");
        assert!(w.blocks.is_empty());
    }

    /// Two radius draws, then two `nextFloat` per cell whatever happens, and
    /// a third only where the ellipse test failed. The count is therefore
    /// bounded below by `2 + 2N` and above by `2 + 3N` **before** any
    /// per-block draw, and the extent N is one of exactly three values.
    #[test]
    fn the_draw_count_brackets_the_cell_count() {
        for seed in 0..24_i64 {
            let mut w = world("minecraft:air");
            let mut r = random(seed);
            // An air floor is not sturdy, so nothing is ever placed and the
            // only draws are the radii and the per-cell tests. That isolates
            // the loop from the provider.
            assert_eq!(place(&mut w, &tags(), &hay(), &mut r, ORIGIN), Ok(true));
            assert!(w.blocks.is_empty(), "nothing can stand on air");
            let n = [50_usize, 70, 98];
            assert!(
                n.iter()
                    .any(|n| r.draws >= 2 + 2 * n && r.draws <= 2 + 3 * n),
                "seed {seed}: {} draws matches no admissible cell count",
                r.draws
            );
        }
    }

    /// A `dirt_path` floor spends a `nextBoolean` per candidate cell and a
    /// sturdy non-path floor spends none. Same seed, same geometry, and the
    /// difference is the arm — which is the branch that would be easiest to
    /// collapse and is worth a whole extra draw per cell.
    #[test]
    fn only_the_dirt_path_arm_draws() {
        let mut path = world("minecraft:dirt_path");
        let mut rp = random(7);
        assert_eq!(place(&mut path, &tags(), &hay(), &mut rp, ORIGIN), Ok(true));

        let mut grass = world("minecraft:grass_block");
        let mut rg = random(7);
        assert_eq!(
            place(&mut grass, &tags(), &hay(), &mut rg, ORIGIN),
            Ok(true)
        );

        assert_eq!(
            rg.bools, 0,
            "the sturdy arm reads isFaceSturdy and must not draw"
        );
        assert!(
            rp.bools > 0,
            "the dirt_path arm spends one nextBoolean per candidate cell"
        );
        // The pile is the same size either way; what differs is how much of
        // it survives the arm.
        assert!(
            path.blocks.len() < grass.blocks.len(),
            "half the dirt_path cells lose their coin flip: {} vs {}",
            path.blocks.len(),
            grass.blocks.len()
        );
    }

    /// The provider re-rolls the axis per block and **discards the config's
    /// own `axis=y`**. A port that honoured the written state would place
    /// every hay block `axis=y`; vanilla's spreads over all three.
    #[test]
    fn the_rotated_provider_ignores_the_configured_axis() {
        let mut seen = std::collections::BTreeSet::new();
        for seed in 0..40_i64 {
            let mut w = world("minecraft:grass_block");
            let mut r = random(seed);
            assert_eq!(place(&mut w, &tags(), &hay(), &mut r, ORIGIN), Ok(true));
            for state in w.blocks.values() {
                seen.insert(state.clone());
            }
        }
        assert_eq!(
            seen,
            [
                "minecraft:hay_block[axis=x]",
                "minecraft:hay_block[axis=y]",
                "minecraft:hay_block[axis=z]",
            ]
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<std::collections::BTreeSet<_>>(),
            "all three axes, or the provider is not re-rolling"
        );
    }

    /// The pile is two layers and no more, and it never reaches past the
    /// radius the two draws chose. Bounds rather than an exact shape: the
    /// shape is the seed's business, the envelope is the code's.
    #[test]
    fn the_pile_stays_inside_its_own_envelope() {
        for seed in 0..16_i64 {
            let mut w = world("minecraft:grass_block");
            let mut r = random(seed);
            assert_eq!(place(&mut w, &tags(), &hay(), &mut r, ORIGIN), Ok(true));
            for (x, y, z) in w.blocks.keys() {
                assert!(
                    (70..=71).contains(y),
                    "seed {seed}: y {y} outside the two layers"
                );
                assert!(
                    x.abs() <= 3 && z.abs() <= 3,
                    "seed {seed}: ({x},{z}) outside 2+nextInt(2)"
                );
            }
        }
    }
}
