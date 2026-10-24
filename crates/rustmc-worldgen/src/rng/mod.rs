//! Bit-exact ports of vanilla's random sources.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `net.minecraft.world.level.levelgen.{Xoroshiro128PlusPlus,
//! XoroshiroRandomSource, LegacyRandomSource, BitRandomSource,
//! MarsagliaPolarGaussian, RandomSupport, WorldgenRandom}`,
//! `net.minecraft.util.Mth#getSeed`, `net.minecraft.util.RandomSource`.
//!
//! This is the bottom of the worldgen ladder: every noise value, every ore,
//! every feature placement is a pure function of these streams, so a single
//! wrong bit here makes every downstream parity number meaningless. Nothing
//! in this module is allowed to exist without a golden-vector test
//! (ORACLE.md §5 rule 1) — see `tests/golden_rng.rs`, which replays streams
//! exported from the JVM oracle.
//!
//! Java semantics that are load-bearing here and are called out again at
//! each site that reproduces them:
//!
//! - `>>>` is a logical shift on the 64-bit pattern; `>>` is arithmetic.
//! - Integer arithmetic wraps silently; every port uses `wrapping_*`.
//! - Mixed-width expressions promote at specific points. `Mth#getSeed`
//!   multiplies `x` in **32-bit** and `z` in **64-bit** — widening early
//!   would be a different function.
//! - `1.110223E-16F` / `5.9604645E-8F` are **float** literals. The float
//!   value is what gets used, even where the surrounding math is double.

pub mod draws;
pub mod gaussian;
pub mod legacy;
mod md5;
pub mod sha256;
pub mod support;
pub mod worldgen_random;
pub mod xoroshiro;

pub use legacy::{LegacyPositionalFactory, LegacyRandomSource};
pub use support::{block_seed, mix_stafford13, Seed128};
pub use worldgen_random::{WorldgenBacking, WorldgenRandom};
pub use xoroshiro::{Xoroshiro128PlusPlus, XoroshiroPositionalFactory, XoroshiroRandomSource};

/// A vanilla random source.
///
/// Mirrors `net.minecraft.util.RandomSource`. Deliberately **not** object-safe
/// by accident: worldgen is the declared P1 performance target (PRODUCT.md),
/// so callers monomorphize over the concrete source rather than paying a
/// virtual call per noise sample. `fork`/`fork_positional` stay inherent
/// methods on the concrete types for the same reason — Java returns the
/// interface there, but the concrete return type carries strictly more
/// information and costs strictly less.
pub trait RandomSource {
    /// Reseed in place. Note the two algorithms disagree about what a `seed`
    /// *is*: legacy scrambles it into a 48-bit LCG state, Xoroshiro upgrades
    /// it to 128 bits. Same argument, different meaning — intentional.
    fn set_seed(&mut self, seed: i64);

    fn next_i32(&mut self) -> i32;

    /// Uniform in `[0, bound)`.
    ///
    /// # Panics
    /// If `bound <= 0` (vanilla throws `IllegalArgumentException`).
    fn next_i32_bound(&mut self, bound: i32) -> i32;

    fn next_i64(&mut self) -> i64;

    fn next_bool(&mut self) -> bool;

    fn next_f32(&mut self) -> f32;

    fn next_f64(&mut self) -> f64;

    /// Marsaglia polar method.
    ///
    /// **Unproven at time of writing** — see [`gaussian`] for the
    /// `Math.log` fdlibm risk and the open goldengen item. Do not build
    /// parity-critical behavior on this until vectors exist.
    fn next_gaussian(&mut self) -> f64;

    /// Uniform in `[min, max_inclusive]`.
    ///
    /// Vanilla computes `nextInt(maxInclusive - min + 1) + min` in wrapping
    /// int arithmetic; a caller that inverts the bounds gets vanilla's
    /// (bad) answer rather than a Rust panic, which is the parity-correct
    /// outcome.
    fn next_i32_between_inclusive(&mut self, min: i32, max_inclusive: i32) -> i32 {
        self.next_i32_bound(max_inclusive.wrapping_sub(min).wrapping_add(1))
            .wrapping_add(min)
    }

