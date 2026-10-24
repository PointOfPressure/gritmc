//! `net.minecraft.util.Crypt` — the primitives online mode is built from.
//!
//! # Why these are written out rather than pulled in
//!
//! This workspace's manifest says every dependency is compile time and audit
//! surface and that additions go through the orchestrator. So SHA-1, AES-128,
//! CFB8 and the RSA arithmetic are here, in about the space three `use` lines
//! would have taken, and each is checked against vanilla's own JCE output
//! rather than against a reading of the standard. There is precedent in this
//! crate: [`crate::wire::offline_uuid`] carries an MD5 for the same reason.
//!
//! # What vanilla actually specifies, and where a reasonable guess is wrong
//!
//! * `Crypt.getCipher(mode, key)` is `AES/CFB8/NoPadding` with
//!   `new IvParameterSpec(key.getEncoded())` — **the key is also the IV**.
//! * `Crypt.encryptUsingKey` is `Cipher.getInstance("RSA")`, which in SunJCE
//!   is `RSA/ECB/PKCS1Padding` — v1.5 padding, not OAEP.
//! * `Crypt.digestData(String serverId, PublicKey pub, SecretKey secret)`
//!   hashes them in a **different order from its own parameter list**:
//!   `digestData(serverId.getBytes(ISO_8859_1), secret.getEncoded(),
//!   pub.getEncoded())`. Reading the signature and hashing key-then-secret is
//!   the obvious mistake and produces a hash that is wrong every time.
//! * The hash the session server is asked about is
//!   `new BigInteger(digest).toString(16)` — a **signed, two's-complement**
//!   integer in base 16. Two consequences a hex-encoder does not have: a
//!   digest whose first byte has the high bit set produces a hash with a
//!   leading `-` and the *two's complement* of the bytes, not the bytes with a
//!   sign stuck on; and a digest with a leading zero byte produces a hash
//!   **shorter than 40 characters**. Both are in the golden — `hash_notch` is
//!   negative and `hash_dash` is 38 characters.
//!
//! # Evidence
//!
//! `crates/rustmc-proto/golden/crypt.json`, from `ci/crypt-golden.sh`: a
//! fixed 1024-bit key, vanilla's own `Crypt` calls for the digest, the six
//! server hashes, the CFB8 stream both whole and in three pieces, one raw AES
//! block, five SHA-1 digests bracketing the padding boundary, and a
//! deterministic PKCS#1 ciphertext the JVM itself confirmed decrypts.

use std::io;

// ---------------------------------------------------------------------------
// SHA-1
// ---------------------------------------------------------------------------

/// SHA-1 (FIPS 180-4 §6.1).
#[must_use]
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut block = [0u8; 64];
    let mut chunks = data.chunks_exact(64);
    for c in &mut chunks {
        block.copy_from_slice(c);
        compress(&mut h, &block);
    }
    // The padding, and the reason the golden carries 55-, 56- and 119-byte
    // cases: a message whose tail is 56..63 bytes needs a second block for the
    // length, and an implementation that only ever appends one block is
    // correct for every length below 56.
    let rest = chunks.remainder();
    let mut tail = [0u8; 128];
    tail[..rest.len()].copy_from_slice(rest);
    tail[rest.len()] = 0x80;
    let bits = (data.len() as u64) * 8;
    let tail_len = if rest.len() < 56 { 64 } else { 128 };
    tail[tail_len - 8..tail_len].copy_from_slice(&bits.to_be_bytes());
    for c in tail[..tail_len].chunks_exact(64) {
        block.copy_from_slice(c);
        compress(&mut h, &block);
    }

    let mut out = [0u8; 20];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

