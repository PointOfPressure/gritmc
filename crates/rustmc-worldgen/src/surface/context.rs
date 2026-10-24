//! The state a surface rule is evaluated against.
//!
//! Reference: `SurfaceRules.Context`.
//!
//! # The caching is behaviour, not an optimisation
//!
//! Vanilla's `Context` carries two counters, `lastUpdateXZ` and
//! `lastUpdateY`, and every condition memoizes against one of them. That is
//! the same shape as `cacheOnce` in `density/`, and it earned the same
//! warning: a value recomputed at the wrong moment is not slower, it is
//! *different*, because the inputs move underneath it.
//!
//! Concretely, `updateXZ` bumps **both** counters and `updateY` bumps only
//! the y one. So a per-column value stays valid across every y in that
//! column, and moving to a new column invalidates the per-y values too. Get
//! that backwards and a rule reads the previous column's surface depth.
//!
//! This port keeps the counters rather than replacing them with `Option`
//! resets, because the asymmetry above is exactly what the counters encode
//! and it is easy to lose when translating to something that looks tidier.

/// Per-position inputs the rules read.
///
/// Deliberately plain data: the values come from the surface system, the
/// chunk and the biome source, and this module's job is to say when they are
/// valid rather than to fetch them.
#[derive(Clone, Debug)]
pub struct SurfaceContext {
    // ---- world shape, constant for the whole run ----
    pub min_y: i32,
    pub height: i32,
    pub sea_level: i32,

    // ---- per column ----
    block_x: i32,
    block_z: i32,
    /// `system.getSurfaceDepth(x, z)` — how thick the surface layer is here.
    surface_depth: i32,
    /// `getSurfaceSecondary`, sampled lazily and cached per column.
    surface_secondary: Option<f64>,
    /// The lowest preliminary surface in this 16-block cell, cached per
    /// column.
    min_surface_level: Option<i32>,

    // ---- per y within the column ----
    block_y: i32,
    /// `Integer.MIN_VALUE` means "no water in this column", which is a
    /// distinct case from "water at the bottom of the world".
    water_height: i32,
    stone_depth_above: i32,
    stone_depth_below: i32,
    /// The interned biome, memoised per y exactly as vanilla memoises its
    /// `Holder<Biome>`. An integer rather than a name: see
    /// [`crate::surface::BiomeId`] for why that cannot change an answer.
    biome: Option<crate::surface::BiomeId>,

    /// Bumped by `update_xz`; keys the per-column caches.
    xz_generation: u64,
    /// Bumped by both `update_xz` and `update_y`; keys the per-y caches.
    y_generation: u64,
}

/// No water in this column — `Integer.MIN_VALUE` in vanilla.
pub const NO_WATER: i32 = i32::MIN;

impl SurfaceContext {
    #[must_use]
    pub const fn new(min_y: i32, height: i32, sea_level: i32) -> Self {
        Self {
            min_y,
            height,
            sea_level,
            block_x: 0,
            block_z: 0,
            surface_depth: 0,
            surface_secondary: None,
            min_surface_level: None,
            block_y: 0,
            water_height: NO_WATER,
            stone_depth_above: 0,
            stone_depth_below: 0,
            biome: None,
            xz_generation: 1,
            y_generation: 1,
        }
    }

    /// `updateXZ` — move to a new column.
    ///
    /// Bumps **both** generations. The per-y values belong to the old column
    /// and must not survive into this one.
    pub fn update_xz(&mut self, block_x: i32, block_z: i32, surface_depth: i32) {
        self.xz_generation += 1;
        self.y_generation += 1;
        self.block_x = block_x;
        self.block_z = block_z;
        self.surface_depth = surface_depth;
        self.surface_secondary = None;
        self.min_surface_level = None;
    }

    /// `updateY` — move down the current column.
    ///
    /// Bumps only the y generation, so the column's surface depth and
    /// secondary noise stay cached.
    pub fn update_y(
        &mut self,
        stone_depth_above: i32,
        stone_depth_below: i32,
        water_height: i32,
        block_y: i32,
    ) {
        self.y_generation += 1;
        self.biome = None;
        self.block_y = block_y;
        self.water_height = water_height;
        self.stone_depth_above = stone_depth_above;
        self.stone_depth_below = stone_depth_below;
    }

    #[must_use]
    pub const fn block_x(&self) -> i32 {
        self.block_x
    }

    #[must_use]
    pub const fn block_y(&self) -> i32 {
        self.block_y
    }

    #[must_use]
    pub const fn block_z(&self) -> i32 {
        self.block_z
    }

    #[must_use]
    pub const fn surface_depth(&self) -> i32 {
        self.surface_depth
    }

    #[must_use]
    pub const fn water_height(&self) -> i32 {
        self.water_height
    }

