//! Binding surface rules to a world seed.
//!
//! Reference: `SurfaceSystem`'s constructor and `RandomState`'s factories.
//!
//! Everything above this module is a pure function of its inputs. This is
//! where the inputs come from: the named surface noises, the positional
//! random factories, and the per-chunk state (the heightmap, the biome
//! source) that the conditions read.
//!
//! # Every random factory is derived by name
//!
//! ```text
//! Xoroshiro(world_seed).fork_positional()          the world's base factory
//!   .from_hash_of("minecraft:clay_bands")            -> the mesa palette
//!   .from_hash_of("minecraft:bedrock_floor")
//!       .fork_positional()                           -> that gradient's fade
//! ```
//!
//! Two consequences that are easy to get wrong. `clay_bands` uses the source
//! **directly**, without a second `fork_positional`, because the palette is
//! generated once from a stream rather than sampled per position. A
//! `vertical_gradient` **does** fork again, because it is sampled per block.
//! And the name is hashed with its `minecraft:` namespace, so dropping the
//! namespace gives a different, entirely valid-looking world.

use super::bands::{band_at, generate_bands, Band};
use super::conditions::{hole, steep, vertical_gradient};
use super::eval::{Sampling, SurfaceOracle};
use super::rule::BlockState;
use super::system::surface_depth;
use crate::noise::{NoiseParameters, NormalNoise};
use crate::rng::{PositionalRandomFactory, RandomSource};
use std::collections::BTreeMap;

crate::trace_switch!(counting, "RUSTMC_SURFACE_NOISE");

/// How many named-noise samples the surface stage **asked for**, and how many
/// it actually **computed**.
///
/// The ratio is the whole question this stage was opened on: vanilla shares
/// one memoised `DoubleSupplier` between every rule site naming a noise, so
/// its computed count is one per column per 2D noise and one per y per 3D
/// one. Ours asked once per rule site until [`WorldSurfaceOracle::flat_samples`]
/// existed. Counted only when `RUSTMC_SURFACE_NOISE` is set, because the
/// atomics would themselves be a cost on the path being measured.
pub mod noise_counts {
    use std::sync::atomic::{AtomicU64, Ordering};

    pub(super) static ASKED: AtomicU64 = AtomicU64::new(0);
    pub(super) static COMPUTED: AtomicU64 = AtomicU64::new(0);
    pub(super) static DEPTH_ASKED: AtomicU64 = AtomicU64::new(0);
    pub(super) static DEPTH_COMPUTED: AtomicU64 = AtomicU64::new(0);

    /// `(asked, computed, depth asked, depth computed)` since the process
    /// started. The second pair is `getSurfaceDepth`, which every `hole`
    /// condition used to resample from scratch.
    #[must_use]
    pub fn read() -> (u64, u64, u64, u64) {
        (
            ASKED.load(Ordering::Relaxed),
            COMPUTED.load(Ordering::Relaxed),
            DEPTH_ASKED.load(Ordering::Relaxed),
            DEPTH_COMPUTED.load(Ordering::Relaxed),
        )
    }
}

/// The noises a surface rule can name, plus the two the system samples
/// itself.
pub mod names {
    /// Sampled by `getSurfaceDepth`, and also nameable from a rule.
    pub const SURFACE: &str = "minecraft:surface";
    /// Sampled by `getSurfaceSecondary` for `stone_depth`'s secondary range.
    pub const SURFACE_SECONDARY: &str = "minecraft:surface_secondary";
    /// Offsets the mesa stripes so they wobble.
    pub const CLAY_BANDS_OFFSET: &str = "minecraft:clay_bands_offset";
    /// The three the frozen-ocean pass samples. None of them is nameable
    /// from a rule — `frozenOceanExtension` reads them directly.
    pub const ICEBERG_SURFACE: &str = "minecraft:iceberg_surface";
    /// Cuts spikes through the surface envelope.
    pub const ICEBERG_PILLAR: &str = "minecraft:iceberg_pillar";
    /// Caps how tall a spike may get.
    pub const ICEBERG_PILLAR_ROOF: &str = "minecraft:iceberg_pillar_roof";
}

