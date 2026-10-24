//! Ore veins — the copper and iron ribbons threaded through deep stone.
//!
//! Reference: `net.minecraft.world.level.levelgen.OreVeinifier`.
//!
//! Runs *after* the aquifer and only where the aquifer declined the block
//! (see [`super::fill`] on why that is `None` and not `Solid`). So veins can
//! only replace stone, never water or air, and a cave passing through a vein
//! cuts it rather than filling with ore.
//!
//! A vein is not a blob of ore. Most of it is **filler** — granite for
//! copper, tuff for iron — with ore scattered through at a richness that
//! rises toward the middle of the vein. That is why they read as geological
//! strata rather than as loot.
//!
//! # Float width is load-bearing here
//!
//! Vanilla's constants are `float` literals and two adjacent comparisons use
//! *different* precision:
//!
//! ```java
//! if (positionalRandom.nextFloat() > 0.7F)      // float vs float
//! if (positionalRandom.nextFloat() < richness)  // float widened to double
//! ```
//!
//! `nextFloat()` returns a `float`, so the first comparison happens entirely
//! in `f32` while the second promotes. Doing both in `f64` — the obvious
//! "cleanup" — moves the 0.7 cutoff by an ulp and changes which blocks are
//! ore. Each comparison below is annotated with the width it runs at.

use super::DensityFunction;
use crate::math;
use crate::rng::{PositionalRandomFactory, RandomSource, XoroshiroPositionalFactory};

/// Which of the two vein families a point belongs to.
///
/// Chosen by the *sign* of the veininess noise, so the two never overlap and
/// each occupies its own y band.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VeinType {
    Copper,
    Iron,
}

impl VeinType {
    /// `VeinType.minY` / `maxY`, both inclusive.
    #[must_use]
    pub const fn y_range(self) -> (i32, i32) {
        match self {
            Self::Copper => (0, 50),
            Self::Iron => (-60, -8),
        }
    }
}

/// What a vein puts at a position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VeinBlock {
    /// `copper_ore` / `deepslate_iron_ore`.
    Ore(VeinType),
    /// `raw_copper_block` / `raw_iron_block` — the rare 2% core.
    RawOreBlock(VeinType),
    /// `granite` / `tuff` — the bulk of the vein.
    Filler(VeinType),
}

/// Which of the veinifier's three inputs is being asked for.
///
/// The caller supplies values on demand rather than up front, because
/// vanilla pulls them **lazily** and most blocks never need more than the
/// first. `veinToggle` is read immediately; `veinRidged` only after the y
/// band, the veininess threshold and a random rejection have all passed;
/// `veinGap` only after that. Sampling all three eagerly is value-identical
/// and was measured at 10.47 ms/chunk against 2-3 ms — the y band alone
/// excludes about three quarters of a column.
///
/// They are supplied rather than sampled here because all three are
/// `interpolated(..)` markers: vanilla reads them off the chunk's cell
/// lattice, not per block. Evaluating them directly would put veins in
/// subtly wrong places — a scattered ore/stone swap, not an obvious break.
/// `ChunkSampler` owns that lattice, so it owns the sampling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VeinAxis {
    Toggle,
    Ridged,
    Gap,
}

/// `OreVeinifier.create`'s closure, as a value.
#[derive(Debug)]
pub struct OreVeinifier {
    vein_toggle: DensityFunction,
    vein_ridged: DensityFunction,
    vein_gap: DensityFunction,
    random: XoroshiroPositionalFactory,
}

/// Below this veininess there is no vein at all.
const VEININESS_THRESHOLD: f32 = 0.4;
/// Veins taper over this many blocks at each end of their y band.
const EDGE_ROUNDOFF_BEGIN: f64 = 20.0;
const MAX_EDGE_ROUNDOFF: f64 = -0.2;
/// 30% of positions inside a vein are skipped outright, which is what makes
/// the edges ragged instead of a clean tube.
const VEIN_SOLIDNESS: f32 = 0.7;
const MIN_RICHNESS: f32 = 0.1;
const MAX_RICHNESS: f32 = 0.3;
const MAX_RICHNESS_THRESHOLD: f32 = 0.6;
const CHANCE_OF_RAW_ORE_BLOCK: f32 = 0.02;
const SKIP_ORE_IF_GAP_NOISE_IS_BELOW: f32 = -0.3;

impl OreVeinifier {
    #[must_use]
    pub const fn new(
        vein_toggle: DensityFunction,
        vein_ridged: DensityFunction,
        vein_gap: DensityFunction,
        random: XoroshiroPositionalFactory,
    ) -> Self {
        Self {
            vein_toggle,
            vein_ridged,
            vein_gap,
            random,
        }
    }

