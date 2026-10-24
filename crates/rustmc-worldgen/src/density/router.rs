//! The overworld noise router — where a world seed becomes terrain density.
//!
//! Reference: `net.minecraft.world.level.levelgen.{NoiseRouterData,
//! RandomState}`.
//!
//! Everything below this module is provable in isolation: a noise value is
//! right or wrong on its own. The router is the first place where a wrong
//! answer looks *plausible* — it will generate, load, and look like
//! Minecraft while being a different world. The only defence is the vanilla
//! vector, so nothing here is trusted without one.
//!
//! # Shape of the graph
//!
//! ```text
//! shift_x / shift_z            offset noise, cached per column
//!   -> continents / erosion / ridges     2D fields sampled at shifted xz
//!        -> ridges_folded                peaks-and-valleys fold
//!             -> offset / factor / jaggedness   (terrain shaper splines)
//!                  -> depth = offset + vertical ramp
//!                       -> initial_density = f(factor, depth + jaggedness)
//!                            -> sloped_cheese = + 3D blended noise
//! ```
//!
//! Vanilla stores each of those as a named registry entry so datapacks can
//! reference them and so shared subgraphs are evaluated once. Ours is a
//! tree, so shared nodes are duplicated — semantically identical, and the
//! duplication is exactly what the cache markers exist to remove once
//! `NoiseChunk` gives them somewhere to cache.

use super::{build, terrain, DensityFunction};
use crate::noise::{BlendedNoise, NoiseParameters, NoiseSeeding};

/// `NoiseRouterData.BLENDING_FACTOR`.
const BLENDING_FACTOR: f64 = 10.0;

/// Noise names the overworld router resolves. Kept as constants so a typo
/// is a compile error rather than a different world.
pub mod names {
    pub const SHIFT: &str = "minecraft:offset";
    pub const CONTINENTALNESS: &str = "minecraft:continentalness";
    pub const EROSION: &str = "minecraft:erosion";
    pub const RIDGE: &str = "minecraft:ridge";
    pub const JAGGED: &str = "minecraft:jagged";
    /// The re-seed key for the 3D terrain noise — see
    /// [`OverworldRouter::new`].
    pub const TERRAIN: &str = "minecraft:terrain";

    pub const TEMPERATURE: &str = "minecraft:temperature";
    pub const VEGETATION: &str = "minecraft:vegetation";

    /// The nether's two climate noises. Distinct registry entries from the
    /// overworld's, and the **only** two noises in the game that
    /// `RandomState$1NoiseWiringHelper.visitNoise` special-cases by identity
    /// — see [`super::OverworldRouter::nether`].
    pub const NETHER_TEMPERATURE: &str = "minecraft:nether/temperature";
    pub const NETHER_VEGETATION: &str = "minecraft:nether/vegetation";

    pub const AQUIFER_BARRIER: &str = "minecraft:aquifer_barrier";
    pub const AQUIFER_FLUID_LEVEL_FLOODEDNESS: &str = "minecraft:aquifer_fluid_level_floodedness";
    pub const AQUIFER_FLUID_LEVEL_SPREAD: &str = "minecraft:aquifer_fluid_level_spread";
    pub const AQUIFER_LAVA: &str = "minecraft:aquifer_lava";

    pub const ORE_VEININESS: &str = "minecraft:ore_veininess";
    pub const ORE_VEIN_A: &str = "minecraft:ore_vein_a";
    pub const ORE_VEIN_B: &str = "minecraft:ore_vein_b";
    pub const ORE_GAP: &str = "minecraft:ore_gap";
}

/// The y band ore veins may occupy, from `OreVeinifier.VeinType`:
/// copper spans 0..=50 and iron -60..=-8, so the union is -60..=50.
///
/// Vanilla derives these with a stream `min`/`max` over the enum rather than
/// writing them down, so adding a vein type would widen the band and change
/// the *density graph*, not just the ore table. Kept as constants here with
/// the derivation stated, because the enum does not exist yet.
const VEIN_MIN_Y: i32 = -60;
const VEIN_MAX_Y: i32 = 50;

/// The registry's `minecraft:y` function —
/// `yClampedGradient(MIN_Y*2, MAX_Y*2, MIN_Y*2, MAX_Y*2)`.
///
/// An identity ramp over twice the widest legal dimension, so inside any
/// real world it is simply y. Both `noodle` and the ore-vein band gate on
/// it. The bottom was previously written as -128, which is indistinguishable
/// anywhere a block can exist and wrong regardless: it would clamp a
/// dimension reaching below -128, and a constant that is only right because
/// nothing tests it is a bug waiting for a deeper world.
fn y_identity_ramp() -> DensityFunction {
    let below = crate::level::MIN_Y * 2;
    let above = crate::level::MAX_Y * 2;
    build::y_clamped_gradient(below, above, f64::from(below), f64::from(above))
}

/// `NoiseSettings.OVERWORLD_NOISE_SETTINGS.getCellHeight()` — the step
/// `preliminary_surface_level` scans the column in, which is why that value
/// is only ever a multiple of 8.
const OVERWORLD_CELL_HEIGHT: i32 = 8;

/// Parameters for every noise the router needs, by full identifier.
///
/// Supplied by the caller rather than loaded here: the extracted registry
/// is Mojang-derived and lives outside the repo, so `rustmc-data` owns
/// loading it and this module stays a pure function of its inputs.
#[derive(Clone, Debug, Default)]
pub struct OverworldNoises {
    by_name: std::collections::BTreeMap<String, NoiseParameters>,
}

impl OverworldNoises {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with(mut self, name: &str, parameters: NoiseParameters) -> Self {
        self.by_name.insert(name.to_string(), parameters);
        self
    }

