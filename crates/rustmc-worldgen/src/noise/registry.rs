//! Named noise instantiation — world seed to concrete noise field.
//!
//! Reference: `net.minecraft.world.level.levelgen.{RandomState, Noises}`.
//!
//! Everything below this point is primitives. This is where a world seed
//! becomes *Minecraft's* noise: continentalness, erosion, ridges and the
//! rest, each seeded so that the same seed produces the same world forever.
//!
//! The chain is short and every link is load-bearing:
//!
//! ```text
//! Xoroshiro(world_seed)            // RandomState.random
//!   .fork_positional()             //   -> PositionalRandomFactory
//!   .from_hash_of("minecraft:<n>") // Noises.instantiate, MD5 of the name
//!   -> NormalNoise::create(.., params)
//! ```
//!
//! Two consequences worth stating, because both are easy to break:
//!
//! - Noises are keyed by **name**, not by order. Adding a noise, or
//!   instantiating them in a different sequence, cannot disturb the others.
//!   That is deliberate — it is what let Mojang add noises across versions
//!   without reshuffling existing worlds.
//! - The name is hashed with MD5 (`from_hash_of`), including the
//!   `minecraft:` namespace. Dropping the namespace silently produces a
//!   different, valid-looking world.

use super::normal::NormalNoise;
use crate::rng::{PositionalRandomFactory, XoroshiroRandomSource};

/// `NormalNoise.NoiseParameters` — the octave layout for one named noise.
#[derive(Clone, Debug, PartialEq)]
pub struct NoiseParameters {
    pub first_octave: i32,
    pub amplitudes: Vec<f64>,
}

impl NoiseParameters {
    #[must_use]
    pub fn new(first_octave: i32, amplitudes: Vec<f64>) -> Self {
        Self {
            first_octave,
            amplitudes,
        }
    }
}

/// The per-world random state that names are resolved against.
///
/// Mirrors the part of `RandomState` that worldgen noise needs. Held by
/// value and cheap to clone: it is two `i64`s.
#[derive(Clone, Copy, Debug)]
pub struct NoiseSeeding {
    factory: crate::rng::XoroshiroPositionalFactory,
}

impl NoiseSeeding {
    /// `RandomState`'s `random` field for the modern (Xoroshiro) algorithm.
    ///
    /// Legacy worlds use `LegacyRandomSource` here
    /// (`settings.useLegacyRandomSource`); that path arrives with the
    /// legacy noise settings and is not wired yet.
    #[must_use]
    pub fn new(world_seed: i64) -> Self {
        Self {
            factory: XoroshiroRandomSource::from_seed(world_seed).fork_positional(),
        }
    }

    /// The raw source behind a name, before any noise is built from it.
    ///
    /// Needed because not everything seeded from a name is a `NormalNoise`:
    /// `RandomState.wrapNew` re-seeds the 3D `BlendedNoise` from
    /// `fromHashOf("minecraft:terrain")`, and BlendedNoise consumes the
    /// source directly.
    #[must_use]
    pub fn source_for(&self, name: &str) -> XoroshiroRandomSource {
        self.factory.from_hash_of(name)
    }

    /// The world's base positional factory, before any name is hashed.
    ///
    /// `SurfaceSystem` takes this directly as its `noiseRandom` — the
    /// quarter-block jitter in `getSurfaceDepth` is drawn from it, not from
    /// a named stream.
    #[must_use]
    pub fn base_factory(&self) -> crate::rng::XoroshiroPositionalFactory {
        self.factory
    }

    /// `Noises.instantiate` — build the named noise for this world.
    ///
    /// `name` must be the full identifier including the namespace, e.g.
    /// `"minecraft:continentalness"`.
    #[must_use]
    pub fn instantiate(&self, name: &str, parameters: &NoiseParameters) -> NormalNoise {
        debug_assert!(
            name.contains(':'),
            "noise name {name:?} is missing its namespace — the namespace is hashed"
        );
        let mut source = self.factory.from_hash_of(name);
        NormalNoise::create(&mut source, parameters.first_octave, &parameters.amplitudes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> NoiseParameters {
        NoiseParameters::new(-7, vec![1.0, 1.0])
    }

    #[test]
    fn names_are_independent_of_each_other() {
        // Instantiation order must not matter — that is the property that
        // lets a version add a noise without changing existing worlds.
        let seeding = NoiseSeeding::new(20260708);
        let a1 = seeding.instantiate("minecraft:continentalness", &params());
        let _unused = seeding.instantiate("minecraft:erosion", &params());
        let a2 = seeding.instantiate("minecraft:continentalness", &params());
        assert_eq!(
            a1.get_value(1.5, 2.5, 3.5).to_bits(),
            a2.get_value(1.5, 2.5, 3.5).to_bits()
        );
    }

    #[test]
    fn different_names_give_different_fields() {
        let seeding = NoiseSeeding::new(20260708);
        let a = seeding.instantiate("minecraft:continentalness", &params());
        let b = seeding.instantiate("minecraft:erosion", &params());
        assert_ne!(
            a.get_value(1.5, 2.5, 3.5).to_bits(),
            b.get_value(1.5, 2.5, 3.5).to_bits()
        );
    }

    #[test]
    fn the_namespace_is_part_of_the_key() {
        // Dropping "minecraft:" is the silent-wrong-world mistake.
        let seeding = NoiseSeeding::new(1);
        let with = seeding.instantiate("minecraft:erosion", &params());
        let without = seeding.instantiate("x:erosion", &params());
        assert_ne!(
            with.get_value(0.5, 0.5, 0.5).to_bits(),
            without.get_value(0.5, 0.5, 0.5).to_bits()
        );
    }

    #[test]
    fn different_world_seeds_give_different_fields() {
        let a = NoiseSeeding::new(1).instantiate("minecraft:continentalness", &params());
        let b = NoiseSeeding::new(2).instantiate("minecraft:continentalness", &params());
        assert_ne!(
            a.get_value(1.5, 2.5, 3.5).to_bits(),
            b.get_value(1.5, 2.5, 3.5).to_bits()
        );
    }
}
