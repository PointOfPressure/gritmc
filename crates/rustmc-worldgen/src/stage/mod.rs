//! The pipeline's stages, each adapting a proved module to the shared canvas.
//!
//! A stage owns no generation logic. `noise` calls `chunk_gen::NoiseFill`,
//! which is gated at 100.000000% over 1.77M blocks; a future `surface` stage
//! will call `surface::build_column`, gated against dumped palettes. The
//! stages exist to say *when* each runs and *where* it writes, which is the
//! one thing no per-stage gate can check.
//!
//! Keeping them thin is deliberate. A stage that reimplemented anything would
//! be a second copy of a pipeline, and a second copy of a pipeline is
//! precisely the shape of the oracle faults this project keeps finding
//! (ORACLE.md §5b): two paths that agree until one of them is updated.

pub mod biome;
pub mod carver;
pub mod decorate;
pub mod fluid;
pub mod noise;
pub mod post_process;
pub mod surface;