    /// # Panics
    /// If the noise is absent — a missing noise is a wrong world, never a
    /// default.
    #[must_use]
    pub fn get(&self, name: &str) -> &NoiseParameters {
        self.by_name
            .get(name)
            .unwrap_or_else(|| panic!("no parameters supplied for noise {name}"))
    }
}

/// The overworld router's terrain path.
#[derive(Clone, Debug)]
pub struct OverworldRouter {
    /// `final_density` — terrain, caves, slide and post-processing. This is
    /// what the chunk generator turns into blocks.
    pub final_density: DensityFunction,
    /// `sloped_cheese` — terrain density before caves and the slide.
    pub sloped_cheese: DensityFunction,
    pub depth: DensityFunction,
    pub factor: DensityFunction,
    pub jaggedness: DensityFunction,
    pub offset: DensityFunction,
    pub continents: DensityFunction,
    pub erosion: DensityFunction,
    pub ridges: DensityFunction,
    pub ridges_folded: DensityFunction,

    // The four the aquifer reads. Not terrain: they decide which caves hold
    // water, where each local water table sits, and whether it is lava.
    pub barrier: DensityFunction,
    pub fluid_level_floodedness: DensityFunction,
    pub fluid_level_spread: DensityFunction,
    pub lava: DensityFunction,

    // Climate. Not used by terrain at all — these feed the biome source.
    pub temperature: DensityFunction,
    pub vegetation: DensityFunction,

    /// A cheap estimate of the surface height, to cell resolution.
    ///
    /// The aquifer needs to know roughly where the ground is to decide
    /// whether a water table is buried or breaks out of a hillside, and it
    /// cannot afford the real terrain graph for that. This reconstructs an
    /// approximate density from `factor`/`offset` alone and scans down it
    /// with [`super::build::find_top_surface`].
    pub preliminary_surface_level: DensityFunction,

    // Ore veins — the copper/iron ribbons in deepslate, applied after the
    // aquifer has chosen a substance.
    pub vein_toggle: DensityFunction,
    pub vein_ridged: DensityFunction,
    pub vein_gap: DensityFunction,
}

impl OverworldRouter {
    /// `NoiseRouterData.nether` — `noNewCaves(slideNetherLike(0, 128))`.
    ///
    /// The type's name is historical: it is vanilla's `NoiseRouter`, and the
    /// overworld was the only dimension that had one when it was written.
    ///
    /// # Why this is nothing like [`Self::new`]
    ///
    /// `noNewCaves` sets **thirteen of the fifteen** router slots to
    /// `zero()`: no continents, erosion, ridges, depth, barrier, aquifer
    /// levels, ore veins, or preliminary surface level. Terrain is one
    /// `BlendedNoise` through a slide and `postProcess`, and the only other
    /// live slots are the two climate fields. That is checkable rather than
    /// remembered — Mojang's own datagen writes the whole router out, and
    /// every field except `temperature`, `vegetation` and `final_density` is
    /// the literal `0.0` in
    /// `worldgen/noise_settings/nether.json`.
    ///
    /// # The seeding, which is where a plausible wrong world comes from
    ///
    /// `nether.json` sets `legacy_random_source: true`, so `RandomState`'s
    /// `random` is `new LegacyRandomSource(seed).forkPositional()` rather
    /// than Xoroshiro. **No noise in the nether's router reaches that
    /// factory**, because `RandomState$1NoiseWiringHelper` intercepts all
    /// three first (read out of the bytecode, 2026-07-29):
    ///
    /// ```text
    ///   newLegacyInstance(k)          = new LegacyRandomSource(seed + k)
    ///   visitNoise(TEMPERATURE_NETHER) -> createLegacyNetherBiome(newLegacyInstance(0))
    ///   visitNoise(VEGETATION_NETHER)  -> createLegacyNetherBiome(newLegacyInstance(1))
    ///   wrapNew(BlendedNoise), legacy  -> withNewRandom(newLegacyInstance(0))
    /// ```
    ///
    /// So the terrain noise and the temperature noise are built from two
    /// *separate, fresh* `LegacyRandomSource(seed)` instances — they do not
    /// share a stream, and neither is forked. Vegetation is `seed + 1`.
    #[must_use]
    pub fn nether(world_seed: i64, noises: &OverworldNoises) -> Self {
        use crate::noise::NormalNoise;
        use crate::rng::LegacyRandomSource;

        // `slideNetherLike(functions, 0, 128)` -> slide(base, 0, 128, 24, 0,
        // 0.9375, -8, 24, 2.5). Cross-checked against the datagen JSON: the
        // top gradient runs y 104 -> 128 (= 0 + 128 - 24 .. 0 + 128 - 0) and
        // the bottom y -8 -> 24, with targets 0.9375 and 2.5.
        let mut terrain_random = LegacyRandomSource::new(world_seed);
        let base_3d_noise = build::old_blended_noise(BlendedNoise::new(
            &mut terrain_random,
            0.25,
            0.375,
            80.0,
            60.0,
            8.0,
        ));
        let final_density = build::post_process(build::slide(
            base_3d_noise,
            0,
            128,
            24,
            0,
            0.9375,
            -8,
            24,
            2.5,
        ));

        // `shiftedNoise2d` with **zero** shifts — not the overworld's
        // `SHIFT_X`/`SHIFT_Z` offset noises. The datagen output is explicit:
        // `"shift_x": 0.0, "shift_y": 0.0, "shift_z": 0.0`.
        let climate = |name: &str, offset: i64| {
            let params = noises.get(name);
            let mut random = LegacyRandomSource::new(world_seed + offset);
            let noise = NormalNoise::create_legacy_nether_biome(
                &mut random,
                params.first_octave,
                &params.amplitudes,
            );
            build::shifted_noise(
                build::constant(0.0),
                build::constant(0.0),
                build::constant(0.0),
                0.25,
                0.0,
                super::NoiseHolder::bound(noise),
            )
        };
        let temperature = climate(names::NETHER_TEMPERATURE, 0);
        let vegetation = climate(names::NETHER_VEGETATION, 1);

        let zero = || build::constant(0.0);
        Self {
            final_density,
            // `sloped_cheese` has no nether counterpart; the router record
            // has no such field and nothing in the fill reads it. Zero is
            // the honest answer, not a placeholder terrain.
            sloped_cheese: zero(),
            depth: zero(),
            factor: zero(),
            jaggedness: zero(),
            offset: zero(),
            continents: zero(),
            erosion: zero(),
            ridges: zero(),
            ridges_folded: zero(),
            barrier: zero(),
            fluid_level_floodedness: zero(),
            fluid_level_spread: zero(),
            lava: zero(),
            temperature,
            vegetation,
            preliminary_surface_level: zero(),
            vein_toggle: zero(),
            vein_ridged: zero(),
            vein_gap: zero(),
        }
    }

