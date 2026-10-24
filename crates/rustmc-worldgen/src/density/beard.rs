//! `Beardifier` — the density term by which a structure raises and flattens
//! the terrain it stands on.
//!
//! Reference: `net.minecraft.world.level.levelgen.Beardifier`, read with
//! `javap -p -c -constants`.
//!
//! # Where it enters, and the two ways of getting that wrong
//!
//! `NoiseChunk`'s constructor — not the datapack — builds the root the fill
//! reads, at offsets 495-515:
//!
//! ```text
//!   497: NoiseRouter.finalDensity()
//!   500: getstatic BeardifierMarker.INSTANCE
//!   503: DensityFunctions.add
//!   506: DensityFunctions.cacheAllInCell
//!   515: mapAll(this::wrap)          <- swaps the real Beardifier in
//! ```
//!
//! So the shape is `cache_all_in_cell(add(final_density, beardifier))`, and
//! two things about it are easy to get wrong in ways that still produce a
//! plausible number:
//!
//! **It is added AFTER the squeeze, not before.** `finalDensity` is already
//! `squeeze(interpolated(..))` — the squeeze is *inside* it — so the `add`
//! lands on the post-squeeze value. Adding the beard before the squeeze
//! pushes it through a non-linear map and yields a smaller, entirely
//! believable, wrong beard.
//!
//! **It is evaluated per block, not per cell corner.** The `add` sits inside
//! `cacheAllInCell` and outside every `interpolated(..)` marker, so the beard
//! is computed at every block position rather than at the eight corners and
//! lerped. That falls out for free here: [`crate::density::chunk::Root`]
//! computes the root function per block and only the interpolated markers
//! beneath it come off the lattice.
//!
//! # Why it is threaded through `FillInputs` and never baked into the router
//!
//! `ChunkGenerator.getBaseHeight` walks a noise column via
//! `iterateNoiseColumn`, which builds its `NoiseChunk` with
//! `BeardifierMarker.INSTANCE` at offset 187 — **the marker, not a
//! beardifier**. `getBaseHeight` is beardless on purpose: structure
//! placement must not see the terrain its own structures raise, or a village
//! would stand on its own foundation and climb.
//!
//! [`crate::stage::noise::base_height`] builds its own `NoiseFill` from a
//! `FillInputs`, so it stays beardless for free as long as the beard travels
//! in that struct. Bake it into the router instead and `getBaseHeight`
//! silently gains a beard.
//!
//! # What has a vanilla oracle here and what does not
//!
//! `ci/beard-golden.sh` dumps vanilla's own kernel, both contribution
//! functions and `Beardifier.compute` over every `TerrainAdjustment`;
//! `tests/golden_beard.rs` checks this module against all of it, bit for bit.
//!
//! **`for_structures_in_chunk` has NO vanilla oracle.** It is ported here
//! from bytecode and covered by unit tests derived from that bytecode — the
//! reference filter, the junction window, the rigid/non-rigid split, the
//! `inflatedBy(24)` — but running vanilla's own version needs a
//! `StructureManager`, which needs a server, so nothing below terminates in a
//! value vanilla produced. That is a real difference in the strength of the
//! two halves of this module and it is stated rather than averaged away: the
//! density term is *measured* against vanilla, the collection is *argued*
//! from vanilla.
//!
//! And it is the collection that decides whether the term fires at all, which
//! is the failure mode this project has already paid for once. A beardifier
//! wired to an empty piece list is indistinguishable, in every world diff,
//! from a beardifier that correctly declines — so `RUSTMC_TRACE_BEARD=1`
//! reports how many starts were handed over and how many chunks actually got
//! a beard. Read it before believing a null result.

use crate::math::{clamped_map, fast_inv_sqrt, length, length_squared};
use crate::structure::pieces::BoundingBox;
use crate::structure::template_pool::TerrainAdaptation;
use std::sync::OnceLock;

/// `Beardifier.BEARD_KERNEL_RADIUS`.
pub const BEARD_KERNEL_RADIUS: i32 = 12;

/// `Beardifier.BEARD_KERNEL_SIZE`.
const BEARD_KERNEL_SIZE: i32 = 24;

/// The flat length of `BEARD_KERNEL`: 24 cubed.
const BEARD_KERNEL_LEN: usize = 13_824;

