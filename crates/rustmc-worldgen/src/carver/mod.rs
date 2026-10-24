//! Carvers — the caves, ravines and tunnels cut out of the noise terrain.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `net.minecraft.world.level.levelgen.carver.*`,
//! `NoiseBasedChunkGenerator#applyCarvers`.
//!
//! This is the stage after [`crate::chunk_gen`] and before features. It is
//! also the first stage that is **not** a pure function of a position: a
//! carver seeded in one chunk reaches up to eight chunks away, so generating
//! chunk `(0, 0)` means replaying the carvers of all 289 chunks in `±8`, in
//! order, and keeping only what lands inside.
//!
//! That structure is the whole difficulty. The noise stage could be checked
//! one block at a time; this can only be checked a chunk at a time, and a
//! single wrong draw anywhere in those 289 replays moves every tunnel after
//! it.
//!
//! # What the stream looks like
//!
//! One `WorldgenRandom(LegacyRandomSource)` is reused for the whole sweep and
//! **reseeded** before every carver:
//!
//! ```text
//! for dx in -8..=8, dz in -8..=8:            # source chunk order
//!   for (index, carver) in biome.carvers:    # biome of the source chunk
//!     random.setLargeFeatureSeed(seed + index, sourceX, sourceZ)
//!     if carver.isStartChunk(random): carver.carve(...)
//! ```
//!
//! So the reseeding makes each carver independent of the ones before it —
//! but only across carvers. *Inside* one `carve` the stream is shared by
//! every tunnel and every provider sample, which is why
//! [`providers`] is so careful about draw counts.

pub mod providers;

pub mod canyon;
pub mod carve;
pub mod cave;
pub mod config;
pub mod driver;
pub mod mask;
