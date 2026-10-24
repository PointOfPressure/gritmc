//! The surface stage — grass on the dirt, sand in the desert, bands in the
//! badlands.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `SurfaceSystem#buildSurface`, `NoiseBasedChunkGenerator#
//! buildSurface`.
//!
//! Another thin adapter: [`crate::surface::system::build_column`] is the rule
//! walk and it is gated against dumped palettes. What this adds is the loop
//! over a chunk's 256 columns and the wiring of the oracle the rules ask.
//!
//! # It walks from `WORLD_SURFACE_WG`, the fossil
//!
//! The top of the walk is the noise stage's heightmap and not a live one:
//! surfaces run before any feature exists, so the two agree at this point in
//! the pipeline — but they will not agree later, and a stage that reached for
//! the live map would keep working until the day it ran twice.
//!
//! # `is_default` is the generator's block, not "anything solid"
//!
//! The rules may only replace the generator's default block — stone in the
//! overworld. That is what stops a surface rule eating an ore vein the noise
//! stage placed, and it is why the check is a closure over the caller's
//! default rather than `!isAir`.

use crate::canvas::Canvas;
use crate::surface::context::SurfaceContext;
use crate::surface::eval::SurfaceOracle;
use crate::surface::rule::{BlockState, Rule};
use crate::surface::system::{build_column, BlockColumn};

/// `getSurfaceDepth`, which is the surface system's rather than the rule
/// evaluator's.
///
/// A separate trait because `SurfaceOracle` is what a *rule* asks and this is
/// what the *walk* asks before the rules see the column. Widening
/// `SurfaceOracle` to carry it would make every test stub implement a method
/// no rule uses.
pub trait SurfaceDepths {
    fn surface_depth_at(&self, x: i32, z: i32) -> i32;

    /// `SurfaceSystem#buildSurface`'s frozen-ocean pass, run after the walk.
    ///
    /// Defaulted to "no icebergs" so a test fixture that stands up a column
    /// of its own does not have to model an ocean it never generates. The
    /// real oracle overrides it; if a fixture ever needs bergs, the default
    /// returning zero is visible in the block count rather than silent.
    fn frozen_ocean_extension(
        &self,
        _column: &mut dyn BlockColumn,
        _x: i32,
        _z: i32,
        _start_y: i32,
        _min_surface_level: i32,
        _sea_level: i32,
    ) -> usize {
        0
    }
}

impl SurfaceDepths for crate::surface::world::WorldSurfaceOracle<'_> {
    fn surface_depth_at(&self, x: i32, z: i32) -> i32 {
        Self::surface_depth_at(self, x, z)
    }

    fn frozen_ocean_extension(
        &self,
        column: &mut dyn BlockColumn,
        x: i32,
        z: i32,
        start_y: i32,
        min_surface_level: i32,
        sea_level: i32,
    ) -> usize {
        Self::frozen_ocean_extension(self, column, x, z, start_y, min_surface_level, sea_level)
    }
}

/// What the surface walk needs of whatever it writes into.
///
/// Both [`Canvas`] and [`crate::canvas::ChunkWriter`] satisfy it, with the
/// same semantics down to dropping a write outside the target. The stage is
/// written against the trait so that one chunk's walk can run on a worker
/// holding **only that chunk**, which is the whole parallel seam: every
/// method here is chunk-local for a walk that stays inside its own 16x16.
pub trait SurfaceTarget {
    fn min_y(&self) -> i32;
    fn height(&self) -> i32;
    /// The **inclusive** top, `getMaxY()`.
    fn max_y(&self) -> i32 {
        self.min_y() + self.height() - 1
    }
    fn world_surface_wg(&self, x: i32, z: i32) -> i32;
    fn block_at(&self, x: i32, y: i32, z: i32) -> &str;
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str);

    /// `ChunkAccess#markPosForPostProcessing`.
    ///
    /// `SurfaceSystem`'s anonymous `BlockColumn` marks **every fluid it
    /// writes** — `setBlock` is
    /// `if (isInsideBuildHeight(y)) { chunk.setBlockState(pos, state);
    /// if (!state.getFluidState().isEmpty()) chunk.markPosForPostProcessing(pos); }`
    /// — and that is one of the two largest sources of marks in a vanilla
    /// chunk. See `docs/ORACLE.md` §23.
    fn mark_post_processing(&mut self, x: i32, y: i32, z: i32);
}

