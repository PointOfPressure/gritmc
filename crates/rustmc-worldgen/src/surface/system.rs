//! The column walk that applies surface rules to a generated chunk.
//!
//! Reference: `SurfaceSystem.buildSurface` and `getSurfaceDepth`.
//!
//! # What the walk is actually tracking
//!
//! It descends each column from one above the world surface to the world
//! floor, maintaining three running values the rules read:
//!
//! - **`stone_depth_above`** — how many solid blocks have been passed since
//!   the last air or fluid. Reset by air, *not* by fluid, which is why a
//!   lake bed still gets a proper surface.
//! - **`stone_depth_below`** — distance to the next non-stone block going
//!   down, found by a lookahead and reused until the walk passes it. That is
//!   what lets a cave ceiling get a different material from its floor.
//! - **`water_height`** — one above the topmost fluid in the current run,
//!   or [`NO_WATER`]. Reset by air.
//!
//! # Only the default block is replaced
//!
//! `if (old == this.defaultBlock)`. Surface rules rewrite stone; they never
//! touch water, air, or the ore veins the noise stage placed. Dropping that
//! check turns every vein into grass at the surface and every aquifer into
//! sand.

use super::context::{SurfaceContext, NO_WATER};
use super::eval::{apply, SurfaceOracle};
use super::rule::{BlockState, Rule};
use crate::level::WAY_BELOW_MIN_Y;

/// The column a walk reads and writes.
///
/// A trait so the walk can run against a `FilledChunk`, a protochunk or a
/// test fixture without any of them needing to know about each other.
///
/// # `get` borrows
///
/// It returned an owned [`BlockState`] and the walk reads **every block of
/// every column** — `(top_y - min_y)` of them, plus a full lookahead scan per
/// stone run. On the r=16 square that was tens of millions of `String`
/// allocations whose only use was three comparisons against literals. The
/// canvas already stores the state as a string it owns, so the walk can look
/// at it in place. Writes are rare enough that `set` still takes an owned
/// state.
pub trait BlockColumn {
    /// The block at this y, or `None` above/below the build height.
    fn get(&self, y: i32) -> Option<&str>;
    fn set(&mut self, y: i32, state: BlockState);
    /// Whether a state stops the surface run — vanilla's `isAir`.
    fn is_air(&self, state: &str) -> bool;
    /// Whether a state is a fluid, i.e. `!getFluidState().isEmpty()`.
    fn is_fluid(&self, state: &str) -> bool;
}

/// `isStone` — not air and not a fluid.
fn is_stone(column: &impl BlockColumn, state: &str) -> bool {
    !column.is_air(state) && !column.is_fluid(state)
}

/// `getSurfaceDepth` — how thick the surface layer is at a column.
///
/// `(int)(noise * 2.75 + 3.0 + random.nextDouble() * 0.25)`, truncated. The
/// random term is a quarter of a block: it is not there to change the depth
/// so much as to break ties, so a flat plain does not band into visible
/// stripes where the noise sits exactly on an integer.
#[must_use]
pub fn surface_depth(surface_noise: f64, random_unit: f64) -> i32 {
    (surface_noise * 2.75 + 3.0 + random_unit * 0.25) as i32
}

