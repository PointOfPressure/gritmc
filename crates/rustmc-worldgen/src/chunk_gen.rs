//! The noise fill — density becomes blocks.
//!
//! Reference: `NoiseBasedChunkGenerator.doFill`, with `NoiseChunk`'s
//! `MaterialRuleList` supplying the per-block decision.
//!
//! Everything under this module answers "how dense is this point". This one
//! answers "what block is there", and the difference is not a threshold: the
//! aquifer can call a point solid that the density function called open (a
//! barrier between two water tables), and the ore veinifier can replace a
//! block the aquifer declined. Two rules, in order, first non-`None` wins —
//! `MaterialRuleList` exactly.
//!
//! # What is deliberately not here
//!
//! Surfaces, carvers, features and structures. A chunk out of this module is
//! stone, water, lava and ore veins with a flat grass-less top — vanilla's
//! `noise` stage, nothing beyond it. That is the right unit to prove against
//! vanilla's own `fillFromNoise` output before biomes exist, because it is
//! the last stage that does not depend on the biome source.
//!
//! # `Beardifier`: the term exists now, and is fed nothing
//!
//! [`crate::density::beard`] is ported and gated bit-for-bit against
//! vanilla's own class — the 13,824-entry kernel, both contribution
//! functions, and `compute` over every `TerrainAdjustment`. It reaches the
//! fill through [`FillInputs::beardifier`], and the nether and overworld
//! paths in `worldgen-assembly::generate` both pass one now.
//!
//! **Two things below were true when they were written and are not now, and
//! both are corrected in place rather than left to read as authority.**
//!
//! * "every caller currently passes `None`, so the fill is still beardless in
//!   every dimension" — the nether wires `nether_fossil` and the overworld
//!   wires the villages and the trial chambers.
//! * "the overworld window this project measures has only `shipwreck` and
//!   `mineshaft` starts, both with no `terrain_adaptation` at all … a new
//!   window is needed to see it at all" — **false, and it was false when it
//!   was written.** The published r=16 overworld window at seed 20260722
//!   holds **three `minecraft:trial_chambers` starts and 692 pieces**,
//!   `terrain_adaptation: encapsulate`, against 393 mineshaft pieces, one
//!   shipwreck and one ruined portal. Read out of the reference's own chunk
//!   NBT with `region-differ starts`. The window that needed inventing was
//!   already there and already published, and the r=7 window the sentence was
//!   really about is a subset of it.
//!
//! What is left is not "collecting a chunk's pieces" either: that is done.
//! What is left is the `through >= Through::Decoration` gate on
//! `village_starts` and `trial_chamber_starts` in `worldgen-assembly`, which
//! makes `RUSTMC_THROUGH=noise|surface|carvers` produce **beardless terrain**
//! — so the stage cut-off documented as the way to separate a terrain
//! difference from a feature placing differently is not that near a bearding
//! structure. Moving the gate puts structure-start work inside every
//! noise-only dump, which is what the perf benches measure, so it is a
//! perf-visible change.
//!
//! The history below is kept because the *reason* the gap went unseen for the
//! whole project is worth more than the gap was.
//!
//! `Beardifier` used to be 0 throughout with no way to be anything else, and
//! this note used to say that was "correct rather than provisional". The
//! argument it gave was wrong in an instructive way.
//!
//! The old reasoning was: `NoiseChunk` substitutes the real beardifier
//! wherever `DensityFunctions.BeardifierMarker.INSTANCE` appears in the
//! density tree, so the beard reaches terrain only through a marker node in
//! the router — and `grep -rl beardifier` over the whole of
//! `generated-data/.../worldgen/` matches nothing, so there is no marker.
//!
//! **The marker is never in the datapack, in any version.** `NoiseChunk`'s own
//! constructor injects it, at offsets 495-506:
//!
//! ```text
//!   noiseRouter.finalDensity()
//!   getstatic DensityFunctions$BeardifierMarker.INSTANCE
//!   invokestatic DensityFunctions.add
//!   invokestatic DensityFunctions.cacheAllInCell
//!   invokeinterface DensityFunction.mapAll(this::wrap)
//! ```
//!
//! and `wrap` swaps the real `beardifier` field in for the marker. So the grep
//! was a test of the wrong thing, and every structure whose
//! `terrain_adaptation` is not `none` raises terrain around itself that this
//! port does not raise.
//!
//! **Measured, nether chunk (58,67), seed 20260722.** Vanilla's own `doFill`
//! run twice over a real `ProtoChunk` — once with the marker, once with a real
//! `Beardifier` built from the `nether_fossil` piece box read out of the
//! reference's own chunk NBT — against vanilla's finished world:
//!
//! ```text
//!   beardless fill   19,078 non-air   699 blocks missing from it
//!   bearded fill     19,691 non-air    75 blocks missing from it
//! ```
//!
//! The beard is 624 blocks in one chunk, and `soul_sand`/`soul_soil` written
//! into positions the beardless fill leaves air is 4,852 blocks over the r=7
//! nether window — 22% of that dimension's residual.
//!
//! **Why no gate caught it.** `SurfaceChunkGolden` passes the marker too, so
//! both sides share the zero; and the overworld window this project measures
//! has only `shipwreck` and `mineshaft` starts, both with no
//! `terrain_adaptation` at all, so the overworld's order-invariant core of
//! zero is genuinely unaffected. A term that is zero on every window you look
//! at is not a term that is zero.
//!
//! Closing it needs structure starts before the noise stage, which is where
//! vanilla computes them. The stage moved, and the density term landed; what
//! remains is the collection.
//!
//! **Which dimension moves first is not the nether.** The nether's 4,852
//! blocks need `nether_fossil`, and no nether structure is ported at all. The
//! overworld already places a bearding structure: all five villages are
//! `beard_thin`. So the first terrain the beard moves will be a village's,
//! and it will be invisible to both published overworld windows — seed
//! 20260722's r=7 window has one `shipwreck` and two `mineshaft` starts, and
//! seed 31415926's has neither a village nor any other bearding start inside
//! it. A new window is needed to see it at all.