fn compress(h: &mut [u32; 5], block: &[u8; 64]) {
    let mut w = [0u32; 80];
    for i in 0..16 {
        w[i] = u32::from_be_bytes([
            block[i * 4],
            block[i * 4 + 1],
            block[i * 4 + 2],
            block[i * 4 + 3],
        ]);
    }
    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }
    let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
    for (i, wi) in w.iter().enumerate() {
        let (f, k) = match i {
            0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999),
            20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
            40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
            _ => (b ^ c ^ d, 0xCA62_C1D6),
        };
        let t = a
            .rotate_left(5)
            .wrapping_add(f)
            .wrapping_add(e)
            .wrapping_add(k)
            .wrapping_add(*wi);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = t;
    }
    h[0] = h[0].wrapping_add(a);
    h[1] = h[1].wrapping_add(b);
    h[2] = h[2].wrapping_add(c);
    h[3] = h[3].wrapping_add(d);
    h[4] = h[4].wrapping_add(e);
}

// ---------------------------------------------------------------------------
// AES-128
// ---------------------------------------------------------------------------

/// `xtime` — multiply by 2 in GF(2^8) modulo the AES polynomial 0x11B.
const fn xtime(b: u8) -> u8 {
    (b << 1) ^ (if b & 0x80 != 0 { 0x1B } else { 0 })
}

const fn gmul(mut a: u8, mut b: u8) -> u8 {
    let mut r = 0u8;
    let mut i = 0;
    while i < 8 {
        if b & 1 != 0 {
            r ^= a;
        }
        a = xtime(a);
        b >>= 1;
        i += 1;
    }
    r
}

/// The S-box, computed rather than tabulated.
///
/// A 256-byte literal is the usual way and is the one thing in a cipher that
/// cannot be reviewed by reading it: one transposed pair is invisible and
/// produces output that looks exactly as random as the right answer. This is
/// the definition instead — the multiplicative inverse in GF(2^8) followed by
/// FIPS-197's affine transform — so it can be checked by eye, and the golden's
/// single AES block localises a mistake in it.
const fn build_sbox() -> [u8; 256] {
    let mut inv = [0u8; 256];
    let mut i = 1u16;
    while i < 256 {
        let mut j = 1u16;
        while j < 256 {
            if gmul(i as u8, j as u8) == 1 {
                inv[i as usize] = j as u8;
                break;
            }
            j += 1;
        }
        i += 1;
    }
    let mut sbox = [0u8; 256];
    let mut k = 0usize;
    while k < 256 {
        let b = inv[k];
        sbox[k] =
            b ^ b.rotate_left(1) ^ b.rotate_left(2) ^ b.rotate_left(3) ^ b.rotate_left(4) ^ 0x63;
        k += 1;
    }
    sbox
}

const SBOX: [u8; 256] = build_sbox();

/// An AES-128 key schedule. Encryption only, which is all CFB8 needs in either
/// direction.
#[derive(Clone)]
pub struct Aes128 {
    round_keys: [[u8; 16]; 11],
}

impl Aes128 {
    /// # Panics
    /// Never — the key length is fixed by the type.
    #[must_use]
    pub fn new(key: &[u8; 16]) -> Self {
        let mut w = [[0u8; 4]; 44];
        for i in 0..4 {
            w[i].copy_from_slice(&key[i * 4..i * 4 + 4]);
        }
        let mut rcon = 1u8;
        for i in 4..44 {
            let mut t = w[i - 1];
            if i % 4 == 0 {
                t = [
                    SBOX[t[1] as usize] ^ rcon,
                    SBOX[t[2] as usize],
                    SBOX[t[3] as usize],
                    SBOX[t[0] as usize],
                ];
                rcon = xtime(rcon);
            }
            for j in 0..4 {
                w[i][j] = w[i - 4][j] ^ t[j];
            }
        }
        let mut round_keys = [[0u8; 16]; 11];
        for r in 0..11 {
            for c in 0..4 {
                round_keys[r][c * 4..c * 4 + 4].copy_from_slice(&w[r * 4 + c]);
            }
        }
        Self { round_keys }
    }

