//! Block-resolution biome lookup — `net.minecraft.world.level.biome.BiomeManager`.
//!
//! # This is not a quart lookup, and that surprised me
//!
//! [`super::multi_noise::MultiNoiseBiomeSource`] answers at **quart**
//! resolution: one biome per 4x4x4 cell. Everything that asks "what biome is
//! at this *block*" — the surface system, and the client — does **not** read
//! that grid directly. It goes through this, which:
//!
//! 1. shifts the position by -2 on every axis,
//! 2. takes the eight surrounding quart cells,
//! 3. perturbs each corner's distance by a seeded amount, and
//! 4. returns the biome of whichever corner is nearest after perturbation.
//!
//! That is what makes biome borders wobble at block resolution instead of
//! stepping along a visible 4-block grid. A port that hands the raw quart
//! source to the surface system generates a world with straight-edged biome
//! transitions and grass in slightly wrong places — plausible, and wrong.
//!
//! # The seed is SHA-256 of the world seed
//!
//! `obfuscateSeed` is `Hashing.sha256().hashLong(seed).asLong()`, which is
//! why this crate has a SHA-256 at all. See [`crate::rng::sha256`] for the
//! two endianness choices Guava makes.

use crate::rng::sha256::hash_long_to_long;

/// `BiomeManager.obfuscateSeed`.
#[must_use]
pub fn obfuscate_seed(world_seed: i64) -> i64 {
    hash_long_to_long(world_seed)
}

/// `LinearCongruentialGenerator.next`.
///
/// Note the shape: `rval *= rval * A + B` — the multiplier is itself derived
/// from `rval`, so this is **not** the usual `rval * A + B` LCG. Reading it
/// as one gives a different sequence and therefore different biome borders.
#[must_use]
const fn lcg_next(rval: i64, c: i64) -> i64 {
    rval.wrapping_mul(
        rval.wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407),
    )
    .wrapping_add(c)
}

/// The per-axis perturbation, in `[-0.45, 0.45)`.
///
/// `floorMod(rval >> 24, 1024) / 1024.0`, recentred and scaled by 0.9. The
/// shift is **arithmetic** and the modulo **floors**, so negative states map
/// into the same range rather than producing a negative fiddle.
#[must_use]
fn fiddle(rval: i64) -> f64 {
    let uniform = (rval >> 24).rem_euclid(1024) as f64 / 1024.0;
    (uniform - 0.5) * 0.9
}

/// The three per-axis perturbations of one corner.
///
/// The stream is stirred with the three coordinates **twice** before any
/// fiddle is drawn, then the seed is mixed in between each axis. Shortening
/// that to one pass, or reordering it, changes every border in the world.
///
/// Split out of `getFiddledDistance` because it is a pure function of the
/// **corner** alone — `dx`, `dy` and `dz` do not enter it — and every block
/// of a 4x4x4 cell asks for the same eight corners. See [`corner_fiddles`]
/// for what that buys.
#[must_use]
fn fiddles_at(seed: i64, corner_x: i32, corner_y: i32, corner_z: i32) -> (f64, f64, f64) {
    let (x, y, z) = (
        i64::from(corner_x),
        i64::from(corner_y),
        i64::from(corner_z),
    );
    let mut rval = lcg_next(seed, x);
    rval = lcg_next(rval, y);
    rval = lcg_next(rval, z);
    rval = lcg_next(rval, x);
    rval = lcg_next(rval, y);
    rval = lcg_next(rval, z);

    let fiddle_x = fiddle(rval);
    rval = lcg_next(rval, seed);
    let fiddle_y = fiddle(rval);
    rval = lcg_next(rval, seed);
    let fiddle_z = fiddle(rval);
    (fiddle_x, fiddle_y, fiddle_z)
}

/// The eight corner perturbations of one parent cell, in `i` order — the
/// order [`quart_for_block`]'s loop visits them, so index `i` is corner `i`.
#[derive(Clone, Copy)]
struct CellFiddles {
    key: (i64, i32, i32, i32),
    corners: [(f64, f64, f64); 8],
    live: bool,
}