/// The per-chunk state the conditions read.
///
/// Supplied by the caller because it owns the chunk: the heightmap comes
/// from the noise stage and the biomes from the biome source, and neither
/// belongs to this module.
pub struct ChunkSurfaceState<'a> {
    /// `WORLD_SURFACE_WG`, chunk-local, indexed `[z * 16 + x]`.
    pub heights: &'a [i32; 256],
    /// Biome at a block position, interned — see [`crate::surface::BiomeId`].
    ///
    /// The hottest call in the stage. It answers ~28,864 times a chunk and
    /// returned a freshly allocated `String` for every one of them until the
    /// id replaced the name.
    pub biome: &'a dyn Fn(i32, i32, i32) -> crate::surface::BiomeId,
    /// The name behind an id. **Diagnostics and un-interned rules only** —
    /// `RUSTMC_TRACE_BLOCK`, and the name-comparing `Condition::Biome` arm
    /// that the golden tests still evaluate. Never reached by the surface
    /// stage, which interns its rule before it runs.
    pub biome_name: &'a dyn Fn(crate::surface::BiomeId) -> String,
    /// The lowest preliminary surface in this position's cell.
    pub min_surface_level: &'a dyn Fn(i32, i32) -> i32,
    /// Whether this position's biome is one the `temperature` condition
    /// accepts — vanilla asks the biome whether it is cold enough to freeze.
    pub temperature: &'a dyn Fn(i32, i32, i32) -> bool,
    /// `Biome#shouldMeltFrozenOceanIcebergSlightly` — `getTemperature(pos,
    /// seaLevel) > 0.1F`, asked at `(x, seaLevel, z)`.
    ///
    /// Separate from `temperature` because it is a different comparison
    /// against a different threshold: that one is `coldEnoughToSnow`, which
    /// is `!(t >= 0.15)`. Folding them together would make a warm frozen
    /// ocean build cold-ocean icebergs.
    pub melt_iceberg_slightly: &'a dyn Fn(i32, i32, i32) -> bool,
}

/// Everything the surface rules derive from the world seed alone.
///
/// Split out because it is **per world, not per chunk**, and building it per
/// chunk is what made the surface stage 79% of the whole pipeline: every
/// named noise gets its octaves instantiated and the 192 clay bands get
/// drawn, once for each chunk that only ever reads them. Vanilla builds its
/// `SurfaceSystem` once per chunk generator for the same reason.
///
/// Nothing here depends on position, so sharing it changes no value — the
/// noises and the bands are pure functions of the seed.
pub struct SurfaceNoises {
    /// The instantiated noises, in the order [`Self::index`] numbers them.
    ///
    /// A `Vec` plus a name index rather than a `BTreeMap<String, _>` because
    /// the slot number is what a per-oracle memo is keyed on, and vanilla's
    /// own samplers are keyed the same way — `Context.noiseSamplers2d` is an
    /// `IdentityHashMap` on the noise's `ResourceKey`, so every rule site
    /// naming one noise shares one sampler.
    noises: Vec<NormalNoise>,
    index: BTreeMap<String, usize>,
    /// `RandomState.random` — the factory every named stream in the surface
    /// system is derived from, and the one thing that differs between a
    /// Xoroshiro dimension and a `legacy_random_source` one.
    root: crate::rng::AnyPositionalFactory,
    /// `SurfaceSystem.noiseRandom` — the world's base positional factory,
    /// used by `getSurfaceDepth`'s quarter-block jitter. The same value as
    /// [`Self::root`]; kept as its own field because vanilla passes it as a
    /// separate constructor argument and the two could stop being equal.
    noise_random: crate::rng::AnyPositionalFactory,
    bands: Vec<Band>,
}