    /// One block, encrypt direction.
    #[must_use]
    pub fn encrypt_block(&self, input: &[u8; 16]) -> [u8; 16] {
        let mut s = *input;
        xor(&mut s, &self.round_keys[0]);
        for r in 1..10 {
            sub_bytes(&mut s);
            shift_rows(&mut s);
            mix_columns(&mut s);
            xor(&mut s, &self.round_keys[r]);
        }
        sub_bytes(&mut s);
        shift_rows(&mut s);
        xor(&mut s, &self.round_keys[10]);
        s
    }
}

fn xor(s: &mut [u8; 16], k: &[u8; 16]) {
    for i in 0..16 {
        s[i] ^= k[i];
    }
}

fn sub_bytes(s: &mut [u8; 16]) {
    for b in s.iter_mut() {
        *b = SBOX[*b as usize];
    }
}

/// The state is column-major: byte `i` is row `i % 4`, column `i / 4`.
fn shift_rows(s: &mut [u8; 16]) {
    let t = *s;
    for row in 1..4 {
        for col in 0..4 {
            s[col * 4 + row] = t[((col + row) % 4) * 4 + row];
        }
    }
}

fn mix_columns(s: &mut [u8; 16]) {
    for c in 0..4 {
        let a = [s[c * 4], s[c * 4 + 1], s[c * 4 + 2], s[c * 4 + 3]];
        s[c * 4] = gmul(a[0], 2) ^ gmul(a[1], 3) ^ a[2] ^ a[3];
        s[c * 4 + 1] = a[0] ^ gmul(a[1], 2) ^ gmul(a[2], 3) ^ a[3];
        s[c * 4 + 2] = a[0] ^ a[1] ^ gmul(a[2], 2) ^ gmul(a[3], 3);
        s[c * 4 + 3] = gmul(a[0], 3) ^ a[1] ^ a[2] ^ gmul(a[3], 2);
    }
}

// ---------------------------------------------------------------------------
// CFB8
// ---------------------------------------------------------------------------

/// One direction of an `AES/CFB8/NoPadding` stream.
///
/// CFB8 is a *stream*: the shift register carries state between calls, so
/// encrypting a 64-byte buffer in one call and in three must give the same
/// bytes. The golden captures both, because an implementation that rebuilt the
/// register per call would agree on the first and disagree on the second.
#[derive(Clone)]
pub struct Cfb8 {
    cipher: Aes128,
    register: [u8; 16],
    decrypting: bool,
}

impl Cfb8 {
    /// `Crypt.getCipher(mode, key)` — the key is also the IV.
    #[must_use]
    pub fn new(key: &[u8; 16], decrypting: bool) -> Self {
        Self {
            cipher: Aes128::new(key),
            register: *key,
            decrypting,
        }
    }

    /// Transform in place, advancing the register.
    pub fn apply(&mut self, buf: &mut [u8]) {
        for b in buf.iter_mut() {
            let keystream = self.cipher.encrypt_block(&self.register)[0];
            // The byte that goes into the register is always the CIPHERTEXT
            // one, which is the input when decrypting and the output when
            // encrypting. Getting this backwards produces a stream that
            // decrypts its own output and nothing else.
            let feed = if self.decrypting { *b } else { *b ^ keystream };
            *b ^= keystream;
            self.register.copy_within(1..16, 0);
            self.register[15] = feed;
        }
    }
}

// ---------------------------------------------------------------------------
// Big unsigned integers, enough for RSA-1024
// ---------------------------------------------------------------------------

/// A big unsigned integer as little-endian 64-bit limbs.
///
/// Only what a modular exponentiation needs. Reduction is Montgomery rather
/// than long division: `n` is an RSA modulus and therefore odd, which is
/// Montgomery's only precondition, and it needs no division at all beyond a
/// five-iteration Newton inverse modulo 2^64.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BigUint {
    limbs: Vec<u64>,
}

impl BigUint {
    #[must_use]
    pub fn from_be_bytes(b: &[u8]) -> Self {
        let mut limbs = Vec::with_capacity(b.len() / 8 + 1);
        let mut i = b.len();
        while i > 0 {
            let start = i.saturating_sub(8);
            let mut v = 0u64;
            for &x in &b[start..i] {
                v = (v << 8) | u64::from(x);
            }
            limbs.push(v);
            i = start;
        }
        let mut n = Self { limbs };
        n.trim();
        n
    }