    /// The three inputs, for the sampler to interpolate.
    #[must_use]
    pub const fn toggle(&self) -> &DensityFunction {
        &self.vein_toggle
    }

    #[must_use]
    pub const fn ridged(&self) -> &DensityFunction {
        &self.vein_ridged
    }

    #[must_use]
    pub const fn gap(&self) -> &DensityFunction {
        &self.vein_gap
    }

    /// `None` means "no vein here" — the caller keeps its default block.
    ///
    /// `sample` is called at most once per axis, in vanilla's order, and only
    /// as far as the early exits allow.
    #[must_use]
    pub fn calculate(
        &self,
        pos: (i32, i32, i32),
        mut sample: impl FnMut(VeinAxis) -> f64,
    ) -> Option<VeinBlock> {
        let (x, y, z) = pos;

        let veininess = sample(VeinAxis::Toggle);
        // The sign picks the family; the magnitude decides whether there is
        // a vein at all. So copper and iron can never occupy one position.
        let vein_type = if veininess > 0.0 {
            VeinType::Copper
        } else {
            VeinType::Iron
        };
        let veininess_ridged = veininess.abs();

        let (min_y, max_y) = vein_type.y_range();
        let distance_from_top = max_y - y;
        let distance_from_bottom = y - min_y;
        if distance_from_bottom < 0 || distance_from_top < 0 {
            return None;
        }

        // Taper: within 20 blocks of either end, veininess is penalised down
        // to -0.2, so veins fade out rather than being sliced flat.
        let distance_from_edge = distance_from_top.min(distance_from_bottom);
        let edge_roundoff = math::clamped_map(
            f64::from(distance_from_edge),
            0.0,
            EDGE_ROUNDOFF_BEGIN,
            MAX_EDGE_ROUNDOFF,
            0.0,
        );
        // f64: both operands are double, the literal widens.
        if veininess_ridged + edge_roundoff < f64::from(VEININESS_THRESHOLD) {
            return None;
        }

        let mut random = self.random.at(x, y, z);

        // f32: nextFloat() is a float and so is the literal, so this
        // comparison never touches f64.
        if random.next_f32() > VEIN_SOLIDNESS {
            return None;
        }
        // The ridge carves the vein hollow; >= 0 is outside it.
        if sample(VeinAxis::Ridged) >= 0.0 {
            return None;
        }

        let richness = math::clamped_map(
            veininess_ridged,
            f64::from(VEININESS_THRESHOLD),
            f64::from(MAX_RICHNESS_THRESHOLD),
            f64::from(MIN_RICHNESS),
            f64::from(MAX_RICHNESS),
        );

        // f64: richness is a double, so this nextFloat() *is* widened —
        // unlike the VEIN_SOLIDNESS test three lines up.
        // `&&` short-circuits, so vein_gap is not sampled when the richness
        // roll already failed — which is vanilla's shape and most of the time.
        let ore_here = f64::from(random.next_f32()) < richness
            && sample(VeinAxis::Gap) > f64::from(SKIP_ORE_IF_GAP_NOISE_IS_BELOW);

        if ore_here {
            // f32 again: both sides are floats.
            if random.next_f32() < CHANCE_OF_RAW_ORE_BLOCK {
                Some(VeinBlock::RawOreBlock(vein_type))
            } else {
                Some(VeinBlock::Ore(vein_type))
            }
        } else {
            Some(VeinBlock::Filler(vein_type))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::density::build;
    use crate::rng::XoroshiroRandomSource;

    /// The veinifier plus a sampler returning the constants it would have
    /// been handed.
    fn veinifier(
        toggle: f64,
        ridged: f64,
        gap: f64,
    ) -> (OreVeinifier, impl Fn(VeinAxis) -> f64 + Clone) {
        (
            OreVeinifier::new(
                build::constant(toggle),
                build::constant(ridged),
                build::constant(gap),
                XoroshiroRandomSource::from_seed(20260725).fork_positional(),
            ),
            move |axis| match axis {
                VeinAxis::Toggle => toggle,
                VeinAxis::Ridged => ridged,
                VeinAxis::Gap => gap,
            },
        )
    }

    #[test]
    fn the_sign_of_veininess_picks_the_family() {
        assert_eq!(VeinType::Copper.y_range(), (0, 50));
        assert_eq!(VeinType::Iron.y_range(), (-60, -8));
        // Positive veininess is copper, and copper's band is above zero, so
        // a strongly positive toggle at y=25 can only ever yield copper.
        let (v, s) = veinifier(1.0, -1.0, 1.0);
        for y in [5, 25, 45] {
            if let Some(block) = v.calculate((3, y, 7), s.clone()) {
                let family = match block {
                    VeinBlock::Ore(t) | VeinBlock::RawOreBlock(t) | VeinBlock::Filler(t) => t,
                };
                assert_eq!(family, VeinType::Copper, "at y={y}");
            }
        }
    }

    #[test]
    fn nothing_generates_outside_the_y_band() {
        let (copper, cs) = veinifier(1.0, -1.0, 1.0);
        for y in [-1, 51, 200, -64] {
            assert_eq!(
                copper.calculate((3, y, 7), cs.clone()),
                None,
                "copper at y={y}"
            );
        }
        let (iron, is_) = veinifier(-1.0, -1.0, 1.0);
        for y in [-61, -7, 0, 100] {
            assert_eq!(
                iron.calculate((3, y, 7), is_.clone()),
                None,
                "iron at y={y}"
            );
        }
    }

    #[test]
    fn weak_veininess_produces_nothing() {
        // 0.39 is under the 0.4 threshold even at the middle of the band,
        // where the edge roundoff is zero.
        let (v, s) = veinifier(0.39, -1.0, 1.0);
        assert_eq!(v.calculate((3, 25, 7), s.clone()), None);
    }

    #[test]
    fn the_edge_taper_suppresses_veins_near_the_band_ends() {
        // Veininess 0.5 clears the threshold in the middle, but at the very
        // edge the -0.2 roundoff drags it to 0.3, under the cutoff.
        let (v, s) = veinifier(0.5, -1.0, 1.0);
        assert_eq!(
            v.calculate((3, 0, 7), s.clone()),
            None,
            "at the bottom edge"
        );
        assert_eq!(v.calculate((3, 50, 7), s.clone()), None, "at the top edge");
        // 20 blocks in, the taper is gone.
        assert!(v.calculate((3, 25, 7), s.clone()).is_some(), "mid-band");
    }

    #[test]
    fn a_non_negative_ridge_hollows_the_vein_out() {
        // veinRidged >= 0 is outside the vein body, whatever the veininess.
        let (v, s) = veinifier(1.0, 0.0, 1.0);
        assert_eq!(v.calculate((3, 25, 7), s.clone()), None);
        let (v, s) = veinifier(1.0, -0.0001, 1.0);
        assert!(v.calculate((3, 25, 7), s.clone()).is_some());
    }

    #[test]
    fn a_low_gap_noise_leaves_filler_rather_than_ore() {
        // vein_gap <= -0.3 blocks ore but not the vein itself, so the whole
        // vein comes out as granite/tuff. This is the difference between a
        // vein being poor and a vein being absent.
        let (v, s) = veinifier(1.0, -1.0, -1.0);
        let mut sampled = 0;
        for y in 20..40 {
            if let Some(block) = v.calculate((3, y, 7), s.clone()) {
                sampled += 1;
                assert!(
                    matches!(block, VeinBlock::Filler(_)),
                    "expected filler at y={y}, got {block:?}"
                );
            }
        }
        assert!(sampled > 0, "no vein blocks generated at all");
    }

    #[test]
    fn results_are_positional_and_reproducible() {
        // Two veinifiers built the same way must agree; the RNG is keyed by
        // position, so evaluation order cannot matter.
        let (a, as_) = veinifier(0.55, -1.0, 1.0);
        let (b, bs) = veinifier(0.55, -1.0, 1.0);
        for y in 15..45 {
            assert_eq!(
                a.calculate((11, y, -3), as_.clone()),
                b.calculate((11, y, -3), bs.clone())
            );
        }
    }

    #[test]
    fn the_solidness_cutoff_is_compared_in_f32() {
        // 0.7f64 and f64::from(0.7f32) are different numbers, and the
        // comparison vanilla makes is the f32 one. A nextFloat() landing
        // between them must be *rejected* (> 0.7f32) even though it is
        // below 0.7f64.
        let as_f32 = f64::from(VEIN_SOLIDNESS);
        assert_ne!(as_f32, 0.7_f64);
        assert!(as_f32 < 0.7_f64, "0.7f32 widens downward");
        // So the f64 test would accept a band of draws the f32 test rejects.
        let between = 0.699_999_99_f64;
        assert!(between > as_f32, "f32 comparison: rejected");
        assert!(between < 0.7_f64, "f64 comparison: accepted");
    }
}
