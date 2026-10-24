//! Noise primitives — the layer above [`crate::rng`] and below the density
//! functions.
//!
//! Reference: `net.minecraft.world.level.levelgen.synth.*`.
//!
//! Every value here is a pure function of (seed, coordinates), which is what
//! lets terrain generation parallelize freely across chunks (CRATES.md
//! regionization rule 4). Proved bit-exact against vanilla-generated vectors
//! in `tests/golden_noise.rs`; the ORACLE.md §5 rules on float order and
//! Java quirks are binding on every line.

pub mod blended;
pub mod improved;
pub mod normal;
pub mod perlin;
pub mod perlin_simplex;
pub mod registry;
pub mod simplex;

pub use blended::BlendedNoise;
pub use improved::ImprovedNoise;
pub use normal::NormalNoise;
pub use perlin::{make_amplitudes, wrap, ForkPositional, PerlinNoise};
pub use perlin_simplex::PerlinSimplexNoise;
pub use registry::{NoiseParameters, NoiseSeeding};
pub use simplex::SimplexNoise;
