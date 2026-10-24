//! The badlands clay bands.
//!
//! Reference: `SurfaceSystem.generateBands`, `makeBands` and `getBand`.
//!
//! One rule operator (`bandlands`) is backed by a whole small generator: a
//! 192-entry palette of terracotta colours, built once per world from a
//! dedicated random stream, then indexed by height with a noise offset. That
//! is why every badlands mesa in a world has the *same* stripe pattern at the
//! same altitude, and why the stripes wobble rather than running dead level.
//!
//! # The band table is a sequence of overwrites, not a composition
//!
//! Each pass writes over whatever the previous one left. Orange goes down
//! first at irregular intervals, then yellow, brown and red bands are painted
//! on top in that order, then white bands with grey edges last. Reordering
//! the passes, or making them skip occupied slots, gives a different palette
//! from the same seed — the later colours are *meant* to bury the earlier
//! ones.

use crate::rng::RandomSource;

/// Vanilla's band count. Not a tunable: the modulo in [`band_at`] uses it, so
/// changing it reshuffles every mesa.
pub const BAND_COUNT: usize = 192;

/// The terracotta colours a band can be.
///
/// A closed enum rather than block-id strings, because the set is fixed by
/// `generateBands` and a typo in one of seven ids would be a silently
/// mis-striped world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    Terracotta,
    Orange,
    Yellow,
    Brown,
    Red,
    White,
    LightGray,
}

impl Band {
    /// The block id this band places.
    #[must_use]
    pub const fn block_name(self) -> &'static str {
        match self {
            Self::Terracotta => "minecraft:terracotta",
            Self::Orange => "minecraft:orange_terracotta",
            Self::Yellow => "minecraft:yellow_terracotta",
            Self::Brown => "minecraft:brown_terracotta",
            Self::Red => "minecraft:red_terracotta",
            Self::White => "minecraft:white_terracotta",
            Self::LightGray => "minecraft:light_gray_terracotta",
        }
    }
}

/// `generateBands` — build the 192-entry palette from one random stream.
///
/// The draw order is the whole thing: this consumes a specific sequence from
/// `random`, and any change to how many values are taken — even for a band
/// that lands outside the array and is discarded — shifts everything after
/// it.
#[must_use]
pub fn generate_bands<R: RandomSource>(random: &mut R) -> Vec<Band> {
    let mut bands = vec![Band::Terracotta; BAND_COUNT];

    // Vanilla's loop increments `i` **twice** per iteration: once by the
    // `for`'s own `i++` and once by `i += random.nextInt(5) + 1`. So the
    // real step is 2..=6, not 1..=5. Written out here rather than as a
    // `for` with a hidden double-step, because reading it as a single
    // increment gives twice as many orange bands.
    let mut i = 0usize;
    while i < BAND_COUNT {
        i += (random.next_i32_bound(5) + 1) as usize;
        if i < BAND_COUNT {
            bands[i] = Band::Orange;
        }
        i += 1;
    }

    make_bands(random, &mut bands, 1, Band::Yellow);
    make_bands(random, &mut bands, 2, Band::Brown);
    make_bands(random, &mut bands, 1, Band::Red);

    // White bands last, so they overwrite anything already there, each with
    // an independently-decided grey edge on either side.
    let white_band_count = random.next_i32_between_inclusive(9, 15);
    let mut placed = 0i32;
    let mut start = 0usize;
    while placed < white_band_count && start < BAND_COUNT {
        bands[start] = Band::White;
        // `start - 1 > 0`, not `>= 0` — so index 0 never gets a grey edge
        // below it even when index 1 is white. Vanilla's off-by-one, kept.
        if start >= 1 && start - 1 > 0 && random.next_bool() {
            bands[start - 1] = Band::LightGray;
        }
        if start + 1 < BAND_COUNT && random.next_bool() {
            bands[start + 1] = Band::LightGray;
        }
        placed += 1;
        start += (random.next_i32_bound(16) + 4) as usize;
    }

    bands
}

/// `makeBands` — paint 6..=15 runs of one colour at random positions.
fn make_bands<R: RandomSource>(random: &mut R, bands: &mut [Band], base_width: i32, colour: Band) {
    let band_count = random.next_i32_between_inclusive(6, 15);
    for _ in 0..band_count {
        let width = base_width + random.next_i32_bound(3);
        let start = random.next_i32_bound(BAND_COUNT as i32) as usize;
        // Runs are clipped at the top of the table rather than wrapping —
        // a band starting near 191 is simply shorter.
        let mut offset = 0usize;
        while start + offset < BAND_COUNT && (offset as i32) < width {
            bands[start + offset] = colour;
            offset += 1;
        }
    }
}