use crate::density::aquifer::{AquiferBounds, AquiferNoises, NoiseBasedAquifer};
use crate::density::cell::CellGeometry;
use crate::density::chunk::{ChunkSampler, Root};
use crate::density::fill::{GlobalFluidPicker, Substance};
use crate::density::flat_cache::FlatCaches;
use crate::density::ore_vein::{OreVeinifier, VeinAxis};
use crate::density::surface_level::SurfaceLevels;
use crate::density::DensityFunction;

// Read once rather than per block of the fill. See `crate::trace_switch`.
crate::trace_switch_value!(dump_fill_density, "RUSTMC_DUMP_FILL_DENSITY");

/// What survives a fill, for the stages that come after it.
///
/// Carvers run long after the fill and ask the **same** aquifer what belongs
/// in the holes they cut, so the aquifer and the caches it reads through are
/// handed on rather than dropped.
pub struct PostFill {
    pub chunk: FilledChunk,
    /// `None` when aquifers are disabled for this dimension.
    pub aquifer: Option<NoiseBasedAquifer>,
    pub flat: FlatCaches,
    pub surface: SurfaceLevels,
}

/// A generated chunk's block column, plus the two heightmaps the noise stage
/// primes.
#[derive(Clone, Debug)]
pub struct FilledChunk {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub min_y: i32,
    pub height: i32,
    /// Indexed `(y - min_y) * 256 + z * 16 + x`, all local to the chunk.
    blocks: Vec<Substance>,
    /// `WORLD_SURFACE_WG` — one above the highest non-air block.
    world_surface: [i32; 256],
    /// `OCEAN_FLOOR_WG` — one above the highest motion-blocking block, so it
    /// sits under a sea rather than on it.
    ocean_floor: [i32; 256],
    /// Positions vanilla marks for post-processing, i.e. fluids that need a
    /// tick scheduled because they border a differing aquifer.
    post_processing: Vec<(i32, i32, i32)>,
}

impl FilledChunk {
    #[must_use]
    fn index(&self, x: i32, y: i32, z: i32) -> usize {
        debug_assert!((0..16).contains(&x) && (0..16).contains(&z));
        debug_assert!(y >= self.min_y && y < self.min_y + self.height);
        ((y - self.min_y) * 256 + z * 16 + x) as usize
    }

    /// Block at chunk-local x/z and world y.
    #[must_use]
    pub fn get(&self, x: i32, y: i32, z: i32) -> Substance {
        self.blocks[self.index(x, y, z)]
    }

    #[must_use]
    pub const fn world_surface(&self, x: i32, z: i32) -> i32 {
        self.world_surface[(z * 16 + x) as usize]
    }

    #[must_use]
    pub const fn ocean_floor(&self, x: i32, z: i32) -> i32 {
        self.ocean_floor[(z * 16 + x) as usize]
    }

    #[must_use]
    pub fn post_processing(&self) -> &[(i32, i32, i32)] {
        &self.post_processing
    }
}

/// Everything the fill needs that is not per-chunk.
///
/// `Copy` because it is a handful of scalars and every chunk needs its own —
/// making callers clone it would be noise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FillSettings {
    pub min_y: i32,
    pub height: i32,
    pub cell_width: i32,
    pub cell_height: i32,
    pub sea_level: i32,
    /// Vanilla's `oreVeinsEnabled` — on for the overworld, off for several
    /// other dimensions.
    pub ore_veins_enabled: bool,
    /// Vanilla's `isAquifersEnabled`. Off means
    /// `Aquifer.createDisabled`: positive density is solid, everything else
    /// is whatever fluid the global picker says sits at that height. Several
    /// dimensions genuinely use that, and the overworld does not — filling
    /// the overworld with it gives dry caves and wrong lakes.
    pub aquifers_enabled: bool,
    /// `NoiseGeneratorSettings.defaultFluid` — water in the overworld, lava
    /// in the nether, and **air** in the end.
    ///
    /// It is not a cosmetic swap. It is the upper of the global fluid
    /// picker's two layers, so it decides what fills every open block below
    /// `sea_level`; leaving it as water gives a nether with a water sea at
    /// y=32 and terrain that is otherwise correct — the exact shape of
    /// mistake that looks like Minecraft and is a different world.
    pub default_fluid: crate::density::fill::Substance,
}