    #[must_use]
    pub const fn stone_depth_above(&self) -> i32 {
        self.stone_depth_above
    }

    #[must_use]
    pub const fn stone_depth_below(&self) -> i32 {
        self.stone_depth_below
    }

    /// The per-column secondary noise, computed once and reused.
    pub fn surface_secondary(&mut self, compute: impl FnOnce(i32, i32) -> f64) -> f64 {
        match self.surface_secondary {
            Some(value) => value,
            None => {
                let value = compute(self.block_x, self.block_z);
                self.surface_secondary = Some(value);
                value
            }
        }
    }

    /// The per-column minimum surface level, computed once and reused.
    pub fn min_surface_level(&mut self, compute: impl FnOnce(i32, i32) -> i32) -> i32 {
        match self.min_surface_level {
            Some(value) => value,
            None => {
                let value = compute(self.block_x, self.block_z);
                self.min_surface_level = Some(value);
                value
            }
        }
    }

    /// The biome at this exact position, computed once per y.
    ///
    /// Returns the id by value. The old shape returned `&str` borrowed from
    /// the cache, which forced every caller to hold a borrow of the whole
    /// context across the comparison; an id is `Copy`, so the borrow ends
    /// here and the caller is free to touch the context again.
    pub fn biome(
        &mut self,
        compute: impl FnOnce(i32, i32, i32) -> crate::surface::BiomeId,
    ) -> crate::surface::BiomeId {
        match self.biome {
            Some(id) => id,
            None => {
                let id = compute(self.block_x, self.block_y, self.block_z);
                self.biome = Some(id);
                id
            }
        }
    }

    /// Set the biome directly, for callers that already know it.
    pub fn set_biome(&mut self, biome: crate::surface::BiomeId) {
        self.biome = Some(biome);
    }

    #[must_use]
    pub const fn xz_generation(&self) -> u64 {
        self.xz_generation
    }

    #[must_use]
    pub const fn y_generation(&self) -> u64 {
        self.y_generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> SurfaceContext {
        SurfaceContext::new(-64, 384, 63)
    }

    #[test]
    fn moving_down_a_column_keeps_the_column_cache() {
        // The whole point of two counters: surface depth and the secondary
        // noise are column properties, so walking down y must not resample
        // them. Vanilla samples the secondary noise once per column.
        let mut ctx = context();
        ctx.update_xz(5, 9, 3);
        let mut calls = 0;
        for y in (0..10).rev() {
            ctx.update_y(0, 0, NO_WATER, y);
            let value = ctx.surface_secondary(|_, _| {
                calls += 1;
                0.5
            });
            assert_eq!(value, 0.5);
        }
        assert_eq!(calls, 1, "the secondary noise was resampled down a column");
    }

    #[test]
    fn moving_to_a_new_column_invalidates_everything() {
        // updateXZ bumps BOTH counters. If it only bumped the xz one, a rule
        // would read the previous column's biome — plausible-looking output,
        // wrong everywhere the biome changes.
        let mut ctx = context();
        ctx.update_xz(0, 0, 3);
        ctx.update_y(0, 0, NO_WATER, 70);
        assert_eq!(ctx.biome(|_, _, _| 7), 7);
        assert_eq!(ctx.surface_secondary(|_, _| 1.0), 1.0);

        let before = (ctx.xz_generation(), ctx.y_generation());
        ctx.update_xz(16, 16, 4);
        let after = (ctx.xz_generation(), ctx.y_generation());
        assert!(after.0 > before.0, "xz generation must advance");
        assert!(after.1 > before.1, "y generation must advance too");
        assert_eq!(
            ctx.surface_secondary(|_, _| 2.0),
            2.0,
            "column cache cleared"
        );
    }

    #[test]
    fn the_biome_is_resampled_per_y_not_per_column() {
        // Biomes are 3D — a cave 40 blocks under a plains hill can be lush
        // caves — so this one deliberately does NOT survive a y move.
        let mut ctx = context();
        ctx.update_xz(0, 0, 3);
        ctx.update_y(0, 0, NO_WATER, 70);
        assert_eq!(ctx.biome(|_, _, _| 11), 11);
        ctx.update_y(0, 0, NO_WATER, 20);
        assert_eq!(ctx.biome(|_, _, _| 22), 22);
    }

    #[test]
    fn no_water_is_a_sentinel_not_a_height() {
        // `water` conditions test `waterHeight == Integer.MIN_VALUE` first
        // and treat it as "always above water". Storing 0 or min_y instead
        // would make dry columns behave like flooded ones.
        let mut ctx = context();
        ctx.update_xz(0, 0, 3);
        ctx.update_y(0, 0, NO_WATER, 70);
        assert_eq!(ctx.water_height(), NO_WATER);
        assert!(ctx.water_height() < ctx.min_y, "below every real block");
    }
}