impl SurfaceTarget for Canvas {
    fn min_y(&self) -> i32 {
        Self::min_y(self)
    }
    fn height(&self) -> i32 {
        Self::height(self)
    }
    fn max_y(&self) -> i32 {
        Self::max_y(self)
    }
    fn world_surface_wg(&self, x: i32, z: i32) -> i32 {
        Self::world_surface_wg(self, x, z)
    }
    fn block_at(&self, x: i32, y: i32, z: i32) -> &str {
        Self::block_at(self, x, y, z)
    }
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        Self::set_block(self, x, y, z, state);
    }
    fn mark_post_processing(&mut self, x: i32, y: i32, z: i32) {
        Self::mark_post_processing(self, x, y, z);
    }
}

impl SurfaceTarget for crate::canvas::ChunkWriter<'_> {
    fn min_y(&self) -> i32 {
        Self::min_y(self)
    }
    fn height(&self) -> i32 {
        Self::height(self)
    }
    fn world_surface_wg(&self, x: i32, z: i32) -> i32 {
        Self::world_surface_wg(self, x, z)
    }
    fn block_at(&self, x: i32, y: i32, z: i32) -> &str {
        Self::block_at(self, x, y, z)
    }
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        Self::set_block(self, x, y, z, state);
    }
    fn mark_post_processing(&mut self, x: i32, y: i32, z: i32) {
        Self::mark_post_processing(self, x, y, z);
    }
}

/// A column of the target, presented the way the rule walk wants it.
///
/// Holds the target by mutable reference for one column: `build_column` reads
/// and writes the same y range, so a copy would have to be written back and
/// the write-back is where an off-by-one lives.
struct CanvasColumn<'a, T: SurfaceTarget> {
    canvas: &'a mut T,
    x: i32,
    z: i32,
}

impl<T: SurfaceTarget> BlockColumn for CanvasColumn<'_, T> {
    fn get(&self, y: i32) -> Option<&str> {
        if y < self.canvas.min_y() || y > self.canvas.max_y() {
            return None;
        }
        // The canvas owns the string; the walk only ever compares it. This
        // used to hand back a fresh `String` per block read, which the walk
        // does `(top_y - min_y)` times per column plus a lookahead scan per
        // stone run.
        Some(self.canvas.block_at(self.x, y, self.z))
    }

    fn set(&mut self, y: i32, state: BlockState) {
        // The build-height guard is `BlockColumn`'s, not the rule walk's:
        // vanilla's `setBlock` drops the write entirely outside it, and so
        // must the mark that follows.
        if y < self.canvas.min_y() || y > self.canvas.max_y() {
            return;
        }
        let fluid = self.is_fluid(&state.0);
        self.canvas.set_block(self.x, y, self.z, &state.0);
        if fluid {
            self.canvas.mark_post_processing(self.x, y, self.z);
        }
    }

    fn is_air(&self, state: &str) -> bool {
        // `isAir` — and `cave_air` is air, which a name check misses. The
        // surface stage runs before carvers, so nothing has placed cave air
        // yet; it is here because the same column type will be reused when
        // something does.
        state == "minecraft:air" || state == "minecraft:cave_air"
    }

    fn is_fluid(&self, state: &str) -> bool {
        // `!getFluidState().isEmpty()`. At this stage the only fluids on the
        // canvas are what the aquifer placed, which is water and lava in
        // their block forms.
        //
        // A byte scan rather than `split_once('[')`: the `char` pattern goes
        // through `CharSearcher` and UTF-8 decoding, and this is called for
        // every block of every column. `CharSearcher::next_match` was 1.5% of
        // the whole pipeline.
        let cut = state
            .as_bytes()
            .iter()
            .position(|b| *b == b'[')
            .unwrap_or(state.len());
        let plain = &state[..cut];
        plain == "minecraft:water" || plain == "minecraft:lava"
    }
}