    /// Burn `rounds` draws without using them.
    ///
    /// `PerlinNoise`'s legacy path skips a zero-amplitude octave by calling
    /// this with the exact draw count an `ImprovedNoise` construction would
    /// have consumed — so the *count* is behavior, not bookkeeping, and every
    /// later octave shifts if it is wrong.
    ///
    /// Vanilla's default draws `nextInt()` per round. `XoroshiroRandomSource`
    /// overrides it to pull the raw generator, which is the same draw
    /// (`nextInt` there is a truncated `nextLong`), so one definition is
    /// faithful to both.
    fn consume_count(&mut self, rounds: i32) {
        for _ in 0..rounds {
            let _ = self.next_i32();
        }
    }
}

/// Delegating impl so a `&mut dyn RandomSource` is itself a `RandomSource`.
///
/// Needed where one feature runs another and the inner one must draw from
/// the *same* stream: the outer feature holds `&mut R`, and handing the
/// inner a trait object is the only way to pass it on without making every
/// feature in the crate generic over an unsized rng.
impl<T: RandomSource + ?Sized> RandomSource for &mut T {
    fn set_seed(&mut self, seed: i64) {
        (**self).set_seed(seed);
    }
    fn next_i32(&mut self) -> i32 {
        (**self).next_i32()
    }
    fn next_i32_bound(&mut self, bound: i32) -> i32 {
        (**self).next_i32_bound(bound)
    }
    fn next_i64(&mut self) -> i64 {
        (**self).next_i64()
    }
    fn next_bool(&mut self) -> bool {
        (**self).next_bool()
    }
    fn next_f32(&mut self) -> f32 {
        (**self).next_f32()
    }
    fn next_f64(&mut self) -> f64 {
        (**self).next_f64()
    }
    fn next_gaussian(&mut self) -> f64 {
        (**self).next_gaussian()
    }
}

/// Builds independent, position-addressed streams from one parent stream.
///
/// Mirrors `net.minecraft.world.level.levelgen.PositionalRandomFactory`. The
/// point is reproducibility without ordering: the stream for a block is a
/// function of its coordinates alone, so chunk generation parallelizes
/// (CRATES.md regionization rule 4) without changing output.
// `from_hash_of` / `from_seed` take `&self` because they are factory methods
// on a factory object, not conversions of one — the parent seed is half the
// input. The names are vanilla's (`fromHashOf`, `fromSeed`) and are kept so
// the port reads against the reference; renaming them to satisfy the lint
// would cost more than the lint is worth here.
#[allow(clippy::wrong_self_convention)]
pub trait PositionalRandomFactory {
    type Source: RandomSource;

    /// Stream for a block position.
    fn at(&self, x: i32, y: i32, z: i32) -> Self::Source;

    /// Stream keyed by a registry name (feature/carver salting).
    fn from_hash_of(&self, name: &str) -> Self::Source;

    /// Stream keyed by a raw seed.
    fn from_seed(&self, seed: i64) -> Self::Source;
}

/// A random source that is one of vanilla's two algorithms, chosen at
/// runtime.
///
/// # Why this exists at all, given the module docs argue against it
///
/// The docs above say callers monomorphize rather than pay a virtual call.
/// That holds everywhere the *algorithm* is a compile-time fact — the
/// overworld's whole ladder — and it stops holding at exactly one boundary:
/// `NoiseGeneratorSettings.useLegacyRandomSource`. `RandomState` picks
/// `WorldgenRandom$Algorithm.newInstance(seed).forkPositional()` from a
/// datapack field, and hands that factory to `SurfaceSystem`. The overworld
/// is Xoroshiro; the nether and the end are legacy. A generic parameter
/// there would infect `SurfaceNoises`, `WorldSurfaceOracle`, the assembly's
/// `SurfaceShared` and every caller of the surface stage, to express a
/// choice made once per world.
///
/// So this is an enum, not a trait object: the dispatch is a predictable
/// branch on a value that never changes during a run, and it is confined to
/// the surface system.
#[derive(Clone, Debug)]
pub enum AnyRandomSource {
    Xoroshiro(XoroshiroRandomSource),
    Legacy(LegacyRandomSource),
}