impl FillSettings {
    /// The overworld's noise settings.
    #[must_use]
    pub const fn overworld() -> Self {
        Self {
            min_y: -64,
            height: 384,
            cell_width: 4,
            cell_height: 8,
            sea_level: 63,
            ore_veins_enabled: true,
            aquifers_enabled: true,
            default_fluid: crate::density::fill::Substance::Water,
        }
    }

    /// The nether's, from `worldgen/noise_settings/nether.json`.
    ///
    /// `min_y`/`height` are the **noise** settings' 0..128, not the
    /// dimension's storage range. `DimensionType` gives the nether
    /// `height = 256` (measured: a vanilla nether chunk on disk has sixteen
    /// sections, y 0..255), so the canvas is twice as tall as the region the
    /// generator fills and everything above y=127 stays air. Vanilla does
    /// the same thing through `NoiseSettings.clampToHeightAccessor`.
    ///
    /// `size_horizontal: 1, size_vertical: 2` are the same cell dimensions
    /// as the overworld: 4 wide, 8 tall.
    #[must_use]
    pub const fn nether() -> Self {
        Self {
            min_y: 0,
            height: 128,
            cell_width: 4,
            cell_height: 8,
            sea_level: 32,
            ore_veins_enabled: false,
            aquifers_enabled: false,
            default_fluid: crate::density::fill::Substance::Lava,
        }
    }

    /// The End's, from `worldgen/noise_settings/end.json`.
    ///
    /// Two things here are unlike either other dimension and both change
    /// every block:
    ///
    /// * **`size_horizontal: 2, size_vertical: 1`** — cells are 8 wide and
    ///   **4** tall, where the overworld and the nether are both 4 wide and 8
    ///   tall. So a chunk is 2x2 cells, not 4x4, and the interpolation
    ///   lattice is a different shape. Copying the other two dimensions'
    ///   4/8 here gives terrain that looks like the End and is not it.
    /// * **`default_fluid` is air and `sea_level` is 0.** There is no fluid
    ///   in the End at all; `Aquifer.createDisabled` still asks the global
    ///   picker what sits below sea level, and with sea level 0 nothing in
    ///   the generated range 0..127 is below it.
    #[must_use]
    pub const fn end() -> Self {
        Self {
            min_y: 0,
            height: 128,
            cell_width: 8,
            cell_height: 4,
            sea_level: 0,
            ore_veins_enabled: false,
            aquifers_enabled: false,
            default_fluid: crate::density::fill::Substance::Air,
        }
    }
}

/// Which heightmap [`NoiseFill::base_height`] is answering.
///
/// `Heightmap$Types.isOpaque()` is the predicate `iterateNoiseColumn` stops
/// on, and the two maps the structures stage reads differ only in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wg {
    /// `WORLD_SURFACE_WG` — `!state.isAir()`.
    WorldSurface,
    /// `OCEAN_FLOOR_WG` — `state.blocksMotion()`.
    OceanFloor,
}

/// Runs the noise fill for one chunk.
///
/// Consumed by [`Self::fill`] rather than reused: the aquifer's caches are
/// sized and keyed to one chunk's lattice, so a second chunk needs a second
/// instance. That is vanilla's lifecycle too — `NoiseChunk.forChunk`.
pub struct NoiseFill {
    sampler: ChunkSampler,
    /// `None` when the settings disable aquifers, in which case the fill uses
    /// `Aquifer.createDisabled`'s rule instead.
    aquifer: Option<NoiseBasedAquifer>,
    surface: SurfaceLevels,
    veins: Option<OreVeinifier>,
    /// The chunk's 2D caches, shared by the sampler, the aquifer and the
    /// surface scan — exactly as one `NoiseChunk` shares them in vanilla.
    flat: FlatCaches,
    picker: GlobalFluidPicker,
    /// Root indices for the vein toggle / ridged / gap functions.
    vein_roots: Option<(usize, usize, usize)>,
    settings: FillSettings,
    chunk_x: i32,
    chunk_z: i32,
}

/// The pieces of the router a fill needs, already built for the world seed.
pub struct FillInputs {
    pub final_density: crate::density::DensityFunction,
    pub aquifer_noises: AquiferNoises,
    pub preliminary_surface_level: crate::density::DensityFunction,
    pub aquifer_random: crate::rng::XoroshiroPositionalFactory,
    pub ore_veinifier: Option<OreVeinifier>,
    /// This chunk's `Beardifier`, or `None` for a beardless fill.
    ///
    /// **It rides here rather than in the router, and that is load-bearing.**
    /// The beardifier is per-chunk data — it holds the structure pieces near
    /// *this* chunk — where every other field is a per-seed graph. More to
    /// the point, `ChunkGenerator.getBaseHeight` walks a noise column with
    /// `BeardifierMarker.INSTANCE` at `iterateNoiseColumn` offset 187, so it
    /// is **beardless on purpose**: structure placement must not stand on
    /// terrain its own structures raised.
    ///
    /// [`crate::stage::noise::base_height`] builds a `NoiseFill` from a
    /// `FillInputs` too, and leaves this `None`. Baking the beard into the
    /// router instead would give `getBaseHeight` a beard silently, and would
    /// destroy the 1,024-column `router walk == canvas == vanilla` equality
    /// that gates the column evaluator.
    pub beardifier: Option<std::sync::Arc<crate::density::beard::Beardifier>>,
}