/// `Beardifier.Rigid(BoundingBox, TerrainAdjustment, int)`.
///
/// The name is vanilla's and is slightly misleading: a `Rigid` is built for
/// **every** piece, not only the ones whose pool projection is `RIGID`. The
/// projection decides the `groundLevelDelta` — a rigid pool element
/// contributes its own, everything else contributes 0 — not whether a piece
/// is collected at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rigid {
    pub bounds: BoundingBox,
    pub adjustment: TerrainAdaptation,
    pub ground_level_delta: i32,
}

/// `JigsawJunction`, reduced to the three fields the beard reads.
///
/// `deltaY` and `destProjection` are carried by vanilla's class and never
/// looked at by `Beardifier.compute`, so they are deliberately absent rather
/// than stored and ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Junction {
    pub source_x: i32,
    pub source_ground_y: i32,
    pub source_z: i32,
}

/// What a structure piece contributes, which depends on its Java class.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PieceShape {
    /// `PoolElementStructurePiece` — a jigsaw piece.
    Pool {
        /// `getElement().getProjection() == RIGID`.
        rigid: bool,
        /// `getGroundLevelDelta()`. Read only when `rigid`.
        ground_level_delta: i32,
        /// `getJunctions()`. Read whether or not the piece is rigid.
        junctions: Vec<Junction>,
    },
    /// Any other `StructurePiece` — a mineshaft corridor, a fossil, a
    /// shipwreck hull. Contributes a `Rigid` with a `groundLevelDelta` of 0
    /// and has no junctions.
    Plain,
}

/// One piece of a start, as [`Beardifier::for_structures_in_chunk`] reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartPiece {
    pub bounds: BoundingBox,
    pub shape: PieceShape,
}

/// One `StructureStart`, reduced to what the beard reads.
///
/// `union_box` is `StructureStart.getBoundingBox()` — the union of every
/// piece — and it is not decoration: it is what
/// `ChunkGenerator.createReferences` tests to decide whether this start is
/// visible to a chunk at all. See
/// [`Beardifier::for_structures_in_chunk`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Start {
    pub adjustment: TerrainAdaptation,
    pub union_box: BoundingBox,
    pub pieces: Vec<StartPiece>,
}

/// `BoundingBox.intersects(int, int, int, int)` — XZ only, inclusive.
///
/// Y is deliberately absent. Both callers of this in `Beardifier`'s path are
/// horizontal tests, and a port that reaches for the six-argument
/// `intersects` instead would drop every structure whose box does not also
/// overlap in height — which for a village on a hillside is most of them.
const fn intersects_xz(bounds: BoundingBox, x0: i32, z0: i32, x1: i32, z1: i32) -> bool {
    bounds.max_x >= x0 && bounds.min_x <= x1 && bounds.max_z >= z0 && bounds.min_z <= z1
}

/// `BoundingBox.inflatedBy(int)`, in the two axes the reference filter reads.
///
/// Vanilla inflates all six sides; `intersects` is XZ-only, so y is left
/// alone rather than inflated and immediately ignored — a y that moved here
/// would be a value nothing reads, which is how a wrong one survives.
const fn inflated_xz(bounds: BoundingBox, by: i32) -> BoundingBox {
    BoundingBox {
        min_x: bounds.min_x - by,
        max_x: bounds.max_x + by,
        min_z: bounds.min_z - by,
        max_z: bounds.max_z + by,
        ..bounds
    }
}

/// `Beardifier.includeBoundingBox` — the null-tolerant union.
fn include(current: Option<BoundingBox>, next: BoundingBox) -> BoundingBox {
    match current {
        None => next,
        Some(current) => {
            BoundingBox::encapsulating(&[current, next]).expect("two boxes always encapsulate")
        }
    }
}

/// `Beardifier` — a chunk's structure pieces, as a density function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Beardifier {
    pieces: Vec<Rigid>,
    junctions: Vec<Junction>,
    /// `affectedBox`. `None` is vanilla's `null`, which is `Beardifier.EMPTY`
    /// and computes 0 everywhere — the state a chunk with no bearding
    /// structure is in.
    affected: Option<BoundingBox>,
}

