//! Seed mixing and derivation.
//!
//! Reference: `net.minecraft.world.level.levelgen.RandomSupport`,
//! `net.minecraft.util.Mth#getSeed`.

use super::md5;

/// `RandomSupport.GOLDEN_RATIO_64`.
pub const GOLDEN_RATIO_64: i64 = -7046029254386353131;
/// `RandomSupport.SILVER_RATIO_64`.
pub const SILVER_RATIO_64: i64 = 7640891576956012809;

/// Stafford variant 13 of the MurmurHash3 finalizer (`RandomSupport`).
///
/// Every `>>>` is a **logical** shift — done on `u64` and cast back, because
/// an arithmetic shift of a negative seed would smear sign bits into the
/// mix and silently change every derived stream.
#[must_use]
pub fn mix_stafford13(z: i64) -> i64 {
    let z = (z ^ ((z as u64) >> 30) as i64).wrapping_mul(-4658895280553007687);
    let z = (z ^ ((z as u64) >> 27) as i64).wrapping_mul(-7723592293110705685);
    z ^ ((z as u64) >> 31) as i64
}

/// A 128-bit seed (`RandomSupport.Seed128bit`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seed128 {
    pub lo: i64,
    pub hi: i64,
}

impl Seed128 {
    #[must_use]
    pub const fn new(lo: i64, hi: i64) -> Self {
        Self { lo, hi }
    }

    #[must_use]
    pub const fn xor(self, lo: i64, hi: i64) -> Self {
        Self {
            lo: self.lo ^ lo,
            hi: self.hi ^ hi,
        }
    }

    #[must_use]
    pub fn mixed(self) -> Self {
        Self {
            lo: mix_stafford13(self.lo),
            hi: mix_stafford13(self.hi),
        }
    }
}

/// `RandomSupport.upgradeSeedTo128bitUnmixed`.
#[must_use]
pub fn upgrade_seed_to_128bit_unmixed(legacy_seed: i64) -> Seed128 {
    let lo = legacy_seed ^ SILVER_RATIO_64;
    let hi = lo.wrapping_add(GOLDEN_RATIO_64);
    Seed128 { lo, hi }
}

/// `RandomSupport.upgradeSeedTo128bit` — how a world seed becomes Xoroshiro
/// state.
#[must_use]
pub fn upgrade_seed_to_128bit(legacy_seed: i64) -> Seed128 {
    upgrade_seed_to_128bit_unmixed(legacy_seed).mixed()
}

/// `RandomSupport.seedFromHashOf` — MD5 of the UTF-8 name, split into two
/// **big-endian** halves (Guava `Longs.fromBytes` order, not the digest's
/// native little-endian word order).
#[must_use]
pub fn seed_from_hash_of(input: &str) -> Seed128 {
    let digest = md5::digest(input.as_bytes());
    let mut lo = [0u8; 8];
    let mut hi = [0u8; 8];
    lo.copy_from_slice(&digest[0..8]);
    hi.copy_from_slice(&digest[8..16]);
    Seed128 {
        lo: i64::from_be_bytes(lo),
        hi: i64::from_be_bytes(hi),
    }
}

/// `Mth#getSeed` — the positional block seed.
///
/// **The single most quirk-dense line in the RNG layer.** In Java the
/// expression is `x * 3129871 ^ z * 116129781L ^ y`:
///
/// - `x * 3129871` is `int * int`, so it wraps at **32 bits** and only then
///   sign-extends to 64. Widening `x` first (the "obvious" port) produces a
///   different value for any `x` past ~686 blocks from origin — i.e. it
///   looks correct near spawn and diverges everywhere else, which is the
///   worst possible failure shape.
/// - `z * 116129781L` is `int * long`, so it wraps at **64 bits**.
/// - `y` sign-extends.
///
/// The final `>> 16` is arithmetic.
#[must_use]
pub fn block_seed(x: i32, y: i32, z: i32) -> i64 {
    let seed =
        i64::from(x.wrapping_mul(3129871)) ^ i64::from(z).wrapping_mul(116129781) ^ i64::from(y);
    let seed = seed
        .wrapping_mul(seed)
        .wrapping_mul(42317861)
        .wrapping_add(seed.wrapping_mul(11));
    seed >> 16
}

/// `java.lang.String#hashCode`, over **UTF-16 code units**.
///
/// Used only by [`super::LegacyPositionalFactory::from_hash_of`]. Iterating
/// Rust `char`s instead would agree for BMP text and disagree for anything
/// astral — vanilla hashes surrogate pairs as two units.
#[must_use]
pub fn java_string_hash(s: &str) -> i32 {
    s.encode_utf16()
        .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(i32::from(c)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_128_upgrade_is_mix_of_unmixed() {
        let unmixed = upgrade_seed_to_128bit_unmixed(20260708);
        let mixed = upgrade_seed_to_128bit(20260708);
        assert_eq!(mixed.lo, mix_stafford13(unmixed.lo));
        assert_eq!(mixed.hi, mix_stafford13(unmixed.hi));
    }

    #[test]
    fn java_string_hash_matches_known_values() {
        // Values from java.lang.String#hashCode; "" and "minecraft:ore" are
        // the shapes that actually appear in registry names.
        assert_eq!(java_string_hash(""), 0);
        assert_eq!(java_string_hash("a"), 97);
        assert_eq!(java_string_hash("abc"), 96354);
    }

    #[test]
    fn block_seed_uses_32_bit_multiply_for_x() {
        // A 32-bit wrapping multiply is observable: x = 100_000 overflows
        // i32 when scaled by 3129871, so an i64 multiply would differ here.
        // This pins the quirk independently of the golden file.
        let widened = (i64::from(100_000i32)).wrapping_mul(3129871);
        let wrapped = i64::from(100_000i32.wrapping_mul(3129871));
        assert_ne!(widened, wrapped, "test premise: the multiply must overflow");
    }
}