impl SurfaceNoises {
    /// Instantiate every noise a rule can name, and draw the clay bands.
    ///
    /// `parameters` must contain every noise a rule names; a missing one is
    /// a panic on first use rather than a silent zero, because a zeroed
    /// noise generates a world that looks almost right.
    #[must_use]
    pub fn new(world_seed: i64, parameters: &BTreeMap<String, NoiseParameters>) -> Self {
        Self::for_dimension(world_seed, parameters, false)
    }

    /// The same, for a dimension whose noise settings set
    /// `legacy_random_source`.
    ///
    /// `RandomState`'s constructor is
    /// `settings.getRandomSource().newInstance(seed).forkPositional()`, and
    /// it hands that factory straight to `new SurfaceSystem(this,
    /// defaultBlock, seaLevel, this.random)`. The overworld is Xoroshiro; the
    /// nether and the end are `LEGACY`. Everything downstream — every named
    /// noise, the clay bands, the `vertical_gradient` streams and
    /// `getSurfaceDepth`'s jitter — comes off it, so this one boolean moves
    /// every surface block in the dimension.
    #[must_use]
    pub fn for_dimension(
        world_seed: i64,
        parameters: &BTreeMap<String, NoiseParameters>,
        legacy_random_source: bool,
    ) -> Self {
        let root = crate::rng::AnyPositionalFactory::for_world(world_seed, legacy_random_source);
        // `Noises.instantiate(noises, random, key)` — `NormalNoise.create` on
        // `random.fromHashOf(key)`. Not `createLegacyNetherBiome`: that is
        // `RandomState$1NoiseWiringHelper.visitNoise`'s special case for the
        // two nether *climate* keys only, and no surface noise is one of them.
        //
        // `parameters` is a `BTreeMap`, so the iteration order — and with it
        // every slot number — is the name order, which is a pure function of
        // the input and identical on every run.
        let mut noises = Vec::with_capacity(parameters.len());
        let mut index = BTreeMap::new();
        for (name, params) in parameters {
            let mut source = root.from_hash_of(name);
            index.insert(name.clone(), noises.len());
            noises.push(NormalNoise::create(
                &mut source,
                params.first_octave,
                &params.amplitudes,
            ));
        }

        // The palette is generated from the stream *directly* — no second
        // fork — because it is built once rather than sampled per position.
        let mut clay_source = root.from_hash_of("minecraft:clay_bands");
        let bands = generate_bands(&mut clay_source);

        Self {
            noises,
            index,
            root,
            noise_random: root,
            bands,
        }
    }

    /// The slot a named noise occupies, panicking with the name if it is
    /// absent — a missing surface noise generates a world that looks almost
    /// right, so it must not read as zero.
    fn slot_of(&self, name: &str) -> usize {
        *self
            .index
            .get(name)
            .unwrap_or_else(|| panic!("no parameters supplied for surface noise {name}"))
    }

    /// How many distinct noises exist, i.e. how many memo slots an oracle
    /// needs.
    fn slots(&self) -> usize {
        self.noises.len()
    }
}

