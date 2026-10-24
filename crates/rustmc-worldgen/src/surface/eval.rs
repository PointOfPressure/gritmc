//! Evaluating a surface rule at a position.
//!
//! Reference: `SurfaceRules`' condition and rule implementations.
//!
//! The arithmetic conditions — `y_above`, `water`, `stone_depth` — are ported
//! here directly, because they are pure functions of the context and each is
//! a one-line formula that is easy to get subtly wrong. The rest need things
//! that live outside this module (surface noises, the preliminary surface
//! level, the biome source, the badlands generator), so they are supplied
//! through [`SurfaceOracle`].
//!
//! Splitting it that way keeps the formulas testable without a world, and
//! keeps the seam honest: a caller cannot accidentally get a *default*
//! answer for a condition it has not wired up, because the trait has no
//! defaults.

use super::context::{SurfaceContext, NO_WATER};
use super::rule::{BlockState, Condition, Rule, StoneDepthSurface};
use crate::math;

// Read once, not per condition. `test` is the hottest predicate in the
// surface stage — every rule of every column asks it — and a `perf record`
// of the r=16 pipeline (2026-07-28, seed 20260722, (64,64)) put the
// `std::env::var` that was on this line at **1290 of 24,097 samples, 5.4% of
// the whole run**, with the switch unset. See `crate::trace_switch`.
crate::trace_switch_value!(trace_block, "RUSTMC_TRACE_BLOCK");

/// Which of `Context`'s two noise samplers a `noise_threshold` reads.
///
/// A bool would work and would read worse at the call site — `noise(name, x,
/// y, z, true)` says nothing about what `true` means, and the whole reason
/// this exists is that the flag was silently absent for a while.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sampling {
    /// `createNoiseSampler2d` — sampled at `(x, 0, z)`, so it is constant
    /// down a column.
    Flat,
    /// `createNoiseSampler3d` — sampled at `(x, y, z)`.
    Volumetric,
}

impl Sampling {
    #[must_use]
    pub const fn from_is_3d(is_3d: bool) -> Self {
        if is_3d {
            Self::Volumetric
        } else {
            Self::Flat
        }
    }
}

/// Everything a rule needs that this module cannot compute itself.
///
/// No default implementations on purpose. A missing condition should be a
/// compile error, not a silent `false` that removes a biome's surface
/// somewhere nobody is looking.
pub trait SurfaceOracle {
    /// `noise_threshold` — the named surface noise at this position.
    ///
    /// The condition, not the noise, decides whether `y` is used: vanilla's
    /// `Context.getNoiseSampler(noise, is3d)` keeps two separate caches over
    /// the same noise registry, so one noise can be read both ways in one
    /// rule tree.
    fn noise(&mut self, name: &str, x: i32, y: i32, z: i32, sampling: Sampling) -> f64;

    /// The per-column secondary surface noise, for `stone_depth`'s
    /// `secondary_depth_range`.
    fn surface_secondary(&mut self, x: i32, z: i32) -> f64;

    /// `vertical_gradient` — the seeded fade between two anchors.
    ///
    /// True below `true_at`, false at and above `false_at`, and a
    /// position-seeded coin flip in between whose bias moves linearly. The
    /// `random_name` seeds it, so two gradients with identical anchors and
    /// different names differ.
    fn vertical_gradient(
        &mut self,
        random_name: &str,
        x: i32,
        y: i32,
        z: i32,
        true_at: i32,
        false_at: i32,
    ) -> bool;

    /// `steep` — a sharp local gradient, which is what keeps grass off
    /// cliff faces.
    fn steep(&mut self, x: i32, z: i32) -> bool;

    /// `hole` — the column has no surface depth at all.
    fn hole(&mut self, x: i32, z: i32) -> bool;

    /// `temperature` — the frozen-ocean test.
    fn temperature(&mut self, x: i32, y: i32, z: i32) -> bool;

    /// The lowest preliminary surface in this position's 16-block cell.
    fn min_surface_level(&mut self, x: i32, z: i32) -> i32;

    /// `biome` — the biome at this exact position, interned.
    ///
    /// An integer rather than a name: see [`crate::surface::BiomeId`]. This is
    /// the single hottest call in the stage — ~28,864 answers a chunk — and
    /// it used to allocate 25 bytes for each of them.
    fn biome(&mut self, x: i32, y: i32, z: i32) -> crate::surface::BiomeId;