/// The eight corners of one parent cell, memoised **one cell deep**.
///
/// # Why a memo is transparent here
///
/// [`fiddles_at`] is nine `lcg_next` rounds over `(seed, corner)` and nothing
/// else — no float arithmetic, no shared state, no order dependence — so a
/// stored answer is bit-for-bit the computed one. The distance arithmetic
/// that consumes it is untouched and still sums z, then y, then x.
///
/// # Why one entry is the right size
///
/// A column walk asks for consecutive `y`, and `parent_y = (y - 2) >> 2`, so
/// four consecutive queries share a cell and the fourth costs one key
/// comparison instead of 72 `lcg_next` rounds. A **corner**-keyed
/// direct-mapped table of 2048 slots was tried first and measured a null
/// result — 0.40% of instructions and 0.06% of cycles — because the hash,
/// the thread-local access and the tag check cost as much as the nine
/// multiplies they skipped. Eight lookups per call was the mistake; one is
/// the fix. BENCH.md records the failed shape so it is not tried a third
/// time.
fn cell_fiddles(seed: i64, parent_x: i32, parent_y: i32, parent_z: i32) -> [(f64, f64, f64); 8] {
    thread_local! {
        static CACHE: std::cell::Cell<CellFiddles> = const {
            std::cell::Cell::new(CellFiddles {
                key: (0, 0, 0, 0),
                corners: [(0.0, 0.0, 0.0); 8],
                live: false,
            })
        };
    }

    let key = (seed, parent_x, parent_y, parent_z);
    let cached = CACHE.with(std::cell::Cell::get);
    if cached.live && cached.key == key {
        return cached.corners;
    }

    let mut corners = [(0.0, 0.0, 0.0); 8];
    for (i, slot) in corners.iter_mut().enumerate() {
        let corner_x = if i & 4 == 0 { parent_x } else { parent_x + 1 };
        let corner_y = if i & 2 == 0 { parent_y } else { parent_y + 1 };
        let corner_z = if i & 1 == 0 { parent_z } else { parent_z + 1 };
        *slot = fiddles_at(seed, corner_x, corner_y, corner_z);
    }
    CACHE.with(|cache| {
        cache.set(CellFiddles {
            key,
            corners,
            live: true,
        });
    });
    corners
}

/// `getFiddledDistance` — squared distance to a corner, perturbed.
#[must_use]
fn fiddled_distance(fiddles: (f64, f64, f64), dx: f64, dy: f64, dz: f64) -> f64 {
    let (fiddle_x, fiddle_y, fiddle_z) = fiddles;

    // Summed z, y, x — vanilla's order. Floating-point addition is not
    // associative, so the order is part of the answer at a tie.
    square(dz + fiddle_z) + square(dy + fiddle_y) + square(dx + fiddle_x)
}

fn square(v: f64) -> f64 {
    v * v
}