impl NoiseFill {
    #[must_use]
    pub fn new(mut inputs: FillInputs, settings: FillSettings, chunk_x: i32, chunk_z: i32) -> Self {
        let chunk_min_x = chunk_x * 16;
        let chunk_min_z = chunk_z * 16;

        // Built over *every* function this chunk will evaluate, before any of
        // them are used. One cache, shared, is not an optimisation detail:
        // vanilla's NoiseChunk wraps the whole router at once, so the aquifer
        // reads erosion and depth through the same quart-snapped table the
        // terrain does. Handing the aquifer unwrapped functions computes
        // values vanilla never computes.
        let mut vein_functions: Vec<DensityFunction> = inputs
            .ore_veinifier
            .as_ref()
            .map(|v| vec![v.toggle().clone(), v.ridged().clone(), v.gap().clone()])
            .unwrap_or_default();
        let flat = {
            let mut roots: Vec<&mut DensityFunction> = vec![
                &mut inputs.final_density,
                &mut inputs.preliminary_surface_level,
                &mut inputs.aquifer_noises.barrier,
                &mut inputs.aquifer_noises.fluid_level_floodedness,
                &mut inputs.aquifer_noises.fluid_level_spread,
                &mut inputs.aquifer_noises.lava,
                &mut inputs.aquifer_noises.erosion,
                &mut inputs.aquifer_noises.depth,
            ];
            roots.extend(vein_functions.iter_mut());
            FlatCaches::build(&mut roots, chunk_min_x, chunk_min_z, 16)
        };

        let picker =
            GlobalFluidPicker::with_default_fluid(settings.sea_level, settings.default_fluid);
        let mut surface = SurfaceLevels::new(inputs.preliminary_surface_level);
        let aquifer = settings.aquifers_enabled.then(|| {
            NoiseBasedAquifer::new(
                inputs.aquifer_noises,
                &flat,
                &mut surface,
                AquiferBounds {
                    chunk_x,
                    chunk_z,
                    min_block_y: settings.min_y,
                    y_block_size: settings.height,
                },
                inputs.aquifer_random,
                picker,
            )
        });

        // One sampler, four roots. The vein functions are `interpolated(..)`
        // markers exactly like the terrain's, so they must come off the same
        // cell lattice — sampling them per block puts veins in the wrong
        // places, which is a scattered ore/stone swap rather than an obvious
        // failure. They are `Staged` because the block-state rule list reads
        // them with `fillingCell == false`, unlike finalDensity which is read
        // out of `cacheAllInCell`; see `density::chunk::Interpolation`.
        let veins = if settings.ore_veins_enabled {
            inputs.ore_veinifier
        } else {
            None
        };
        // `NoiseChunk`'s constructor, offsets 495-515:
        //
        //     cacheAllInCell(add(finalDensity, beardifier))
        //
        // The `add` is applied **after** `FlatCaches::build` has walked the
        // roots above, deliberately: the beardifier has no flat caches and no
        // markers of any kind, so wrapping before the walk would shift every
        // slot index for no reason. Vanilla's `wrap` likewise substitutes the
        // beardifier into an already-numbered tree.
        //
        // The node is skipped entirely when the chunk beards nothing, where
        // vanilla adds `Beardifier.EMPTY` and gets 0.0. `x + 0.0 == x` for
        // every finite `x`, so the two agree — except that `-0.0 + 0.0` is
        // `+0.0`. Nothing downstream distinguishes the two zeros, but
        // skipping the node makes a beardless chunk byte-identical by
        // construction rather than by that argument, and a gate that rests on
        // an argument is a gate that has to be re-earned every time it moves.
        let final_density = match inputs.beardifier {
            Some(beardifier) if !beardifier.is_empty() => crate::density::build::add(
                inputs.final_density,
                crate::density::DensityFunction::Beardifier(beardifier),
            ),
            _ => inputs.final_density,
        };
        let mut roots = vec![Root::cell_filled(final_density)];
        let vein_roots = if veins.is_some() {
            let base = roots.len();
            for function in vein_functions.drain(..) {
                roots.push(Root::staged(function));
            }
            Some((base, base + 1, base + 2))
        } else {
            None
        };
        let sampler = ChunkSampler::new(
            roots,
            &flat,
            CellGeometry::new(
                chunk_min_x,
                chunk_min_z,
                settings.min_y,
                settings.height,
                settings.cell_width,
                settings.cell_height,
            ),
        );

        Self {
            sampler,
            aquifer,
            surface,
            veins,
            flat,
            picker,
            vein_roots,
            settings,
            chunk_x,
            chunk_z,
        }
    }

    /// `doFill`.
    ///
    /// Walks the chunk in vanilla's order — cell x, cell z, then **downward**
    /// through cell y and the blocks inside each cell. The direction is not
    /// arbitrary: the heightmaps are primed by taking the first
    /// qualifying block seen from the top, so a bottom-up walk would need a
    /// different update rule to get the same answer.
    #[must_use]
    pub fn fill(self) -> FilledChunk {
        self.fill_retaining().chunk
    }

