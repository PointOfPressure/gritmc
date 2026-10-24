//! Improved Perlin noise — one octave.
//!
//! Reference: `net.minecraft.world.level.levelgen.synth.ImprovedNoise`, with
//! the gradient table and dot product from `...synth.SimplexNoise`.

use crate::math::{floor, lerp2, lerp3, smoothstep, smoothstep_derivative};
use crate::rng::RandomSource;

/// `SimplexNoise.GRADIENT`.
///
/// The last four rows repeat earlier ones — that is Ken Perlin's original
/// 12-direction set padded to 16 so the index can be a cheap `& 15` instead
/// of a modulo. It is not a typo, and "tidying" it changes every noise value
/// in the game.
const GRADIENT: [[i32; 3]; 16] = [
    [1, 1, 0],
    [-1, 1, 0],
    [1, -1, 0],
    [-1, -1, 0],
    [1, 0, 1],
    [-1, 0, 1],
    [1, 0, -1],
    [-1, 0, -1],
    [0, 1, 1],
    [0, -1, 1],
    [0, 1, -1],
    [0, -1, -1],
    [1, 1, 0],
    [0, -1, 1],
    [-1, 1, 0],
    [0, -1, -1],
];

/// `ImprovedNoise.SHIFT_UP_EPSILON`. A **float** literal used in double
/// context — see [`crate::rng::xoroshiro`] for why that distinction is
/// spelled out rather than typed as an `f64` constant.
const SHIFT_UP_EPSILON: f64 = 1.0e-7_f32 as f64;

/// `SimplexNoise#dot` — gradient components are `int`, widened per-term.
fn grad_dot(hash: i32, x: f64, y: f64, z: f64) -> f64 {
    let g = GRADIENT[(hash & 15) as usize];
    f64::from(g[0]) * x + f64::from(g[1]) * y + f64::from(g[2]) * z
}

/// One octave of improved Perlin noise.
#[derive(Clone, Debug)]
pub struct ImprovedNoise {
    /// Permutation table. Vanilla stores `byte[]` and reads it back as
    /// `p[x & 0xFF] & 0xFF`; `u8` is the same values without the round trip
    /// through sign extension.
    permutation: [u8; 256],
    pub xo: f64,
    pub yo: f64,
    pub zo: f64,
}

impl ImprovedNoise {
    /// Consumes exactly 3 doubles + 256 bounded ints from `random` — the
    /// count is part of the contract, because `PerlinNoise` skips unused
    /// octaves by burning the same number of draws.
    pub fn new<R: RandomSource>(random: &mut R) -> Self {
        let xo = random.next_f64() * 256.0;
        let yo = random.next_f64() * 256.0;
        let zo = random.next_f64() * 256.0;

        let mut permutation = [0u8; 256];
        for (i, slot) in permutation.iter_mut().enumerate() {
            *slot = i as u8;
        }

        // Fisher-Yates, but iterating *upward* with a forward-only offset.
        // Note the bound shrinks as `256 - i`, so the last iteration always
        // draws `next_i32_bound(1)` — a draw that cannot change anything but
        // must still happen.
        for i in 0..256 {
            let offset = random.next_i32_bound(256 - i as i32) as usize;
            permutation.swap(i, i + offset);
        }

        Self {
            permutation,
            xo,
            yo,
            zo,
        }
    }

    /// `p(int)` — masked lookup, widened back to a positive index.
    fn p(&self, x: i32) -> i32 {
        i32::from(self.permutation[(x & 0xFF) as usize])
    }

    /// `noise(x, y, z)`.
    #[must_use]
    pub fn noise(&self, x: f64, y: f64, z: f64) -> f64 {
        self.noise_with_fudge(x, y, z, 0.0, 0.0)
    }