    /// The name behind an id, for **tracing only**.
    ///
    /// Never on the evaluation path. `RUSTMC_TRACE_BLOCK` prints which biome
    /// the rule actually saw, and losing that would cost more than the
    /// allocation it saves: eleven conditions were once eliminated by reading
    /// the datapack instead of asking, and that is what this switch exists to
    /// stop. The default is for the stub oracles that never answer a biome
    /// condition at all.
    fn biome_name(&self, id: crate::surface::BiomeId) -> String {
        format!("#{id}")
    }

    /// `bandlands` — the badlands colour banding, a whole generator in one
    /// operator.
    fn bandlands(&mut self, x: i32, y: i32, z: i32) -> Option<BlockState>;
}

/// `SurfaceRules.Condition.test`.
pub fn test(
    condition: &Condition,
    ctx: &mut SurfaceContext,
    oracle: &mut impl SurfaceOracle,
) -> bool {
    let answer = test_inner(condition, ctx, oracle);
    // `RUSTMC_TRACE_BLOCK=x,y,z` prints every condition asked at one block
    // and what it answered.
    //
    // A whole-world diff says a block is wrong; walking the datapack says
    // which conditions *could* have produced it. Neither says which one
    // actually said no, and that gap has cost this project two long
    // investigations — see the packed_ice section of docs/BENCH.md, where
    // eleven conditions were eliminated by reading rather than by asking.
    if let Some(target) = trace_block() {
        let here = format!("{},{},{}", ctx.block_x(), ctx.block_y(), ctx.block_z());
        if here == target {
            // The *value* as well as the verdict, for the conditions whose
            // inputs are numbers. A `false` with no number attached is what
            // sent this investigation round the houses once already.
            let detail = match condition {
                Condition::AbovePreliminarySurface => {
                    let corners = crate::surface::conditions::min_surface_cell_corners(
                        ctx.block_x(),
                        ctx.block_z(),
                    );
                    let vals: Vec<i32> = corners
                        .iter()
                        .map(|(cx, cz)| oracle.min_surface_level(*cx, *cz))
                        .collect();
                    format!(
                        "  (blockY {} vs minSurfaceLevel {}; corners {:?} at {:?})",
                        ctx.block_y(),
                        ctx.min_surface_level(|x, z| oracle.min_surface_level(x, z)),
                        vals,
                        corners
                    )
                }
                Condition::StoneDepth { .. } => {
                    format!("  (stoneDepthAbove {})", ctx.stone_depth_above())
                }
                Condition::Water { .. } => format!("  (waterHeight {})", ctx.water_height()),
                // Which biome the *rule* saw, which is the fuzzed
                // `BiomeManager` lookup and not the quart grid the biome
                // gate compares. The two are gated separately for a reason:
                // a chunk can match cell for cell on the grid and still hand
                // a rule a different biome one block off a cell edge.
                Condition::Biome(_) | Condition::BiomeIds(_) => {
                    let id = ctx.biome(|x, y, z| oracle.biome(x, y, z));
                    format!("  (biome {} #{id})", oracle.biome_name(id))
                }
                _ => String::new(),
            };
            println!("  {answer:>5}  {condition:?}{detail}");
        }
    }
    answer
}

