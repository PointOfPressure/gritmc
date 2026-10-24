//! Wire primitives above VarInt: strings, fixed-width big-endian numbers,
//! UUIDs, bools. Everything the handshake→play flow needs; the wave-1
//! serde-style packet derive builds on these.

use crate::varint::{read_varint, write_varint};
use std::io::{self, Read, Write};

/// Protocol string cap (chars) for generic strings; UTF-8 bytes may be 3×.
const MAX_STRING: i32 = 32767;

pub fn write_string(out: &mut impl Write, s: &str) -> io::Result<()> {
    write_varint(out, s.len() as i32)?;
    out.write_all(s.as_bytes())
}

pub fn read_string(r: &mut impl Read) -> io::Result<String> {
    let len = read_varint(r)?;
    if !(0..=MAX_STRING * 3).contains(&len) {
        return Err(io::Error::other(format!(
            "string length {len} out of range"
        )));
    }
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)?;
    String::from_utf8(buf).map_err(io::Error::other)
}

macro_rules! be_num {
    ($write:ident, $read:ident, $t:ty) => {
        pub fn $write(out: &mut impl Write, v: $t) -> io::Result<()> {
            out.write_all(&v.to_be_bytes())
        }
        pub fn $read(r: &mut impl Read) -> io::Result<$t> {
            let mut b = [0u8; std::mem::size_of::<$t>()];
            r.read_exact(&mut b)?;
            Ok(<$t>::from_be_bytes(b))
        }
    };
}

be_num!(write_u8, read_u8, u8);
be_num!(write_u16, read_u16, u16);
be_num!(write_i16, read_i16, i16);
be_num!(write_i32, read_i32, i32);
be_num!(write_i64, read_i64, i64);
be_num!(write_u128, read_u128, u128);
be_num!(write_f32, read_f32, f32);
be_num!(write_f64, read_f64, f64);

pub fn write_bool(out: &mut impl Write, v: bool) -> io::Result<()> {
    out.write_all(&[v as u8])
}

pub fn read_bool(r: &mut impl Read) -> io::Result<bool> {
    Ok(read_u8(r)? != 0)
}

/// Offline-mode UUID: UUIDv3 (md5, RFC 4122) of "OfflinePlayer:<name>" —
/// reference: java.util.UUID.nameUUIDFromBytes as used by the vanilla server
/// for offline profiles. Needed so a wire bot's identity matches what the
/// server computes (ops.json entries, etc).
pub fn offline_uuid(name: &str) -> u128 {
    let digest = md5(format!("OfflinePlayer:{name}").as_bytes());
    let mut b = digest;
    b[6] = (b[6] & 0x0F) | 0x30; // version 3
    b[8] = (b[8] & 0x3F) | 0x80; // IETF variant
    u128::from_be_bytes(b)
}

/// Minimal MD5 (RFC 1321) — only consumer is [`offline_uuid`]; not a general
/// crypto surface. Implemented rather than pulling a crypto dep for 16 bytes.
fn md5(input: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    // floor(abs(sin(i+1)) * 2^32), the standard RFC 1321 table.
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];

    let mut msg = input.to_vec();
    let bit_len = (input.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    let (mut a0, mut b0, mut c0, mut d0) =
        (0x67452301u32, 0xefcdab89u32, 0x98badcfeu32, 0x10325476u32);
    for chunk in msg.chunks_exact(64) {
        let m: Vec<u32> = chunk
            .chunks_exact(4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
            .collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f2 = f.wrapping_add(a).wrapping_add(K[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f2.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = [0u8; 16];
    out[..4].copy_from_slice(&a0.to_le_bytes());
    out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes());
    out[12..].copy_from_slice(&d0.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_roundtrip() {
        let mut buf = Vec::new();
        write_string(&mut buf, "rustmc §k unicode ✓").unwrap();
        assert_eq!(
            read_string(&mut buf.as_slice()).unwrap(),
            "rustmc §k unicode ✓"
        );
    }

    #[test]
    fn md5_rfc1321_vectors() {
        let hex = |b: [u8; 16]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        assert_eq!(hex(md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            hex(md5(b"The quick brown fox jumps over the lazy dog")),
            "9e107d9d372bb6826bd81d3542a419d6"
        );
    }

    #[test]
    fn offline_uuid_shape() {
        let u = offline_uuid("oracle");
        let bytes = u.to_be_bytes();
        assert_eq!(bytes[6] >> 4, 3, "UUID version must be 3");
        assert_eq!(bytes[8] >> 6, 0b10, "IETF variant");
        // Same name → same UUID (the server computes this independently).
        assert_eq!(u, offline_uuid("oracle"));
        assert_ne!(u, offline_uuid("oracle2"));
    }
}