    /// `doFill`, keeping the pieces a later stage needs.
    ///
    /// Carvers run long after the fill and ask the **same** aquifer what
    /// belongs in the holes they cut. Vanilla keeps the `NoiseChunk` alive
    /// across both stages for exactly that reason, so this returns the
    /// aquifer and the caches it reads through rather than dropping them.
    ///
    /// Rebuilding a fresh aquifer instead would give the same *values* — its
    /// location cache is a memo of positional draws, so it is a pure function
    /// of position — but it would recompute every one of them, and it would
    /// lose `should_schedule_fluid_update`, which decides which positions get
    /// a fluid tick scheduled.
    #[must_use]
    pub fn fill_retaining(mut self) -> PostFill {
        let s = &self.settings;
        let min_y = s.min_y;
        let height = s.height;
        let cell_width = s.cell_width;
        let cell_height = s.cell_height;
        let cell_min_y = min_y.div_euclid(cell_height);
        let cell_count_y = height.div_euclid(cell_height);
        let cell_count_xz = 16 / cell_width;

        let chunk_start_x = self.chunk_x * 16;
        let chunk_start_z = self.chunk_z * 16;

        let mut chunk = FilledChunk {
            chunk_x: self.chunk_x,
            chunk_z: self.chunk_z,
            min_y,
            height,
            blocks: vec![Substance::Air; (height * 256) as usize],
            // Heightmaps start at the world floor and only ever rise, so an
            // all-air column reports min_y — vanilla's empty value.
            world_surface: [min_y; 256],
            ocean_floor: [min_y; 256],
            post_processing: Vec::new(),
        };

        for cell_x in 0..cell_count_xz {
            for cell_z in 0..cell_count_xz {
                for cell_y in (0..cell_count_y).rev() {
                    for y_in_cell in (0..cell_height).rev() {
                        let pos_y = (cell_min_y + cell_y) * cell_height + y_in_cell;

                        for x_in_cell in 0..cell_width {
                            let pos_x = chunk_start_x + cell_x * cell_width + x_in_cell;

                            for z_in_cell in 0..cell_width {
                                let pos_z = chunk_start_z + cell_z * cell_width + z_in_cell;
                                let state = self.material(pos_x, pos_y, pos_z);

                                // Air is never written: the section is air
                                // already, and skipping it is also what
                                // keeps air out of both heightmaps.
                                if state.is_air() {
                                    continue;
                                }

                                let local_x = pos_x - chunk_start_x;
                                let local_z = pos_z - chunk_start_z;
                                let index = chunk.index(local_x, pos_y, local_z);
                                chunk.blocks[index] = state;

                                let column = (local_z * 16 + local_x) as usize;
                                if chunk.world_surface[column] <= pos_y {
                                    chunk.world_surface[column] = pos_y + 1;
                                }
                                if state.blocks_motion() && chunk.ocean_floor[column] <= pos_y {
                                    chunk.ocean_floor[column] = pos_y + 1;
                                }

                                let schedule = self
                                    .aquifer
                                    .as_ref()
                                    .is_some_and(NoiseBasedAquifer::should_schedule_fluid_update);
                                if schedule && state.is_fluid() {
                                    chunk.post_processing.push((pos_x, pos_y, pos_z));
                                }
                            }
                        }
                    }
                }
            }
        }

        PostFill {
            chunk,
            aquifer: self.aquifer,
            flat: self.flat,
            surface: self.surface,
        }
    }

