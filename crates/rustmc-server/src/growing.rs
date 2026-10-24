//! One canvas per world, grown a ring at a time — and a chunk, once
//! delivered, is never written again.
//!
//! # The property this has, and the stronger one it deliberately does not
//!
//! [`crate::tiles`] gives every tile its own generation of its own square.
//! `docs/DETERMINISM.md` §31 measured what that costs and §32 priced the two
//! ways out of it. The result was not the expected one:
//!
//! * **Extent-independence is unreachable, not merely unreached.** A
//!   delivered chunk can only be a pure function of the seed and its position
//!   if no feature reads a neighbour's features, and vanilla's features read
//!   them — `WorldGenRegion.getChunk` caps the *request* at `CARVERS` and then
//!   hands back the live, mutable `ProtoChunk`. Honouring the cap zeroes the
//!   widening gate and costs 8,762 -> 42,873 mismatches against
//!   `oracle-work/van-s1-c62` on the seed-20260722 r=7 window at (64,64), and
//!   takes the End city window off 100.000000%. §32.3.
//! * So **"a tile is exactly the slice of one pass it claims to be" is not a
//!   property vanilla has**, and it is retired rather than owed.
//!
//! What is left is §32.6, and it is what a player can actually feel: the world
//! must not change behind them as they walk. That does **not** need a
//! delivered chunk to be independent of the generated extent. It needs the
//! canvas to be one canvas, and a chunk, once handed out, never to be written
//! again.
//!
//! # Why the pyramid makes that cheap
//!
//! `ChunkPyramid.GENERATION_PYRAMID`'s `FEATURES` step declares
//! `blockStateWriteRadius(1)`: decoration is the only step that writes outside
//! its own chunk, and it reaches exactly one chunk. So **once every chunk of a
//! chunk's own 3x3 has decorated, nothing in the pyramid can ever write into
//! it again** — there is no later step with a write radius at all, and every
//! chunk that could reach it has already had its turn.
//!
//! That is the whole mechanism:
//!
//! * one [`Canvas`] for the world, not one per tile;
//! * a [`ChunkStatus`] per chunk, upgraded at most once;
//! * a growth pass runs each stage over the chunks it *added* and nothing
//!   else — [`rustmc_worldgen::pipeline::Resumed`] is what carries that;
//! * a chunk is delivered only when its own 3x3 has decorated, and the ring
//!   is read off `ChunkStep.getAccumulatedRadiusOf(FEATURES)` rather than
//!   written down as a 1.
//!
//! # What it is not
//!
//! It is **not** a claim that a grown world equals a world generated at the
//! final size in one pass. It cannot be, for the reason in the first bullet
//! above, and [`GrowingWorld`] does not pretend otherwise: the chunks it
//! delivers at radius `r` are the chunks a pass at radius `r` produced, frozen
//! there for good, and a later ring decorates around them rather than
//! re-deciding them.
//!
//! # The failure it is built to have
//!
//! [`GrowingWorld::delivering_early`] hands out chunks one ring further than
//! the pyramid allows. It exists because the property has to be able to fail: a
//! chunk delivered before its own 3x3 has decorated is one the next ring's
//! decoration writes into, and a gate that could not show that would be a gate
//! measuring its own instrument. `tests/growing_world.rs` runs both delivery
//! rules over the same grown bytes.
//!
//! # What it reads — `docs/DETERMINISM.md` §33
//!
//! Four seed/centre ladders taken from radius 0 to 1 to 2, every block of
//! every column of every delivered chunk:
//!
//! ```text
//!   growing one canvas                         0 blocks moved, all 8 rungs
//!   regenerating the bigger square (tiles.rs)  4,059
//!   the ring a one-ring-early delivery hands   50,378
//!     out, off the same grown bytes
//!   chunks carved to deliver radius 2          81, against 155
//! ```
//!
//! The regenerate arm reproduces `tests/tiled_world.rs`'s eight numbers to the
//! block, so 4,059 -> 0 is one instrument's before and after. Both injections
//! have been run and were red; the one that removes the generate-once filter
//! reddens at **seed 20260722**, where `tiled_world.rs` reads 0.

use std::collections::{BTreeMap, BTreeSet};
use std::io;

use rustmc_worldgen::canvas::Canvas;
use rustmc_worldgen::pipeline::{extents_for, Area, Resumed, Through};
use rustmc_worldgen::status::{generation_pyramid, ChunkStatus};

