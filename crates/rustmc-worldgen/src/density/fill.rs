//! Density to substance — the last step before block states.
//!
//! Reference: `net.minecraft.world.level.levelgen.Aquifer` (the
//! `createDisabled` rule and `FluidStatus`), driven by
//! `NoiseBasedChunkGenerator`'s global fluid picker.
//!
//! # Scope: this is the baseline, not the overworld
//!
//! `Aquifer.createDisabled` is the simple rule — positive density is solid,
//! everything else is whatever fluid the picker says sits at that height.
//! Several dimensions genuinely use it, and it makes the block-fill loop
//! testable before the real aquifer exists.
//!
//! **The overworld does not use it.** Its noise settings enable
//! `NoiseBasedAquifer`, which carves local water tables so caves flood
//! independently of sea level. Filling the overworld with this rule gives
//! dry caves and wrong lakes — correct-looking terrain with the water in
//! the wrong places. That port is wave-4 Group G; this module is
//! deliberately labelled so nobody mistakes one for the other.

/// What occupies a block position.
///
/// Deliberately not a block-state id: the density layer has no registry and
/// should not grow one. `rustmc-data` maps these to real states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Substance {
    /// Positive density — the generator's default block (stone in the
    /// overworld, but the caller decides).
    Solid,
    Water,
    Lava,
    Air,
    /// An ore vein replaced the default block. Carries which block, because
    /// a vein is mostly filler with ore scattered through it.
    Vein(super::ore_vein::VeinBlock),
}

impl Substance {
    /// Whether this stops an entity falling through — `blocksMotion`, which
    /// is what the `OCEAN_FLOOR` heightmap tracks.
    ///
    /// Fluids do not: an ocean's floor is the stone under the water, not the
    /// waterline, which is the entire reason the two heightmaps differ.
    #[must_use]
    pub const fn blocks_motion(self) -> bool {
        match self {
            Self::Solid | Self::Vein(_) => true,
            Self::Water | Self::Lava | Self::Air => false,
        }
    }

    #[must_use]
    pub const fn is_air(self) -> bool {
        matches!(self, Self::Air)
    }

    /// Whether a fluid tick could apply here — vanilla's
    /// `!state.getFluidState().isEmpty()`.
    #[must_use]
    pub const fn is_fluid(self) -> bool {
        matches!(self, Self::Water | Self::Lava)
    }
}

/// `Aquifer.FluidStatus` — a fluid and the level it fills to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FluidStatus {
    pub level: i32,
    pub fluid: Substance,
}

impl FluidStatus {
    #[must_use]
    pub const fn new(level: i32, fluid: Substance) -> Self {
        Self { level, fluid }
    }

    /// `FluidStatus.at` — the level is **exclusive**: a block exactly at
    /// `level` is air, which is why an ocean's surface sits one below sea
    /// level rather than on it.
    #[must_use]
    pub const fn at(&self, block_y: i32) -> Substance {
        if block_y < self.level {
            self.fluid
        } else {
            Substance::Air
        }
    }
}

/// The global fluid picker `NoiseBasedChunkGenerator` builds.
///
/// Two layers: lava in the deep, water up to sea level. The lava level is
/// the one that makes lava lakes appear at the bottom of the world rather
/// than the whole cave system flooding.
#[derive(Clone, Copy, Debug)]
pub struct GlobalFluidPicker {
    lava: FluidStatus,
    water: FluidStatus,
}

impl GlobalFluidPicker {
    /// `sea_level` comes from the noise settings (63 for the overworld);
    /// `lava_level` is vanilla's fixed -54.
    #[must_use]
    pub const fn new(sea_level: i32, lava_level: i32) -> Self {
        Self {
            lava: FluidStatus::new(lava_level, Substance::Lava),
            water: FluidStatus::new(sea_level, Substance::Water),
        }
    }

    /// The overworld's picker: sea level 63, lava below -54.
    #[must_use]
    pub const fn overworld(sea_level: i32) -> Self {
        Self::new(sea_level, -54)
    }