    /// `noise(x, y, z, yScale, yFudge)` — the deprecated overload.
    ///
    /// The fudge quantises `y` onto a lattice of `y_scale` before sampling
    /// the gradients, while the *interpolation* weight still uses the
    /// unquantised `yr`. Passing the quantised value to both (the natural
    /// mistake) yields plausible terrain that is not vanilla's.
    #[must_use]
    pub fn noise_with_fudge(&self, x: f64, y: f64, z: f64, y_scale: f64, y_fudge: f64) -> f64 {
        let x = x + self.xo;
        let y = y + self.yo;
        let z = z + self.zo;

        let xf = floor(x);
        let yf = floor(y);
        let zf = floor(z);

        let xr = x - f64::from(xf);
        let yr = y - f64::from(yf);
        let zr = z - f64::from(zf);

        let yr_fudge = if y_scale == 0.0 {
            0.0
        } else {
            let fudge_limit = if y_fudge >= 0.0 && y_fudge < yr {
                y_fudge
            } else {
                yr
            };
            // `Mth.floor` returns an int here, so this is int * double.
            f64::from(floor(fudge_limit / y_scale + SHIFT_UP_EPSILON)) * y_scale
        };

        self.sample_and_lerp(xf, yf, zf, xr, yr - yr_fudge, zr, yr)
    }

    #[allow(clippy::too_many_arguments)] // Mirrors vanilla's private helper.
    fn sample_and_lerp(
        &self,
        x: i32,
        y: i32,
        z: i32,
        xr: f64,
        yr: f64,
        zr: f64,
        yr_original: f64,
    ) -> f64 {
        let x0 = self.p(x);
        let x1 = self.p(x + 1);
        let xy00 = self.p(x0 + y);
        let xy01 = self.p(x0 + y + 1);
        let xy10 = self.p(x1 + y);
        let xy11 = self.p(x1 + y + 1);

        let d000 = grad_dot(self.p(xy00 + z), xr, yr, zr);
        let d100 = grad_dot(self.p(xy10 + z), xr - 1.0, yr, zr);
        let d010 = grad_dot(self.p(xy01 + z), xr, yr - 1.0, zr);
        let d110 = grad_dot(self.p(xy11 + z), xr - 1.0, yr - 1.0, zr);
        let d001 = grad_dot(self.p(xy00 + z + 1), xr, yr, zr - 1.0);
        let d101 = grad_dot(self.p(xy10 + z + 1), xr - 1.0, yr, zr - 1.0);
        let d011 = grad_dot(self.p(xy01 + z + 1), xr, yr - 1.0, zr - 1.0);
        let d111 = grad_dot(self.p(xy11 + z + 1), xr - 1.0, yr - 1.0, zr - 1.0);

        // The y weight uses the *original* yr, not the fudged one.
        let x_alpha = smoothstep(xr);
        let y_alpha = smoothstep(yr_original);
        let z_alpha = smoothstep(zr);

        lerp3(
            x_alpha, y_alpha, z_alpha, d000, d100, d010, d110, d001, d101, d011, d111,
        )
    }