impl Beardifier {
    /// `Beardifier(List<Rigid>, List<JigsawJunction>, BoundingBox)`.
    #[must_use]
    pub fn new(
        pieces: Vec<Rigid>,
        junctions: Vec<Junction>,
        affected: Option<BoundingBox>,
    ) -> Self {
        Self {
            pieces,
            junctions,
            affected,
        }
    }

    /// `Beardifier.EMPTY` — no pieces, no junctions, no affected box.
    #[must_use]
    pub fn empty() -> Self {
        Self::new(Vec::new(), Vec::new(), None)
    }

    /// Whether this beardifier can ever return non-zero.
    ///
    /// Vanilla has no such method; it always builds the `add` node and lets
    /// `EMPTY` return 0. This exists so the caller can skip constructing the
    /// node at all, which keeps a chunk with no bearding structure provably
    /// byte-identical rather than identical-up-to-signed-zero.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.affected.is_none()
    }

    /// `Beardifier.forStructuresInChunk(StructureManager, ChunkPos)`.
    ///
    /// # The reference filter, which vanilla applies somewhere else
    ///
    /// Vanilla's first line is
    /// `structureManager.startsForStructure(chunkPos, adaptation != NONE)`,
    /// and that is **not** "every start in the world". It reads the chunk's
    /// `getAllReferences()` map, which `ChunkGenerator.createReferences`
    /// populated one status earlier: for each chunk it scans the starts of
    /// the 17x17 chunks around it and records a reference only where
    ///
    /// ```text
    ///   start.getBoundingBox().intersects(minX, minZ, minX + 15, minZ + 15)
    /// ```
    ///
    /// — the start's bounding box against the chunk's own 16x16 footprint,
    /// in XZ only.
    ///
    /// **`getBoundingBox()` is not the union of the pieces.** That is the
    /// whole of the correction below, and the paragraph this replaces got it
    /// wrong while calling itself load-bearing:
    ///
    /// ```text
    ///   StructureStart.getBoundingBox()
    ///     = structure.adjustBoundingBox(pieceContainer.calculateBoundingBox())
    ///   Structure.adjustBoundingBox(box)
    ///     = terrainAdaptation == NONE ? box : box.inflatedBy(12)
    /// ```
    ///
    /// So every start that reaches this method has already been tested with
    /// its union **inflated by twelve** — the same twelve as
    /// [`BEARD_KERNEL_RADIUS`], which is not a coincidence: the inflation
    /// exists so that a chunk holding none of a structure's blocks, but
    /// lying inside its beard, still gets a reference to it.
    ///
    /// The old text said the reference test rejects a start whose nearest
    /// piece is five blocks outside the chunk. It does not, and it cannot:
    /// with the inflation in place the test is **provably redundant** against
    /// the per-piece filter below. Every piece box is contained in the union,
    /// so if some piece meets `[minX-12, minX+27] x [minZ-12, minZ+27]` then
    /// some point of the union does, and the union inflated by twelve
    /// therefore meets `[minX, minX+15] x [minZ, minZ+15]`. It is kept
    /// because vanilla keeps it and because it is a cheap early-out, not
    /// because it decides anything.
    ///
    /// Measured, seed 20260722, chunk (78,53), Threadripper: the
    /// `trial_chambers` start at chunk (75,55) has union
    /// `[1144,-42,800 .. 1247,5,881]`, one block short of that chunk's
    /// `minX = 1248`. Un-inflated, the start was dropped and the chunk got no
    /// beard at all; vanilla's own `doFill` given those pieces writes 217
    /// blocks there.
    ///
    /// There is no reference map in this pipeline, so the test is applied
    /// here, against each start's own union box. `starts` is therefore
    /// allowed to be every bearding start in the area.
    ///
    /// # Order
    ///
    /// The sum in [`Beardifier::compute`] is floating point and therefore not
    /// associative, so the order pieces are collected in is part of the
    /// answer. Vanilla's is: the reference map's entry order (one entry per
    /// structure type), then the start's `LongSet`, then `getPieces()` in
    /// list order. With one bearding structure type in play the first is
    /// moot; the second only matters where two starts of the same type reach
    /// one chunk, which villages are spaced too far apart to do. `starts` is
    /// consumed in the order given and the caller owns that.
    #[must_use]
    pub fn for_structures_in_chunk(starts: &[Start], chunk_x: i32, chunk_z: i32) -> Self {
        let min_x = chunk_x * 16;
        let min_z = chunk_z * 16;

        let mut pieces: Vec<Rigid> = Vec::new();
        let mut junctions: Vec<Junction> = Vec::new();
        let mut affected: Option<BoundingBox> = None;

        for start in starts {
            // `forStructuresInChunk`'s own predicate, at offset 2.
            if start.adjustment == TerrainAdaptation::None {
                continue;
            }
            // `StructureStart.isValid()` — a start with no pieces is not one.
            if start.pieces.is_empty() {
                continue;
            }
            // The reference filter described above. The union is inflated by
            // twelve first, because `Structure.adjustBoundingBox` inflates it
            // before `createReferences` ever sees it and every start here has
            // a non-`None` adaptation by the test just above.
            if !intersects_xz(
                inflated_xz(start.union_box, BEARD_KERNEL_RADIUS),
                min_x,
                min_z,
                min_x + 15,
                min_z + 15,
            ) {
                continue;
            }

            for piece in &start.pieces {
                // `StructurePiece.isCloseToChunk(chunkPos, 12)`, which is
                // `boundingBox.intersects(minX - 12, minZ - 12, minX + 27,
                // minZ + 27)`.
                if !intersects_xz(
                    piece.bounds,
                    min_x - BEARD_KERNEL_RADIUS,
                    min_z - BEARD_KERNEL_RADIUS,
                    min_x + 15 + BEARD_KERNEL_RADIUS,
                    min_z + 15 + BEARD_KERNEL_RADIUS,
                ) {
                    continue;
                }

                match &piece.shape {
                    // A `PoolElementStructurePiece`. Note the asymmetry: only
                    // a RIGID projection contributes a `Rigid` **and** grows
                    // the affected box, but the junctions of a
                    // TERRAIN_MATCHING piece are still collected. A port that
                    // wraps the whole arm in the projection test loses them.
                    PieceShape::Pool {
                        rigid,
                        ground_level_delta,
                        junctions: piece_junctions,
                    } => {
                        if *rigid {
                            pieces.push(Rigid {
                                bounds: piece.bounds,
                                adjustment: start.adjustment,
                                ground_level_delta: *ground_level_delta,
                            });
                            affected = Some(include(affected, piece.bounds));
                        }
                        for junction in piece_junctions {
                            // The junction window, offsets 264-308. Strict on
                            // all four sides and asymmetric: `> min - 12` but
                            // `< min + 15 + 12`.
                            if junction.source_x > min_x - BEARD_KERNEL_RADIUS
                                && junction.source_z > min_z - BEARD_KERNEL_RADIUS
                                && junction.source_x < min_x + 15 + BEARD_KERNEL_RADIUS
                                && junction.source_z < min_z + 15 + BEARD_KERNEL_RADIUS
                            {
                                junctions.push(*junction);
                                // `new BoundingBox(BlockPos)` — the single
                                // block at the junction's source.
                                affected = Some(include(
                                    affected,
                                    BoundingBox::new(
                                        junction.source_x,
                                        junction.source_ground_y,
                                        junction.source_z,
                                        junction.source_x,
                                        junction.source_ground_y,
                                        junction.source_z,
                                    ),
                                ));
                            }
                        }
                    }
                    // Anything that is not a pool piece: a `Rigid` with a
                    // `groundLevelDelta` of **0**, not the element's.
                    PieceShape::Plain => {
                        pieces.push(Rigid {
                            bounds: piece.bounds,
                            adjustment: start.adjustment,
                            ground_level_delta: 0,
                        });
                        affected = Some(include(affected, piece.bounds));
                    }
                }
            }
        }

        // `inflatedBy(24)` at offset 413 — see `BoundingBox::inflated_by`.
        Self::new(pieces, junctions, affected.map(|b| b.inflated_by(24)))
    }

    /// `Beardifier.compute(FunctionContext)`.
    #[must_use]
    pub fn compute(&self, x: i32, y: i32, z: i32) -> f64 {
        let Some(affected) = self.affected else {
            return 0.0;
        };
        if !affected.is_inside(x, y, z) {
            return 0.0;
        }

        let mut total = 0.0;
        for piece in &self.pieces {
            let bounds = piece.bounds;

            // `Math.max(0, Math.max(minX - x, x - maxX))` — the axis distance
            // to the box, zero inside it. Written as vanilla writes it rather
            // than as an `if inside`, because the two differ nowhere and the
            // bytecode is the thing being ported.
            let dx = 0.max((bounds.min_x - x).max(x - bounds.max_x));
            let dz = 0.max((bounds.min_z - z).max(z - bounds.max_z));

            // The piece's own ground line, and the height above it. Note this
            // is `minY + groundLevelDelta`, so a piece whose delta puts its
            // floor above its box bottom beards from the floor, not the box.
            let ground = bounds.min_y + piece.ground_level_delta;
            let above_ground = y - ground;

            // Two switches on the same value, and they do NOT partition the
            // enum the same way: `BURY` shares the first arm with
            // `BEARD_THIN` and the second with nothing.
            let dy = match piece.adjustment {
                TerrainAdaptation::None => 0,
                TerrainAdaptation::Bury | TerrainAdaptation::BeardThin => above_ground,
                TerrainAdaptation::BeardBox => 0.max((ground - y).max(y - bounds.max_y)),
                TerrainAdaptation::Encapsulate => 0.max((bounds.min_y - y).max(y - bounds.max_y)),
            };

            total += match piece.adjustment {
                TerrainAdaptation::None => 0.0,
                TerrainAdaptation::Bury => {
                    bury_contribution(f64::from(dx), f64::from(dy) / 2.0, f64::from(dz))
                }
                TerrainAdaptation::BeardThin | TerrainAdaptation::BeardBox => {
                    beard_contribution(dx, dy, dz, above_ground) * 0.8
                }
                TerrainAdaptation::Encapsulate => {
                    bury_contribution(
                        f64::from(dx) / 2.0,
                        f64::from(dy) / 2.0,
                        f64::from(dz) / 2.0,
                    ) * 0.8
                }
            };
        }

        for junction in &self.junctions {
            let jx = x - junction.source_x;
            let jy = y - junction.source_ground_y;
            let jz = z - junction.source_z;
            // The junction passes its own `jy` as BOTH the kernel's y index
            // and the height that sets the term's sign, where a piece passes
            // two different numbers. 0.4 rather than the pieces' 0.8.
            total += beard_contribution(jx, jy, jz, jy) * 0.4;
        }

        total
    }
}