    /// `ChunkGenerator.getBaseHeight`, via `iterateNoiseColumn`.
    ///
    /// **This needs no filled chunk**, which is the entire point of it. The
    /// structures stage currently reads `WORLD_SURFACE_WG`/`OCEAN_FLOOR_WG`
    /// off the canvas, and those are the noise stage's own fossils — so the
    /// stage cannot move above the thing that produces them, and vanilla runs
    /// `STRUCTURE_STARTS` *before* `NOISE`. Vanilla reads this instead, which
    /// walks a noise column against the router and touches no chunk.
    ///
    /// # Why the containing chunk's lattice rather than a one-cell one
    ///
    /// Vanilla builds `NoiseChunk(1, .., cellX * cellWidth, cellZ * cellWidth, ..)`
    /// — a single cell anchored at the cell containing the column. This walks
    /// the same column on the *chunk's* lattice instead. The two give the same
    /// numbers because cells are globally aligned: a cell's corners are at
    /// absolute multiples of `cell_width`, so the cell containing `(x, z)` has
    /// the same eight corners whichever origin the lattice declares. Only the
    /// array bounds differ, and [`ChunkSampler::evaluate`] resolves a position
    /// to its cell from the block coordinates rather than from a walk order.
    ///
    /// It is done this way because it is **additive**: no existing constructor,
    /// geometry or cache changes, and the chunk fill stays exactly the code
    /// that is byte-identical to vanilla's own `doFill`. The cost is building
    /// a chunk's flat caches per query instead of a cell's, which is nothing
    /// beside a structure placement's other work and can be memoised later.
    ///
    /// **It is gated, not argued**: `worldgen-dump wg-heightmaps` prints this
    /// beside the canvas fossil for all 256 columns of a chunk, and the canvas
    /// fossil is itself proved equal to vanilla's own `getBaseHeight` over
    /// 1,024 columns (`docs/WORKLIST.md`).
    ///
    /// # Beardless, deliberately
    ///
    /// `iterateNoiseColumn` passes `BeardifierMarker.INSTANCE` at offset 187,
    /// so vanilla's structure placement never sees terrain its own structures
    /// raise. Nothing here adds a beard either, and nothing here may — once
    /// the beard exists this and the canvas fossil stop being equal, and that
    /// is correct rather than a regression.
    #[must_use]
    pub fn base_height(mut self, x: i32, z: i32, kind: Wg) -> i32 {
        let s = &self.settings;
        let min_y = s.min_y;
        let cell_height = s.cell_height;
        let cell_min_y = min_y.div_euclid(cell_height);
        let cell_count_y = s.height.div_euclid(cell_height);

        // Downward, exactly as `iterateNoiseColumn` walks: the first block
        // from the top that satisfies the predicate wins, and the answer is
        // `y + 1` — vanilla's `OptionalInt.of(y + 1)`, and the same `pos_y + 1`
        // the fill records into its heightmaps.
        for cell_y in (0..cell_count_y).rev() {
            for y_in_cell in (0..cell_height).rev() {
                let pos_y = (cell_min_y + cell_y) * cell_height + y_in_cell;
                let state = self.material(x, pos_y, z);
                let matched = match kind {
                    Wg::WorldSurface => !state.is_air(),
                    Wg::OceanFloor => state.blocks_motion(),
                };
                if matched {
                    return pos_y + 1;
                }
            }
        }
        // `OptionalInt.empty().orElse(level.getMinY())`.
        min_y
    }

    /// `ChunkGenerator.getBaseColumn` — the whole column's blocks, not just
    /// its height.
    ///
    /// Vanilla builds a `BlockState[cellCountY * cellHeight]`, fills it with
    /// `iterateNoiseColumn(.., column, null)` and wraps it in a `NoiseColumn`
    /// anchored at `noiseSettings.minY()`. The returned vector is indexed the
    /// same way: element 0 is `settings.min_y`.
    ///
    /// # Same walk as `base_height`, and that is not incidental
    ///
    /// `iterateNoiseColumn` is one method with two callers: with a non-null
    /// array it fills a column, with a null one and a predicate it answers a
    /// height. Both walk **downward** through the cells. This shares
    /// [`Self::material`] with [`Self::base_height`] for the same reason, and
    /// keeps the downward order rather than filling upward, because the
    /// aquifer and the flat caches carry per-position state and a different
    /// visit order is a different set of cache fills.
    ///
    /// # Beardless, for the same reason `base_height` is
    ///
    /// `iterateNoiseColumn` passes `BeardifierMarker.INSTANCE` at offset 187.
    /// A structure asking what the ground looks like must not be shown the
    /// ground its own kind raises. The beard reaches a fill only through
    /// [`FillInputs::beardifier`], and every caller that builds a
    /// `NoiseFill` for this leaves it `None`.
    #[must_use]
    pub fn base_column(mut self, x: i32, z: i32) -> Vec<Substance> {
        let s = &self.settings;
        let min_y = s.min_y;
        let cell_height = s.cell_height;
        let cell_min_y = min_y.div_euclid(cell_height);
        let cell_count_y = s.height.div_euclid(cell_height);

        let len = usize::try_from(cell_count_y * cell_height).unwrap_or(0);
        let mut column = vec![Substance::Air; len];
        for cell_y in (0..cell_count_y).rev() {
            for y_in_cell in (0..cell_height).rev() {
                let pos_y = (cell_min_y + cell_y) * cell_height + y_in_cell;
                let substance = self.material(x, pos_y, z);
                if let Ok(index) = usize::try_from(pos_y - min_y) {
                    if index < column.len() {
                        column[index] = substance;
                    }
                }
            }
        }
        column
    }

