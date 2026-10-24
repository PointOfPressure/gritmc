//! anvil-probe — read one block out of a region directory, or write one into
//! it.
//!
//! This exists to make `ci/t2-vanilla-loads-ours.sh` possible: that gate has
//! to put a block into a region file **through our writer** and then ask a
//! real vanilla server what it sees there. Both halves need a way to talk
//! about a single position on disk, and the differ is a read-only measuring
//! instrument that should stay one.
//!
//!   get  <region-dir> <x> <y> <z>          print the canonical state, or ABSENT
//!   poke <region-dir> <x> <y> <z> <state>  set it, merging into the region
//!
//! `poke` goes through [`rustmc_world::anvil::write_region_merging`], so it
//! exercises the same path a server flush does — including the part that has
//! to leave the region's other 1,023 chunks alone.

use rustmc_world::anvil;
use std::path::Path;
use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  \
         anvil-probe get  <region-dir> <x> <y> <z>\n  \
         anvil-probe poke <region-dir> <x> <y> <z> <block-state>"
    );
    ExitCode::from(2)
}

/// The chunk a block position falls in: 16 blocks to a chunk, **arithmetic**
/// shift.
///
/// `>> 4` and not `/ 16`, and the difference is the whole reason this is a
/// named function rather than an expression inside [`load`]: division in Rust
/// truncates toward zero, so `-1 / 16` is `0` and `-1 >> 4` is `-1`. Every
/// window this project measures is centred, so half of every one of them is
/// at negative coordinates, and the wrong operator here reads and writes the
/// wrong chunk only on the negative side — which looks like a defect in
/// whatever wrote the world.
fn chunk_of(x: i32, z: i32) -> (i32, i32) {
    (x >> 4, z >> 4)
}

/// The Anvil region file holding a chunk: 32x32 chunks to a region, same
/// arithmetic-shift argument as [`chunk_of`].
fn region_file(cx: i32, cz: i32) -> String {
    format!("r.{}.{}.mca", cx >> 5, cz >> 5)
}

/// Load the one region file covering a chunk, and find the column in it.
///
/// Returns the whole region's columns alongside the index of the one asked
/// for, because `poke` has to hand the column back to the writer.
fn load(region_dir: &Path, x: i32, z: i32) -> Result<(Vec<anvil::ChunkColumn>, usize), String> {
    let (cx, cz) = chunk_of(x, z);
    let path = region_dir.join(region_file(cx, cz));
    let chunks = anvil::read_region(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let at = chunks
        .iter()
        .position(|c| c.x == cx && c.z == cz)
        .ok_or_else(|| format!("chunk ({cx}, {cz}) is not present in {}", path.display()))?;
    Ok((chunks, at))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else {
        return usage();
    };
    let want = if cmd == "poke" { 6 } else { 5 };
    if args.len() != want {
        return usage();
    }
    let dir = Path::new(&args[1]);
    let coord = |i: usize| {
        args[i]
            .parse::<i32>()
            .map_err(|e| format!("{}: {e}", args[i]))
    };
    let (x, y, z) = match (coord(2), coord(3), coord(4)) {
        (Ok(x), Ok(y), Ok(z)) => (x, y, z),
        _ => return usage(),
    };

    match cmd.as_str() {
        "get" => match load(dir, x, z) {
            Ok((chunks, at)) => {
                // ABSENT rather than an error: "this column has no section
                // covering that y" is a real answer about the file, and a
                // caller choosing a probe position needs to be able to see it
                // rather than have it look like a broken read.
                println!("{}", chunks[at].block_at(x, y, z).unwrap_or("ABSENT"));
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("anvil-probe: {e}");
                ExitCode::FAILURE
            }
        },
        "poke" => {
            let state = &args[5];
            match load(dir, x, z) {
                Ok((mut chunks, at)) => {
                    if !chunks[at].set_block_at(x, y, z, state) {
                        // Refuse rather than write. A poke that changed
                        // nothing would still produce a rewritten region file,
                        // and a gate built on it would pass whether or not the
                        // writer works — the block it asserts on was already
                        // there.
                        eprintln!(
                            "anvil-probe: ({x}, {y}, {z}) is already {} — poking it would \
                             make the write unfalsifiable",
                            chunks[at].block_at(x, y, z).unwrap_or("ABSENT")
                        );
                        return ExitCode::FAILURE;
                    }
                    // Only the touched column is handed over; every other
                    // chunk in the region has to survive by being left alone.
                    let one = chunks.swap_remove(at);
                    match anvil::write_region_merging(dir, std::slice::from_ref(&one)) {
                        Ok(()) => {
                            println!(
                                "poked ({x}, {y}, {z}) = {state} in chunk ({}, {})",
                                one.x, one.z
                            );
                            ExitCode::SUCCESS
                        }
                        Err(e) => {
                            eprintln!("anvil-probe: write failed: {e}");
                            ExitCode::FAILURE
                        }
                    }
                }
                Err(e) => {
                    eprintln!("anvil-probe: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => usage(),
    }
}

// ---------------------------------------------------------------------------

/// **This target executed zero assertions until 2026-08-16.**
///
/// `anvil-probe poke` writes a block into a real region file that a real
/// vanilla server is then asked to read (`ci/t2-vanilla-loads-ours.sh`). If it
/// addresses the wrong chunk, the gate's subject and its question are two
/// different positions and the verdict is about neither.
#[cfg(test)]
mod tests {
    use super::{chunk_of, region_file};

    /// The boundary either side of zero, where `/` and `>>` disagree.
    #[test]
    fn block_to_chunk_floors_rather_than_truncating() {
        assert_eq!(chunk_of(0, 0), (0, 0));
        assert_eq!(chunk_of(15, 15), (0, 0));
        assert_eq!(chunk_of(16, 16), (1, 1));
        assert_eq!(
            chunk_of(-1, -1),
            (-1, -1),
            "-1/16 is 0 and would be the wrong chunk"
        );
        assert_eq!(chunk_of(-16, -16), (-1, -1));
        assert_eq!(chunk_of(-17, -17), (-2, -2));
    }

    /// 32x32 chunks to a region, and the same floor/truncate split one power
    /// of two up.
    #[test]
    fn chunk_to_region_file_floors_rather_than_truncating() {
        assert_eq!(region_file(0, 0), "r.0.0.mca");
        assert_eq!(region_file(31, 31), "r.0.0.mca");
        assert_eq!(region_file(32, 32), "r.1.1.mca");
        assert_eq!(region_file(-1, -1), "r.-1.-1.mca");
        assert_eq!(region_file(-32, -32), "r.-1.-1.mca");
        assert_eq!(region_file(-33, -33), "r.-2.-2.mca");
    }

    /// The composition is what `load` actually does, and it is the one a
    /// caller reasons about: a block just west of the origin lives in
    /// `r.-1.-1.mca`, not in `r.0.0.mca`.
    #[test]
    fn a_block_just_west_of_the_origin_is_in_the_negative_region() {
        let (cx, cz) = chunk_of(-1, -1);
        assert_eq!(region_file(cx, cz), "r.-1.-1.mca");
        let (cx, cz) = chunk_of(1, 1);
        assert_eq!(region_file(cx, cz), "r.0.0.mca");
    }
}