    /// The picker for a dimension whose `defaultFluid` is not water.
    ///
    /// `NoiseBasedChunkGenerator` builds the upper layer from
    /// `settings.defaultFluid()`, which is lava in the nether and air in the
    /// end; the lower layer is always lava at -54. Naming the upper layer
    /// `water` in this struct was fine while only the overworld existed.
    #[must_use]
    pub const fn with_default_fluid(sea_level: i32, default_fluid: Substance) -> Self {
        Self {
            lava: FluidStatus::new(-54, Substance::Lava),
            water: FluidStatus::new(sea_level, default_fluid),
        }
    }

    /// `computeFluid` — lava wins only below *both* thresholds.
    #[must_use]
    pub const fn compute_fluid(&self, block_y: i32) -> FluidStatus {
        let boundary = if self.lava.level < self.water.level {
            self.lava.level
        } else {
            self.water.level
        };
        if block_y < boundary {
            self.lava
        } else {
            self.water
        }
    }
}

/// `Aquifer.createDisabled(...).computeSubstance`.
///
/// Note the comparison is strictly `> 0.0`, so a density of exactly zero —
/// and negative zero — is *not* solid.
///
/// # Why `None` rather than `Substance::Solid`
///
/// Vanilla returns a nullable `BlockState` and `null` means "I decline;
/// let the next rule decide". `MaterialRuleList` runs the aquifer first and
/// `OreVeinifier` second, taking the first non-null — so ore veins appear
/// **only** where the aquifer returned null. Collapsing null into `Solid`
/// here would read as the aquifer having claimed the block, and every
/// copper and iron vein in the world would vanish.
///
/// A caller that genuinely wants a block substitutes its own default block
/// for `None`; that is the generator's `defaultBlock`, not necessarily
/// stone.
#[must_use]
pub fn compute_substance_disabled(
    picker: &GlobalFluidPicker,
    block_y: i32,
    density: f64,
) -> Option<Substance> {
    if density > 0.0 {
        None
    } else {
        Some(picker.compute_fluid(block_y).at(block_y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEA_LEVEL: i32 = 63;

    fn picker() -> GlobalFluidPicker {
        GlobalFluidPicker::overworld(SEA_LEVEL)
    }

    #[test]
    fn positive_density_declines_at_any_height() {
        // None, not Solid: the aquifer is saying "not mine", which is what
        // lets the ore veinifier have the block.
        for y in [-64, -55, 0, 62, 63, 200, 319] {
            assert_eq!(
                compute_substance_disabled(&picker(), y, 0.001),
                None,
                "y={y}"
            );
        }
    }

    #[test]
    fn zero_density_is_claimed_by_the_fluid_rule() {
        // `> 0.0` is strict, and -0.0 compares equal to 0.0, so both fall
        // through to the fluid rule rather than being declined.
        assert!(compute_substance_disabled(&picker(), 0, 0.0).is_some());
        assert!(compute_substance_disabled(&picker(), 0, -0.0).is_some());
    }

    #[test]
    fn sea_level_is_exclusive() {
        // The topmost water block is one below sea level; y == sea level is
        // air. Off by one here and every ocean gains a surface layer.
        assert_eq!(
            compute_substance_disabled(&picker(), SEA_LEVEL - 1, -1.0),
            Some(Substance::Water)
        );
        assert_eq!(
            compute_substance_disabled(&picker(), SEA_LEVEL, -1.0),
            Some(Substance::Air)
        );
    }

    #[test]
    fn lava_only_below_its_own_level() {
        assert_eq!(
            compute_substance_disabled(&picker(), -55, -1.0),
            Some(Substance::Lava)
        );
        assert_eq!(
            compute_substance_disabled(&picker(), -54, -1.0),
            Some(Substance::Water),
            "at the lava level it is water, not lava"
        );
    }

    #[test]
    fn above_the_world_is_air() {
        assert_eq!(
            compute_substance_disabled(&picker(), 319, -1.0),
            Some(Substance::Air)
        );
    }
}