fn test_inner(
    condition: &Condition,
    ctx: &mut SurfaceContext,
    oracle: &mut impl SurfaceOracle,
) -> bool {
    match condition {
        Condition::Not(inner) => !test(inner, ctx, oracle),

        // The interned form, which is what the surface stage evaluates. One
        // integer compare per listed biome against an id the context already
        // holds — no allocation, no `BTreeMap`, no walking past ten equal
        // `minecraft:` bytes before a comparison can decide anything.
        Condition::BiomeIds(wanted) => {
            // Cached per y on the context, so a column with several biome
            // conditions samples the biome source once, not once each.
            let actual = ctx.biome(|x, y, z| oracle.biome(x, y, z));
            wanted.contains(&actual)
        }

        // The un-interned form. `Rule::intern_biomes` rewrites these away
        // before the stage runs, so this arm is reached only by callers that
        // never interned — the parse and golden tests, which compare names.
        // It is kept, rather than made unreachable, because a rule that
        // silently stopped matching would be invisible: same shape of defect
        // as a gamerule whose failure nobody read.
        Condition::Biome(wanted) => {
            let actual = ctx.biome(|x, y, z| oracle.biome(x, y, z));
            // `contains` rather than `iter().any()`: the note this replaces
            // avoided `contains` because it needs a `&String` and the answer
            // was a borrowed `&str`, so matching cost an allocation per
            // condition tested. `biome_name` already returns an owned
            // `String`, so there is nothing left to avoid.
            let name = oracle.biome_name(actual);
            wanted.contains(&name)
        }

        Condition::NoiseThreshold {
            noise,
            min_threshold,
            max_threshold,
            is_3d,
        } => {
            let value = oracle.noise(
                noise,
                ctx.block_x(),
                ctx.block_y(),
                ctx.block_z(),
                Sampling::from_is_3d(*is_3d),
            );
            value >= *min_threshold && value <= *max_threshold
        }

        Condition::VerticalGradient {
            random_name,
            true_at_and_below,
            false_at_and_above,
        } => {
            let true_at = true_at_and_below.resolve(ctx.min_y, ctx.height);
            let false_at = false_at_and_above.resolve(ctx.min_y, ctx.height);
            oracle.vertical_gradient(
                random_name,
                ctx.block_x(),
                ctx.block_y(),
                ctx.block_z(),
                true_at,
                false_at,
            )
        }

        // `blockY + (addStoneDepth ? stoneDepthAbove : 0)
        //     >= anchor + surfaceDepth * multiplier`
        Condition::YAbove {
            anchor,
            surface_depth_multiplier,
            add_stone_depth,
        } => {
            let stone = if *add_stone_depth {
                ctx.stone_depth_above()
            } else {
                0
            };
            ctx.block_y() + stone
                >= anchor.resolve(ctx.min_y, ctx.height)
                    + ctx.surface_depth() * surface_depth_multiplier
        }

        // A dry column short-circuits to true — `waterHeight == MIN_VALUE`
        // means "always above water", not "water at the world floor".
        Condition::Water {
            offset,
            surface_depth_multiplier,
            add_stone_depth,
        } => {
            if ctx.water_height() == NO_WATER {
                return true;
            }
            let stone = if *add_stone_depth {
                ctx.stone_depth_above()
            } else {
                0
            };
            ctx.block_y() + stone
                >= ctx.water_height() + offset + ctx.surface_depth() * surface_depth_multiplier
        }

        // `stoneDepth <= 1 + offset + surfaceDepth + secondary`. The `1 +`
        // is why a depth of exactly the offset still counts as inside.
        Condition::StoneDepth {
            offset,
            add_surface_depth,
            secondary_depth_range,
            surface_type,
        } => {
            let stone_depth = match surface_type {
                StoneDepthSurface::Ceiling => ctx.stone_depth_below(),
                StoneDepthSurface::Floor => ctx.stone_depth_above(),
            };
            let surface_depth = if *add_surface_depth {
                ctx.surface_depth()
            } else {
                0
            };
            let secondary = if *secondary_depth_range == 0 {
                0
            } else {
                // `Mth.map` without a clamp, then truncated by the cast to
                // int — so a secondary noise outside [-1, 1] extrapolates
                // rather than saturating.
                let mapped = math::map(
                    ctx.surface_secondary(|x, z| oracle.surface_secondary(x, z)),
                    -1.0,
                    1.0,
                    0.0,
                    f64::from(*secondary_depth_range),
                );
                mapped as i32
            };
            stone_depth <= 1 + offset + surface_depth + secondary
        }

        Condition::Hole => oracle.hole(ctx.block_x(), ctx.block_z()),

        Condition::Steep => oracle.steep(ctx.block_x(), ctx.block_z()),

        Condition::Temperature => oracle.temperature(ctx.block_x(), ctx.block_y(), ctx.block_z()),

        // `blockY >= minSurfaceLevel` — whether this position is high enough
        // that a surface is worth building at all.
        Condition::AbovePreliminarySurface => {
            let min = ctx.min_surface_level(|x, z| oracle.min_surface_level(x, z));
            ctx.block_y() >= min
        }
    }
}