/// A [`SurfaceOracle`] backed by a real world seed.
pub struct WorldSurfaceOracle<'a> {
    /// The per-world half, shared across every chunk. See [`SurfaceNoises`].
    shared: std::sync::Arc<SurfaceNoises>,
    /// One positional factory per `random_name`, derived on demand and kept,
    /// mirroring `RandomState.getOrCreateRandomFactory`'s cache. The cache
    /// is not an optimisation — deriving a factory twice would be fine, but
    /// keeping them makes it obvious that a name maps to exactly one stream.
    gradient_factories: BTreeMap<String, crate::rng::AnyPositionalFactory>,
    /// Vanilla's `Context.noiseSamplers2d` — one memoised sampler per noise,
    /// **shared by every rule site that names it**, holding the last value
    /// and the position it was taken at.
    ///
    /// The overworld surface rule names `minecraft:surface` at nineteen
    /// separate `noise_threshold` sites and reads it again from every `hole`;
    /// vanilla samples it once per column and hands the same `DoubleSupplier`
    /// to all of them. Without this each site resampled a multi-octave
    /// `NormalNoise` from scratch.
    ///
    /// Keyed on the exact position rather than on vanilla's `lastUpdateXZ`
    /// counter, which makes it a memo of a pure function and therefore
    /// incapable of changing a value: `NormalNoise::get_value` depends on
    /// nothing else. A counter would additionally have to be *invalidated*
    /// correctly, and that is where a cache turns into behaviour.
    flat_samples: Vec<Option<(i32, i32, f64)>>,
    /// The same for `createNoiseSampler3d`, which vanilla memoises per y.
    volumetric_samples: Vec<Option<(i32, i32, i32, f64)>>,
    /// `getSurfaceDepth`, remembered for the column being walked.
    ///
    /// `HoleCondition` in vanilla is a `LazyXZCondition` reading the
    /// `surfaceDepth` field that `updateXZ` already computed — it does not
    /// resample. Ours recomputed a `NormalNoise` **and** seeded a positional
    /// random at every `hole` site. A `Cell` because `surface_depth_at` is
    /// `&self`; the oracle is built per chunk on the worker that walks it and
    /// is never shared.
    depth_sample: std::cell::Cell<Option<(i32, i32, i32)>>,
    state: ChunkSurfaceState<'a>,
}

impl<'a> WorldSurfaceOracle<'a> {
    /// Build for a world seed.
    ///
    /// `parameters` must contain every noise a rule names; a missing one is
    /// a panic on first use rather than a silent zero, because a zeroed
    /// noise generates a world that looks almost right.
    #[must_use]
    pub fn new(
        world_seed: i64,
        parameters: &BTreeMap<String, NoiseParameters>,
        state: ChunkSurfaceState<'a>,
    ) -> Self {
        Self::with_noises(
            std::sync::Arc::new(SurfaceNoises::new(world_seed, parameters)),
            state,
        )
    }

    /// Bind an already-built [`SurfaceNoises`] to one chunk's state.
    ///
    /// This is the constructor a generator should use: the expensive half is
    /// built once for the world and every chunk pays only for the `Arc`
    /// clone and an empty cache.
    #[must_use]
    pub fn with_noises(
        shared: std::sync::Arc<SurfaceNoises>,
        state: ChunkSurfaceState<'a>,
    ) -> Self {
        let slots = shared.slots();
        Self {
            flat_samples: vec![None; slots],
            volumetric_samples: vec![None; slots],
            depth_sample: std::cell::Cell::new(None),
            shared,
            // Not carried across chunks. It *could* be — the factories are a
            // pure function of the name and the seed, which is exactly why
            // vanilla's own cache lives on `RandomState` — but keeping it
            // per oracle is what the code did when the parity numbers were
            // taken, and a cache is the wrong place to find out that a
            // refactor was not as pure as it looked.
            gradient_factories: BTreeMap::new(),
            state,
        }
    }

    /// Named `named_noise` rather than `noise` because the trait method is
    /// also called `noise` and the inherent one would shadow it.
    fn named_noise(&self, name: &str) -> &NormalNoise {
        &self.shared.noises[self.shared.slot_of(name)]
    }

