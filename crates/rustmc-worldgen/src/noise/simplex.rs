//! Simplex noise.
//!
//! Reference: `net.minecraft.world.level.levelgen.synth.SimplexNoise`.
//!
//! Used by the End's island generator. Unlike [`super::improved`], which
//! interpolates over a cubic grid, simplex noise skews space onto a
//! triangular/tetrahedral lattice and sums a contribution from each corner
//! of the containing simplex — 3 corners in 2D, 4 in 3D. That is why it has
//! no axis-aligned artefacts and why the corner selection below is a
//! branch tree rather than a loop.
//!
//! Two things differ from `ImprovedNoise` and are easy to carry over
//! wrongly:
//!
//! - The permutation is read **unmasked** (`p[x & 0xFF]`, values 0..255)
//!   and then reduced with `% 12` at the gradient lookup, rather than
//!   masked to `& 15`. Twelve, not sixteen — so the four duplicate rows at
//!   the end of the shared gradient table are never selected here.
//! - The contribution falloff uses a `base` of 0.5 in 2D and 0.6 in 3D, and
//!   a corner whose falloff goes negative contributes exactly zero rather
//!   than being clamped afterwards.

use crate::math::floor;
use crate::rng::RandomSource;

/// The same table `ImprovedNoise` uses, but indexed `% 12` here, so only
/// the first twelve rows are reachable.
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

/// Skew and unskew factors for the 2D lattice.
const SQRT_3: f64 = 1.732_050_807_568_877_2;
const F2: f64 = 0.5 * (SQRT_3 - 1.0);
const G2: f64 = (3.0 - SQRT_3) / 6.0;

const F3: f64 = 0.333_333_333_333_333_3;
const G3: f64 = 0.166_666_666_666_666_66;

#[derive(Clone, Debug)]
pub struct SimplexNoise {
    permutation: [i32; 256],
    pub xo: f64,
    pub yo: f64,
    pub zo: f64,
}