    /// `NoiseRouterData.end` — `noNewCaves(slideEndLike(sloped_cheese, 0, 128))`.
    ///
    /// # Why this takes no noise parameters
    ///
    /// The End's router names **no** `NormalNoise` at all. Every climate slot
    /// is the literal `0.0` in `worldgen/noise_settings/end.json`, `barrier`
    /// and the three aquifer slots are `0.0`, and the two live functions are
    /// `erosion` and `final_density`. Both are built from
    /// `EndIslandDensityFunction` and one `BlendedNoise` — no named noise, so
    /// no parameter set, so no argument.
    ///
    /// # The two functions, from the datagen JSON
    ///
    /// ```text
    ///   end/base_3d_noise = old_blended_noise(0.25, 0.25, 80.0, 160.0, 4.0)
    ///   end/sloped_cheese = add(end_islands, end/base_3d_noise)
    ///   erosion           = cache_2d(end_islands)
    ///   final_density     = postProcess(slide(sloped_cheese,
    ///                                         0, 128, 72, -184, -23.4375,
    ///                                         4, 32, -0.234375))
    /// ```
    ///
    /// `slideEndLike`'s top gradient therefore runs y 56 -> 312 (= 0 + 128 -
    /// 72 .. 0 + 128 + 184) and its bottom y 4 -> 32. The top one is aimed
    /// **past the top of the dimension**, which is why End islands have flat
    /// tops rather than a ceiling taper.
    ///
    /// # The seeding, which is the whole dimension
    ///
    /// `end.json` sets `legacy_random_source: true`, so `RandomState`'s
    /// `random` is legacy — and, exactly as in the nether,
    /// `RandomState$1NoiseWiringHelper` intercepts before any noise reaches
    /// it (bytecode, `wrapNew`, offsets 0-71):
    ///
    /// ```text
    ///   wrapNew(BlendedNoise), legacy -> withNewRandom(newLegacyInstance(0))
    ///                                  = new LegacyRandomSource(seed)
    ///   wrapNew(EndIslandDensityFunction) -> new EndIslandDensityFunction(seed)
    /// ```
    ///
    /// So the island field is seeded from the **raw world seed**, not from a
    /// fork and not from a hash — and the terrain noise from a second,
    /// separate `LegacyRandomSource(seed)`. The two do not share a stream.
    ///
    /// The island field appears in both live slots. One instance is shared
    /// between them here; vanilla parses two and memoises by identity, and
    /// since `EndIslandDensityFunction` is a pure function of the seed the
    /// two are indistinguishable.
    #[must_use]
    pub fn end(world_seed: i64) -> Self {
        use crate::rng::LegacyRandomSource;

        let islands = build::end_islands(world_seed);

        let mut terrain_random = LegacyRandomSource::new(world_seed);
        let base_3d_noise = build::old_blended_noise(BlendedNoise::new(
            &mut terrain_random,
            0.25,
            0.25,
            80.0,
            160.0,
            4.0,
        ));
        let sloped_cheese = build::add(islands.clone(), base_3d_noise);
        let final_density = build::post_process(build::slide(
            sloped_cheese.clone(),
            0,
            128,
            72,
            -184,
            -23.437_5,
            4,
            32,
            -0.234_375,
        ));

        let zero = || build::constant(0.0);
        Self {
            final_density,
            sloped_cheese,
            depth: zero(),
            factor: zero(),
            jaggedness: zero(),
            offset: zero(),
            continents: zero(),
            // The one slot `TheEndBiomeSource` reads. It is not erosion in
            // any climatic sense — it is the island height field, parked in
            // the erosion slot because `Climate.Sampler` is the only channel
            // a biome source has to the router.
            erosion: build::cache_2d(islands),
            ridges: zero(),
            ridges_folded: zero(),
            barrier: zero(),
            fluid_level_floodedness: zero(),
            fluid_level_spread: zero(),
            lava: zero(),
            temperature: zero(),
            vegetation: zero(),
            preliminary_surface_level: zero(),
            vein_toggle: zero(),
            vein_ridged: zero(),
            vein_gap: zero(),
        }
    }