    /// `MaterialRuleList.calculate` — the aquifer, then ore veins, first
    /// non-`None` wins; `None` from both means the default block.
    fn material(&mut self, x: i32, y: i32, z: i32) -> Substance {
        let density = self.sampler.density(&self.flat, x, y, z);
        // `RUSTMC_DUMP_FILL_DENSITY=x,z` prints the density this fill hands
        // the aquifer, for a whole column. It is deliberately *not* the value
        // `golden_interpolated` compares: that gate samples through an API,
        // and vanilla's fill reads its interpolator's own state, so the two
        // are different numbers on vanilla's side too. See docs/BENCH.md.
        if let Some(target) = dump_fill_density() {
            if target == format!("{x},{z}").as_str() {
                println!("FILLDENSITY {x},{y},{z} = {density}");
            }
        }
        let claimed = match &mut self.aquifer {
            Some(aquifer) => {
                aquifer.compute_substance(&self.flat, &mut self.surface, (x, y, z), density)
            }
            None => crate::density::fill::compute_substance_disabled(&self.picker, y, density),
        };
        if let Some(substance) = claimed {
            return substance;
        }
        // Only reached where the aquifer declined, which is why veins can
        // never replace water or air.
        //
        // The three inputs are pulled lazily, in vanilla's order. Most blocks
        // never get past the first: the vein y band is -60..=50 out of a
        // 384-block column, so roughly three quarters exit after `Toggle`.
        let Self {
            veins,
            vein_roots,
            sampler,
            flat,
            ..
        } = self;
        if let (Some(veins), Some((toggle, ridged, gap))) = (veins.as_ref(), *vein_roots) {
            let sample = |axis| {
                let root = match axis {
                    VeinAxis::Toggle => toggle,
                    VeinAxis::Ridged => ridged,
                    VeinAxis::Gap => gap,
                };
                sampler.evaluate(flat, root, x, y, z)
            };
            if let Some(block) = veins.calculate((x, y, z), sample) {
                return Substance::Vein(block);
            }
        }
        Substance::Solid
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::density::build;
    use crate::density::router::{OverworldNoises, OverworldRouter};
    use crate::noise::NoiseParameters;
    use crate::rng::XoroshiroRandomSource;

    /// A fill driven by constants rather than the real router, so the loop
    /// structure is under test and not the noise.
    fn flat_fill(density: f64, chunk_x: i32, chunk_z: i32) -> FilledChunk {
        let seeding = XoroshiroRandomSource::from_seed(7).fork_positional();
        NoiseFill::new(
            FillInputs {
                final_density: build::constant(density),
                aquifer_noises: AquiferNoises {
                    barrier: build::constant(0.0),
                    // Fully flooded, so the aquifer gives sea level rather
                    // than the dry sentinel.
                    fluid_level_floodedness: build::constant(1.0),
                    fluid_level_spread: build::constant(0.0),
                    lava: build::constant(0.0),
                    erosion: build::constant(0.0),
                    depth: build::constant(0.0),
                },
                preliminary_surface_level: build::constant(64.0),
                aquifer_random: seeding,
                ore_veinifier: None,
                // No beard: this fixture predates structures entirely.
                beardifier: None,
            },
            FillSettings {
                ore_veins_enabled: false,
                ..FillSettings::overworld()
            },
            chunk_x,
            chunk_z,
        )
        .fill()
    }

    #[test]
    fn solid_density_fills_the_whole_column() {
        let chunk = flat_fill(1.0, 0, 0);
        for x in 0..16 {
            for z in 0..16 {
                assert_eq!(chunk.get(x, 0, z), Substance::Solid, "({x}, {z})");
                assert_eq!(chunk.get(x, 319, z), Substance::Solid);
                // Solid to the very top, so both heightmaps hit the ceiling.
                assert_eq!(chunk.world_surface(x, z), 320);
                assert_eq!(chunk.ocean_floor(x, z), 320);
            }
        }
    }

    #[test]
    fn open_density_gives_sea_level_water_over_lava() {
        let chunk = flat_fill(-1.0, 0, 0);
        // Air above sea level, water below it, lava below -54: the global
        // picker's layering, which is what an aquifer-free column reduces to.
        assert_eq!(chunk.get(8, 100, 8), Substance::Air);
        assert_eq!(chunk.get(8, 62, 8), Substance::Water);
        assert_eq!(
            chunk.get(8, 63, 8),
            Substance::Air,
            "sea level is exclusive"
        );
        assert_eq!(chunk.get(8, -60, 8), Substance::Lava);
    }

    #[test]
    fn heightmaps_ignore_fluids_differently() {
        let chunk = flat_fill(-1.0, 0, 0);
        // The water surface is at 62, so WORLD_SURFACE is 63. Nothing blocks
        // motion anywhere in the column, so OCEAN_FLOOR stays at the floor.
        assert_eq!(chunk.world_surface(0, 0), 63);
        assert_eq!(chunk.ocean_floor(0, 0), -64);
    }

    #[test]
    fn the_fill_covers_every_block_exactly_once() {
        // A guard on the loop bounds: cell_count * cell_size must tile the
        // chunk with no gap and no overlap. A gap shows up as leftover air
        // in a fully solid world.
        let chunk = flat_fill(1.0, 0, 0);
        let mut solid = 0usize;
        for y in -64..320 {
            for x in 0..16 {
                for z in 0..16 {
                    if chunk.get(x, y, z) == Substance::Solid {
                        solid += 1;
                    }
                }
            }
        }
        assert_eq!(solid, 16 * 16 * 384);
    }

    #[test]
    fn chunks_away_from_the_origin_are_indexed_locally() {
        // The block array is chunk-local but the density is sampled at world
        // coordinates; mixing the two is the classic way to generate the
        // origin chunk everywhere.
        let chunk = flat_fill(1.0, -7, 12);
        assert_eq!(chunk.chunk_x, -7);
        assert_eq!(chunk.get(0, 0, 0), Substance::Solid);
        assert_eq!(chunk.get(15, 319, 15), Substance::Solid);
    }

    /// The real overworld router, with parameters from the golden vectors.
    fn overworld_noises() -> OverworldNoises {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/golden/router_vectors.json"
        ))
        .expect("router vectors");
        let doc: serde_json::Value = serde_json::from_str(&text).expect("json");
        let mut set = OverworldNoises::new();
        for entry in doc["parameters"].as_array().expect("parameters") {
            let amplitudes: Vec<f64> = entry["amplitudes"]
                .as_array()
                .expect("amplitudes")
                .iter()
                .map(|v| {
                    f64::from_bits(v.as_str().expect("bits").parse::<i64>().expect("i64") as u64)
                })
                .collect();
            set = set.with(
                entry["name"].as_str().expect("name"),
                NoiseParameters::new(
                    entry["firstOctave"].as_i64().expect("firstOctave") as i32,
                    amplitudes,
                ),
            );
        }
        set
    }

    #[test]
    fn a_real_overworld_chunk_generates_and_looks_like_terrain() {
        // Not a parity check — that needs vanilla's own fill to compare
        // against. This asserts the whole stack runs end to end and produces
        // something shaped like a world: bedrock-level stone, air at the top,
        // and a surface somewhere sane.
        let noises = overworld_noises();
        let router = OverworldRouter::new(20260725, &noises, false);
        let seeding = crate::noise::NoiseSeeding::new(20260725);

        let chunk = NoiseFill::new(
            FillInputs {
                final_density: router.final_density.clone(),
                aquifer_noises: AquiferNoises {
                    barrier: router.barrier.clone(),
                    fluid_level_floodedness: router.fluid_level_floodedness.clone(),
                    fluid_level_spread: router.fluid_level_spread.clone(),
                    lava: router.lava.clone(),
                    erosion: router.erosion.clone(),
                    depth: router.depth.clone(),
                },
                preliminary_surface_level: router.preliminary_surface_level.clone(),
                aquifer_random: seeding.source_for("minecraft:aquifer").fork_positional(),
                ore_veinifier: Some(OreVeinifier::new(
                    router.vein_toggle.clone(),
                    router.vein_ridged.clone(),
                    router.vein_gap.clone(),
                    seeding.source_for("minecraft:ore").fork_positional(),
                )),
                // No beard: this fixture predates structures entirely.
                beardifier: None,
            },
            FillSettings::overworld(),
            0,
            0,
        )
        .fill();

        // The world floor is always solid in the overworld's slide.
        let mut floor_solid = 0;
        for x in 0..16 {
            for z in 0..16 {
                if chunk.get(x, -64, z) != Substance::Air {
                    floor_solid += 1;
                }
                assert_eq!(chunk.get(x, 319, z), Substance::Air, "sky at ({x}, {z})");
            }
        }
        assert_eq!(floor_solid, 256, "the world floor should be solid");

        // Surfaces inside the world, and not all identical — a constant
        // heightmap would mean the density graph never reached the fill.
        let surfaces: Vec<i32> = (0..16)
            .flat_map(|x| (0..16).map(move |z| (x, z)))
            .map(|(x, z)| chunk.world_surface(x, z))
            .collect();
        assert!(
            surfaces.iter().all(|h| (-64..=320).contains(h)),
            "a surface escaped the world"
        );
        assert!(
            surfaces.iter().any(|h| *h != surfaces[0]),
            "the terrain came out perfectly flat"
        );
    }

    #[test]
    fn disabling_aquifers_falls_back_to_the_global_fluid_rule() {
        // `Aquifer.createDisabled`: positive density is solid, everything
        // else is whatever the picker says sits at that height. Several
        // dimensions genuinely use this, and it is also the ablation that
        // isolates the aquifer's cost in the benchmark.
        let seeding = XoroshiroRandomSource::from_seed(1).fork_positional();
        let chunk = NoiseFill::new(
            FillInputs {
                final_density: build::constant(-1.0),
                aquifer_noises: AquiferNoises {
                    // Deliberately absurd: with aquifers off none of these
                    // may reach the output at all.
                    barrier: build::constant(1000.0),
                    fluid_level_floodedness: build::constant(1000.0),
                    fluid_level_spread: build::constant(1000.0),
                    lava: build::constant(1000.0),
                    erosion: build::constant(1000.0),
                    depth: build::constant(1000.0),
                },
                preliminary_surface_level: build::constant(64.0),
                aquifer_random: seeding,
                ore_veinifier: None,
                // No beard: this fixture predates structures entirely.
                beardifier: None,
            },
            FillSettings {
                ore_veins_enabled: false,
                aquifers_enabled: false,
                ..FillSettings::overworld()
            },
            0,
            0,
        )
        .fill();

        // Exactly the global picker's layering: air above sea level, water
        // below it, lava below -54.
        assert_eq!(chunk.get(8, 100, 8), Substance::Air);
        assert_eq!(
            chunk.get(8, 63, 8),
            Substance::Air,
            "sea level is exclusive"
        );
        assert_eq!(chunk.get(8, 62, 8), Substance::Water);
        assert_eq!(chunk.get(8, -60, 8), Substance::Lava);
        assert!(
            chunk.post_processing().is_empty(),
            "the disabled rule never schedules a fluid update"
        );
    }
}