impl RandomSource for AnyRandomSource {
    fn set_seed(&mut self, seed: i64) {
        match self {
            Self::Xoroshiro(r) => r.set_seed(seed),
            Self::Legacy(r) => r.set_seed(seed),
        }
    }
    fn next_i32(&mut self) -> i32 {
        match self {
            Self::Xoroshiro(r) => r.next_i32(),
            Self::Legacy(r) => r.next_i32(),
        }
    }
    fn next_i32_bound(&mut self, bound: i32) -> i32 {
        match self {
            Self::Xoroshiro(r) => r.next_i32_bound(bound),
            Self::Legacy(r) => r.next_i32_bound(bound),
        }
    }
    fn next_i64(&mut self) -> i64 {
        match self {
            Self::Xoroshiro(r) => r.next_i64(),
            Self::Legacy(r) => r.next_i64(),
        }
    }
    fn next_bool(&mut self) -> bool {
        match self {
            Self::Xoroshiro(r) => r.next_bool(),
            Self::Legacy(r) => r.next_bool(),
        }
    }
    fn next_f32(&mut self) -> f32 {
        match self {
            Self::Xoroshiro(r) => r.next_f32(),
            Self::Legacy(r) => r.next_f32(),
        }
    }
    fn next_f64(&mut self) -> f64 {
        match self {
            Self::Xoroshiro(r) => r.next_f64(),
            Self::Legacy(r) => r.next_f64(),
        }
    }
    fn next_gaussian(&mut self) -> f64 {
        match self {
            Self::Xoroshiro(r) => r.next_gaussian(),
            Self::Legacy(r) => r.next_gaussian(),
        }
    }
    /// **Delegated, not defaulted.** `XoroshiroRandomSource` overrides
    /// `consume_count` to pull the raw generator; taking the trait's default
    /// here would draw through `next_i32` instead. A wrapper that silently
    /// drops an override is how an octave skip goes wrong.
    fn consume_count(&mut self, rounds: i32) {
        match self {
            Self::Xoroshiro(r) => r.consume_count(rounds),
            Self::Legacy(r) => r.consume_count(rounds),
        }
    }
}

/// The positional factory matching [`AnyRandomSource`].
#[derive(Clone, Copy, Debug)]
pub enum AnyPositionalFactory {
    Xoroshiro(XoroshiroPositionalFactory),
    Legacy(LegacyPositionalFactory),
}

impl AnyPositionalFactory {
    /// `RandomState.random` for a world — Xoroshiro or legacy, from
    /// `NoiseGeneratorSettings.useLegacyRandomSource`.
    ///
    /// Both are `new <Algorithm>(seed).forkPositional()`, which is what
    /// `RandomState`'s constructor does with the algorithm the settings name.
    #[must_use]
    pub fn for_world(world_seed: i64, legacy: bool) -> Self {
        if legacy {
            Self::Legacy(LegacyRandomSource::new(world_seed).fork_positional())
        } else {
            Self::Xoroshiro(XoroshiroRandomSource::from_seed(world_seed).fork_positional())
        }
    }

    /// `factory.fromHashOf(name).forkPositional()` —
    /// `RandomState.getOrCreateRandomFactory`.
    #[must_use]
    pub fn fork_from_hash_of(&self, name: &str) -> Self {
        match self {
            Self::Xoroshiro(f) => Self::Xoroshiro(f.from_hash_of(name).fork_positional()),
            Self::Legacy(f) => Self::Legacy(f.from_hash_of(name).fork_positional()),
        }
    }
}

impl PositionalRandomFactory for AnyPositionalFactory {
    type Source = AnyRandomSource;

    fn at(&self, x: i32, y: i32, z: i32) -> Self::Source {
        match self {
            Self::Xoroshiro(f) => AnyRandomSource::Xoroshiro(f.at(x, y, z)),
            Self::Legacy(f) => AnyRandomSource::Legacy(f.at(x, y, z)),
        }
    }

    fn from_hash_of(&self, name: &str) -> Self::Source {
        match self {
            Self::Xoroshiro(f) => AnyRandomSource::Xoroshiro(f.from_hash_of(name)),
            Self::Legacy(f) => AnyRandomSource::Legacy(f.from_hash_of(name)),
        }
    }

    fn from_seed(&self, seed: i64) -> Self::Source {
        match self {
            Self::Xoroshiro(f) => AnyRandomSource::Xoroshiro(f.from_seed(seed)),
            Self::Legacy(f) => AnyRandomSource::Legacy(f.from_seed(seed)),
        }
    }
}