/// `Beardifier.isInKernelRange`.
const fn in_kernel_range(index: i32) -> bool {
    index >= 0 && index < BEARD_KERNEL_SIZE
}

/// `Beardifier.getBeardContribution(int, int, int, int)`.
///
/// `pub` so `tests/golden_beard.rs` can gate it directly against vanilla's
/// own private method rather than only through `compute`. A composed gate
/// cannot say which of five pieces is wrong; that is the whole reason this
/// module is split the way it is.
///
/// `above_ground` is vanilla's fourth argument and is **not** the same as
/// `dy`: `dy` indexes the kernel, `above_ground` sets the sign and magnitude
/// of the multiplier. For `BEARD_BOX` they are genuinely different numbers,
/// which is the case a port that collapses them into one still passes on
/// `BEARD_THIN`.
pub fn beard_contribution(dx: i32, dy: i32, dz: i32, above_ground: i32) -> f64 {
    let kx = dx + BEARD_KERNEL_RADIUS;
    let ky = dy + BEARD_KERNEL_RADIUS;
    let kz = dz + BEARD_KERNEL_RADIUS;
    if !in_kernel_range(kx) || !in_kernel_range(ky) || !in_kernel_range(kz) {
        return 0.0;
    }

    let height = f64::from(above_ground) + 0.5;
    let len_sq = length_squared(f64::from(dx), height, f64::from(dz));

    // `-height * fastInvSqrt(lenSq / 2.0) / 2.0`. `fastInvSqrt`, not
    // `1.0 / sqrt` — see `math::fast_inv_sqrt`, which carries the reason.
    let scale = -height * fast_inv_sqrt(len_sq / 2.0) / 2.0;

    let index = kz * BEARD_KERNEL_SIZE * BEARD_KERNEL_SIZE + kx * BEARD_KERNEL_SIZE + ky;
    #[allow(clippy::cast_sign_loss)]
    let entry = kernel()[index as usize];
    scale * f64::from(entry)
}