    /// Build the router for `world_seed`.
    ///
    /// `amplified` selects the amplified world type, which changes only the
    /// shaper's value transformers.
    #[must_use]
    pub fn new(world_seed: i64, noises: &OverworldNoises, amplified: bool) -> Self {
        let seeding = NoiseSeeding::new(world_seed);
        let bind = |name: &str, params: &NoiseParameters| {
            super::NoiseHolder::bound(seeding.instantiate(name, params))
        };

        // `cacheOnce` slots are handed out here, at construction, because
        // such a marker's identity is a *logical* function and our graph is a
        // tree. `cheese` below is cloned into three places, and only a slot
        // fixed before the clones makes those three reads share one cache;
        // a numbering pass afterwards would give each clone its own slot and
        // the memoization would never hit. Vanilla gets the sharing for free
        // from having one object referenced three times.
        let mut cache_slot = 0usize;

        // Column-constant offsets. Both read the *same* offset noise; the
        // axis permutation is what makes them independent.
        let shift_x = build::flat_cache(build::cache_2d(build::shift_a(bind(
            names::SHIFT,
            noises.get(names::SHIFT),
        ))));
        let shift_z = build::flat_cache(build::cache_2d(build::shift_b(bind(
            names::SHIFT,
            noises.get(names::SHIFT),
        ))));

        // `shiftedNoise2d(x, z, scale, noise)` fixes the y shift to zero and
        // the y scale to 0.0 — these are 2D fields, constant down a column.
        let field = |params: &NoiseParameters, name: &str| {
            build::flat_cache(build::shifted_noise(
                shift_x.clone(),
                build::constant(0.0),
                shift_z.clone(),
                0.25,
                0.0,
                bind(name, params),
            ))
        };

        let continents = field(noises.get(names::CONTINENTALNESS), names::CONTINENTALNESS);
        let erosion = field(noises.get(names::EROSION), names::EROSION);
        let ridges = field(noises.get(names::RIDGE), names::RIDGE);
        let ridges_folded = peaks_and_valleys(ridges.clone());

        // ⚠️ The shaper's parameter names invert here, and it is the easiest
        // mistake in the whole router: TerrainProvider's `weirdness` is the
        // **unfolded** RIDGES field, and its `ridges` is RIDGES_FOLDED.
        let weirdness_coord = ridges.clone();
        let ridges_coord = ridges_folded.clone();

        // The 3D terrain noise is created unseeded by the registry and then
        // re-seeded per world from `fromHashOf("minecraft:terrain")`
        // (RandomState.wrapNew). Building it from the world seed directly
        // would give a different — and wrong — field.
        let mut terrain_random = NoiseSeeding::new(world_seed).source_for(names::TERRAIN);
        let base_3d_noise = build::old_blended_noise(BlendedNoise::new(
            &mut terrain_random,
            0.25,
            0.125,
            80.0,
            160.0,
            8.0,
        ));

        let offset = build::spline_with_blending(
            &mut cache_slot,
            build::add(
                // `DensityFunctions.constant(-0.50375F)` — a FLOAT literal.
                // The f64 spelling differs by ~2.6e-8, which propagates
                // through depth into every terrain height.
                build::constant(f64::from(-0.503_75_f32)),
                build::spline(terrain::overworld_offset(
                    &continents,
                    &erosion,
                    &ridges_coord,
                    amplified,
                )),
            ),
            build::blend_offset(),
        );

        let factor = build::spline_with_blending(
            &mut cache_slot,
            build::spline(terrain::overworld_factor(
                &continents,
                &erosion,
                &weirdness_coord,
                &ridges_coord,
                amplified,
            )),
            build::constant(BLENDING_FACTOR),
        );

        let unscaled_jaggedness = build::spline_with_blending(
            &mut cache_slot,
            build::spline(terrain::overworld_jaggedness(
                &continents,
                &erosion,
                &weirdness_coord,
                &ridges_coord,
                amplified,
            )),
            // BLENDING_JAGGEDNESS is `zero()`, which is a Constant — so this
            // takes lerp's direct expansion, unlike `offset` above.
            build::constant(0.0),
        );

        let depth = build::offset_to_depth(offset.clone());

        // The jagged noise is sampled at a very coarse xz scale and zero y
        // scale, then half-negated so jaggedness only adds upward.
        let jagged_noise =
            build::noise(bind(names::JAGGED, noises.get(names::JAGGED)), 1500.0, 0.0);
        let jaggedness = build::mul(
            unscaled_jaggedness.clone(),
            build::map(super::Mapped::HalfNegative, jagged_noise),
        );

        let initial_density = build::noise_gradient_density(
            factor.clone(),
            build::add(depth.clone(), jaggedness.clone()),
        );
        let sloped_cheese = build::add(initial_density, base_3d_noise);

        // ---- caves -------------------------------------------------
        // `slopedCheese` is the expensive one — it contains BlendedNoise —
        // and the cave path reads it as the rangeChoice input and again
        // inside whichever branch wins.
        let cheese = build::cache_once(&mut cache_slot, sloped_cheese.clone());

        let spaghetti_roughness = {
            let noise_fn = build::noise(
                bind(
                    "minecraft:spaghetti_roughness",
                    noises.get("minecraft:spaghetti_roughness"),
                ),
                1.0,
                1.0,
            );
            let modulator = mapped_noise(
                bind(
                    "minecraft:spaghetti_roughness_modulator",
                    noises.get("minecraft:spaghetti_roughness_modulator"),
                ),
                1.0,
                1.0,
                0.0,
                -0.1,
            );
            build::cache_once(
                &mut cache_slot,
                build::mul(
                    modulator,
                    build::add(
                        build::map(super::Mapped::Abs, noise_fn),
                        build::constant(-0.4),
                    ),
                ),
            )
        };

        let entrances = {
            let rarity = build::cache_once(
                &mut cache_slot,
                build::noise(
                    bind(
                        "minecraft:spaghetti_3d_rarity",
                        noises.get("minecraft:spaghetti_3d_rarity"),
                    ),
                    2.0,
                    1.0,
                ),
            );
            let thickness = mapped_noise(
                bind(
                    "minecraft:spaghetti_3d_thickness",
                    noises.get("minecraft:spaghetti_3d_thickness"),
                ),
                1.0,
                1.0,
                -0.065,
                -0.088,
            );
            let cave = |name: &str| {
                wrap_rarity_3d(rarity.clone(), |r| {
                    build::noise_for_rarity(bind(name, noises.get(name)), r)
                })
            };
            let spaghetti_3d = build::clamp(
                build::add(
                    build::max(
                        cave("minecraft:spaghetti_3d_1"),
                        cave("minecraft:spaghetti_3d_2"),
                    ),
                    thickness,
                ),
                -1.0,
                1.0,
            );
            let entrance_noise = build::noise(
                bind(
                    "minecraft:cave_entrance",
                    noises.get("minecraft:cave_entrance"),
                ),
                0.75,
                0.5,
            );
            let big_entrances = build::add(
                build::add(entrance_noise, build::constant(0.37)),
                build::y_clamped_gradient(-10, 30, 0.3, 0.0),
            );
            build::cache_once(
                &mut cache_slot,
                build::min(
                    big_entrances,
                    build::add(spaghetti_roughness.clone(), spaghetti_3d),
                ),
            )
        };

        let spaghetti_2d_thickness = build::cache_once(
            &mut cache_slot,
            mapped_noise(
                bind(
                    "minecraft:spaghetti_2d_thickness",
                    noises.get("minecraft:spaghetti_2d_thickness"),
                ),
                2.0,
                1.0,
                -0.6,
                -1.3,
            ),
        );

        let spaghetti_2d = {
            let rarity = build::noise(
                bind(
                    "minecraft:spaghetti_2d_modulator",
                    noises.get("minecraft:spaghetti_2d_modulator"),
                ),
                2.0,
                1.0,
            );
            let cave = wrap_rarity_2d(rarity, |r| {
                build::noise_for_rarity(
                    bind(
                        "minecraft:spaghetti_2d",
                        noises.get("minecraft:spaghetti_2d"),
                    ),
                    r,
                )
            });
            // `mappedNoise(SPAGHETTI_2D_ELEVATION, 0.0, floorDiv(-64, 8), 8.0)`
            // is the FOUR-arg overload — (noise, yScale, minTarget,
            // maxTarget), with xzScale defaulting to 1.0. Reading it as the
            // five-arg form gives yScale 1.0 and swaps the targets, which is
            // wrong in a narrow y band only: exactly the shape that survives
            // a handful of spread test points.
            let elevation = mapped_noise(
                bind(
                    "minecraft:spaghetti_2d_elevation",
                    noises.get("minecraft:spaghetti_2d_elevation"),
                ),
                1.0,
                0.0,
                f64::from((-64i32).div_euclid(8)),
                8.0,
            );
            // 26.2 added this flat_cache; the pre-26.2 decompile lacks it.
            let sloped = build::map(
                super::Mapped::Abs,
                build::add(
                    build::flat_cache(elevation),
                    build::y_clamped_gradient(-64, 320, 8.0, -40.0),
                ),
            );
            let layer_ridged = build::map(
                super::Mapped::Cube,
                build::add(sloped, spaghetti_2d_thickness.clone()),
            );
            let cave_noise = build::add(
                cave,
                build::mul(build::constant(0.083), spaghetti_2d_thickness.clone()),
            );
            build::clamp(build::max(cave_noise, layer_ridged), -1.0, 1.0)
        };

        let pillars = {
            let source = build::noise(
                bind("minecraft:pillar", noises.get("minecraft:pillar")),
                25.0,
                0.3,
            );
            let rareness = mapped_noise(
                bind(
                    "minecraft:pillar_rareness",
                    noises.get("minecraft:pillar_rareness"),
                ),
                1.0,
                1.0,
                0.0,
                -2.0,
            );
            let thickness = mapped_noise(
                bind(
                    "minecraft:pillar_thickness",
                    noises.get("minecraft:pillar_thickness"),
                ),
                1.0,
                1.0,
                0.0,
                1.1,
            );
            let with_rareness = build::add(build::mul(source, build::constant(2.0)), rareness);
            build::cache_once(
                &mut cache_slot,
                build::mul(with_rareness, build::map(super::Mapped::Cube, thickness)),
            )
        };

        let underground = {
            let layer = build::noise(
                bind("minecraft:cave_layer", noises.get("minecraft:cave_layer")),
                1.0,
                8.0,
            );
            let layerized = build::mul(
                build::constant(4.0),
                build::map(super::Mapped::Square, layer),
            );
            let cheese_noise = build::noise(
                bind("minecraft:cave_cheese", noises.get("minecraft:cave_cheese")),
                1.0,
                0.666_666_666_666_666_6,
            );
            let solidified = build::add(
                build::clamp(build::add(build::constant(0.27), cheese_noise), -1.0, 1.0),
                build::clamp(
                    build::add(
                        build::constant(1.5),
                        build::mul(build::constant(-0.64), cheese.clone()),
                    ),
                    0.0,
                    0.5,
                ),
            );
            let base = build::add(layerized, solidified);
            let subtractions = build::min(
                build::min(base, entrances.clone()),
                build::add(spaghetti_2d, spaghetti_roughness.clone()),
            );
            // Below 0.03 the pillar contribution is switched off entirely by
            // dropping to -1e6, which loses every `max` it meets.
            let pillars_cut = build::range_choice(
                pillars.clone(),
                -1_000_000.0,
                0.03,
                build::constant(-1_000_000.0),
                pillars,
            );
            build::max(subtractions, pillars_cut)
        };

        let surface_with_entrances =
            build::min(cheese.clone(), build::mul(build::constant(5.0), entrances));
        let caves = build::range_choice(
            cheese.clone(),
            -1_000_000.0,
            1.562_5,
            surface_with_entrances,
            underground,
        );

        let noodle = {
            let y = y_identity_ramp();
            let limited = |inner: DensityFunction, out_of_range: f64| {
                build::interpolated(build::range_choice(
                    y.clone(),
                    -60.0,
                    321.0,
                    inner,
                    build::constant(out_of_range),
                ))
            };
            let toggle = limited(
                build::noise(
                    bind("minecraft:noodle", noises.get("minecraft:noodle")),
                    1.0,
                    1.0,
                ),
                -1.0,
            );
            let thickness = limited(
                mapped_noise(
                    bind(
                        "minecraft:noodle_thickness",
                        noises.get("minecraft:noodle_thickness"),
                    ),
                    1.0,
                    1.0,
                    -0.05,
                    -0.1,
                ),
                0.0,
            );
            const RIDGE_FREQ: f64 = 2.666_666_666_666_666_5;
            let ridge_a = limited(
                build::noise(
                    bind(
                        "minecraft:noodle_ridge_a",
                        noises.get("minecraft:noodle_ridge_a"),
                    ),
                    RIDGE_FREQ,
                    RIDGE_FREQ,
                ),
                0.0,
            );
            let ridge_b = limited(
                build::noise(
                    bind(
                        "minecraft:noodle_ridge_b",
                        noises.get("minecraft:noodle_ridge_b"),
                    ),
                    RIDGE_FREQ,
                    RIDGE_FREQ,
                ),
                0.0,
            );
            let ridged = build::mul(
                build::constant(1.5),
                build::max(
                    build::map(super::Mapped::Abs, ridge_a),
                    build::map(super::Mapped::Abs, ridge_b),
                ),
            );
            build::range_choice(
                toggle,
                -1_000_000.0,
                0.0,
                build::constant(64.0),
                build::add(thickness, ridged),
            )
        };

        let final_density = build::min(
            build::post_process(build::slide_overworld(amplified, caves)),
            noodle,
        );

        // `noise(data, x)` is the **yScale** overload — xzScale defaults to
        // 1.0. Reading it as xzScale is the same misread that produced the
        // `spaghetti_2d_elevation` bug, and here it would stretch every
        // aquifer horizontally instead of vertically.
        let aquifer_noise =
            |name: &str, y_scale: f64| build::noise(bind(name, noises.get(name)), 1.0, y_scale);
        let barrier = aquifer_noise(names::AQUIFER_BARRIER, 0.5);
        let fluid_level_floodedness = aquifer_noise(names::AQUIFER_FLUID_LEVEL_FLOODEDNESS, 0.67);
        let fluid_level_spread =
            aquifer_noise(names::AQUIFER_FLUID_LEVEL_SPREAD, 0.714_285_714_285_714_3);
        let lava = aquifer_noise(names::AQUIFER_LAVA, 1.0);

        // `shiftedNoise2d` with the same 0.25 scale and shift pair as
        // continentalness, but **not** wrapped in `flatCache`. Continents,
        // erosion and ridges are registry entries that vanilla flat-caches;
        // temperature and vegetation are built inline in `overworld()` and
        // are not. Reusing the `field` helper here would quietly quantise
        // climate to quart resolution once FlatCache stops being a
        // passthrough.
        let climate = |name: &str| {
            build::shifted_noise(
                shift_x.clone(),
                build::constant(0.0),
                shift_z.clone(),
                0.25,
                0.0,
                bind(name, noises.get(name)),
            )
        };
        let temperature = climate(names::TEMPERATURE);
        let vegetation = climate(names::VEGETATION);

        let preliminary_surface_level = {
            let cached_factor = build::cache_2d(factor.clone());
            let cached_offset = build::cache_2d(offset.clone());

            // remap(.., 1.5, -1.5, -64, 320): the input runs high-to-low, so
            // the factor is negative — an inverted mapping, not a typo.
            let upper_bound = build::clamp(
                build::remap(
                    build::add(
                        build::mul(
                            build::constant(0.273_437_5),
                            build::map(super::Mapped::Invert, cached_factor.clone()),
                        ),
                        build::mul(build::constant(-1.0), cached_offset.clone()),
                    ),
                    1.5,
                    -1.5,
                    -64.0,
                    320.0,
                ),
                -40.0,
                320.0,
            );

            // Depth is re-derived from the *cached* offset rather than reusing
            // the router's `depth`; ported literally because the two differ
            // in caching, which is observable through cache2d's keying.
            let density = build::add(
                build::slide_overworld(
                    amplified,
                    build::clamp(
                        build::add(
                            build::noise_gradient_density(
                                cached_factor,
                                build::offset_to_depth(cached_offset),
                            ),
                            build::constant(-0.703_125),
                        ),
                        -64.0,
                        64.0,
                    ),
                ),
                build::constant(-0.390_625),
            );

            build::find_top_surface(density, upper_bound, -64, OVERWORLD_CELL_HEIGHT)
        };

        let vein_limited = |inner: DensityFunction| {
            build::interpolated(build::range_choice(
                y_identity_ramp(),
                f64::from(VEIN_MIN_Y),
                f64::from(VEIN_MAX_Y + 1),
                inner,
                build::constant(0.0),
            ))
        };
        let vein_noise = |name: &str, scale: f64| {
            vein_limited(build::noise(bind(name, noises.get(name)), scale, scale))
        };
        let vein_toggle = vein_noise(names::ORE_VEININESS, 1.5);
        const ORE_RIDGE_FREQ: f64 = 4.0;
        let vein_ridged = build::add(
            // `constant(-0.08F)` — an f32 literal widened to f64, so this is
            // -0.0799999982118606567382812500, not -0.08.
            build::constant(f64::from(-0.08_f32)),
            build::max(
                build::map(
                    super::Mapped::Abs,
                    vein_noise(names::ORE_VEIN_A, ORE_RIDGE_FREQ),
                ),
                build::map(
                    super::Mapped::Abs,
                    vein_noise(names::ORE_VEIN_B, ORE_RIDGE_FREQ),
                ),
            ),
        );
        let vein_gap = build::noise(bind(names::ORE_GAP, noises.get(names::ORE_GAP)), 1.0, 1.0);

        Self {
            final_density,
            sloped_cheese,
            depth,
            factor,
            jaggedness: unscaled_jaggedness,
            offset,
            continents,
            erosion,
            ridges,
            ridges_folded,
            barrier,
            fluid_level_floodedness,
            fluid_level_spread,
            lava,
            temperature,
            vegetation,
            preliminary_surface_level,
            vein_toggle,
            vein_ridged,
            vein_gap,
        }
    }
}