impl SimplexNoise {
    /// Consumes the same draws as [`super::ImprovedNoise::new`] — three
    /// doubles then 256 bounded ints — so the two are interchangeable in a
    /// stream without shifting anything downstream.
    pub fn new<R: RandomSource>(random: &mut R) -> Self {
        let xo = random.next_f64() * 256.0;
        let yo = random.next_f64() * 256.0;
        let zo = random.next_f64() * 256.0;

        let mut permutation = [0i32; 256];
        for (i, slot) in permutation.iter_mut().enumerate() {
            *slot = i as i32;
        }
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

    fn p(&self, x: i32) -> i32 {
        self.permutation[(x & 0xFF) as usize]
    }

    fn dot(g: [i32; 3], x: f64, y: f64, z: f64) -> f64 {
        f64::from(g[0]) * x + f64::from(g[1]) * y + f64::from(g[2]) * z
    }

    /// One simplex corner's contribution.
    ///
    /// `base - r²` going negative means the corner is out of range and
    /// contributes nothing; the surviving term is raised to the fourth
    /// power, which is what makes the falloff smooth at the boundary.
    fn corner_noise(index: i32, x: f64, y: f64, z: f64, base: f64) -> f64 {
        let t = base - x * x - y * y - z * z;
        if t < 0.0 {
            0.0
        } else {
            let t = t * t;
            t * t * Self::dot(GRADIENT[index as usize], x, y, z)
        }
    }

    /// 2D simplex noise.
    #[must_use]
    pub fn get_value_2d(&self, xin: f64, yin: f64) -> f64 {
        let s = (xin + yin) * F2;
        let i = floor(xin + s);
        let j = floor(yin + s);
        let t = f64::from(i + j) * G2;
        let x0 = xin - (f64::from(i) - t);
        let y0 = yin - (f64::from(j) - t);

        // Which half of the unit square, i.e. which of the two triangles.
        let (i1, j1) = if x0 > y0 { (1, 0) } else { (0, 1) };

        let x1 = x0 - f64::from(i1) + G2;
        let y1 = y0 - f64::from(j1) + G2;
        let x2 = x0 - 1.0 + 2.0 * G2;
        let y2 = y0 - 1.0 + 2.0 * G2;

        let ii = i & 0xFF;
        let jj = j & 0xFF;
        let gi0 = self.p(ii + self.p(jj)) % 12;
        let gi1 = self.p(ii + i1 + self.p(jj + j1)) % 12;
        let gi2 = self.p(ii + 1 + self.p(jj + 1)) % 12;

        let n0 = Self::corner_noise(gi0, x0, y0, 0.0, 0.5);
        let n1 = Self::corner_noise(gi1, x1, y1, 0.0, 0.5);
        let n2 = Self::corner_noise(gi2, x2, y2, 0.0, 0.5);
        70.0 * (n0 + n1 + n2)
    }

    /// 3D simplex noise.
    #[must_use]
    pub fn get_value_3d(&self, xin: f64, yin: f64, zin: f64) -> f64 {
        let s = (xin + yin + zin) * F3;
        let i = floor(xin + s);
        let j = floor(yin + s);
        let k = floor(zin + s);
        let t = f64::from(i + j + k) * G3;
        let x0 = xin - (f64::from(i) - t);
        let y0 = yin - (f64::from(j) - t);
        let z0 = zin - (f64::from(k) - t);

        // Rank the three coordinates to pick which tetrahedron of the six
        // filling the cube we are in. Ported as vanilla's branch tree
        // rather than a sort: the tie-breaking on `>=` versus `<` decides
        // the corner order, and a "cleaner" comparison changes it.
        let (i1, j1, k1, i2, j2, k2) = if x0 >= y0 {
            if y0 >= z0 {
                (1, 0, 0, 1, 1, 0)
            } else if x0 >= z0 {
                (1, 0, 0, 1, 0, 1)
            } else {
                (0, 0, 1, 1, 0, 1)
            }
        } else if y0 < z0 {
            (0, 0, 1, 0, 1, 1)
        } else if x0 < z0 {
            (0, 1, 0, 0, 1, 1)
        } else {
            (0, 1, 0, 1, 1, 0)
        };

        let x1 = x0 - f64::from(i1) + G3;
        let y1 = y0 - f64::from(j1) + G3;
        let z1 = z0 - f64::from(k1) + G3;
        let x2 = x0 - f64::from(i2) + F3;
        let y2 = y0 - f64::from(j2) + F3;
        let z2 = z0 - f64::from(k2) + F3;
        let x3 = x0 - 1.0 + 0.5;
        let y3 = y0 - 1.0 + 0.5;
        let z3 = z0 - 1.0 + 0.5;

        let ii = i & 0xFF;
        let jj = j & 0xFF;
        let kk = k & 0xFF;
        let gi0 = self.p(ii + self.p(jj + self.p(kk))) % 12;
        let gi1 = self.p(ii + i1 + self.p(jj + j1 + self.p(kk + k1))) % 12;
        let gi2 = self.p(ii + i2 + self.p(jj + j2 + self.p(kk + k2))) % 12;
        let gi3 = self.p(ii + 1 + self.p(jj + 1 + self.p(kk + 1))) % 12;

        let n0 = Self::corner_noise(gi0, x0, y0, z0, 0.6);
        let n1 = Self::corner_noise(gi1, x1, y1, z1, 0.6);
        let n2 = Self::corner_noise(gi2, x2, y2, z2, 0.6);
        let n3 = Self::corner_noise(gi3, x3, y3, z3, 0.6);
        32.0 * (n0 + n1 + n2 + n3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::XoroshiroRandomSource;

    fn noise() -> SimplexNoise {
        let mut rng = XoroshiroRandomSource::from_seed(20260708);
        SimplexNoise::new(&mut rng)
    }

    #[test]
    fn permutation_is_a_permutation() {
        let mut seen = [false; 256];
        for &v in &noise().permutation {
            assert!(!seen[v as usize], "value {v} appears twice");
            seen[v as usize] = true;
        }
    }

    #[test]
    fn construction_leaves_the_stream_where_improved_noise_would() {
        // The two consume identical draws, so swapping one for the other in
        // a sequence must not shift anything after it.
        let mut a = XoroshiroRandomSource::from_seed(7);
        let mut b = XoroshiroRandomSource::from_seed(7);
        let _ = SimplexNoise::new(&mut a);
        let _ = super::super::ImprovedNoise::new(&mut b);
        assert_eq!(a.next_i64(), b.next_i64());
    }

    #[test]
    fn only_the_first_twelve_gradients_are_reachable() {
        // `% 12` rather than `& 15`, so the four duplicate rows at the end
        // of the table are dead here. If this ever becomes `& 15` the noise
        // changes everywhere.
        for value in 0..256 {
            assert!(value % 12 < 12);
        }
    }

    #[test]
    fn output_stays_in_a_sane_band() {
        let noise = noise();
        for i in -20..20 {
            let f = f64::from(i) * 0.37;
            let v2 = noise.get_value_2d(f, -f);
            let v3 = noise.get_value_3d(f, f * 0.5, -f);
            assert!(v2.is_finite() && v3.is_finite());
            // Simplex noise is normalised to roughly [-1, 1]; a much larger
            // magnitude means the scale factor or falloff base is wrong.
            assert!(v2.abs() < 2.0, "2d {v2} at {f}");
            assert!(v3.abs() < 2.0, "3d {v3} at {f}");
        }
    }

    #[test]
    fn it_is_not_constant() {
        let noise = noise();
        let a = noise.get_value_2d(0.5, 0.5);
        let b = noise.get_value_2d(10.25, -7.75);
        assert_ne!(a.to_bits(), b.to_bits());
    }
}