/// A world that grows over one canvas.
pub struct GrowingWorld {
    seed: i64,
    /// The chunk every square is centred on.
    center: (i32, i32),
    /// The one canvas. `None` until the first pass.
    canvas: Option<Canvas>,
    /// Every chunk this world has touched, and how far it got.
    ///
    /// The status map §32.6 asks for, kept per chunk rather than as a pair of
    /// squares: the growth a server does is a player walking east, not a
    /// concentric ladder, and a pair of squares cannot say that.
    status: BTreeMap<(i32, i32), ChunkStatus>,
    /// How many rings **past** what the pyramid allows this world hands out.
    ///
    /// Zero is the honest rule. One is the injection — see the module docs.
    deliver_early: i32,
}

impl GrowingWorld {
    /// A world that has generated nothing yet.
    #[must_use]
    pub const fn new(seed: i64, center: (i32, i32)) -> Self {
        Self {
            seed,
            center,
            canvas: None,
            status: BTreeMap::new(),
            deliver_early: 0,
        }
    }

    /// The same world, delivering `rings` rings earlier than the pyramid
    /// allows.
    ///
    /// The injection, and it is a constructor rather than an environment
    /// variable so that both delivery rules can be read off **one** grown
    /// canvas in one process. Two runs would be two worlds and the comparison
    /// would be measuring the second generation as much as the rule.
    #[must_use]
    pub const fn delivering_early(mut self, rings: i32) -> Self {
        self.deliver_early = rings;
        self
    }

    /// How far a chunk's own 3x3 must have got before it may be handed out:
    /// `ChunkStep.getAccumulatedRadiusOf(FEATURES)` for a chunk reaching
    /// `FULL`, which the jar puts at 1.
    #[must_use]
    pub fn delivery_reach(&self) -> i32 {
        let step = generation_pyramid().step_to(ChunkStatus::Full);
        step.layer_radius(ChunkStatus::Features) - self.deliver_early
    }

    /// Grow the delivered square to `radius`, generating only what that adds.
    ///
    /// Returns how many chunks this pass had to carve, which is the whole of
    /// the *cost* claim in `docs/DETERMINISM.md` §32.6 and is measured rather
    /// than asserted: [`crate::tiles`] regenerates the bigger square whole, so
    /// the same ladder costs it every chunk of every square.
    ///
    /// # Errors
    /// Whatever the generator returned.
    pub fn grow_to(&mut self, radius: i32) -> io::Result<usize> {
        let area = Area::square(self.center.0, self.center.1, radius);
        let (decorated, generated) = extents_for(area);
        let carving = generated
            .chunks()
            .filter(|&(x, z)| !self.status_of(x, z).is_or_after(ChunkStatus::Carvers))
            .count();
        let resume = self.canvas.take().map(|canvas| Resumed {
            canvas,
            carved: self.chunks_at_least(ChunkStatus::Carvers),
            decorated: self.chunks_at_least(ChunkStatus::Features),
        });
        let out = worldgen_assembly::generate::generate_resuming(
            self.seed,
            area,
            Through::Decoration,
            None,
            resume,
        )
        .map_err(io::Error::other)?;
        self.canvas = Some(out.canvas);
        // The order matters: `decorated` is inside `generated`, so raising the
        // terrain statuses first and the decoration statuses second leaves
        // each chunk at the highest status it actually reached.
        for (x, z) in generated.chunks() {
            self.raise(x, z, ChunkStatus::Carvers);
        }
        for (x, z) in decorated.chunks() {
            self.raise(x, z, ChunkStatus::Features);
        }
        Ok(carving)
    }

    /// Raise a chunk's status, never lower it.
    fn raise(&mut self, chunk_x: i32, chunk_z: i32, status: ChunkStatus) {
        let slot = self
            .status
            .entry((chunk_x, chunk_z))
            .or_insert(ChunkStatus::Empty);
        *slot = (*slot).max(status);
    }

    /// Every chunk that has reached `status` or later.
    fn chunks_at_least(&self, status: ChunkStatus) -> BTreeSet<(i32, i32)> {
        self.status
            .iter()
            .filter(|(_, reached)| reached.is_or_after(status))
            .map(|(position, _)| *position)
            .collect()
    }

