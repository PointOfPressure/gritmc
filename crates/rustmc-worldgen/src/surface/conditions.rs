//! The conditions with real algorithms behind them.
//!
//! Reference: `SurfaceRules`' `VerticalGradientConditionSource`,
//! `SteepMaterialCondition`, `HoleCondition` and
//! `AbovePreliminarySurfaceCondition`.
//!
//! These are separated from [`super::eval`] because they need a world — a
//! positional RNG, the heightmap, the preliminary surface level — while the
//! arithmetic conditions do not. Keeping them here means each can be tested
//! against its own inputs rather than against a generated chunk.

use crate::math;
use crate::rng::{PositionalRandomFactory, RandomSource};

/// `vertical_gradient` — a seeded fade between two heights.
///
/// Below `true_at_and_below` it is always true, at or above
/// `false_at_and_above` always false, and between them a **position-seeded
/// coin flip** whose bias falls linearly from 1 to 0. That randomness is
/// what makes the bedrock floor a ragged few layers rather than a flat slab,
/// and it is per block, so the same y gives different answers at different
/// x/z.
///
/// The comparison is `nextFloat() < probability`, in `f32` against an `f64`
/// probability — so the float is widened, not the probability narrowed.
#[must_use]
pub fn vertical_gradient<F: PositionalRandomFactory>(
    random: &F,
    x: i32,
    y: i32,
    z: i32,
    true_at_and_below: i32,
    false_at_and_above: i32,
) -> bool {
    if y <= true_at_and_below {
        return true;
    }
    if y >= false_at_and_above {
        return false;
    }
    // `Mth.map` with no clamp — the guards above already bound y, and using
    // clampedMap here would be indistinguishable *and* would hide it if the
    // guards were ever removed.
    let probability = math::map(
        f64::from(y),
        f64::from(true_at_and_below),
        f64::from(false_at_and_above),
        1.0,
        0.0,
    );
    let mut source = random.at(x, y, z);
    f64::from(source.next_f32()) < probability
}

/// `hole` — the column has no surface layer at all.
///
/// Literally `surfaceDepth <= 0`. It is not about caves: a "hole" is a
/// column the surface-depth noise made zero or negative, which is how
/// vanilla punches gravel and stone patches through an otherwise continuous
/// grass layer.
#[must_use]
pub const fn hole(surface_depth: i32) -> bool {
    surface_depth <= 0
}

/// `steep` — a sharp local gradient, which keeps grass off cliff faces.
///
/// Two things about this are surprising enough to be worth stating, because
/// both look like bugs and are neither:
///
/// - It compares **north against south** and **west against east**, each in
///   one direction only. A slope that falls the other way is not "steep".
///   That asymmetry is visible in worlds: one face of a ridge keeps its
///   grass and the opposite face does not.
/// - The neighbour lookups are **clamped to the chunk**, not wrapped to the
///   neighbouring chunk. At a chunk edge the sample is the edge column
///   itself, so `steep` is subtly different along chunk borders. Reading
///   across the border would need a neighbour that may not be generated
///   yet, so vanilla does not.
///
/// `heights` is the `WORLD_SURFACE_WG` heightmap indexed `[z * 16 + x]` in
/// chunk-local coordinates.
#[must_use]
pub fn steep(heights: &[i32; 256], block_x: i32, block_z: i32) -> bool {
    let local_x = block_x & 15;
    let local_z = block_z & 15;
    let at = |x: i32, z: i32| heights[(z * 16 + x) as usize];

    let north = (local_z - 1).max(0);
    let south = (local_z + 1).min(15);
    if at(local_x, south) >= at(local_x, north) + 4 {
        return true;
    }

    let west = (local_x - 1).max(0);
    let east = (local_x + 1).min(15);
    at(west, local_z) >= at(east, local_z) + 4
}