    /// Big-endian bytes, left-padded to `len`.
    ///
    /// # Errors
    /// If the value does not fit.
    pub fn to_be_bytes(&self, len: usize) -> io::Result<Vec<u8>> {
        let mut out = vec![0u8; len];
        for (i, limb) in self.limbs.iter().enumerate() {
            for j in 0..8 {
                let byte = ((limb >> (j * 8)) & 0xFF) as u8;
                let pos = i * 8 + j;
                if pos >= len {
                    if byte != 0 {
                        return Err(io::Error::other("integer does not fit"));
                    }
                    continue;
                }
                out[len - 1 - pos] = byte;
            }
        }
        Ok(out)
    }

    fn trim(&mut self) {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
    }

    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    fn bit(&self, i: usize) -> bool {
        self.limbs
            .get(i / 64)
            .is_some_and(|l| (l >> (i % 64)) & 1 == 1)
    }

    fn bits(&self) -> usize {
        match self.limbs.last() {
            None => 0,
            Some(l) => (self.limbs.len() - 1) * 64 + (64 - l.leading_zeros() as usize),
        }
    }

    fn cmp_limbs(a: &[u64], b: &[u64]) -> std::cmp::Ordering {
        let n = a.len().max(b.len());
        for i in (0..n).rev() {
            let x = a.get(i).copied().unwrap_or(0);
            let y = b.get(i).copied().unwrap_or(0);
            if x != y {
                return x.cmp(&y);
            }
        }
        std::cmp::Ordering::Equal
    }

    /// `self = self - other`, assuming `self >= other`.
    fn sub_in_place(a: &mut [u64], b: &[u64]) {
        let mut borrow = 0u64;
        for (i, x) in a.iter_mut().enumerate() {
            let y = b.get(i).copied().unwrap_or(0);
            let (r1, o1) = x.overflowing_sub(y);
            let (r2, o2) = r1.overflowing_sub(borrow);
            *x = r2;
            borrow = u64::from(o1 || o2);
        }
    }

    /// `(self * 2) mod m`, for building R^2 without a division.
    fn double_mod(v: &mut Vec<u64>, m: &[u64]) {
        let mut carry = 0u64;
        for l in v.iter_mut() {
            let new_carry = *l >> 63;
            *l = (*l << 1) | carry;
            carry = new_carry;
        }
        if carry != 0 {
            v.push(carry);
        }
        while Self::cmp_limbs(v, m) != std::cmp::Ordering::Less {
            Self::sub_in_place(v, m);
            while v.last() == Some(&0) {
                v.pop();
            }
        }
    }

    /// `self^exp mod m`, `m` odd.
    ///
    /// # Errors
    /// If the modulus is even or zero — Montgomery has no inverse then, and
    /// returning a wrong answer would be worse than refusing.
    pub fn mod_pow(&self, exp: &Self, m: &Self) -> io::Result<Self> {
        if m.limbs.is_empty() || m.limbs[0] & 1 == 0 {
            return Err(io::Error::other("modulus must be odd and non-zero"));
        }
        let n = m.limbs.len();
        // n' = -m^-1 mod 2^64, by Newton iteration: five doublings take a
        // 1-bit inverse to a 64-bit one.
        let mut ninv = 1u64;
        for _ in 0..6 {
            ninv = ninv.wrapping_mul(2u64.wrapping_sub(m.limbs[0].wrapping_mul(ninv)));
        }
        let ninv = ninv.wrapping_neg();

        // R^2 mod m, by doubling R = 2^(64n) twice more — 128n doublings, each
        // a shift and at most one subtract.
        let mut r2 = vec![1u64];
        for _ in 0..(128 * n) {
            Self::double_mod(&mut r2, &m.limbs);
        }
        r2.resize(n, 0);

        let mont = |a: &[u64], b: &[u64]| Self::mont_mul(a, b, &m.limbs, ninv);
        let mut base = self.limbs.clone();
        base.resize(n.max(base.len()), 0);
        // Reduce the base below m first; a base larger than the modulus is
        // legal input and Montgomery form assumes it is not.
        while Self::cmp_limbs(&base, &m.limbs) != std::cmp::Ordering::Less {
            Self::sub_in_place(&mut base, &m.limbs);
        }
        base.resize(n, 0);

        let mut acc = mont(&[1], &r2); // 1 in Montgomery form
        let mut cur = mont(&base, &r2); // base in Montgomery form
        for i in 0..exp.bits() {
            if exp.bit(i) {
                acc = mont(&acc, &cur);
            }
            cur = mont(&cur, &cur);
        }
        let mut out = Self {
            limbs: mont(&acc, &[1]),
        };
        out.trim();
        Ok(out)
    }