    /// Whether this chunk may be handed to a player.
    ///
    /// Derived from the status map and the pyramid, not from the radius the
    /// caller last asked for. That is deliberate: the closing criterion is
    /// that the delivered set is *what the pyramid says it may be*, so a
    /// delivered set read back off the request would agree with the request
    /// however wrong both were.
    #[must_use]
    pub fn is_delivered(&self, chunk_x: i32, chunk_z: i32) -> bool {
        let reach = self.delivery_reach();
        (-reach..=reach).all(|dz| {
            (-reach..=reach).all(|dx| {
                self.status
                    .get(&(chunk_x + dx, chunk_z + dz))
                    .is_some_and(|reached| reached.is_or_after(ChunkStatus::Features))
            })
        })
    }

    /// Every deliverable chunk, in the canvas's own x-then-z order.
    #[must_use]
    pub fn delivered(&self) -> Vec<(i32, i32)> {
        let mut out: Vec<(i32, i32)> = self
            .status
            .keys()
            .copied()
            .filter(|&(x, z)| self.is_delivered(x, z))
            .collect();
        out.sort_unstable();
        out
    }

    /// The status of a chunk, `Empty` for one this world has never touched.
    #[must_use]
    pub fn status_of(&self, chunk_x: i32, chunk_z: i32) -> ChunkStatus {
        self.status
            .get(&(chunk_x, chunk_z))
            .copied()
            .unwrap_or(ChunkStatus::Empty)
    }

    /// The one canvas, once something has been generated.
    #[must_use]
    pub const fn canvas(&self) -> Option<&Canvas> {
        self.canvas.as_ref()
    }

    /// The block at a position, or the void where nothing is generated.
    ///
    /// The same read [`crate::world::World::block_at`] makes once the overlay
    /// has answered `None`, which is every block a player has not changed.
    #[must_use]
    pub fn block_at(&self, x: i32, y: i32, z: i32) -> &str {
        self.canvas
            .as_ref()
            .map_or("minecraft:air", |canvas| canvas.block_at(x, y, z))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The delivery rule is the pyramid's, read out of the jar rather than
    /// written down here. If `FEATURES` ever moves off accumulated radius 1
    /// for a chunk reaching `FULL`, this is the line that says so.
    #[test]
    fn a_chunk_is_delivered_once_its_own_3x3_has_decorated() {
        let world = GrowingWorld::new(0, (0, 0));
        assert_eq!(world.delivery_reach(), 1);
        assert_eq!(world.delivering_early(1).delivery_reach(), 0);
    }

    /// The status map, without generating anything: a pass that delivered
    /// radius `r` leaves `r + 1` decorated and `r + 2` carved, and the
    /// delivered set that falls out of the 3x3 rule is exactly `r`.
    ///
    /// Arithmetic rather than terrain, so it runs in T0 where a mistake in it
    /// is cheap to find. The generated world is what
    /// `tests/growing_world.rs` measures.
    #[test]
    fn the_delivered_set_is_the_square_the_pyramid_allows() {
        let mut world = GrowingWorld::new(0, (0, 0));
        let (decorated, generated) = extents_for(Area::square(0, 0, 1));
        for (x, z) in generated.chunks() {
            world.raise(x, z, ChunkStatus::Carvers);
        }
        for (x, z) in decorated.chunks() {
            world.raise(x, z, ChunkStatus::Features);
        }
        assert_eq!(decorated.chunk_count(), 25, "3x3 grown by FEATURES @ 1");
        assert_eq!(generated.chunk_count(), 49, "3x3 grown by CARVERS @ 2");
        assert_eq!(
            world.delivered(),
            Area::square(0, 0, 1).chunks().collect::<Vec<_>>(),
            "the delivered set is the square that was asked for and no more"
        );
        // And one ring early is the ring that the next growth decorates into.
        assert_eq!(
            world.delivering_early(1).delivered().len(),
            decorated.chunk_count(),
            "delivering early hands out every decorated chunk, including the \
             ring whose neighbours have not decorated yet"
        );
    }

    /// A status never falls, whatever order the passes raise it in.
    #[test]
    fn a_status_only_ever_rises() {
        let mut world = GrowingWorld::new(0, (0, 0));
        world.raise(3, 4, ChunkStatus::Features);
        world.raise(3, 4, ChunkStatus::Carvers);
        assert_eq!(world.status_of(3, 4), ChunkStatus::Features);
        assert_eq!(world.status_of(9, 9), ChunkStatus::Empty);
    }
}