/// `DensityFunctions.mappedNoise` -> `mapFromUnitTo`.
///
/// Rescales a unit-ish noise into `[min, max]` as
/// `middle + factor * noise`, which is not the same as a lerp and must not
/// be rewritten as one.
fn mapped_noise(
    noise: super::NoiseHolder,
    xz_scale: f64,
    y_scale: f64,
    min_target: f64,
    max_target: f64,
) -> DensityFunction {
    let middle = (min_target + max_target) * 0.5;
    let factor = (max_target - min_target) * 0.5;
    build::add(
        build::constant(middle),
        build::mul(
            build::constant(factor),
            build::noise(noise, xz_scale, y_scale),
        ),
    )
}

/// `QuantizedSpaghettiRarity.wrapRarity3d` — 26.2's replacement for the
/// removed `WeirdScaledSampler`.
fn wrap_rarity_3d(
    input: DensityFunction,
    mut noise_for: impl FnMut(f64) -> DensityFunction,
) -> DensityFunction {
    build::map(
        super::Mapped::Abs,
        build::interval_select(
            input,
            vec![-0.5, 0.0, 0.5],
            vec![
                noise_for(0.75),
                noise_for(1.0),
                noise_for(1.5),
                noise_for(2.0),
            ],
        ),
    )
}

