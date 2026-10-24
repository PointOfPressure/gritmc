//! VarInt/VarLong — the protocol's core primitives. LEB128-style, 7 bits per
//! byte, low group first; i32 caps at 5 bytes, i64 at 10.
//! Reference: net.minecraft.network.VarInt / VarLong (cited, not copied).

use std::io::{self, Read, Write};

pub fn write_varint(out: &mut impl Write, value: i32) -> io::Result<()> {
    let mut v = value as u32;
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            return out.write_all(&[byte]);
        }
        out.write_all(&[byte | 0x80])?;
    }
}

pub fn read_varint(r: &mut impl Read) -> io::Result<i32> {
    let mut v: u32 = 0;
    for i in 0..5 {
        let mut b = [0u8; 1];
        r.read_exact(&mut b)?;
        v |= u32::from(b[0] & 0x7F) << (7 * i);
        if b[0] & 0x80 == 0 {
            // Byte 5 may only carry the top 4 bits of an i32.
            if i == 4 && b[0] & 0x70 != 0 {
                return Err(io::Error::other("VarInt overflows i32"));
            }
            return Ok(v as i32);
        }
    }
    Err(io::Error::other("VarInt longer than 5 bytes"))
}

pub fn write_varlong(out: &mut impl Write, value: i64) -> io::Result<()> {
    let mut v = value as u64;
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            return out.write_all(&[byte]);
        }
        out.write_all(&[byte | 0x80])?;
    }
}

pub fn read_varlong(r: &mut impl Read) -> io::Result<i64> {
    let mut v: u64 = 0;
    for i in 0..10 {
        let mut b = [0u8; 1];
        r.read_exact(&mut b)?;
        v |= u64::from(b[0] & 0x7F) << (7 * i);
        if b[0] & 0x80 == 0 {
            if i == 9 && b[0] & 0x7E != 0 {
                return Err(io::Error::other("VarLong overflows i64"));
            }
            return Ok(v as i64);
        }
    }
    Err(io::Error::other("VarLong longer than 10 bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(v: i32) -> Vec<u8> {
        let mut out = Vec::new();
        write_varint(&mut out, v).unwrap();
        out
    }

    #[test]
    fn known_vectors() {
        // Canonical encodings from the protocol spec.
        assert_eq!(enc(0), [0x00]);
        assert_eq!(enc(1), [0x01]);
        assert_eq!(enc(127), [0x7F]);
        assert_eq!(enc(128), [0x80, 0x01]);
        assert_eq!(enc(255), [0xFF, 0x01]);
        assert_eq!(enc(25565), [0xDD, 0xC7, 0x01]);
        assert_eq!(enc(2097151), [0xFF, 0xFF, 0x7F]);
        assert_eq!(enc(i32::MAX), [0xFF, 0xFF, 0xFF, 0xFF, 0x07]);
        assert_eq!(enc(-1), [0xFF, 0xFF, 0xFF, 0xFF, 0x0F]);
        assert_eq!(enc(i32::MIN), [0x80, 0x80, 0x80, 0x80, 0x08]);
    }

    #[test]
    fn roundtrip_int() {
        for v in [0, 1, -1, 127, 128, 300, 25565, i32::MAX, i32::MIN, -2097152] {
            let e = enc(v);
            assert_eq!(read_varint(&mut e.as_slice()).unwrap(), v, "value {v}");
            assert_eq!(e.len(), crate::varint_len(v), "len {v}");
        }
    }

    #[test]
    fn roundtrip_long() {
        for v in [0i64, 1, -1, i64::MAX, i64::MIN, 1 << 40, -(1 << 40)] {
            let mut e = Vec::new();
            write_varlong(&mut e, v).unwrap();
            assert_eq!(read_varlong(&mut e.as_slice()).unwrap(), v, "value {v}");
        }
    }

    #[test]
    fn rejects_overlong() {
        assert!(read_varint(&mut [0x80u8, 0x80, 0x80, 0x80, 0x80, 0x01].as_slice()).is_err());
    }
}