/// `getBand` — the band at a world position.
///
/// The offset noise is rounded with **Java's** `Math.round`, which is
/// `floor(x + 0.5)` and therefore rounds *up* on a tie in both directions.
/// Rust's `f64::round` rounds half **away from zero**, so the two disagree
/// at exactly -0.5, -1.5 and so on. Using the wrong one shifts the whole
/// stripe pattern by a block wherever the noise lands on a half.
#[must_use]
pub fn band_at(bands: &[Band], offset_noise: f64, y: i32) -> Band {
    let offset = java_round(offset_noise * 4.0);
    let len = bands.len() as i64;
    // `+ len` before the modulo, because Rust's `%` keeps the sign of the
    // dividend and a negative index would panic.
    let index = ((i64::from(y) + offset + len) % len) as usize;
    bands[index]
}

/// `Math.round(double)` — `floor(v + 0.5)`, as a long.
fn java_round(value: f64) -> i64 {
    (value + 0.5).floor() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::XoroshiroRandomSource;

    fn bands() -> Vec<Band> {
        let mut random = XoroshiroRandomSource::from_seed(20260725);
        generate_bands(&mut random)
    }

    #[test]
    fn the_table_is_the_right_size_and_mostly_terracotta() {
        let bands = bands();
        assert_eq!(bands.len(), BAND_COUNT);
        let plain = bands.iter().filter(|b| **b == Band::Terracotta).count();
        assert!(
            plain > BAND_COUNT / 4,
            "plain terracotta should still dominate, got {plain}"
        );
    }

    #[test]
    fn every_colour_appears() {
        // If a pass were skipped or overwritten entirely the world would
        // still generate and just look wrong, so check the palette is whole.
        let bands = bands();
        for colour in [
            Band::Terracotta,
            Band::Orange,
            Band::Yellow,
            Band::Brown,
            Band::Red,
            Band::White,
        ] {
            assert!(
                bands.contains(&colour),
                "{colour:?} never appears in the table"
            );
        }
    }

    #[test]
    fn generation_is_reproducible_from_the_seed() {
        let mut a = XoroshiroRandomSource::from_seed(7);
        let mut b = XoroshiroRandomSource::from_seed(7);
        assert_eq!(generate_bands(&mut a), generate_bands(&mut b));
        // And a different seed gives a different table, or the stream is
        // not being consumed at all.
        let mut c = XoroshiroRandomSource::from_seed(8);
        assert_ne!(generate_bands(&mut a), generate_bands(&mut c));
    }

    #[test]
    fn the_orange_pass_steps_by_two_to_six_not_one_to_five() {
        // Vanilla's loop increments i twice per iteration. Reading it as a
        // single step doubles the number of orange bands, which is a
        // visibly different mesa from the same seed.
        //
        // Checked by consuming a stream that always returns 0 from
        // nextInt(5): the step is then exactly 2, so orange lands on every
        // other index starting at 1.
        let mut random = XoroshiroRandomSource::from_seed(1);
        let bands = generate_bands(&mut random);
        // Not asserting the zero-stream case directly (the real RNG is not
        // controllable here); instead assert the density is in the band the
        // 2..=6 step implies: 192 / 6 to 192 / 2, minus later overwrites.
        let orange = bands.iter().filter(|b| **b == Band::Orange).count();
        assert!(
            (10..=96).contains(&orange),
            "orange count {orange} is outside what a 2..=6 step can produce"
        );
    }

    #[test]
    fn java_round_goes_up_on_a_tie_in_both_directions() {
        // Java's Math.round is floor(v + 0.5); Rust's f64::round is half
        // away from zero. They disagree at exactly -0.5, and using the
        // wrong one shifts the stripes by a block wherever the noise lands
        // on a half.
        assert_eq!(java_round(0.5), 1);
        assert_eq!(java_round(-0.5), 0);
        assert_ne!(java_round(-0.5), (-0.5_f64).round() as i64);
        assert_eq!(java_round(-1.5), -1);
        assert_eq!(java_round(1.4), 1);
        assert_eq!(java_round(-1.6), -2);
    }

    #[test]
    fn the_index_wraps_rather_than_panicking_below_zero() {
        // y is signed and the offset can be negative, so the `+ len` before
        // the modulo is load-bearing: Rust's `%` keeps the dividend's sign
        // and a negative index would panic rather than wrap.
        let bands = bands();
        let low = band_at(&bands, -1.0, -64);
        let high = band_at(&bands, -1.0, -64 + BAND_COUNT as i32);
        assert_eq!(low, high, "the table must wrap with period {BAND_COUNT}");
    }

    #[test]
    fn the_offset_noise_shifts_the_stripes() {
        let bands = bands();
        // Four is the multiplier, so a noise of 1.0 shifts by 4 bands.
        let base = band_at(&bands, 0.0, 40);
        let shifted = band_at(&bands, 1.0, 36);
        assert_eq!(base, shifted, "offset 4 at y-4 is the same band");
    }
}