/// Walk one column, applying `rule` to every default block.
///
/// `top_y` is the world-surface heightmap value **plus one** — vanilla
/// starts one above the highest non-air block so the topmost solid block is
/// itself visited.
///
/// `is_default` decides which blocks the rules may replace; see the module
/// docs on why that check exists.
///
/// The column's x and z are not parameters: `ctx` already carries them from
/// `update_xz`, and passing them twice invites the two to disagree.
pub fn build_column(
    rule: &Rule,
    ctx: &mut SurfaceContext,
    oracle: &mut impl SurfaceOracle,
    column: &mut impl BlockColumn,
    top_y: i32,
    bottom_y: i32,
    is_default: &impl Fn(&str) -> bool,
) {
    let mut stone_above = 0i32;
    let mut water_height = NO_WATER;
    // `Integer.MAX_VALUE` means "no lookahead done yet"; the first solid
    // block triggers one.
    let mut next_ceiling_stone_y = i32::MAX;

    for y in (bottom_y..=top_y).rev() {
        let Some(old) = column.get(y) else { continue };

        if column.is_air(old) {
            // Air ends the run entirely — both the depth and any water.
            stone_above = 0;
            water_height = NO_WATER;
            continue;
        }

        if column.is_fluid(old) {
            // Fluid does NOT reset the stone depth: a lake bed is still a
            // surface, and resetting here would leave it bare stone. It
            // only records where the water starts.
            if water_height == NO_WATER {
                water_height = y + 1;
            }
            continue;
        }

        // Solid. Find the bottom of this stone run once and reuse it until
        // the walk passes it — this is the lookahead that makes cave
        // ceilings distinguishable from floors.
        if next_ceiling_stone_y >= y {
            next_ceiling_stone_y = WAY_BELOW_MIN_Y;
            // Scans to `bottom_y - 1`, one past the floor, so a column that
            // is solid all the way down still terminates with a defined
            // answer rather than leaving the sentinel in place.
            for lookahead in (bottom_y - 1..y).rev() {
                match column.get(lookahead) {
                    Some(state) if is_stone(column, state) => {}
                    _ => {
                        next_ceiling_stone_y = lookahead + 1;
                        break;
                    }
                }
            }
        }

        stone_above += 1;
        let stone_below = y - next_ceiling_stone_y + 1;
        ctx.update_y(stone_above, stone_below, water_height, y);

        if is_default(old) {
            if let Some(state) = apply(rule, ctx, oracle) {
                column.set(y, state);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::eval::Sampling;
    use crate::surface::rule::Condition;

    /// A column of named blocks, indexed by y.
    struct TestColumn {
        min_y: i32,
        blocks: Vec<BlockState>,
    }

    impl TestColumn {
        fn new(min_y: i32, names: &[&str]) -> Self {
            Self {
                min_y,
                blocks: names.iter().map(|n| BlockState((*n).into())).collect(),
            }
        }

        fn at(&self, y: i32) -> &str {
            &self.blocks[(y - self.min_y) as usize].0
        }
    }

    impl BlockColumn for TestColumn {
        fn get(&self, y: i32) -> Option<&str> {
            usize::try_from(y - self.min_y)
                .ok()
                .and_then(|i| self.blocks.get(i))
                .map(|s| s.0.as_str())
        }
        fn set(&mut self, y: i32, state: BlockState) {
            if let Ok(i) = usize::try_from(y - self.min_y) {
                if i < self.blocks.len() {
                    self.blocks[i] = state;
                }
            }
        }
        fn is_air(&self, state: &str) -> bool {
            state == "air"
        }
        fn is_fluid(&self, state: &str) -> bool {
            state == "water" || state == "lava"
        }
    }

    /// Refuses every condition that needs a world, so these tests can only
    /// exercise the walk's own bookkeeping — if one of them ever reaches a
    /// noise it will say so rather than silently take a default.
    struct Recorder;

    impl SurfaceOracle for Recorder {
        fn noise(&mut self, _: &str, _: i32, _: i32, _: i32, _: Sampling) -> f64 {
            unreachable!("no noise conditions in these tests")
        }
        fn surface_secondary(&mut self, _: i32, _: i32) -> f64 {
            unreachable!()
        }
        fn vertical_gradient(&mut self, _: &str, _: i32, _: i32, _: i32, _: i32, _: i32) -> bool {
            unreachable!()
        }
        fn steep(&mut self, _: i32, _: i32) -> bool {
            unreachable!()
        }
        fn hole(&mut self, _: i32, _: i32) -> bool {
            unreachable!()
        }
        fn temperature(&mut self, _: i32, _: i32, _: i32) -> bool {
            unreachable!()
        }
        fn min_surface_level(&mut self, _: i32, _: i32) -> i32 {
            unreachable!()
        }
        fn biome(&mut self, _: i32, _: i32, _: i32) -> crate::surface::BiomeId {
            unreachable!()
        }
        fn bandlands(&mut self, _: i32, _: i32, _: i32) -> Option<BlockState> {
            unreachable!()
        }
    }

    fn grass_rule() -> Rule {
        Rule::Condition {
            if_true: Condition::StoneDepth {
                offset: 0,
                add_surface_depth: false,
                secondary_depth_range: 0,
                surface_type: super::super::rule::StoneDepthSurface::Floor,
            },
            then_run: Box::new(Rule::Block(BlockState("grass".into()))),
        }
    }

    fn run(column: &mut TestColumn, rule: &Rule) {
        let min_y = column.min_y;
        let top = min_y + column.blocks.len() as i32 - 1;
        let mut ctx = SurfaceContext::new(min_y, column.blocks.len() as i32, 63);
        ctx.update_xz(0, 0, 1);
        let mut oracle = Recorder;
        build_column(
            rule,
            &mut ctx,
            &mut oracle,
            column,
            top,
            min_y,
            &|s: &str| s == "stone",
        );
    }

    #[test]
    fn only_the_default_block_is_replaced() {
        // Ore veins and fluids the noise stage placed must survive. Without
        // the check, every exposed vein becomes grass.
        let mut column = TestColumn::new(0, &["stone", "copper_ore", "stone", "air"]);
        run(&mut column, &grass_rule());
        assert_eq!(column.at(2), "grass", "top stone became surface");
        assert_eq!(column.at(1), "copper_ore", "the vein survived");
    }

    #[test]
    fn a_lake_bed_still_gets_a_surface() {
        // air, water, water, stone, stone. Fluid does not reset the depth
        // counter, so the first stone under the water is still depth 1 and
        // gets its surface. Resetting on fluid would leave every lake bed
        // as bare stone.
        let mut column = TestColumn::new(0, &["stone", "stone", "water", "water", "air"]);
        run(&mut column, &grass_rule());
        assert_eq!(column.at(1), "grass", "the lake bed is a surface");
        assert_eq!(column.at(0), "stone", "and only one block deep");
    }

    #[test]
    fn stone_above_water_keeps_counting_through_it() {
        // The surprising half of the same rule, pinned because it looked
        // like a bug first: with an overhang — stone, water, stone, air —
        // the stone *under* the water continues the count started by the
        // stone above it, rather than starting fresh.
        //
        // My first version of the test above asserted the opposite and
        // failed. Vanilla's fluid branch only records `waterHeight`; it
        // never touches `stoneAboveDepth`, and that is the whole difference
        // between a lake bed (depth 1, gets grass) and the underside of an
        // overhang (depth 2, does not).
        let mut column = TestColumn::new(0, &["stone", "stone", "water", "stone", "air"]);
        run(&mut column, &grass_rule());
        assert_eq!(column.at(3), "grass", "the overhang's top");
        assert_eq!(
            column.at(1),
            "stone",
            "under the overhang the count continued, so no second surface"
        );
    }

    #[test]
    fn a_second_run_below_air_starts_its_depth_again() {
        // An overhang: solid, air, solid. Both tops get a surface, because
        // air genuinely ends the run.
        let mut column = TestColumn::new(0, &["stone", "stone", "air", "stone", "stone", "air"]);
        run(&mut column, &grass_rule());
        assert_eq!(column.at(4), "grass", "top of the upper run");
        assert_eq!(column.at(3), "stone");
        assert_eq!(column.at(1), "grass", "top of the lower run");
        assert_eq!(column.at(0), "stone");
    }

    #[test]
    fn the_walk_starts_one_above_the_surface() {
        // Vanilla starts at `heightmap + 1` so the topmost solid block is
        // itself visited. Starting *at* the heightmap skips it, and every
        // surface in the world is one block too low.
        let mut column = TestColumn::new(0, &["stone", "stone"]);
        run(&mut column, &grass_rule());
        assert_eq!(column.at(1), "grass", "the highest block was visited");
    }

    #[test]
    fn surface_depth_truncates_toward_zero() {
        // `(int)(noise * 2.75 + 3.0 + random * 0.25)`. The quarter-block
        // random term exists to break ties so a flat plain does not band
        // into stripes where the noise lands on an integer.
        assert_eq!(surface_depth(0.0, 0.0), 3);
        assert_eq!(surface_depth(0.0, 0.99), 3);
        assert_eq!(surface_depth(1.0, 0.0), 5); // 5.75 -> 5
        assert_eq!(surface_depth(-1.0, 0.0), 0); // 0.25 -> 0
                                                 // Negative results truncate toward zero, as the Java cast does.
        assert_eq!(surface_depth(-2.0, 0.0), -2); // -2.5 -> -2
    }
}