/// The **chunk generator's** vertical extent, as distinct from the level's.
///
/// `NoiseSettings.min_y` / `NoiseSettings.height`, i.e. `ChunkGenerator`'s
/// `getMinY()` / `getGenDepth()`. Its own type so a caller cannot pass the
/// canvas's numbers by habit: they are the same in the overworld and
/// different in the nether, which makes this the sort of mistake that is
/// invisible until a second dimension exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GenExtent {
    pub min_y: i32,
    pub height: i32,
}

/// Run the surface rules over one chunk.
///
/// `min_surface_level` is the blended preliminary surface for a column, which
/// the caller computes because it needs the density router; `oracle` is built
/// per chunk by the caller for the same reason.
#[allow(
    clippy::too_many_arguments,
    reason = "\
    the surface system's own inputs: the world, the rules, the oracle, the
    sea level, the default block, the blended preliminary surface, and which
    chunk. Bundling them would only move the list."
)]
pub fn build_chunk<T: SurfaceTarget, O: SurfaceOracle + SurfaceDepths>(
    canvas: &mut T,
    rule: &Rule,
    oracle: &mut O,
    sea_level: i32,
    default_block: &str,
    generator: GenExtent,
    min_surface_level: &dyn Fn(i32, i32, i32) -> i32,
    chunk_x: i32,
    chunk_z: i32,
) {
    // `new WorldGenerationContext(generator, level)`:
    //   minY   = max(level.getMinY(),   generator.getMinY())
    //   height = min(level.getHeight(), generator.getGenDepth())
    // The canvas is the *level*; `generator` is the noise settings' own
    // extent. They are the same -64/384 in the overworld and they are NOT in
    // the nether, where the level stores 256 blocks and the generator makes
    // 128 — so `below_top(5)` is y 122, not y 250. Resolving a vertical
    // anchor against the wrong one silently disables the bedrock roof and
    // lets the floor's surface rule paint the ceiling.
    let min_y = canvas.min_y().max(generator.min_y);
    let height = canvas.height().min(generator.height);
    let default_block = default_block.to_string();
    let is_default = move |s: &str| s == default_block;

    for local_z in 0..16i32 {
        for local_x in 0..16i32 {
            let x = chunk_x * 16 + local_x;
            let z = chunk_z * 16 + local_z;
            let top = canvas.world_surface_wg(x, z);

            let depth = oracle.surface_depth_at(x, z);
            let mut ctx = SurfaceContext::new(min_y, height, sea_level);
            ctx.update_xz(x, z, depth);
            let blended = min_surface_level(x, z, depth);
            let _ = ctx.min_surface_level(|_, _| blended);

            let mut column = CanvasColumn { canvas, x, z };
            build_column(rule, &mut ctx, oracle, &mut column, top, min_y, &is_default);
            // The frozen-ocean pass runs *after* the walk and writes into the
            // water column, which the walk never offers to a rule. Vanilla
            // does the same, at the same point, with the same `top`.
            oracle.frozen_ocean_extension(&mut column, x, z, top, blended, sea_level);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{Area, Pipeline};
    use crate::surface::eval::Sampling;

    /// An oracle that answers plainly, so the test is about the *stage* — the
    /// column loop, the fossil it walks from, and what it is allowed to
    /// replace. The rules themselves are gated against dumped palettes.
    struct Plain {
        depth: i32,
    }

    impl SurfaceOracle for Plain {
        fn noise(&mut self, _: &str, _: i32, _: i32, _: i32, _: Sampling) -> f64 {
            0.0
        }
        fn surface_secondary(&mut self, _: i32, _: i32) -> f64 {
            0.0
        }
        fn vertical_gradient(&mut self, _: &str, _: i32, _: i32, _: i32, _: i32, _: i32) -> bool {
            false
        }
        fn steep(&mut self, _: i32, _: i32) -> bool {
            false
        }
        fn hole(&mut self, _: i32, _: i32) -> bool {
            false
        }
        fn temperature(&mut self, _: i32, _: i32, _: i32) -> bool {
            false
        }
        fn min_surface_level(&mut self, _: i32, _: i32) -> i32 {
            0
        }
        fn biome(&mut self, _: i32, _: i32, _: i32) -> crate::surface::BiomeId {
            0
        }
        fn biome_name(&self, _: crate::surface::BiomeId) -> String {
            "minecraft:plains".to_string()
        }
        fn bandlands(&mut self, _: i32, _: i32, _: i32) -> Option<BlockState> {
            Some(BlockState("minecraft:terracotta".to_string()))
        }
    }

    impl SurfaceDepths for Plain {
        fn surface_depth_at(&self, _: i32, _: i32) -> i32 {
            self.depth
        }
    }

    /// A flat stone world with a rule that puts grass on top: the stage walks
    /// from the fossil heightmap and replaces the default block there.
    #[test]
    fn the_stage_walks_from_the_wg_fossil() {
        let mut pipeline = Pipeline::new(Area::square(0, 0, 0), -64, 384, "minecraft:air");
        {
            let canvas = pipeline.canvas_mut();
            for z in 0..16 {
                for x in 0..16 {
                    for y in -64..=70 {
                        canvas.set_block(x, y, z, "minecraft:stone");
                    }
                }
            }
            canvas.set_wg_heightmaps(0, 0, &[71; 256], &[71; 256]);
        }

        let rule = Rule::Block(BlockState("minecraft:grass_block[snowy=false]".to_string()));
        let mut oracle = Plain { depth: 1 };
        build_chunk(
            pipeline.canvas_mut(),
            &rule,
            &mut oracle,
            63,
            "minecraft:stone",
            GenExtent {
                min_y: -64,
                height: 384,
            },
            &|_, _, _| 0,
            0,
            0,
        );

        let canvas = pipeline.canvas();
        assert_eq!(
            canvas.block_at(3, 70, 3),
            "minecraft:grass_block[snowy=false]",
            "the top of the fossil column is replaced"
        );
        assert_eq!(
            canvas.block_at(3, 71, 3),
            "minecraft:air",
            "and nothing is written above it"
        );
    }

    /// Only the generator's default block may be replaced — an ore vein the
    /// noise stage placed survives the surface rules.
    #[test]
    fn a_rule_may_not_eat_a_block_that_is_not_the_default() {
        let mut pipeline = Pipeline::new(Area::square(0, 0, 0), -64, 384, "minecraft:air");
        {
            let canvas = pipeline.canvas_mut();
            for y in -64..=70 {
                canvas.set_block(0, y, 0, "minecraft:stone");
            }
            canvas.set_block(0, 70, 0, "minecraft:copper_ore");
            canvas.set_wg_heightmaps(0, 0, &[71; 256], &[71; 256]);
        }

        let rule = Rule::Block(BlockState("minecraft:grass_block[snowy=false]".to_string()));
        let mut oracle = Plain { depth: 1 };
        build_chunk(
            pipeline.canvas_mut(),
            &rule,
            &mut oracle,
            63,
            "minecraft:stone",
            GenExtent {
                min_y: -64,
                height: 384,
            },
            &|_, _, _| 0,
            0,
            0,
        );
        assert_eq!(
            pipeline.canvas().block_at(0, 70, 0),
            "minecraft:copper_ore",
            "the vein is not the default block, so the rule may not have it"
        );
    }
}