/// `QuantizedSpaghettiRarity.wrapRarity2d`.
fn wrap_rarity_2d(
    input: DensityFunction,
    mut noise_for: impl FnMut(f64) -> DensityFunction,
) -> DensityFunction {
    build::map(
        super::Mapped::Abs,
        build::interval_select(
            input,
            vec![-0.75, -0.5, 0.5, 0.75],
            vec![
                noise_for(0.5),
                noise_for(0.75),
                noise_for(1.0),
                noise_for(2.0),
                noise_for(3.0),
            ],
        ),
    )
}

/// `NoiseRouterData.peaksAndValleys(DensityFunction)` — the density-function
/// form of the fold.
///
/// Must agree with [`terrain::peaks_and_valleys`], which is the `f32` scalar
/// form used inside splines. The constants differ in *spelling* between the
/// two (`-0.6666666666666666` here versus `0.6666667F` there) because one is
/// a double and the other a float, and that is intentional in vanilla too.
#[must_use]
pub fn peaks_and_valleys(weirdness: DensityFunction) -> DensityFunction {
    build::mul(
        build::add(
            build::map(
                super::Mapped::Abs,
                build::add(
                    build::map(super::Mapped::Abs, weirdness),
                    build::constant(-0.666_666_666_666_666_6),
                ),
            ),
            build::constant(-0.333_333_333_333_333_3),
        ),
        build::constant(-3.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::density::FunctionContext;

    /// Plausible shapes only — the golden test supplies the real registry
    /// values. Every noise the router reads must be present, because a
    /// missing one panics rather than silently defaulting.
    fn noises() -> OverworldNoises {
        let generic = NoiseParameters::new(-7, vec![1.0, 1.0]);
        let mut set = OverworldNoises::new()
            .with(
                names::SHIFT,
                NoiseParameters::new(-3, vec![1.0, 1.0, 1.0, 0.0]),
            )
            .with(
                names::CONTINENTALNESS,
                NoiseParameters::new(-9, vec![1.0, 1.0, 2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.0]),
            )
            .with(
                names::EROSION,
                NoiseParameters::new(-9, vec![1.0, 1.0, 0.0, 1.0, 1.0]),
            )
            .with(
                names::RIDGE,
                NoiseParameters::new(-7, vec![1.0, 2.0, 1.0, 0.0, 0.0, 0.0]),
            )
            .with(names::JAGGED, NoiseParameters::new(-16, vec![1.0; 16]));
        for name in [
            "minecraft:spaghetti_roughness",
            "minecraft:spaghetti_roughness_modulator",
            "minecraft:spaghetti_3d_rarity",
            "minecraft:spaghetti_3d_thickness",
            "minecraft:spaghetti_3d_1",
            "minecraft:spaghetti_3d_2",
            "minecraft:cave_entrance",
            "minecraft:spaghetti_2d",
            "minecraft:spaghetti_2d_modulator",
            "minecraft:spaghetti_2d_elevation",
            "minecraft:spaghetti_2d_thickness",
            "minecraft:cave_layer",
            "minecraft:cave_cheese",
            "minecraft:pillar",
            "minecraft:pillar_rareness",
            "minecraft:pillar_thickness",
            "minecraft:noodle",
            "minecraft:noodle_thickness",
            "minecraft:noodle_ridge_a",
            "minecraft:noodle_ridge_b",
            names::TEMPERATURE,
            names::VEGETATION,
            names::AQUIFER_BARRIER,
            names::AQUIFER_FLUID_LEVEL_FLOODEDNESS,
            names::AQUIFER_FLUID_LEVEL_SPREAD,
            names::AQUIFER_LAVA,
            names::ORE_VEININESS,
            names::ORE_VEIN_A,
            names::ORE_VEIN_B,
            names::ORE_GAP,
        ] {
            set = set.with(name, generic.clone());
        }
        set
    }

    #[test]
    fn the_router_evaluates_across_the_world_height() {
        let router = OverworldRouter::new(20260708, &noises(), false);
        for y in [-64, -32, 0, 64, 128, 256, 320] {
            for (x, z) in [(0, 0), (137, -509), (-2048, 2048)] {
                let value = router.final_density.compute(FunctionContext::new(x, y, z));
                assert!(
                    value.is_finite(),
                    "final_density non-finite at ({x}, {y}, {z})"
                );
            }
        }
    }

    #[test]
    fn depth_decreases_with_height() {
        // The vertical ramp dominates offset, so deeper must mean denser —
        // if this inverts, terrain generates upside down.
        let router = OverworldRouter::new(1, &noises(), false);
        let low = router.depth.compute(FunctionContext::new(0, -64, 0));
        let high = router.depth.compute(FunctionContext::new(0, 320, 0));
        assert!(low > high, "depth should fall with height: {low} -> {high}");
    }

    #[test]
    fn two_dimensional_fields_are_constant_down_a_column() {
        // continents/erosion/ridges are sampled with a zero y scale, so a
        // column must give one value. A non-zero y scale here would be an
        // invisible-but-total terrain change.
        let router = OverworldRouter::new(7, &noises(), false);
        let a = router
            .continents
            .compute(FunctionContext::new(31, -64, -17));
        let b = router
            .continents
            .compute(FunctionContext::new(31, 200, -17));
        assert_eq!(a.to_bits(), b.to_bits());
    }

    #[test]
    fn amplified_changes_the_terrain() {
        let normal = OverworldRouter::new(5, &noises(), false);
        let amplified = OverworldRouter::new(5, &noises(), true);
        let ctx = FunctionContext::new(100, 80, 100);
        assert_ne!(
            normal.sloped_cheese.compute(ctx).to_bits(),
            amplified.sloped_cheese.compute(ctx).to_bits()
        );
    }

    #[test]
    fn climate_fields_are_constant_down_a_column() {
        // temperature and vegetation are shiftedNoise2d with a y scale of
        // 0.0, so y must not reach them. If it ever does, biomes start
        // varying with height and the whole biome layer is wrong.
        let router = OverworldRouter::new(7, &noises(), false);
        for field in [&router.temperature, &router.vegetation] {
            let at_floor = field.compute(FunctionContext::new(11, -64, -23));
            for y in [-40, 0, 63, 200, 319] {
                assert_eq!(
                    field.compute(FunctionContext::new(11, y, -23)).to_bits(),
                    at_floor.to_bits(),
                    "climate varied with y at {y}"
                );
            }
        }
    }

    #[test]
    fn vein_fields_switch_off_outside_their_y_band() {
        // yLimitedInterpolatable falls back to a constant outside
        // [VEIN_MIN_Y, VEIN_MAX_Y]. Copper tops out at 50 and iron bottoms
        // at -60, so ore veins simply do not exist above or below that.
        let router = OverworldRouter::new(7, &noises(), false);
        for y in [VEIN_MIN_Y - 1, VEIN_MAX_Y + 1, -64, 319] {
            assert_eq!(
                router.vein_toggle.compute(FunctionContext::new(5, y, 5)),
                0.0,
                "vein_toggle live at y={y}"
            );
        }
        // Inside the band it is a real noise, so at least one sample must
        // differ — otherwise the range test above proves nothing.
        let inside: Vec<f64> = (VEIN_MIN_Y..=VEIN_MAX_Y)
            .step_by(7)
            .map(|y| router.vein_toggle.compute(FunctionContext::new(5, y, 5)))
            .collect();
        assert!(
            inside.iter().any(|v| *v != 0.0),
            "vein_toggle is zero inside its own band too"
        );
    }

    #[test]
    fn vein_ridged_carries_the_widened_float_constant() {
        // `constant(-0.08F)` is an f32 literal widened to f64. Writing -0.08
        // directly would be a different number, and it biases every vein.
        let widened = f64::from(-0.08_f32);
        assert_ne!(widened, -0.08);
        // Pinned exactly rather than "close enough": the whole point is that
        // the two are different numbers, so a tolerance would defeat the
        // test. The gap is ~1.8e-9, which is far too small to notice in a
        // rendered world and far too large to be bit-exact.
        assert_eq!(widened, -0.079_999_998_211_860_66);
        // min = the constant + min of max(|a|, |b|), which is >= 0.
        let router = OverworldRouter::new(7, &noises(), false);
        assert_eq!(router.vein_ridged.min_value(), widened);
    }

    #[test]
    fn preliminary_surface_level_lands_on_a_cell_boundary() {
        // findTopSurface steps by the cell height from a snapped ceiling, so
        // every value it can return is congruent to the floor mod 8. The
        // aquifer relies on that granularity.
        let router = OverworldRouter::new(7, &noises(), false);
        for x in [-200, -13, 0, 7, 411] {
            for z in [-509, -4, 0, 63, 1024] {
                let v = router
                    .preliminary_surface_level
                    .compute(FunctionContext::new(x, 0, z));
                assert!(
                    (-64.0..=320.0).contains(&v),
                    "surface {v} out of world at ({x}, {z})"
                );
                let as_int = v as i32;
                assert_eq!(f64::from(as_int), v, "surface {v} is not an integer");
                assert_eq!(
                    (as_int - -64).rem_euclid(OVERWORLD_CELL_HEIGHT),
                    0,
                    "surface {v} at ({x}, {z}) is not on a cell boundary"
                );
            }
        }
    }

    #[test]
    fn preliminary_surface_level_ignores_the_y_it_is_asked_about() {
        // It is a column property: the y in the context selects nothing, and
        // the scan always starts from the same snapped ceiling. A dependence
        // here would make the aquifer's answer depend on which block first
        // asked for it.
        let router = OverworldRouter::new(7, &noises(), false);
        let at_zero = router
            .preliminary_surface_level
            .compute(FunctionContext::new(37, 0, -11));
        for y in [-64, 100, 319] {
            assert_eq!(
                router
                    .preliminary_surface_level
                    .compute(FunctionContext::new(37, y, -11))
                    .to_bits(),
                at_zero.to_bits()
            );
        }
    }
}