    /// `noiseWithDerivative` — value plus the gradient, **accumulated into**
    /// `derivative` rather than assigned (vanilla uses `+=`, so callers sum
    /// octaves into one buffer).
    pub fn noise_with_derivative(&self, x: f64, y: f64, z: f64, derivative: &mut [f64; 3]) -> f64 {
        let x = x + self.xo;
        let y = y + self.yo;
        let z = z + self.zo;

        let xf = floor(x);
        let yf = floor(y);
        let zf = floor(z);

        let xr = x - f64::from(xf);
        let yr = y - f64::from(yf);
        let zr = z - f64::from(zf);

        let x0 = self.p(xf);
        let x1 = self.p(xf + 1);
        let xy00 = self.p(x0 + yf);
        let xy01 = self.p(x0 + yf + 1);
        let xy10 = self.p(x1 + yf);
        let xy11 = self.p(x1 + yf + 1);

        let g = |hash: i32| GRADIENT[(hash & 15) as usize];
        let g000 = g(self.p(xy00 + zf));
        let g100 = g(self.p(xy10 + zf));
        let g010 = g(self.p(xy01 + zf));
        let g110 = g(self.p(xy11 + zf));
        let g001 = g(self.p(xy00 + zf + 1));
        let g101 = g(self.p(xy10 + zf + 1));
        let g011 = g(self.p(xy01 + zf + 1));
        let g111 = g(self.p(xy11 + zf + 1));

        let dot = |g: [i32; 3], x: f64, y: f64, z: f64| {
            f64::from(g[0]) * x + f64::from(g[1]) * y + f64::from(g[2]) * z
        };
        let d000 = dot(g000, xr, yr, zr);
        let d100 = dot(g100, xr - 1.0, yr, zr);
        let d010 = dot(g010, xr, yr - 1.0, zr);
        let d110 = dot(g110, xr - 1.0, yr - 1.0, zr);
        let d001 = dot(g001, xr, yr, zr - 1.0);
        let d101 = dot(g101, xr - 1.0, yr, zr - 1.0);
        let d011 = dot(g011, xr, yr - 1.0, zr - 1.0);
        let d111 = dot(g111, xr - 1.0, yr - 1.0, zr - 1.0);

        let x_alpha = smoothstep(xr);
        let y_alpha = smoothstep(yr);
        let z_alpha = smoothstep(zr);

        let component = |i: usize| {
            lerp3(
                x_alpha,
                y_alpha,
                z_alpha,
                f64::from(g000[i]),
                f64::from(g100[i]),
                f64::from(g010[i]),
                f64::from(g110[i]),
                f64::from(g001[i]),
                f64::from(g101[i]),
                f64::from(g011[i]),
                f64::from(g111[i]),
            )
        };
        let d1x = component(0);
        let d1y = component(1);
        let d1z = component(2);

        let d2x = lerp2(
            y_alpha,
            z_alpha,
            d100 - d000,
            d110 - d010,
            d101 - d001,
            d111 - d011,
        );
        let d2y = lerp2(
            z_alpha,
            x_alpha,
            d010 - d000,
            d011 - d001,
            d110 - d100,
            d111 - d101,
        );
        let d2z = lerp2(
            x_alpha,
            y_alpha,
            d001 - d000,
            d101 - d100,
            d011 - d010,
            d111 - d110,
        );

        derivative[0] += d1x + smoothstep_derivative(xr) * d2x;
        derivative[1] += d1y + smoothstep_derivative(yr) * d2y;
        derivative[2] += d1z + smoothstep_derivative(zr) * d2z;

        lerp3(
            x_alpha, y_alpha, z_alpha, d000, d100, d010, d110, d001, d101, d011, d111,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::XoroshiroRandomSource;

    #[test]
    fn permutation_is_a_permutation() {
        let mut rng = XoroshiroRandomSource::from_seed(20260708);
        let noise = ImprovedNoise::new(&mut rng);
        let mut seen = [false; 256];
        for &v in &noise.permutation {
            assert!(!seen[v as usize], "value {v} appears twice");
            seen[v as usize] = true;
        }
    }

    #[test]
    fn offsets_are_scaled_into_the_lattice() {
        let mut rng = XoroshiroRandomSource::from_seed(20260708);
        let noise = ImprovedNoise::new(&mut rng);
        for offset in [noise.xo, noise.yo, noise.zo] {
            assert!(
                (0.0..256.0).contains(&offset),
                "offset {offset} outside [0, 256)"
            );
        }
    }

    #[test]
    fn construction_advances_the_stream_deterministically() {
        // Two sources at the same seed must build byte-identical tables and
        // leave the stream at the same place — the property PerlinNoise
        // relies on when it seeds each octave from a positional fork.
        let mut a = XoroshiroRandomSource::from_seed(7);
        let mut b = XoroshiroRandomSource::from_seed(7);
        let na = ImprovedNoise::new(&mut a);
        let nb = ImprovedNoise::new(&mut b);
        assert_eq!(na.permutation, nb.permutation);
        assert_eq!(a.next_i64(), b.next_i64());
    }
}
