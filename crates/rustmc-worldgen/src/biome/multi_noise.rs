//! Position to biome.
//!
//! Reference: `net.minecraft.world.level.biome.{Climate.Sampler,
//! MultiNoiseBiomeSource}`.
//!
//! Biomes are stored and sampled at **quart** resolution — one value per 4x4x4
//! block cell — which is why a biome border is never finer than four blocks
//! and why `sample` takes quart coordinates rather than block ones.
//!
//! # The six axes are not the six router fields they are named after
//!
//! `RandomState`'s sampler wires the router up like this:
//!
//! | climate axis      | router field  |
//! |-------------------|---------------|
//! | temperature       | `temperature` |
//! | **humidity**      | `vegetation`  |
//! | continentalness   | `continents`  |
//! | erosion           | `erosion`     |
//! | depth             | `depth`       |
//! | **weirdness**     | `ridges`      |
//!
//! Two of the six are renamed in transit. Wiring `ridges` to the axis that
//! sounds like it — there isn't one — or reading `humidity` off a
//! nonexistent humidity noise gives a world that generates fine and has the
//! wrong biomes everywhere.

use super::climate::{quantize_coord, ParameterList, TargetPoint};
use crate::density::flat_cache::FlatCaches;
use crate::density::{DensityFunction, FunctionContext};

/// `Climate.Sampler` — the six density functions that define climate space.
#[derive(Clone, Debug)]
pub struct ClimateSampler {
    pub temperature: DensityFunction,
    pub humidity: DensityFunction,
    pub continentalness: DensityFunction,
    pub erosion: DensityFunction,
    pub depth: DensityFunction,
    pub weirdness: DensityFunction,
}

impl ClimateSampler {
    /// `RandomState.sampler()` — build from a router, with vanilla's
    /// vegetation-to-humidity and ridges-to-weirdness renaming.
    #[must_use]
    pub fn from_router(router: &crate::density::router::OverworldRouter) -> Self {
        Self {
            temperature: router.temperature.clone(),
            humidity: router.vegetation.clone(),
            continentalness: router.continents.clone(),
            erosion: router.erosion.clone(),
            depth: router.depth.clone(),
            weirdness: router.ridges.clone(),
        }
    }

    /// `Climate.Sampler.sample` — quart coordinates in, climate point out.
    ///
    /// Each density function returns a `double` and is narrowed to `float`
    /// **before** quantisation. That narrowing is a real precision step, not
    /// bookkeeping: quantising the double directly would occasionally land on
    /// the other side of a box edge and pick a different biome.
    #[must_use]
    pub fn sample(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> TargetPoint {
        self.sample_in(&FlatCaches::none(), quart_x, quart_y, quart_z)
    }

    /// The same, reading every `flat_cache` marker out of `flat`.
    ///
    /// A `FlatCaches` that does not cover the position, or an empty one, is
    /// not a special case: the marker falls through and computes exactly, so
    /// this and [`Self::sample`] agree everywhere by construction.
    #[must_use]
    pub fn sample_in(
        &self,
        flat: &FlatCaches,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
    ) -> TargetPoint {
        let ctx = FunctionContext::with_flat_caches(quart_x << 2, quart_y << 2, quart_z << 2, flat);
        let axis = |f: &DensityFunction| quantize_coord(f.compute(ctx) as f32);
        TargetPoint {
            temperature: axis(&self.temperature),
            humidity: axis(&self.humidity),
            continentalness: axis(&self.continentalness),
            erosion: axis(&self.erosion),
            depth: axis(&self.depth),
            weirdness: axis(&self.weirdness),
        }
    }
}

/// One chunk's climate sampler, with vanilla's per-chunk 2D caches live.
///
/// # This is what vanilla does, not an approximation of it
///
/// `NoiseBasedChunkGenerator#doCreateBiomes` fills a chunk's biomes through
/// `NoiseChunk#cachedClimateSampler`, and that method maps **each of the six
/// router functions** through `NoiseChunk::wrapNew` before handing them to
/// `Climate.Sampler` (javap, 26.2 server jar). `wrapNew` is what turns a
/// `flat_cache` marker into a filled 5x5 quart table for the chunk. So
/// vanilla evaluates `continents`, `erosion`, `ridges` and the terrain
/// `offset` behind `depth` **25 times per chunk**; a port that samples them
/// through a bare context evaluates each of them once per cell, 1,536 times,
/// and the whole difference is wasted work.
///
/// # Why it cannot move a biome
///
/// [`FlatCaches::get`] snaps its lookup to quart resolution and returns the
/// value computed at the snapped position. The biome fill asks at
/// `quart << 2`, which is already quart-aligned, so the snap is the identity
/// and the cached value is the value the direct evaluation would have
/// produced. Positions the table does not cover fall through and compute.
/// Nothing about the search that consumes the point changes.
///
/// # The two axes vanilla leaves uncached, and why caching them is still exact
///
/// `temperature` and `vegetation` are built inline in `NoiseRouterData
/// #overworld` rather than registered, so they carry no `flat_cache` marker
/// and vanilla evaluates each of them once per cell. Both are
/// `shiftedNoise2d`: `y_scale` is `0.0` and `shift_y` is `constant(0.0)`, so
/// the sampled y is `block_y * 0.0 + 0.0`, which is `+0.0` for every block y
/// including negative ones. **The value cannot depend on y**, so a marker
/// wrapped around them here — filled at the same quart column, at y = 0 —
/// returns the number the direct evaluation returns. That is 16 evaluations
/// a chunk instead of 1,536, and `y_is_not_an_input_to_the_2d_climate_axes`
/// is the test that says so against the real router rather than by reading
/// the constructor.
///
/// The wrapping happens on **this copy** of the sampler, not in the router.
/// The router's are deliberately unwrapped because the surface rules and the
/// decoration filter sample climate at positions that are *not* quart
/// aligned, where a `flat_cache` would genuinely change the answer.
pub struct ChunkClimate {
    sampler: ClimateSampler,
    wrapped: Vec<DensityFunction>,
    flat: FlatCaches,
}

impl ChunkClimate {
    /// Number the markers once, for a worker that will walk many chunks.
    #[must_use]
    pub fn new(sampler: &ClimateSampler) -> Self {
        let mut sampler = sampler.clone();
        // See the type docs: these two are the only axes with no marker of
        // their own, and both are y-independent, so a chunk-local one is
        // value-transparent.
        sampler.temperature = crate::density::build::flat_cache(sampler.temperature);
        sampler.humidity = crate::density::build::flat_cache(sampler.humidity);
        let wrapped = FlatCaches::prepare(&mut [
            &mut sampler.temperature,
            &mut sampler.humidity,
            &mut sampler.continentalness,
            &mut sampler.erosion,
            &mut sampler.depth,
            &mut sampler.weirdness,
        ]);
        Self {
            sampler,
            wrapped,
            flat: FlatCaches::none(),
        }
    }

