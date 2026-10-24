//! Gate `play/level_chunk_with_light` against real vanilla bytes.
//!
//! The capture is 117 packet bodies taken from a vanilla 26.2 server at seed
//! 31415926 by:
//!
//! ```sh
//! ci/vanilla-server.sh start <dir> <port> minecraft:normal 31415926
//! CHUNK_DUMP=<file> python3 ci/regdump.py 127.0.0.1 <port> chunkbot
//! ```
//!
//! It is Mojang-derived, so like every oracle world it is never committed;
//! point `RUSTMC_CHUNK_CAPTURE` at it to run these. A clean clone skips,
//! loudly enough to be noticed in the output but without failing — the same
//! contract `golden_carver_chunk` uses for its region.

use rustmc_server::chunk::{ContainerKind, LevelChunkWithLight, Palette, OVERWORLD_SECTIONS};

/// `RMCCHK01`, count, then per entry: i32 x, i32 z, u32 len, body.
fn capture() -> Option<Vec<(i32, i32, Vec<u8>)>> {
    let path = std::env::var("RUSTMC_CHUNK_CAPTURE").ok()?;
    let data = std::fs::read(&path).unwrap_or_else(|e| panic!("RUSTMC_CHUNK_CAPTURE={path}: {e}"));
    assert_eq!(&data[..8], b"RMCCHK01", "not a chunk capture: {path}");
    let n = u32::from_be_bytes(data[8..12].try_into().unwrap()) as usize;
    let mut out = Vec::with_capacity(n);
    let mut i = 12;
    for _ in 0..n {
        let x = i32::from_be_bytes(data[i..i + 4].try_into().unwrap());
        let z = i32::from_be_bytes(data[i + 4..i + 8].try_into().unwrap());
        let len = u32::from_be_bytes(data[i + 8..i + 12].try_into().unwrap()) as usize;
        i += 12;
        out.push((x, z, data[i..i + len].to_vec()));
        i += len;
    }
    assert_eq!(i, data.len(), "capture has trailing bytes");
    Some(out)
}

/// The whole point: decode every captured packet and re-encode it, and demand
/// the original bytes back.
///
/// `decode` refuses a parse that leaves trailing bytes, so passing this means
/// every byte of all 117 packets is accounted for by a named field — not that
/// the parser happened to stop somewhere plausible.
#[test]
fn every_captured_packet_survives_a_round_trip() {
    let Some(chunks) = capture() else {
        eprintln!("SKIP golden_chunk_packet: set RUSTMC_CHUNK_CAPTURE (see the module docs)");
        return;
    };
    assert!(!chunks.is_empty(), "capture is empty");
    for (x, z, body) in &chunks {
        let p = LevelChunkWithLight::decode(body, OVERWORLD_SECTIONS)
            .unwrap_or_else(|e| panic!("chunk ({x}, {z}): decode: {e}"));
        assert_eq!(p.chunk_x, *x);
        assert_eq!(p.chunk_z, *z);
        assert_eq!(p.sections.len(), OVERWORLD_SECTIONS);
        let again = p
            .encode()
            .unwrap_or_else(|e| panic!("chunk ({x}, {z}): encode: {e}"));
        assert!(
            again == *body,
            "chunk ({x}, {z}): re-encoded {} bytes against {} captured; first difference at {:?}",
            again.len(),
            body.len(),
            again.iter().zip(body).position(|(a, b)| a != b)
        );
    }
    eprintln!("round-tripped {} captured chunk packets", chunks.len());
}

/// Unpacking and repacking a container's cells is lossless.
///
/// The round-trip above carries the packed longs through untouched, so on its
/// own it would pass even if the bit layout were misunderstood. This goes
/// through the values: every container in the capture is unpacked to one
/// value per cell and repacked, and must give the same longs back.
#[test]
fn container_bit_packing_is_lossless() {
    let Some(chunks) = capture() else {
        eprintln!("SKIP golden_chunk_packet: set RUSTMC_CHUNK_CAPTURE");
        return;
    };
    let mut checked = 0u32;
    for (x, z, body) in &chunks {
        let p = LevelChunkWithLight::decode(body, OVERWORLD_SECTIONS).expect("decode");
        for (i, s) in p.sections.iter().enumerate() {
            for (c, kind) in [
                (&s.states, ContainerKind::BlockStates),
                (&s.biomes, ContainerKind::Biomes),
            ] {
                let values = c.values(kind);
                let rebuilt = rustmc_server::chunk::repack(&values, c, kind);
                assert_eq!(
                    rebuilt, c.data,
                    "chunk ({x}, {z}) section {i} {kind:?}: repacking {} bits lost data",
                    c.bits
                );
                checked += 1;
            }
        }
    }
    eprintln!("re-packed {checked} containers");
}

/// What the capture actually exercises, asserted rather than assumed.
///
/// It reaches `single` and `indirect` only — **no container in 117 chunks is
/// a direct palette**, so that regime is read from the bytecode and is not
/// covered by these bytes. This test exists to make that gap fail loudly if a
/// future capture does reach it, at which point the direct path stops being
/// unverified and this assertion should be updated deliberately.
#[test]
fn the_capture_covers_single_and_indirect_but_not_direct() {
    let Some(chunks) = capture() else {
        eprintln!("SKIP golden_chunk_packet: set RUSTMC_CHUNK_CAPTURE");
        return;
    };
    let (mut single, mut indirect, mut direct) = (0u32, 0u32, 0u32);
    let mut block_bits = std::collections::BTreeSet::new();
    let mut biome_bits = std::collections::BTreeSet::new();
    for (_, _, body) in &chunks {
        let p = LevelChunkWithLight::decode(body, OVERWORLD_SECTIONS).expect("decode");
        for s in &p.sections {
            for (c, is_block) in [(&s.states, true), (&s.biomes, false)] {
                match c.palette {
                    Palette::Single(_) => single += 1,
                    Palette::Indirect(_) => indirect += 1,
                    Palette::Direct => direct += 1,
                }
                if c.bits > 0 {
                    if is_block {
                        block_bits.insert(c.bits);
                    } else {
                        biome_bits.insert(c.bits);
                    }
                }
            }
        }
    }
    eprintln!("single {single}, indirect {indirect}, direct {direct}");
    eprintln!("block bits seen {block_bits:?}, biome bits seen {biome_bits:?}");
    assert!(single > 0 && indirect > 0, "capture should reach both");
    assert_eq!(
        direct, 0,
        "a direct palette appeared — the unverified path is now reachable"
    );
    // `Strategy$1` floors an indirect block palette at four bits, so a block
    // container below that would mean the threshold table is wrong.
    assert!(
        block_bits.iter().all(|&b| (4..=8).contains(&b)),
        "block bits outside 4..=8: {block_bits:?}"
    );
    assert!(
        biome_bits.iter().all(|&b| (1..=3).contains(&b)),
        "biome bits outside 1..=3: {biome_bits:?}"
    );
}