    /// CIOS Montgomery multiplication: `a * b * R^-1 mod m`.
    fn mont_mul(a: &[u64], b: &[u64], m: &[u64], ninv: u64) -> Vec<u64> {
        let n = m.len();
        let mut t = vec![0u64; n + 2];
        for i in 0..n {
            let ai = a.get(i).copied().unwrap_or(0);
            let mut carry = 0u128;
            for (j, tj) in t.iter_mut().enumerate().take(n) {
                let bj = u128::from(b.get(j).copied().unwrap_or(0));
                let v = u128::from(*tj) + u128::from(ai) * bj + carry;
                *tj = v as u64;
                carry = v >> 64;
            }
            let v = u128::from(t[n]) + carry;
            t[n] = v as u64;
            t[n + 1] = (v >> 64) as u64;

            let u = t[0].wrapping_mul(ninv);
            let mut carry = 0u128;
            for (tj, mj) in t.iter_mut().zip(m.iter()).take(n) {
                let v = u128::from(*tj) + u128::from(u) * u128::from(*mj) + carry;
                *tj = v as u64;
                carry = v >> 64;
            }
            let v = u128::from(t[n]) + carry;
            t[n] = v as u64;
            t[n + 1] += (v >> 64) as u64;
            t.copy_within(1.., 0);
            t[n + 1] = 0;
        }
        let mut out = t[..n].to_vec();
        if t[n] != 0 || Self::cmp_limbs(&out, m) != std::cmp::Ordering::Less {
            Self::sub_in_place(&mut out, m);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// RSA
// ---------------------------------------------------------------------------

/// An RSA key, as much of one as this server needs.
#[derive(Clone, Debug)]
pub struct RsaKey {
    pub modulus: BigUint,
    pub exponent: BigUint,
    /// The modulus size in bytes — 128 for the 1024-bit key vanilla generates.
    pub size: usize,
}

/// `Crypt.decryptUsingKey(privateKey, bytes)`, i.e.
/// `RSA/ECB/PKCS1Padding` in decrypt mode.
///
/// The block is `00 02 <at least 8 non-zero pad bytes> 00 <data>`, and every
/// one of those conditions is checked: a decryptor that skips to the first
/// zero byte without checking the leading `00 02` accepts a forged block, and
/// one that accepts a zero pad byte accepts a truncated one.
///
/// # Errors
/// If the ciphertext is the wrong length or the recovered block is not a valid
/// PKCS#1 v1.5 type-2 block.
pub fn rsa_decrypt_pkcs1(key: &RsaKey, ciphertext: &[u8]) -> io::Result<Vec<u8>> {
    if ciphertext.len() != key.size {
        return Err(io::Error::other(format!(
            "ciphertext is {} bytes, modulus is {}",
            ciphertext.len(),
            key.size
        )));
    }
    let c = BigUint::from_be_bytes(ciphertext);
    let m = c.mod_pow(&key.exponent, &key.modulus)?;
    let block = m.to_be_bytes(key.size)?;
    if block[0] != 0x00 || block[1] != 0x02 {
        return Err(io::Error::other("not a PKCS#1 v1.5 type-2 block"));
    }
    let sep = block[2..]
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| io::Error::other("PKCS#1 block has no separator"))?;
    if sep < 8 {
        return Err(io::Error::other("PKCS#1 padding is shorter than 8 bytes"));
    }
    Ok(block[2 + sep + 1..].to_vec())
}

// ---------------------------------------------------------------------------
// DER
// ---------------------------------------------------------------------------

/// The X.509 `SubjectPublicKeyInfo` a `ClientboundHelloPacket` carries.
///
/// `ServerLoginPacketListenerImpl` sends `getPublic().getEncoded()` verbatim,
/// and for a JCE RSA key that is
///
/// ```text
///   SEQUENCE {
///     SEQUENCE { OID 1.2.840.113549.1.1.1, NULL },
///     BIT STRING { SEQUENCE { INTEGER modulus, INTEGER exponent } }
///   }
/// ```
///
/// The two INTEGERs are signed, so a modulus whose top bit is set — which is
/// every RSA modulus — gets a leading `00`. Dropping it is the classic
/// mistake and yields an encoding a client rejects.
#[must_use]
pub fn encode_rsa_public_key(modulus: &[u8], exponent: &[u8]) -> Vec<u8> {
    let rsa = der_seq(&[der_int(modulus), der_int(exponent)].concat());
    // A BIT STRING's contents are preceded by a byte counting the unused bits
    // in the final octet, which is zero for anything byte-aligned.
    let mut bits = vec![0u8];
    bits.extend_from_slice(&rsa);
    let bit_string = der_tlv(0x03, &bits);
    let alg = der_seq(
        &[
            der_tlv(
                0x06,
                &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01],
            ),
            der_tlv(0x05, &[]),
        ]
        .concat(),
    );
    der_seq(&[alg, bit_string].concat())
}

fn der_tlv(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    if body.len() < 0x80 {
        out.push(body.len() as u8);
    } else {
        let len = body.len();
        let bytes = len.to_be_bytes();
        let first = bytes.iter().position(|&b| b != 0).unwrap_or(7);
        out.push(0x80 | (bytes.len() - first) as u8);
        out.extend_from_slice(&bytes[first..]);
    }
    out.extend_from_slice(body);
    out
}

fn der_seq(body: &[u8]) -> Vec<u8> {
    der_tlv(0x30, body)
}

fn der_int(v: &[u8]) -> Vec<u8> {
    let start = v.iter().position(|&b| b != 0).unwrap_or(v.len());
    let mut body = Vec::new();
    if start == v.len() {
        body.push(0);
    } else {
        if v[start] & 0x80 != 0 {
            body.push(0);
        }
        body.extend_from_slice(&v[start..]);
    }
    der_tlv(0x02, &body)
}

/// Read a DER TLV at `pos`, returning `(tag, body range end, body start)`.
fn der_read(b: &[u8], pos: usize) -> io::Result<(u8, usize, usize)> {
    let tag = *b.get(pos).ok_or_else(|| io::Error::other("DER: eof"))?;
    let l0 = *b.get(pos + 1).ok_or_else(|| io::Error::other("DER: eof"))?;
    let (len, start) = if l0 & 0x80 == 0 {
        (l0 as usize, pos + 2)
    } else {
        let n = (l0 & 0x7F) as usize;
        if n == 0 || n > 8 {
            return Err(io::Error::other("DER: bad length"));
        }
        let mut len = 0usize;
        for i in 0..n {
            len = (len << 8)
                | *b.get(pos + 2 + i)
                    .ok_or_else(|| io::Error::other("DER: eof"))? as usize;
        }
        (len, pos + 2 + n)
    };
    if start + len > b.len() {
        return Err(io::Error::other("DER: truncated"));
    }
    Ok((tag, start + len, start))
}

/// Pull the modulus and private exponent out of a PKCS#8 `PrivateKeyInfo`.
///
/// PKCS#8 wraps a PKCS#1 `RSAPrivateKey` in an OCTET STRING, so this is two
/// layers: `SEQUENCE { INTEGER 0, SEQUENCE {alg}, OCTET STRING { SEQUENCE {
/// INTEGER version, INTEGER n, INTEGER e, INTEGER d, … } } }`.
///
/// # Errors
/// If the bytes are not that shape.
pub fn parse_pkcs8_rsa_private(der: &[u8]) -> io::Result<RsaKey> {
    let (tag, _, outer) = der_read(der, 0)?;
    if tag != 0x30 {
        return Err(io::Error::other("PKCS#8: not a SEQUENCE"));
    }
    // version
    let (_, after_ver, _) = der_read(der, outer)?;
    // algorithm identifier
    let (_, after_alg, _) = der_read(der, after_ver)?;
    let (tag, _, oct) = der_read(der, after_alg)?;
    if tag != 0x04 {
        return Err(io::Error::other("PKCS#8: no OCTET STRING"));
    }
    let (tag, _, inner) = der_read(der, oct)?;
    if tag != 0x30 {
        return Err(io::Error::other("PKCS#1: not a SEQUENCE"));
    }
    let (_, after_v, _) = der_read(der, inner)?;
    let (_, after_n, n_start) = der_read(der, after_v)?;
    let (_, after_e, _) = der_read(der, after_n)?;
    let (_, d_end, d_start) = der_read(der, after_e)?;
    let modulus = BigUint::from_be_bytes(&der[n_start..after_n]);
    let exponent = BigUint::from_be_bytes(&der[d_start..d_end]);
    // The DER INTEGER's leading zero is part of the encoding, not the number,
    // and `from_be_bytes` drops it; the modulus size has to come from the
    // *number*, because it is what a ciphertext's length is checked against.
    let size = modulus.bits().div_ceil(8);
    Ok(RsaKey {
        modulus,
        exponent,
        size,
    })
}

// ---------------------------------------------------------------------------
// The server hash
// ---------------------------------------------------------------------------

/// `new BigInteger(Crypt.digestData(serverId, publicKey, secret)).toString(16)`.
///
/// Three things a hex-encoder gets wrong, all of them in the golden:
///
/// * the hash order is `serverId`, then the **secret**, then the public key —
///   the reverse of `digestData`'s own parameter list;
/// * the integer is **signed**, so a digest with its top bit set produces the
///   two's complement with a leading `-`;
/// * leading zeros are dropped, so the string is not always 40 characters.
#[must_use]
pub fn server_id_hash(server_id: &str, public_key_der: &[u8], secret: &[u8]) -> String {
    let mut data = Vec::new();
    // `String.getBytes("ISO_8859_1")` — one byte per char, the low 8 bits.
    // Not UTF-8: vanilla's `serverId` is always "" so nothing depends on it
    // today, but a UTF-8 encoder here would be wrong for any id above U+007F
    // and right for every test that used an ASCII one.
    data.extend(server_id.chars().map(|c| (c as u32 & 0xFF) as u8));
    data.extend_from_slice(secret);
    data.extend_from_slice(public_key_der);
    signed_hex(&sha1(&data))
}

/// `new BigInteger(bytes).toString(16)` for a 20-byte digest.
fn signed_hex(digest: &[u8; 20]) -> String {
    if digest[0] & 0x80 == 0 {
        let s = digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let t = s.trim_start_matches('0');
        return if t.is_empty() { "0".into() } else { t.into() };
    }
    // Negative: negate the two's-complement value and print with a sign.
    let mut v = *digest;
    for b in v.iter_mut() {
        *b = !*b;
    }
    let mut i = 19i32;
    loop {
        let (r, carry) = v[i as usize].overflowing_add(1);
        v[i as usize] = r;
        if !carry || i == 0 {
            break;
        }
        i -= 1;
    }
    let s = v.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let t = s.trim_start_matches('0');
    format!("-{}", if t.is_empty() { "0" } else { t })
}