/// `Context.getMinSurfaceLevel` — the height below which no surface is built.
///
/// Bilinear over a **16-block** cell, not a quart one: the preliminary
/// surface is sampled at the four corners of the cell containing this
/// position and blended across it. So `above_preliminary_surface` changes
/// smoothly rather than in 16-block steps, which is what stops surface
/// material appearing in flat terraces on a slope.
///
/// The `- 8` is `HOW_FAR_BELOW_PRELIMINARY_SURFACE_LEVEL_TO_BUILD_SURFACE`:
/// vanilla builds surface a little *below* the estimate, because the
/// estimate is only accurate to a cell.
///
/// `corners` are `[(x0,z0), (x1,z0), (x0,z1), (x1,z1)]`, matching
/// `Mth.lerp2`'s argument order — transposing the middle two mirrors the
/// blend diagonally.
#[must_use]
pub fn min_surface_level(corners: [i32; 4], block_x: i32, block_z: i32, surface_depth: i32) -> i32 {
    // `(blockX & 15) / 16.0F` in vanilla. The values 0..15 over 16 are all
    // exactly representable, so the f32 division and an f64 one agree
    // here — but the division is by the *cell* width, not the quart width,
    // and using 4.0 would blend four times too fast.
    let fx = f64::from(block_x & 15) / 16.0;
    let fz = f64::from(block_z & 15) / 16.0;
    let blended = math::lerp2(
        fx,
        fz,
        f64::from(corners[0]),
        f64::from(corners[1]),
        f64::from(corners[2]),
        f64::from(corners[3]),
    );
    math::floor(blended) + surface_depth - 8
}