/// `Beardifier.getBuryContribution(double, double, double)`.
///
/// `pub` for the same reason as [`beard_contribution`].
pub fn bury_contribution(x: f64, y: f64, z: f64) -> f64 {
    clamped_map(length(x, y, z), 0.0, 6.0, 1.0, 0.0)
}

/// `Beardifier.computeBeardContribution(int, int, int)` — the integer
/// overload, which offsets `y` by half a block and defers.
fn compute_beard_contribution_at(x: i32, y: i32, z: i32) -> f64 {
    compute_beard_contribution(x, f64::from(y) + 0.5, z)
}

/// `Beardifier.computeBeardContribution(int, double, int)` — a Gaussian in
/// the squared distance.
///
/// `Math.pow(Math.E, ..)`, exactly as the bytecode has it at offsets 10-20,
/// and **not** `exp(..)`. The two are the same function mathematically and
/// are not required to agree in the last bit; the golden compares raw float
/// bits precisely so that assumption is tested rather than made.
fn compute_beard_contribution(x: i32, y: f64, z: i32) -> f64 {
    let distance_squared = length_squared(f64::from(x), y, f64::from(z));
    std::f64::consts::E.powf(-distance_squared / 16.0)
}

/// `Beardifier.BEARD_KERNEL`, built once on first use.
///
/// `pub` so the gate can compare all 13,824 entries as raw bits.
///
/// Stored as `f32` because vanilla stores `float[]` and widens each entry to
/// `double` at the point of use. Holding it as `f64` would agree to fifteen
/// decimal places and be wrong in the last bit of every single lookup.
pub fn kernel() -> &'static [f32; BEARD_KERNEL_LEN] {
    static KERNEL: OnceLock<[f32; BEARD_KERNEL_LEN]> = OnceLock::new();
    KERNEL.get_or_init(|| {
        let mut kernel = [0.0f32; BEARD_KERNEL_LEN];
        // The loop nest is vanilla's `lambda$static$0`, index and all. The
        // outer variable feeds the *z* term of the value and the *first*
        // factor of the index, which is the transposition that makes this
        // worth transcribing rather than rederiving.
        for i in 0..BEARD_KERNEL_SIZE {
            for j in 0..BEARD_KERNEL_SIZE {
                for l in 0..BEARD_KERNEL_SIZE {
                    let index =
                        i * BEARD_KERNEL_SIZE * BEARD_KERNEL_SIZE + j * BEARD_KERNEL_SIZE + l;
                    let value = compute_beard_contribution_at(
                        j - BEARD_KERNEL_RADIUS,
                        l - BEARD_KERNEL_RADIUS,
                        i - BEARD_KERNEL_RADIUS,
                    );
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    {
                        kernel[index as usize] = value as f32;
                    }
                }
            }
        }
        kernel
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kernel_is_symmetric_in_x_and_z_but_not_in_y() {
        // The value at (dx, dy, dz) is `exp(-(dx^2 + (dy+0.5)^2 + dz^2)/16)`,
        // so x and z are interchangeable and y is offset by half a block.
        // This is a shape check on the index arithmetic, not on the values —
        // the values are gated against vanilla in `tests/golden_beard.rs`.
        let at = |dx: i32, dy: i32, dz: i32| {
            let index = (dz + 12) * 576 + (dx + 12) * 24 + (dy + 12);
            kernel()[index as usize]
        };
        assert!((at(3, 1, 7) - at(7, 1, 3)).abs() < f32::EPSILON);
        assert!((at(0, 0, 0) - at(0, -1, 0)).abs() < f32::EPSILON);
        assert!(
            at(0, 0, 0) > at(0, 1, 0),
            "y is offset by +0.5, so 0 beats 1"
        );
    }

    #[test]
    fn an_empty_beardifier_is_zero_everywhere() {
        let empty = Beardifier::empty();
        assert!(empty.is_empty());
        for y in -64..320 {
            assert_eq!(empty.compute(17, y, -3), 0.0);
        }
    }

    #[test]
    fn a_none_piece_contributes_nothing_even_inside_its_own_box() {
        // `TerrainAdjustment.NONE` takes the first arm of both switches, and
        // `forStructuresInChunk` filters it out before a piece is ever built.
        // Both facts have to hold: the filter is the caller's and this is the
        // backstop.
        let bounds = BoundingBox::new(0, 60, 0, 8, 66, 8);
        let beardifier = Beardifier::new(
            vec![Rigid {
                bounds,
                adjustment: TerrainAdaptation::None,
                ground_level_delta: 1,
            }],
            Vec::new(),
            Some(bounds.inflated_by(24)),
        );
        assert_eq!(beardifier.compute(4, 63, 4), 0.0);
    }

    /// A helper for the collection tests: one rigid pool piece.
    fn pool_piece(bounds: BoundingBox, junctions: Vec<Junction>, rigid: bool) -> StartPiece {
        StartPiece {
            bounds,
            shape: PieceShape::Pool {
                rigid,
                ground_level_delta: 1,
                junctions,
            },
        }
    }

    #[test]
    fn a_start_just_outside_a_chunk_still_beards_it_because_the_reference_box_is_inflated() {
        // **This test asserted the opposite until 2026-08-09, and the code,
        // the doc comment and the test all agreed while all three were
        // wrong.** The old version put a piece five blocks west of chunk
        // (0,0) and required `is_empty()`, reasoning that `createReferences`
        // tests the union box against the chunk's bare 16x16.
        //
        // It tests `StructureStart.getBoundingBox()`, and that is
        // `structure.adjustBoundingBox(union)`, which is `union.inflatedBy(12)`
        // for every adaptation but `NONE`. So the reference exists and vanilla
        // beards this chunk.
        //
        // Measured rather than reasoned: seed 20260722 chunk (78,53), whose
        // `trial_chambers` union ends at x=1247 against the chunk's
        // x=1248, holds 217 blocks that vanilla's own `doFill` writes when it
        // is given those pieces and does not write when it is not.
        let bounds = BoundingBox::new(-20, 60, -20, -5, 66, -5);
        let start = Start {
            adjustment: TerrainAdaptation::BeardThin,
            union_box: bounds,
            pieces: vec![pool_piece(bounds, Vec::new(), true)],
        };
        let near = Beardifier::for_structures_in_chunk(std::slice::from_ref(&start), 0, 0);
        assert!(
            !near.is_empty(),
            "the union ends at -5, inflated to +7, which reaches chunk (0,0)"
        );

        // The start's own chunk, so the assertion above cannot pass for the
        // trivial reason that everything is bearded.
        let own = Beardifier::for_structures_in_chunk(std::slice::from_ref(&start), -1, -1);
        assert!(!own.is_empty(), "the start's own chunk must be bearded");

        // And the filter must still reject: a piece whose box is further than
        // 12 from the chunk in x is out of the kernel's reach entirely, and a
        // port that simply deleted the reference test would keep it.
        let far = BoundingBox::new(-60, 60, -20, -40, 66, -5);
        let far_start = Start {
            adjustment: TerrainAdaptation::BeardThin,
            union_box: far,
            pieces: vec![pool_piece(far, Vec::new(), true)],
        };
        assert!(
            Beardifier::for_structures_in_chunk(&[far_start], 0, 0).is_empty(),
            "40 blocks west is outside the 12-block kernel in every axis"
        );
    }

    #[test]
    fn a_terrain_matching_piece_yields_no_rigid_but_keeps_its_junctions() {
        // The two halves of the pool arm are not nested: only a RIGID
        // projection contributes a `Rigid` and grows the affected box, while
        // the junction loop runs for both. A port that wraps the whole arm in
        // the projection test loses every junction of a terrain-matching
        // piece, and junctions are what smooth a village's paths.
        let bounds = BoundingBox::new(0, 60, 0, 15, 66, 15);
        let junction = Junction {
            source_x: 8,
            source_ground_y: 63,
            source_z: 8,
        };
        let start = Start {
            adjustment: TerrainAdaptation::BeardThin,
            union_box: bounds,
            pieces: vec![pool_piece(bounds, vec![junction], false)],
        };
        let beardifier = Beardifier::for_structures_in_chunk(&[start], 0, 0);
        assert!(
            beardifier.pieces.is_empty(),
            "no rigid from a matching piece"
        );
        assert_eq!(
            beardifier.junctions,
            vec![junction],
            "the junction survives"
        );
        assert!(!beardifier.is_empty(), "the junction grew the affected box");
    }

    #[test]
    fn a_plain_piece_contributes_a_ground_level_delta_of_zero() {
        // `new Rigid(piece.getBoundingBox(), adjustment, 0)` at offset 374 —
        // `iconst_0`, not the element's delta. Only pool pieces carry one.
        let bounds = BoundingBox::new(0, 60, 0, 15, 66, 15);
        let start = Start {
            adjustment: TerrainAdaptation::BeardThin,
            union_box: bounds,
            pieces: vec![StartPiece {
                bounds,
                shape: PieceShape::Plain,
            }],
        };
        let beardifier = Beardifier::for_structures_in_chunk(&[start], 0, 0);
        assert_eq!(beardifier.pieces.len(), 1);
        assert_eq!(beardifier.pieces[0].ground_level_delta, 0);
    }

    #[test]
    fn the_junction_window_is_strict_and_asymmetric() {
        // Offsets 264-308: `sx > minX - 12 && sz > minZ - 12 &&
        // sx < minX + 15 + 12 && sz < minZ + 15 + 12`. Strict on all four
        // sides, and the two ends are NOT the same distance from the chunk —
        // 12 below the minimum, 27 above it. Chunk (0,0) therefore accepts
        // -11..=26 and refuses -12 and 27.
        let bounds = BoundingBox::new(0, 60, 0, 15, 66, 15);
        let at = |x: i32| Junction {
            source_x: x,
            source_ground_y: 63,
            source_z: 8,
        };
        let build = |junction: Junction| {
            Beardifier::for_structures_in_chunk(
                &[Start {
                    adjustment: TerrainAdaptation::BeardThin,
                    union_box: bounds,
                    pieces: vec![pool_piece(bounds, vec![junction], false)],
                }],
                0,
                0,
            )
            .junctions
            .len()
        };
        assert_eq!(build(at(-12)), 0, "-12 is excluded: the test is strict");
        assert_eq!(build(at(-11)), 1, "-11 is the first accepted");
        assert_eq!(build(at(26)), 1, "26 is the last accepted");
        assert_eq!(build(at(27)), 0, "27 is excluded: minX + 15 + 12");
    }

    #[test]
    fn the_affected_box_is_the_union_inflated_by_twenty_four() {
        let bounds = BoundingBox::new(4, 60, 4, 11, 66, 11);
        let start = Start {
            adjustment: TerrainAdaptation::BeardThin,
            union_box: bounds,
            pieces: vec![pool_piece(bounds, Vec::new(), true)],
        };
        let beardifier = Beardifier::for_structures_in_chunk(&[start], 0, 0);
        assert_eq!(
            beardifier.affected,
            Some(BoundingBox::new(-20, 36, -20, 35, 90, 35)),
            "inflatedBy(24) at offset 413, not by the kernel radius of 12"
        );
    }

    #[test]
    fn a_none_start_is_filtered_before_its_pieces_are_read() {
        // `forStructuresInChunk`'s predicate, offset 2. The backstop in
        // `compute` covers the same case; both are vanilla's and both are
        // here, because the filter is what stops a mineshaft's hundreds of
        // corridor boxes from being walked for every chunk it touches.
        let bounds = BoundingBox::new(0, 60, 0, 15, 66, 15);
        let start = Start {
            adjustment: TerrainAdaptation::None,
            union_box: bounds,
            pieces: vec![pool_piece(bounds, Vec::new(), true)],
        };
        assert!(Beardifier::for_structures_in_chunk(&[start], 0, 0).is_empty());
    }

    #[test]
    fn outside_the_affected_box_short_circuits_before_the_pieces() {
        // A point the pieces would answer for, excluded by an affected box
        // that does not contain it. Vanilla returns before the loop; a port
        // that checked the box per piece instead would answer non-zero here.
        let bounds = BoundingBox::new(0, 60, 0, 8, 66, 8);
        let inside = Beardifier::new(
            vec![Rigid {
                bounds,
                adjustment: TerrainAdaptation::BeardThin,
                ground_level_delta: 1,
            }],
            Vec::new(),
            Some(bounds.inflated_by(24)),
        );
        let clipped = Beardifier::new(
            vec![Rigid {
                bounds,
                adjustment: TerrainAdaptation::BeardThin,
                ground_level_delta: 1,
            }],
            Vec::new(),
            Some(BoundingBox::new(100, 60, 100, 108, 66, 108)),
        );
        assert!(inside.compute(4, 62, 4) != 0.0);
        assert_eq!(clipped.compute(4, 62, 4), 0.0);
    }
}