    /// One 2D sample of a named noise, served from the per-noise memo when
    /// the column has not moved. See [`Self::flat_samples`].
    fn flat_sample(&mut self, name: &str, x: i32, z: i32) -> f64 {
        let slot = self.shared.slot_of(name);
        if counting() {
            noise_counts::ASKED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if let Some((mx, mz, value)) = self.flat_samples[slot] {
            if mx == x && mz == z {
                return value;
            }
        }
        if counting() {
            noise_counts::COMPUTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        let value = self.shared.noises[slot].get_value(f64::from(x), 0.0, f64::from(z));
        self.flat_samples[slot] = Some((x, z, value));
        value
    }

    /// `getSurfaceDepth` — the column's surface thickness.
    pub fn surface_depth_at(&self, x: i32, z: i32) -> i32 {
        if counting() {
            noise_counts::DEPTH_ASKED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if let Some((mx, mz, depth)) = self.depth_sample.get() {
            if mx == x && mz == z {
                return depth;
            }
        }
        if counting() {
            noise_counts::DEPTH_COMPUTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        let noise = self
            .named_noise(names::SURFACE)
            .get_value(f64::from(x), 0.0, f64::from(z));
        let mut random = self.shared.noise_random.at(x, 0, z);
        let depth = surface_depth(noise, random.next_f64());
        self.depth_sample.set(Some((x, z, depth)));
        depth
    }

    #[must_use]
    pub fn bands(&self) -> &[Band] {
        &self.shared.bands
    }

    /// `SurfaceSystem#buildSurface`'s second pass over a frozen-ocean column.
    ///
    /// Runs after the rule walk and writes into the water column, which no
    /// rule can reach — see [`crate::surface::iceberg`]. `start_y` is
    /// `getHeight(WORLD_SURFACE_WG, x, z) + 1`, the same value the walk
    /// starts from, and is also the y the biome is sampled at.
    ///
    /// Returns how many blocks it wrote.
    pub fn frozen_ocean_extension(
        &self,
        column: &mut dyn crate::surface::system::BlockColumn,
        x: i32,
        z: i32,
        start_y: i32,
        min_surface_level: i32,
        sea_level: i32,
    ) -> usize {
        // Resolved back to a name rather than plumbing two more ids through.
        // `frozenOceanExtension` runs once per COLUMN — at most 256 calls a
        // chunk against the ~28,864 the rule walk makes, so this is 0.9% of
        // the biome answers and the two literals stay legible next to the
        // vanilla method they come from.
        let biome = (self.state.biome)(x, start_y, z);
        let name = (self.state.biome_name)(biome);
        if name != "minecraft:frozen_ocean" && name != "minecraft:deep_frozen_ocean" {
            return 0;
        }
        let (fx, fz) = (f64::from(x), f64::from(z));
        let inputs = crate::surface::iceberg::IcebergInputs {
            surface: self
                .named_noise(names::ICEBERG_SURFACE)
                .get_value(fx, 0.0, fz),
            pillar: self
                .named_noise(names::ICEBERG_PILLAR)
                .get_value(fx * 1.28, 0.0, fz * 1.28),
            pillar_roof: self.named_noise(names::ICEBERG_PILLAR_ROOF).get_value(
                fx * 1.17,
                0.0,
                fz * 1.17,
            ),
            melt_slightly: (self.state.melt_iceberg_slightly)(x, sea_level, z),
        };
        let mut random = self.shared.noise_random.at(x, 0, z);
        crate::surface::iceberg::frozen_ocean_extension(
            column,
            &mut random,
            inputs,
            min_surface_level,
            start_y,
            sea_level,
        )
    }
}

impl SurfaceOracle for WorldSurfaceOracle<'_> {
    fn noise(&mut self, name: &str, x: i32, y: i32, z: i32, sampling: Sampling) -> f64 {
        // Vanilla memoises each sampler on the context's XZ or Y counter and
        // shares one sampler between every rule site naming that noise. This
        // is the same memo keyed on the position instead of the counter,
        // which cannot change a value because `NormalNoise::get_value` is a
        // pure function of it. See [`Self::flat_samples`].
        match sampling {
            Sampling::Flat => self.flat_sample(name, x, z),
            Sampling::Volumetric => {
                let slot = self.shared.slot_of(name);
                if counting() {
                    noise_counts::ASKED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                if let Some((mx, my, mz, value)) = self.volumetric_samples[slot] {
                    if mx == x && my == y && mz == z {
                        return value;
                    }
                }
                if counting() {
                    noise_counts::COMPUTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                let value =
                    self.shared.noises[slot].get_value(f64::from(x), f64::from(y), f64::from(z));
                self.volumetric_samples[slot] = Some((x, y, z, value));
                value
            }
        }
    }

    fn surface_secondary(&mut self, x: i32, z: i32) -> f64 {
        self.flat_sample(names::SURFACE_SECONDARY, x, z)
    }

    fn vertical_gradient(
        &mut self,
        random_name: &str,
        x: i32,
        y: i32,
        z: i32,
        true_at: i32,
        false_at: i32,
    ) -> bool {
        // Derived once per name and kept, as RandomState does. Forked again
        // because this one *is* sampled per position, unlike clay_bands.
        //
        // `get` before `entry`: `entry` takes an owned key, so the old shape
        // allocated a `String` on every call including the hits, and this is
        // called once per y wherever a bedrock gradient is in range.
        //
        // ONE lookup on the hit path, not two. The shape above was
        // `contains_key` followed by `[random_name]`, which walks the same
        // `BTreeMap<String, _>` twice — two string comparisons per node, and
        // every key here shares the `minecraft:` prefix so each comparison
        // reads ten equal bytes before it can differ. The bedrock-floor
        // gradient sits at the top of the overworld rule sequence, so this is
        // reached at every y of every column, ~34k times a chunk. Copying out
        // is free: `AnyPositionalFactory` is `Copy`, so the hit path never
        // touches the map again and the value is the same object either way.
        let factory = match self.gradient_factories.get(random_name) {
            Some(found) => *found,
            None => {
                let made = self.shared.root.fork_from_hash_of(random_name);
                self.gradient_factories
                    .insert(random_name.to_string(), made);
                made
            }
        };
        vertical_gradient(&factory, x, y, z, true_at, false_at)
    }

    fn steep(&mut self, x: i32, z: i32) -> bool {
        steep(self.state.heights, x, z)
    }

    fn hole(&mut self, x: i32, z: i32) -> bool {
        hole(self.surface_depth_at(x, z))
    }

    fn temperature(&mut self, x: i32, y: i32, z: i32) -> bool {
        (self.state.temperature)(x, y, z)
    }

    fn min_surface_level(&mut self, x: i32, z: i32) -> i32 {
        (self.state.min_surface_level)(x, z)
    }

    fn biome(&mut self, x: i32, y: i32, z: i32) -> crate::surface::BiomeId {
        (self.state.biome)(x, y, z)
    }

    fn biome_name(&self, id: crate::surface::BiomeId) -> String {
        (self.state.biome_name)(id)
    }

    fn bandlands(&mut self, x: i32, y: i32, z: i32) -> Option<BlockState> {
        // Column-constant, so it takes the same per-noise memo the rule sites
        // use — the offset only moves when the column does.
        let offset = self.flat_sample(names::CLAY_BANDS_OFFSET, x, z);
        Some(BlockState(
            band_at(&self.shared.bands, offset, y)
                .block_name()
                .to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::context::{SurfaceContext, NO_WATER};
    use crate::surface::eval::apply;
    use crate::surface::rule::{parse_rule, Rule};
    use serde_json::json;

    fn parameters() -> BTreeMap<String, NoiseParameters> {
        // Plausible shapes; the golden test against vanilla supplies the
        // real registry values. Every noise a rule can name must be here,
        // because a missing one panics rather than silently reading zero.
        let generic = NoiseParameters::new(-6, vec![1.0, 1.0, 1.0]);
        [
            names::SURFACE,
            names::SURFACE_SECONDARY,
            names::CLAY_BANDS_OFFSET,
        ]
        .into_iter()
        .map(|n| (n.to_string(), generic.clone()))
        .collect()
    }

    fn state<'a>(heights: &'a [i32; 256], biome: &'a str) -> ChunkSurfaceState<'a> {
        ChunkSurfaceState {
            heights,
            // One biome, so one id. These tests never evaluate a biome
            // condition; they check the band palette and the gradient.
            biome: &|_: i32, _: i32, _: i32| 0,
            biome_name: Box::leak(Box::new(move |_: crate::surface::BiomeId| {
                biome.to_string()
            })),
            min_surface_level: &|_, _| 0,
            temperature: &|_, _, _| false,
            melt_iceberg_slightly: &|_, _, _| false,
        }
    }

    #[test]
    fn the_same_seed_builds_the_same_band_palette() {
        let heights = [64i32; 256];
        let params = parameters();
        let a = WorldSurfaceOracle::new(7, &params, state(&heights, "minecraft:plains"));
        let b = WorldSurfaceOracle::new(7, &params, state(&heights, "minecraft:plains"));
        assert_eq!(a.bands(), b.bands());

        let c = WorldSurfaceOracle::new(8, &params, state(&heights, "minecraft:plains"));
        assert_ne!(a.bands(), c.bands(), "a different seed must restripe");
    }

    #[test]
    fn surface_depth_varies_across_a_chunk() {
        // A constant depth would mean the noise never reached it — the
        // world would still generate, with a uniform soil layer everywhere.
        let heights = [64i32; 256];
        let params = parameters();
        let oracle =
            WorldSurfaceOracle::new(20260725, &params, state(&heights, "minecraft:plains"));
        let depths: Vec<i32> = (0..16)
            .flat_map(|x| (0..16).map(move |z| (x, z)))
            .map(|(x, z)| oracle.surface_depth_at(x, z))
            .collect();
        assert!(
            depths.iter().any(|d| *d != depths[0]),
            "every column had depth {}, so the noise is not wired",
            depths[0]
        );
    }

    #[test]
    fn a_biome_rule_places_its_block() {
        // The smallest end-to-end path: seed -> oracle -> rule -> block.
        let heights = [64i32; 256];
        let params = parameters();
        let rule = parse_rule(&json!({
            "type": "minecraft:condition",
            "if_true": {"type": "minecraft:biome", "biome_is": "minecraft:desert"},
            "then_run": {"type": "minecraft:block",
                         "result_state": {"Name": "minecraft:sand"}}
        }))
        .unwrap();

        let mut ctx = SurfaceContext::new(-64, 384, 63);
        ctx.update_xz(3, 5, 3);
        ctx.update_y(1, 1, NO_WATER, 70);

        let mut desert = WorldSurfaceOracle::new(1, &params, state(&heights, "minecraft:desert"));
        assert_eq!(
            apply(&rule, &mut ctx, &mut desert).map(|b| b.0),
            Some("minecraft:sand".to_string())
        );

        // A different biome declines rather than placing anything.
        let mut plains = WorldSurfaceOracle::new(1, &params, state(&heights, "minecraft:plains"));
        let mut ctx = SurfaceContext::new(-64, 384, 63);
        ctx.update_xz(3, 5, 3);
        ctx.update_y(1, 1, NO_WATER, 70);
        assert_eq!(apply(&rule, &mut ctx, &mut plains), None);
    }

    #[test]
    fn bandlands_returns_a_terracotta() {
        let heights = [64i32; 256];
        let params = parameters();
        let mut oracle = WorldSurfaceOracle::new(3, &params, state(&heights, "minecraft:badlands"));
        let mut ctx = SurfaceContext::new(-64, 384, 63);
        ctx.update_xz(0, 0, 3);
        ctx.update_y(1, 1, NO_WATER, 90);
        let block = apply(&Rule::Bandlands, &mut ctx, &mut oracle).expect("a band");
        assert!(
            block.0.ends_with("terracotta"),
            "bandlands produced {}",
            block.0
        );
    }

    /// The per-noise memo is a memo of a pure function, so a long
    /// interleaved sequence of reads must give exactly what a *fresh* oracle
    /// gives for each position on its own.
    ///
    /// This is the assertion a byte-compare cannot make on its own: a world
    /// can come out identical while a cache is still one column stale
    /// somewhere no rule looks. The sequence deliberately revisits columns
    /// out of order and mixes the 2D and 3D reads of the same noise, which is
    /// the case vanilla keeps in two separate sampler maps.
    #[test]
    fn the_noise_memo_never_serves_another_position() {
        let heights = [64i32; 256];
        let params = parameters();
        let mut memoised =
            WorldSurfaceOracle::new(20260722, &params, state(&heights, "minecraft:plains"));

        let columns = [(0, 0), (1, 0), (0, 0), (17, -3), (1, 0), (-9, 40), (0, 0)];
        for (x, z) in columns {
            for y in [70, 12, 70, -30] {
                let mut fresh =
                    WorldSurfaceOracle::new(20260722, &params, state(&heights, "minecraft:plains"));
                for name in [names::SURFACE, names::SURFACE_SECONDARY] {
                    assert_eq!(
                        memoised.noise(name, x, y, z, Sampling::Flat),
                        fresh.noise(name, x, y, z, Sampling::Flat),
                        "{name} 2D at ({x}, {y}, {z})"
                    );
                    assert_eq!(
                        memoised.noise(name, x, y, z, Sampling::Volumetric),
                        fresh.noise(name, x, y, z, Sampling::Volumetric),
                        "{name} 3D at ({x}, {y}, {z})"
                    );
                }
                assert_eq!(
                    memoised.surface_secondary(x, z),
                    fresh.surface_secondary(x, z),
                    "secondary at ({x}, {z})"
                );
                assert_eq!(memoised.hole(x, z), fresh.hole(x, z), "hole at ({x}, {z})");
                assert_eq!(
                    memoised.surface_depth_at(x, z),
                    fresh.surface_depth_at(x, z),
                    "surface depth at ({x}, {z})"
                );
            }
        }
    }

    /// A 2D read and a 3D read of one noise are different values and must not
    /// share a slot — vanilla keeps `noiseSamplers2d` and `noiseSamplers3d`
    /// as two maps over the same registry for exactly this reason.
    #[test]
    fn the_two_samplings_of_one_noise_do_not_collide() {
        let heights = [64i32; 256];
        let params = parameters();
        let mut oracle =
            WorldSurfaceOracle::new(20260722, &params, state(&heights, "minecraft:plains"));
        let flat = oracle.noise(names::SURFACE, 5, 91, 7, Sampling::Flat);
        let volumetric = oracle.noise(names::SURFACE, 5, 91, 7, Sampling::Volumetric);
        assert_ne!(
            flat, volumetric,
            "a 3D read at y=91 must not be the y=0 slice"
        );
        // And re-reading each still gives its own answer, not the other's.
        assert_eq!(oracle.noise(names::SURFACE, 5, 91, 7, Sampling::Flat), flat);
        assert_eq!(
            oracle.noise(names::SURFACE, 5, 91, 7, Sampling::Volumetric),
            volumetric
        );
    }

    #[test]
    fn a_gradient_name_maps_to_exactly_one_stream() {
        // Two calls with the same name must agree, and a different name
        // must be able to disagree — otherwise `random_name` does nothing
        // and every gradient in the world shares a pattern.
        let heights = [64i32; 256];
        let params = parameters();
        let mut oracle = WorldSurfaceOracle::new(11, &params, state(&heights, "minecraft:plains"));

        let first: Vec<bool> = (0..64)
            .map(|i| oracle.vertical_gradient("minecraft:bedrock_floor", i, -62, i * 3, -64, -59))
            .collect();
        let again: Vec<bool> = (0..64)
            .map(|i| oracle.vertical_gradient("minecraft:bedrock_floor", i, -62, i * 3, -64, -59))
            .collect();
        assert_eq!(first, again, "the same name must be the same stream");

        let other: Vec<bool> = (0..64)
            .map(|i| oracle.vertical_gradient("minecraft:bedrock_roof", i, -62, i * 3, -64, -59))
            .collect();
        assert_ne!(first, other, "a different name must be a different stream");
    }
}