/// The four 16-block cell corners `min_surface_level` blends between, as
/// block coordinates.
#[must_use]
pub const fn min_surface_cell_corners(block_x: i32, block_z: i32) -> [(i32, i32); 4] {
    let cell_x = block_x >> 4;
    let cell_z = block_z >> 4;
    [
        (cell_x << 4, cell_z << 4),
        ((cell_x + 1) << 4, cell_z << 4),
        (cell_x << 4, (cell_z + 1) << 4),
        ((cell_x + 1) << 4, (cell_z + 1) << 4),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::{XoroshiroPositionalFactory, XoroshiroRandomSource};

    fn factory() -> XoroshiroPositionalFactory {
        XoroshiroRandomSource::from_seed(20260725).fork_positional()
    }

    #[test]
    fn the_gradient_is_certain_outside_its_band() {
        let f = factory();
        // At or below the true anchor, and at or above the false one, no
        // randomness is consulted at all.
        assert!(vertical_gradient(&f, 0, -64, 0, -60, -55));
        assert!(vertical_gradient(&f, 0, -60, 0, -60, -55));
        assert!(!vertical_gradient(&f, 0, -55, 0, -60, -55));
        assert!(!vertical_gradient(&f, 0, 100, 0, -60, -55));
    }

    #[test]
    fn the_gradient_fades_across_the_band() {
        // Between the anchors the probability falls from 1 to 0, so a wide
        // sample near the bottom should be mostly true and near the top
        // mostly false. This is the property that makes bedrock ragged.
        let f = factory();
        let (low, high) = (-64, -59);
        let count = |y: i32| {
            (0..400)
                .filter(|i| vertical_gradient(&f, *i, y, i * 7, low, high))
                .count()
        };
        let near_bottom = count(-63);
        let near_top = count(-60);
        assert!(
            near_bottom > near_top,
            "expected the fade to fall with height: {near_bottom} then {near_top}"
        );
        assert!(near_bottom > 250, "near the floor it should mostly hold");
        assert!(near_top < 150, "near the ceiling it should mostly not");
    }

    #[test]
    fn the_gradient_is_positional_not_per_column() {
        // Same y, different x/z must be able to differ — otherwise the
        // bedrock floor comes out as flat slabs instead of a ragged edge.
        let f = factory();
        let y = -62;
        let answers: Vec<bool> = (0..64)
            .map(|i| vertical_gradient(&f, i, y, i * 3, -64, -59))
            .collect();
        assert!(
            answers.iter().any(|a| *a) && answers.iter().any(|a| !*a),
            "every position agreed, so the seeding is not positional"
        );
    }

    #[test]
    fn the_gradient_is_reproducible() {
        let f = factory();
        for y in -64..-55 {
            assert_eq!(
                vertical_gradient(&f, 11, y, -7, -64, -59),
                vertical_gradient(&f, 11, y, -7, -64, -59)
            );
        }
    }

    #[test]
    fn a_hole_is_a_column_with_no_surface_depth() {
        // Not a cave — a column the depth noise drove to zero, which is how
        // gravel and stone patches appear in a grass field.
        assert!(hole(0));
        assert!(hole(-1));
        assert!(!hole(1));
    }

    #[test]
    fn steep_compares_one_direction_per_axis() {
        // south >= north + 4, then west >= east + 4. A slope the other way
        // is deliberately not steep, which is why one face of a ridge keeps
        // its grass and the opposite face does not.
        let mut heights = [64i32; 256];
        let at = |x: i32, z: i32| (z * 16 + x) as usize;

        // South much higher than north at (8, 8): steep.
        heights[at(8, 9)] = 70;
        assert!(steep(&heights, 8, 8));

        // The mirror image — north higher than south — is not.
        let mut mirrored = [64i32; 256];
        mirrored[at(8, 7)] = 70;
        assert!(!steep(&mirrored, 8, 8));
    }

    #[test]
    fn steep_clamps_to_the_chunk_rather_than_wrapping() {
        // At x = 0 the "west" sample is column 0 itself, not column 15 and
        // not the neighbouring chunk — which may not be generated yet. So a
        // west-facing cliff at the chunk edge compares a column against
        // itself and reads as flat.
        let mut heights = [64i32; 256];
        let at = |x: i32, z: i32| (z * 16 + x) as usize;
        heights[at(1, 8)] = 40;
        // West (clamped to 0) is 64, east is 40, so 64 >= 44: steep.
        assert!(steep(&heights, 0, 8));

        // And the clamp means column 15's value is never consulted from
        // column 0, however extreme it is.
        let mut wrapped = [64i32; 256];
        wrapped[at(15, 8)] = 200;
        assert!(!steep(&wrapped, 0, 8), "the far edge must not be read");
    }

    #[test]
    fn min_surface_level_blends_across_the_cell() {
        // Corners 64/80 west-to-east: the middle of the cell must land
        // between them, not snap to one. A 16-block step here would put
        // surface material in flat terraces on any slope.
        let corners = [64, 80, 64, 80];
        let left = min_surface_level(corners, 0, 0, 0);
        let middle = min_surface_level(corners, 8, 0, 0);
        let right = min_surface_level(corners, 15, 0, 0);
        assert_eq!(left, 64 - 8);
        assert_eq!(middle, 72 - 8);
        assert!(right > middle && right < 80 - 8 + 1);
    }

    #[test]
    fn min_surface_level_folds_in_the_surface_depth_and_the_eight() {
        let flat = [64, 64, 64, 64];
        assert_eq!(min_surface_level(flat, 0, 0, 0), 56);
        assert_eq!(min_surface_level(flat, 0, 0, 3), 59);
    }

    #[test]
    fn the_corner_order_is_x_then_z() {
        // `[(x0,z0), (x1,z0), (x0,z1), (x1,z1)]`. Transposing the middle two
        // mirrors the blend diagonally — still smooth, still wrong.
        let corners = [0, 100, 0, 100]; // varies with x only
        assert_eq!(min_surface_level(corners, 0, 15, 0), 0 - 8);
        let transposed = [0, 0, 100, 100]; // varies with z only
        assert_eq!(min_surface_level(transposed, 15, 0, 0), 0 - 8);
    }

    #[test]
    fn the_cell_corners_are_sixteen_apart_and_floor_on_negatives() {
        assert_eq!(
            min_surface_cell_corners(5, 5),
            [(0, 0), (16, 0), (0, 16), (16, 16)]
        );
        // Block -1 belongs to cell -1, so its corners are -16 and 0.
        assert_eq!(
            min_surface_cell_corners(-1, -1),
            [(-16, -16), (0, -16), (-16, 0), (0, 0)]
        );
    }

    #[test]
    fn steep_reads_the_heightmap_in_z_major_order() {
        // `[z * 16 + x]`. Transposing it would make every north-south test
        // an east-west one, which is a plausible-looking world with grass
        // on the wrong cliff faces.
        let mut heights = [64i32; 256];
        heights[9 * 16 + 8] = 70; // (x=8, z=9), i.e. south of (8, 8)
        assert!(steep(&heights, 8, 8));
        let mut transposed = [64i32; 256];
        transposed[8 * 16 + 9] = 70; // (x=9, z=8), i.e. east of (8, 8)
        assert!(!steep(&transposed, 8, 8));
    }
}