    /// Refill the tables for a chunk. Must be called before sampling it.
    pub fn move_to(&mut self, chunk_x: i32, chunk_z: i32) {
        self.flat = FlatCaches::fill(&self.wrapped, chunk_x * 16, chunk_z * 16, 16);
    }

    #[must_use]
    pub fn sample(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> TargetPoint {
        self.sampler
            .sample_in(&self.flat, quart_x, quart_y, quart_z)
    }
}

/// `MultiNoiseBiomeSource` — a climate sampler plus a biome table.
#[derive(Debug)]
pub struct MultiNoiseBiomeSource<T> {
    sampler: ClimateSampler,
    parameters: ParameterList<T>,
}

impl<T> MultiNoiseBiomeSource<T> {
    #[must_use]
    pub const fn new(sampler: ClimateSampler, parameters: ParameterList<T>) -> Self {
        Self {
            sampler,
            parameters,
        }
    }

    /// `getNoiseBiome`.
    ///
    /// `&mut self` because the underlying search is order-dependent on exact
    /// ties — see [`super::climate`]. Vanilla hides this in a `ThreadLocal`;
    /// here it is visible, so a caller that needs to reproduce vanilla's
    /// answers knows it must also reproduce vanilla's query order.
    pub fn get_noise_biome(&mut self, quart_x: i32, quart_y: i32, quart_z: i32) -> &T {
        let target = self.sampler.sample(quart_x, quart_y, quart_z);
        self.parameters.find_value(target)
    }