/// `SurfaceRules.SurfaceRule.tryApply` — `None` means "no opinion", and the
/// caller keeps whatever the noise stage put there.
pub fn apply(
    rule: &Rule,
    ctx: &mut SurfaceContext,
    oracle: &mut impl SurfaceOracle,
) -> Option<BlockState> {
    match rule {
        // First child with an opinion wins. Not "last", and not "merge" —
        // the ordering in the tree is the priority.
        Rule::Sequence(children) => children.iter().find_map(|child| apply(child, ctx, oracle)),

        Rule::Condition { if_true, then_run } => {
            if test(if_true, ctx, oracle) {
                apply(then_run, ctx, oracle)
            } else {
                None
            }
        }

        Rule::Block(state) => Some(state.clone()),

        Rule::Bandlands => oracle.bandlands(ctx.block_x(), ctx.block_y(), ctx.block_z()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::rule::VerticalAnchor;

    /// An oracle that answers everything with a fixed value, so a test can
    /// isolate one condition. Panics for anything a test did not opt into,
    /// rather than returning a default that would quietly pass.
    #[derive(Default)]
    struct Fixed {
        noise: Option<f64>,
        secondary: Option<f64>,
        gradient: Option<bool>,
        steep: Option<bool>,
        hole: Option<bool>,
        temperature: Option<bool>,
        min_surface: Option<i32>,
        biome: Option<String>,
        bandlands: Option<BlockState>,
        last_sampling: Option<Sampling>,
    }

    impl SurfaceOracle for Fixed {
        fn noise(&mut self, _: &str, _: i32, y: i32, _: i32, sampling: Sampling) -> f64 {
            self.last_sampling = Some(sampling);
            // A 2D sampler must not see the y at all, so the fixture makes
            // that observable rather than trusting the flag to be passed.
            match sampling {
                Sampling::Flat => self.noise.expect("test did not supply a noise"),
                Sampling::Volumetric => f64::from(y),
            }
        }
        fn surface_secondary(&mut self, _: i32, _: i32) -> f64 {
            self.secondary.expect("test did not supply a secondary")
        }
        fn vertical_gradient(&mut self, _: &str, _: i32, _: i32, _: i32, _: i32, _: i32) -> bool {
            self.gradient.expect("test did not supply a gradient")
        }
        fn steep(&mut self, _: i32, _: i32) -> bool {
            self.steep.expect("test did not supply steep")
        }
        fn hole(&mut self, _: i32, _: i32) -> bool {
            self.hole.expect("test did not supply hole")
        }
        fn temperature(&mut self, _: i32, _: i32, _: i32) -> bool {
            self.temperature.expect("test did not supply temperature")
        }
        fn min_surface_level(&mut self, _: i32, _: i32) -> i32 {
            self.min_surface.expect("test did not supply min surface")
        }
        fn biome(&mut self, _: i32, _: i32, _: i32) -> crate::surface::BiomeId {
            assert!(self.biome.is_some(), "test did not supply a biome");
            0
        }
        fn biome_name(&self, _: crate::surface::BiomeId) -> String {
            self.biome.clone().expect("test did not supply a biome")
        }
        fn bandlands(&mut self, _: i32, _: i32, _: i32) -> Option<BlockState> {
            self.bandlands.clone()
        }
    }

    fn ctx_at(y: i32, surface_depth: i32, water: i32, stone_above: i32) -> SurfaceContext {
        let mut ctx = SurfaceContext::new(-64, 384, 63);
        ctx.update_xz(0, 0, surface_depth);
        ctx.update_y(stone_above, 0, water, y);
        ctx
    }

    #[test]
    fn y_above_folds_in_the_surface_depth() {
        // `blockY >= anchor + surfaceDepth * multiplier`. With depth 3 and a
        // multiplier of 2 the effective threshold is 60 + 6.
        let condition = Condition::YAbove {
            anchor: VerticalAnchor::Absolute(60),
            surface_depth_multiplier: 2,
            add_stone_depth: false,
        };
        let mut oracle = Fixed::default();
        assert!(test(
            &condition,
            &mut ctx_at(66, 3, NO_WATER, 0),
            &mut oracle
        ));
        assert!(!test(
            &condition,
            &mut ctx_at(65, 3, NO_WATER, 0),
            &mut oracle
        ));
        // A zero multiplier ignores the depth entirely.
        let plain = Condition::YAbove {
            anchor: VerticalAnchor::Absolute(60),
            surface_depth_multiplier: 0,
            add_stone_depth: false,
        };
        assert!(test(&plain, &mut ctx_at(60, 99, NO_WATER, 0), &mut oracle));
    }

    #[test]
    fn a_dry_column_is_always_above_water() {
        // The sentinel short-circuits before any arithmetic. Treating
        // NO_WATER as a real height would make every dry column behave as
        // though it were flooded from the world floor.
        let condition = Condition::Water {
            offset: 0,
            surface_depth_multiplier: 0,
            add_stone_depth: false,
        };
        let mut oracle = Fixed::default();
        assert!(test(
            &condition,
            &mut ctx_at(-60, 3, NO_WATER, 0),
            &mut oracle
        ));
        // With real water, depth decides.
        assert!(test(&condition, &mut ctx_at(64, 3, 63, 0), &mut oracle));
        assert!(!test(&condition, &mut ctx_at(62, 3, 63, 0), &mut oracle));
    }

    #[test]
    fn stone_depth_is_inclusive_by_one() {
        // `stoneDepth <= 1 + offset + ...`, so offset 0 admits depths 0 and
        // 1. Dropping the `1 +` makes every surface layer one block thin.
        let condition = Condition::StoneDepth {
            offset: 0,
            add_surface_depth: false,
            secondary_depth_range: 0,
            surface_type: StoneDepthSurface::Floor,
        };
        let mut oracle = Fixed::default();
        assert!(test(
            &condition,
            &mut ctx_at(70, 3, NO_WATER, 1),
            &mut oracle
        ));
        assert!(!test(
            &condition,
            &mut ctx_at(70, 3, NO_WATER, 2),
            &mut oracle
        ));
    }

    #[test]
    fn stone_depth_reads_the_other_end_for_a_ceiling() {
        // Floor counts down from the surface, ceiling counts up from below.
        // Swapping them puts cave ceilings' material on their floors.
        let floor = Condition::StoneDepth {
            offset: 0,
            add_surface_depth: false,
            secondary_depth_range: 0,
            surface_type: StoneDepthSurface::Floor,
        };
        let ceiling = Condition::StoneDepth {
            offset: 0,
            add_surface_depth: false,
            secondary_depth_range: 0,
            surface_type: StoneDepthSurface::Ceiling,
        };
        let mut oracle = Fixed::default();
        let mut ctx = SurfaceContext::new(-64, 384, 63);
        ctx.update_xz(0, 0, 3);
        // Deep from above, shallow from below.
        ctx.update_y(9, 0, NO_WATER, 40);
        assert!(!test(&floor, &mut ctx, &mut oracle));
        assert!(test(&ceiling, &mut ctx, &mut oracle));
    }

    #[test]
    fn a_sequence_takes_the_first_opinion_not_the_last() {
        let rule = Rule::Sequence(vec![
            Rule::Condition {
                if_true: Condition::Steep,
                then_run: Box::new(Rule::Block(BlockState("minecraft:stone".into()))),
            },
            Rule::Block(BlockState("minecraft:grass_block".into())),
        ]);
        let mut ctx = ctx_at(70, 3, NO_WATER, 0);

        let mut steep = Fixed {
            steep: Some(true),
            ..Fixed::default()
        };
        assert_eq!(
            apply(&rule, &mut ctx, &mut steep),
            Some(BlockState("minecraft:stone".into()))
        );

        let mut flat = Fixed {
            steep: Some(false),
            ..Fixed::default()
        };
        assert_eq!(
            apply(&rule, &mut ctx, &mut flat),
            Some(BlockState("minecraft:grass_block".into()))
        );
    }

    #[test]
    fn a_failed_condition_yields_no_opinion_rather_than_air() {
        // `None` means "keep what the noise stage put here". Returning a
        // block — even air — would carve the terrain.
        let rule = Rule::Condition {
            if_true: Condition::Hole,
            then_run: Box::new(Rule::Block(BlockState("minecraft:air".into()))),
        };
        let mut oracle = Fixed {
            hole: Some(false),
            ..Fixed::default()
        };
        assert_eq!(
            apply(&rule, &mut ctx_at(70, 3, NO_WATER, 0), &mut oracle),
            None
        );
    }

    #[test]
    fn noise_threshold_is_inclusive_at_both_ends() {
        let condition = Condition::NoiseThreshold {
            noise: "minecraft:surface".into(),
            min_threshold: -0.5,
            max_threshold: 0.5,
            is_3d: false,
        };
        for (value, expected) in [(-0.5, true), (0.0, true), (0.5, true), (0.51, false)] {
            let mut oracle = Fixed {
                noise: Some(value),
                ..Fixed::default()
            };
            assert_eq!(
                test(&condition, &mut ctx_at(70, 3, NO_WATER, 0), &mut oracle),
                expected,
                "noise {value}"
            );
        }
    }

    #[test]
    fn not_inverts() {
        let mut oracle = Fixed {
            steep: Some(true),
            ..Fixed::default()
        };
        let condition = Condition::Not(Box::new(Condition::Steep));
        assert!(!test(
            &condition,
            &mut ctx_at(70, 3, NO_WATER, 0),
            &mut oracle
        ));
    }
}

/// Relabelling the biome palette must not change a single verdict.
///
/// # Why this test exists, and what it is standing in for
///
/// A falsification sweep on 2026-08-06 rebuilt the generating path's palette
/// in reverse — an injective relabelling of the same biome set — and reported
/// **4 of 4 region files changed**, without naming a consumer. The warning on
/// [`crate::surface::ORDER_IS_NOT_OBSERVABLE`] came from that, and it is now
/// retracted — the 4-of-4 does not reproduce. A warning nobody can act on is
/// the worst kind of caveat, and this test is half of what replaces it.
///
/// The dump that settles it is `ci/biome-palette-order.sh`, about twenty
/// minutes at r=16 on a 24-thread box. This is the same claim narrowed to the part a
/// unit test can hold, and it runs in T0 in microseconds: every comparison the
/// interned biome condition makes is an **equality**, so the verdict is a
/// function of *which* biome is under the cursor and never of the integer it
/// was assigned.
///
/// It is deliberately a `#[cfg(test)]` module inside the lib and not a file in
/// `tests/`. `ci/t0.sh` runs `cargo test --workspace --lib --bins`; an
/// integration test would never run in any gate, which is exactly how
/// nineteen `oracle-runner` tests went unrun for weeks with one of them
/// failing.
///
/// What would make it red: a `binary_search` over the unsorted `BiomeIds`
/// vector, a `<` or `min`/`max` on an id, an id used as a hash or RNG input,
/// or an id indexing a table that is not built in the palette's own loop.
#[cfg(test)]
mod relabelling_is_undetectable {
    use super::*;
    use crate::surface::BiomePalette;

    /// Answers one fixed biome id and nothing else. Every other oracle method
    /// panics, so a test that drifts onto a non-biome condition fails loudly
    /// instead of quietly measuring something else.
    struct OneBiome {
        id: crate::surface::BiomeId,
        name: String,
    }

    impl SurfaceOracle for OneBiome {
        fn noise(&mut self, _: &str, _: i32, _: i32, _: i32, _: Sampling) -> f64 {
            unreachable!("this test evaluates biome conditions only")
        }
        fn surface_secondary(&mut self, _: i32, _: i32) -> f64 {
            unreachable!("this test evaluates biome conditions only")
        }
        fn vertical_gradient(&mut self, _: &str, _: i32, _: i32, _: i32, _: i32, _: i32) -> bool {
            unreachable!("this test evaluates biome conditions only")
        }
        fn steep(&mut self, _: i32, _: i32) -> bool {
            unreachable!("this test evaluates biome conditions only")
        }
        fn hole(&mut self, _: i32, _: i32) -> bool {
            unreachable!("this test evaluates biome conditions only")
        }
        fn temperature(&mut self, _: i32, _: i32, _: i32) -> bool {
            unreachable!("this test evaluates biome conditions only")
        }
        fn min_surface_level(&mut self, _: i32, _: i32) -> i32 {
            unreachable!("this test evaluates biome conditions only")
        }
        fn biome(&mut self, _: i32, _: i32, _: i32) -> crate::surface::BiomeId {
            self.id
        }
        fn biome_name(&self, _: crate::surface::BiomeId) -> String {
            self.name.clone()
        }
        fn bandlands(&mut self, _: i32, _: i32, _: i32) -> Option<BlockState> {
            unreachable!("this test evaluates biome conditions only")
        }
    }

    /// The whole directory, in the order the generating path would issue ids.
    const DIRECTORY: [&str; 6] = [
        "minecraft:badlands",
        "minecraft:desert",
        "minecraft:frozen_ocean",
        "minecraft:plains",
        "minecraft:swamp",
        "minecraft:taiga",
    ];

    /// A tree with the two shapes `intern_biomes` has to walk through — a bare
    /// condition and one under a `Not` — and two overlapping name lists, so a
    /// relabelling that merged or reordered ids could show up as a changed
    /// verdict rather than as a changed id.
    fn tree() -> Vec<Condition> {
        vec![
            Condition::Biome(vec![
                "minecraft:desert".to_string(),
                "minecraft:badlands".to_string(),
            ]),
            Condition::Biome(vec!["minecraft:plains".to_string()]),
            Condition::Not(Box::new(Condition::Biome(vec![
                "minecraft:taiga".to_string(),
                "minecraft:desert".to_string(),
            ]))),
            Condition::Biome(vec![
                "minecraft:swamp".to_string(),
                "minecraft:frozen_ocean".to_string(),
                "minecraft:taiga".to_string(),
            ]),
        ]
    }

    /// Every condition against every biome, under one palette.
    fn verdicts(palette: &BiomePalette) -> Vec<bool> {
        let mut out = Vec::new();
        for condition in tree() {
            let mut interned = condition;
            let mut missing = Vec::new();
            interned.intern_biomes(&|name| palette.id_of(name), &mut missing);
            assert!(missing.is_empty(), "unresolved biome names: {missing:?}");
            // The rewrite has to have happened, or this test compares the
            // name path against itself and proves nothing about ids.
            assert!(
                !matches!(interned, Condition::Biome(_)),
                "intern_biomes left a name-keyed condition: the interned path is not under test"
            );
            for name in DIRECTORY {
                let mut oracle = OneBiome {
                    id: palette.id_of(name).expect("in the directory"),
                    name: name.to_string(),
                };
                let mut ctx = SurfaceContext::new(-64, 384, 63);
                ctx.update_xz(0, 0, 1);
                ctx.update_y(0, 0, NO_WATER, 64);
                out.push(test(&interned, &mut ctx, &mut oracle));
            }
        }
        out
    }

    #[test]
    fn reversing_the_palette_changes_no_verdict() {
        let forward = BiomePalette::from_names(DIRECTORY);
        let reversed = {
            let mut names: Vec<&str> = DIRECTORY.to_vec();
            names.reverse();
            BiomePalette::from_names(names)
        };
        // The relabelling has to be real, or the comparison below is two
        // copies of one palette agreeing with each other.
        assert_ne!(
            forward.id_of("minecraft:badlands"),
            reversed.id_of("minecraft:badlands"),
            "the two palettes issued the same id: nothing was relabelled"
        );
        assert_eq!(
            verdicts(&forward),
            verdicts(&reversed),
            "a biome condition's verdict moved under an injective relabelling — \
             some comparison on BiomeId is no longer an equality"
        );
    }

    #[test]
    fn shifting_every_id_changes_no_verdict() {
        // Order preserved, values moved: the discriminator `poison`/`shift` in
        // `ci/biome-palette-order.sh` uses, at unit-test cost. A consumer that
        // read the id's value but not its order would survive the reversal
        // test above by symmetry and die here.
        let forward = BiomePalette::from_names(DIRECTORY);
        let shifted = {
            let mut names: Vec<String> = (0..7).map(|i| format!("rustmc:pad_{i}")).collect();
            names.extend(DIRECTORY.iter().map(|s| (*s).to_string()));
            BiomePalette::from_names(names)
        };
        assert_eq!(
            shifted.id_of("minecraft:badlands"),
            Some(7),
            "the padding did not shift the directory"
        );
        assert_eq!(
            verdicts(&forward),
            verdicts(&shifted),
            "a biome condition's verdict moved when every id gained a constant — \
             some consumer reads the integer, not the biome"
        );
    }

    /// The test above can only mean something if these verdicts are not all
    /// the same value. Four conditions over six biomes that all answered
    /// `false` would compare equal under any relabelling whatever.
    #[test]
    fn the_verdicts_are_not_degenerate() {
        let v = verdicts(&BiomePalette::from_names(DIRECTORY));
        assert_eq!(v.len(), 24, "four conditions over six biomes");
        assert!(
            v.iter().any(|&b| b) && v.iter().any(|&b| !b),
            "every verdict came back the same, so relabelling could not have shown up: {v:?}"
        );
    }
}