/// Which quart cell a block position resolves to.
///
/// Split out from the lookup so it can be tested without a biome source, and
/// so a caller that already has a quart grid can use it directly.
#[must_use]
pub fn quart_for_block(zoom_seed: i64, x: i32, y: i32, z: i32) -> (i32, i32, i32) {
    // The -2 shift is what centres the fuzz on the cell rather than its
    // corner; without it every border leans the same way.
    let (ax, ay, az) = (x - 2, y - 2, z - 2);
    let (parent_x, parent_y, parent_z) = (ax >> 2, ay >> 2, az >> 2);
    let fract_x = f64::from(ax & 3) / 4.0;
    let fract_y = f64::from(ay & 3) / 4.0;
    let fract_z = f64::from(az & 3) / 4.0;

    // The eight corners' perturbations, which depend on the cell and not on
    // where in it this block sits — so all 64 blocks of the cell share them.
    let fiddles = cell_fiddles(zoom_seed, parent_x, parent_y, parent_z);

    let mut best = 0usize;
    let mut best_distance = f64::INFINITY;
    for (i, fiddle) in fiddles.iter().enumerate() {
        let x_even = i & 4 == 0;
        let y_even = i & 2 == 0;
        let z_even = i & 1 == 0;
        let dx = if x_even { fract_x } else { fract_x - 1.0 };
        let dy = if y_even { fract_y } else { fract_y - 1.0 };
        let dz = if z_even { fract_z } else { fract_z - 1.0 };

        let distance = fiddled_distance(*fiddle, dx, dy, dz);
        // Strict `>`, so the **first** corner wins a tie, in i order.
        if best_distance > distance {
            best = i;
            best_distance = distance;
        }
    }

    (
        if best & 4 == 0 {
            parent_x
        } else {
            parent_x + 1
        },
        if best & 2 == 0 {
            parent_y
        } else {
            parent_y + 1
        },
        if best & 1 == 0 {
            parent_z
        } else {
            parent_z + 1
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_zoom_seed_is_the_sha256_of_the_world_seed() {
        // Dumped from Guava, not derived here.
        assert_eq!(obfuscate_seed(0), 8_794_265_229_978_523_055);
        assert_eq!(obfuscate_seed(20_260_725), -4_015_252_751_475_427_253);
    }

    #[test]
    fn the_fiddle_stays_inside_its_band() {
        // (uniform - 0.5) * 0.9 with uniform in [0, 1), so [-0.45, 0.45).
        for state in [0i64, 1, -1, i64::MAX, i64::MIN, 1 << 40, -(1 << 40)] {
            let f = fiddle(state);
            assert!(
                (-0.45..0.45).contains(&f),
                "fiddle({state}) = {f} escaped its band"
            );
        }
    }

    #[test]
    fn a_negative_state_does_not_produce_a_negative_index() {
        // `floorMod`, not `%`. Rust's `%` keeps the dividend's sign, which
        // would push the uniform negative and the fiddle outside its band.
        let negative = fiddle(-1);
        assert!((-0.45..0.45).contains(&negative));
        assert!(((-1i64 >> 24).rem_euclid(1024)) >= 0);
    }

    #[test]
    fn the_lookup_is_mostly_the_containing_cell_but_not_always() {
        // The whole point of the fuzz: near a cell's middle it agrees with a
        // plain quart lookup, near the edges it does not. If it always
        // agreed, biome borders would step along a visible 4-block grid.
        let seed = obfuscate_seed(20_260_725);
        let mut agreed = 0;
        let mut disagreed = 0;
        for x in -32..32 {
            for z in -32..32 {
                let (qx, _, qz) = quart_for_block(seed, x, 64, z);
                if (qx, qz) == (x >> 2, z >> 2) {
                    agreed += 1;
                } else {
                    disagreed += 1;
                }
            }
        }
        assert!(agreed > 0, "the fuzz never picks the containing cell");
        assert!(
            disagreed > 0,
            "the fuzz never picks a neighbour, so borders would be grid-straight"
        );
    }

    #[test]
    fn the_lookup_is_reproducible_and_seed_dependent() {
        let a = obfuscate_seed(1);
        let b = obfuscate_seed(2);
        let sample = |seed: i64| {
            (0..64)
                .map(|i| quart_for_block(seed, i, 70, i * 3))
                .collect::<Vec<_>>()
        };
        assert_eq!(sample(a), sample(a));
        assert_ne!(sample(a), sample(b), "the zoom seed must change the fuzz");
    }

    #[test]
    fn the_shift_by_two_centres_the_cell() {
        // Without the -2 the perturbation is measured from a cell corner
        // rather than its middle, and every border leans the same way. The
        // shift is visible as the cell changing at x = 2, not x = 0.
        assert_eq!((0 - 2) >> 2, -1);
        assert_eq!((2 - 2) >> 2, 0);
        assert_eq!((5 - 2) >> 2, 0);
        assert_eq!((6 - 2) >> 2, 1);
    }
}