    #[must_use]
    pub const fn sampler(&self) -> &ClimateSampler {
        &self.sampler
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::density::build;

    fn sampler(values: [f64; 6]) -> ClimateSampler {
        ClimateSampler {
            temperature: build::constant(values[0]),
            humidity: build::constant(values[1]),
            continentalness: build::constant(values[2]),
            erosion: build::constant(values[3]),
            depth: build::constant(values[4]),
            weirdness: build::constant(values[5]),
        }
    }

    /// A sampler with the router's real shapes: two `shiftedNoise2d` axes
    /// with no marker (temperature, vegetation), three flat-cached ones
    /// (continents, erosion, ridges) and a y-dependent depth. Constants prove
    /// nothing about caching, and the first version of these tests used them.
    fn realistic_sampler() -> ClimateSampler {
        use crate::density::NoiseHolder;
        use crate::noise::{NoiseParameters, NoiseSeeding};
        let seeding = NoiseSeeding::new(20_260_722);
        let params = NoiseParameters::new(-6, vec![1.0, 1.0, 1.0]);
        let mut nth = 0;
        let mut noise2d = || {
            nth += 1;
            let holder =
                NoiseHolder::bound(seeding.instantiate(&format!("minecraft:axis{nth}"), &params));
            build::shifted_noise(
                build::constant(f64::from(nth)),
                build::constant(0.0),
                build::constant(-f64::from(nth)),
                0.25,
                0.0,
                holder,
            )
        };
        ClimateSampler {
            temperature: noise2d(),
            humidity: noise2d(),
            continentalness: build::flat_cache(noise2d()),
            erosion: build::flat_cache(noise2d()),
            weirdness: build::flat_cache(noise2d()),
            depth: build::add(
                build::y_clamped_gradient(-64, 320, 1.5, -1.5),
                build::flat_cache(noise2d()),
            ),
        }
    }

    /// The claim [`ChunkClimate`] rests on for the two uncached axes: a
    /// `shiftedNoise2d` returns the same value at every y, because `y_scale`
    /// is `0.0`. Checked at negative y as well — `-64 * 0.0` is `-0.0`, and
    /// a port that summed it differently could land a ulp away.
    #[test]
    fn y_is_not_an_input_to_the_2d_climate_axes() {
        let s = realistic_sampler();
        let base = s.sample(9, 0, -7);
        for quart_y in [-16, -5, -1, 1, 20, 79] {
            let at = s.sample(9, quart_y, -7);
            assert_eq!(at.temperature, base.temperature, "temperature at {quart_y}");
            assert_eq!(at.humidity, base.humidity, "humidity at {quart_y}");
        }
        // And the control: depth *is* y-dependent, so a test that saw no
        // difference anywhere would be measuring a dead sampler.
        assert_ne!(s.sample(9, 79, -7).depth, base.depth);
    }

    /// The whole class, against the thing it replaces: every cell of a chunk
    /// must sample identically through the per-chunk caches and through a
    /// bare context. This is the unit-level twin of the region-file `cmp`.
    #[test]
    fn the_per_chunk_caches_change_no_climate_value() {
        let s = realistic_sampler();
        let mut climate = ChunkClimate::new(&s);
        for (chunk_x, chunk_z) in [(0, 0), (-1, 3), (37, -42)] {
            climate.move_to(chunk_x, chunk_z);
            for (quart_x, quart_y, quart_z) in
                crate::stage::biome::cells(-64, 384, chunk_x, chunk_z)
            {
                assert_eq!(
                    climate.sample(quart_x, quart_y, quart_z),
                    s.sample(quart_x, quart_y, quart_z),
                    "cell {quart_x} {quart_y} {quart_z} of chunk {chunk_x} {chunk_z}"
                );
            }
        }
    }

    #[test]
    fn quart_coordinates_are_scaled_to_blocks() {
        // A y-dependent function proves the coordinate actually reaches it:
        // quart 3 must be sampled at block 12, not block 3.
        let s = ClimateSampler {
            temperature: build::y_clamped_gradient(-1000, 1000, -1000.0, 1000.0),
            ..sampler([0.0; 6])
        };
        assert_eq!(s.sample(0, 3, 0).temperature, quantize_coord(12.0));
        assert_eq!(s.sample(0, -1, 0).temperature, quantize_coord(-4.0));
    }

    #[test]
    fn the_double_is_narrowed_to_float_before_quantising() {
        // A value whose f32 and f64 quantisations differ. Narrowing first is
        // what vanilla does, and the two land on different integers, so this
        // is observable rather than theoretical.
        // Found by search, not guessed: the first attempt used a value where
        // both routes agree and the test proved nothing. Here the f32 route
        // rounds up across the integer boundary (17816) and the f64 route
        // truncates below it (17815) — one whole quantisation step, which is
        // enough to cross a biome box edge.
        let awkward = 1.781_599_939_445_625_7_f64;
        let via_f32 = quantize_coord(awkward as f32);
        let via_f64 = (awkward * 10000.0) as i64;
        assert_ne!(
            via_f32, via_f64,
            "picked a value where the two agree; the test proves nothing"
        );

        let s = sampler([awkward, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(s.sample(0, 0, 0).temperature, via_f32);
    }

    #[test]
    fn the_axes_are_wired_in_vanillas_order() {
        // Each axis gets a distinct constant, so a swapped pair shows up.
        let s = sampler([0.1, 0.2, 0.3, 0.4, 0.5, 0.6]);
        let t = s.sample(0, 0, 0);
        assert_eq!(t.temperature, quantize_coord(0.1));
        assert_eq!(t.humidity, quantize_coord(0.2));
        assert_eq!(t.continentalness, quantize_coord(0.3));
        assert_eq!(t.erosion, quantize_coord(0.4));
        assert_eq!(t.depth, quantize_coord(0.5));
        assert_eq!(t.weirdness, quantize_coord(0.6));
    }
}
