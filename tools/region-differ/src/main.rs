//! region-differ — bit-compares two Anvil region directories, full block
//! state + biomes. rustmc's in-repo replacement for the Minecom-side diff
//! script (ORACLE.md top block); G3's measuring instrument.
//!
//!   region-differ scan <dir>
//!       full-chunk count + chunk bbox (the gen script polls this)
//!   region-differ diff <dir-a> <dir-b> [--bbox x1 z1 x2 z2] [--json]
//!       exactness over full chunks present in both (optionally clipped to
//!       a chunk-coordinate bbox); prints top mismatching chunks
//!
//! Exactness format matches the inherited ratchet's: percentage, 6dp.
//!
//! # Why a disagreement carries a y-range
//!
//! A confusion matrix says *which* blocks differ. It does not say *where*,
//! and those answer different questions: 600 blocks of one material swapped
//! for another are a feature that landed in the wrong place if they sit in a
//! twenty-block band, and a rule evaluated wrongly if they run the height of
//! the world. `golden_surface_chunk` needed both to find its one real port
//! bug — the matrix said "terrain, not surface materials" and the histogram
//! said "spread through bands no surface rule reaches" — and the whole-world
//! diff was reporting only the first.
//!
//! So every pair now carries the y-range it occurs in and its fullest band.
//! Cheap to compute, and it turns "628 blocks of sulfur and cinnabar
//! swapped" into a question with one answer instead of three.
//!
//! # `--twin`, and why a raw mismatch count overstates the defect
//!
//! Two vanilla worlds of one seed, generated the same way, do **not** agree.
//! Chunk generation is threaded, decoration order therefore varies between
//! runs, and every order-sensitive feature moves with it. On the r=7 window at
//! seed 20260722 that is 4,105 blocks between `vanilla-r7` and `vanilla-r7-b`
//! — a floor no port can go below, and one this tool used to charge to the
//! port anyway.
//!
//! `--twin <dir>` takes a second *vanilla* reference and counts only the
//! positions where `dir-a` and the twin **agree**. A position the two vanilla
//! runs spell differently is not evidence about us in either direction, so it
//! leaves the numerator and the denominator both. The first time it was run it
//! moved the overworld r=7 residual from 2,549 to 1,130.
//!
//! It is a *lower bound on the noise*, not the noise: two samples cannot find
//! every position a third order would move. A family that shuffles — seagrass
//! is the clearest — keeps a share of its scatter in the masked number, so a
//! masked count that is still dominated by one shuffling family is a reason to
//! harvest a third reference, not to go fix the feature.

mod becensus;
mod topdown;

use rustmc_world::anvil::{read_region, ChunkColumn};
use std::collections::BTreeMap;
use std::io;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("scan") if args.len() >= 2 => cmd_scan(&args),
        Some("census") if args.len() >= 2 => cmd_census(&args),
        Some("starts") if args.len() >= 2 => cmd_starts(&args),
        Some("diff") if args.len() >= 3 => cmd_diff(&args),
        Some("topdown") if args.len() >= 2 => cmd_topdown(&args),
        Some("becensus") if args.len() >= 2 => becensus::cmd_becensus(&args),
        Some("bediff") if args.len() >= 3 => becensus::cmd_bediff(&args),
        Some("bereach") if args.len() >= 3 => becensus::cmd_bereach(&args),
        _ => {
            eprintln!("usage: region-differ scan <dir> [--bbox x1 z1 x2 z2]");
            eprintln!(
                "       region-differ census <dir> [--bbox ...] [--ybox ylo yhi] [--match <substr>] [--states]"
            );
            eprintln!("       region-differ starts <dir> [--bbox x1 z1 x2 z2] [--match <substr>]");
            eprintln!("       region-differ diff <dir-a> <dir-b> [--bbox x1 z1 x2 z2] [--ybox ylo yhi] [--twin <dir>] [--positions] [--by-biome] [--json]");
            eprintln!("       region-differ topdown <dir> --out <file.png> [--bbox ...] [--ybox ylo yhi] [--scale n] [--mark <positions-file>]");
            eprintln!(
                "       region-differ becensus <dir> [--bbox x1 z1 x2 z2] [--fields] [--positions] [--match <id-substr>]"
            );
            eprintln!("       region-differ bediff <vanilla-dir> <ours-dir> [--bbox x1 z1 x2 z2] [--match <id-substr>] [--positions] [--ignore-key k]...");
            eprintln!("       region-differ bereach <vanilla-dir> <ours-dir> [--bbox x1 z1 x2 z2] [--positions]");
            std::process::exit(2);
        }
    };
    if let Err(e) = r {
        eprintln!("FAIL: {e}");
        std::process::exit(1);
    }
}

/// `--ybox <y-lo> <y-hi>`, inclusive, in world block y.
///
/// # Why a percentage without this can be nonsense
///
/// A chunk's *storage* range is not its *generated* range. `DimensionType`
/// gives the nether 256 blocks, its noise settings generate 128, and the top
/// eight sections of every nether chunk are guaranteed air on both sides — so
/// **half of a nether parity denominator is free**, and a nether figure and
/// an overworld figure taken over whole columns are not comparable numbers.
/// Rule 3 says name the region a count was taken over; a region has three
/// dimensions.
fn parse_ybox(args: &[String]) -> Option<(i32, i32)> {
    match ybox_from(args) {
        Ok(v) => v,
        Err(e) => fatal(&e),
    }
}

pub(crate) fn parse_bbox(args: &[String]) -> Option<(i32, i32, i32, i32)> {
    match bbox_from(args) {
        Ok(v) => v,
        Err(e) => fatal(&e),
    }
}

/// A flag that was given and could not be read is a refusal, never a default.
///
/// `AGENT-BRIEF`: an unrecognised knob is not a turned one. This is the
/// print-and-exit half, kept out of [`bbox_from`] and [`ybox_from`] so those
/// stay pure and testable — a `std::process::exit` inside the parser would
/// take the test harness with it.
fn fatal(msg: &str) -> ! {
    eprintln!("FAIL: {msg}");
    std::process::exit(2);
}

/// `Ok(None)` the flag is absent, `Ok(Some(..))` it parsed, `Err` it was given
/// and is malformed.
///
/// # THE SILENT UNBOUNDED WINDOW — found 2026-08-16 by making this target assert
///
/// This was `filter_map(|s| s.parse().ok())` behind a `v.len() == 4` test,
/// returning a bare `Option`, so **a malformed `--bbox` was indistinguishable
/// from no `--bbox` at all**. It is not a contrived input: the operands are
/// positional, so `--bbox 1 2 3 --match minecraft:sulfur` reads `--match` as
/// the fourth number, drops it, sees three integers and answers `None`.
/// `cmd_census` then runs over **every chunk in the directory**, which is the
/// failure `docs/AGENT-BRIEF.md` records as having pushed 35.6M blocks down an
/// ssh pipe and taken the box off the network — and a census that silently
/// widens its own window is also a census whose number answers a different
/// question from the one asked.
///
/// Three arguments too few does the same thing at the end of the argv, via
/// the `get(i + 1..i + 5)?` slice returning `None`.
fn bbox_from(args: &[String]) -> Result<Option<(i32, i32, i32, i32)>, String> {
    let Some(i) = args.iter().position(|s| s == "--bbox") else {
        return Ok(None);
    };
    let v = ints(args, i, 4, "--bbox", "x1 z1 x2 z2")?;
    Ok(Some((v[0], v[1], v[2], v[3])))
}

/// `--ybox <y-lo> <y-hi>`, inclusive, in world block y. Same contract as
/// [`bbox_from`], and the same fault: a `--ybox` that did not parse used to be
/// a whole-column denominator, which is exactly the comparison the doc comment
/// above says is not a comparison.
fn ybox_from(args: &[String]) -> Result<Option<(i32, i32)>, String> {
    let Some(i) = args.iter().position(|s| s == "--ybox") else {
        return Ok(None);
    };
    let v = ints(args, i, 2, "--ybox", "y-lo y-hi")?;
    Ok(Some((v[0], v[1])))
}

fn ints(args: &[String], at: usize, n: usize, flag: &str, shape: &str) -> Result<Vec<i32>, String> {
    let operands = args.get(at + 1..at + 1 + n).ok_or_else(|| {
        format!(
            "{flag} needs {n} integers ({shape}); only {} follow it",
            args.len() - at - 1
        )
    })?;
    let mut out = Vec::with_capacity(n);
    for s in operands {
        out.push(
            s.parse::<i32>()
                .map_err(|_| format!("{flag} needs {n} integers ({shape}); {s:?} is not one"))?,
        );
    }
    Ok(out)
}

/// Chunks keyed by world chunk coords; only Status=full survives loading
/// here — partially generated border chunks are not comparable state.
pub(crate) fn load_dir(dir: &Path) -> io::Result<BTreeMap<(i32, i32), ChunkColumn>> {
    let mut out = BTreeMap::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "mca"))
        .collect();
    entries.sort(); // deterministic load order
    for path in entries {
        // Zero-length region files appear transiently while the server is
        // live; treat as empty rather than erroring (scan polls mid-run).
        if std::fs::metadata(&path)?.len() == 0 {
            continue;
        }
        for chunk in read_region(&path)? {
            if chunk.status == "minecraft:full" {
                out.insert((chunk.x, chunk.z), chunk);
            }
        }
    }
    Ok(out)
}

fn cmd_scan(args: &[String]) -> io::Result<()> {
    let mut chunks = load_dir(Path::new(&args[1]))?;
    if let Some((x1, z1, x2, z2)) = parse_bbox(args) {
        chunks.retain(|&(x, z), _| x >= x1 && x <= x2 && z >= z1 && z <= z2);
    }
    if chunks.is_empty() {
        println!("SCAN full=0");
        return Ok(());
    }
    let xs: Vec<i32> = chunks.keys().map(|c| c.0).collect();
    let zs: Vec<i32> = chunks.keys().map(|c| c.1).collect();
    println!(
        "SCAN full={} bbox={},{} -> {},{}",
        chunks.len(),
        xs.iter().min().unwrap(),
        zs.iter().min().unwrap(),
        xs.iter().max().unwrap(),
        zs.iter().max().unwrap()
    );
    Ok(())
}

/// `starts <dir> [--bbox ...] [--match <substr>]` — every structure piece box
/// a reference wrote into its own chunk NBT, as
/// `PIECE <structure> <startChunkX>,<startChunkZ> <piece> x1 y1 z1 x2 y2 z2`.
///
/// The start chunk is in the line because a start's **union box** is what
/// vanilla's reference filter tests, and a union taken over two starts'
/// pieces at once is not any box vanilla ever computes.
///
/// # Why a reference's own NBT and not our placement
///
/// A beard is a density term computed from a structure's piece boxes, and the
/// question "how much terrain does the beard move" cannot be answered with
/// boxes this port derived: if the port's boxes were wrong the beard would be
/// wrong in the same direction and the comparison would agree with itself.
/// Vanilla writes its boxes into the region file, so the reference is also a
/// printout of the input the beardifier was given.
///
/// `crates/rustmc-world` already parses them — `golden_structure_starts` reads
/// 659 real starts through this same path — and nothing exposed them to a
/// command line.
fn cmd_starts(args: &[String]) -> io::Result<()> {
    let mut chunks = load_dir(Path::new(&args[1]))?;
    if let Some((x1, z1, x2, z2)) = parse_bbox(args) {
        chunks.retain(|&(x, z), _| x >= x1 && x <= x2 && z >= z1 && z <= z2);
    }
    let filter = args
        .iter()
        .position(|s| s == "--match")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let mut pieces = 0u64;
    let mut starts = 0u64;
    for chunk in chunks.values() {
        for start in &chunk.structure_starts {
            if filter
                .as_ref()
                .is_some_and(|f| !start.id.contains(f.as_str()))
            {
                continue;
            }
            starts += 1;
            for piece in &start.pieces {
                let b = piece.bounding_box;
                println!(
                    "PIECE {} {},{} {} {} {} {} {} {} {}",
                    start.id,
                    start.chunk_x,
                    start.chunk_z,
                    if piece.id.is_empty() { "-" } else { &piece.id },
                    b[0],
                    b[1],
                    b[2],
                    b[3],
                    b[4],
                    b[5]
                );
                pieces += 1;
            }
        }
    }
    println!("STARTS starts={starts} pieces={pieces}");
    Ok(())
}

/// How many of each block a region holds.
///
/// `diff` answers "where do two worlds disagree", which is only ever a lower
/// bound on a feature being wrong: a feature placed twice as often as it
/// should be still agrees with vanilla everywhere the extra copies happen to
/// land on the same block. The totals are what separate "displaced" from
/// "too many", and that distinction decides which half of a placement chain
/// to read.
///
/// Block **names** by default, not states — `sulfur_spike` has twelve states
/// and counting them apart hides the thing you are usually asking about.
/// `--states` keeps them separate when the properties are the question.
fn cmd_census(args: &[String]) -> io::Result<()> {
    let mut chunks = load_dir(Path::new(&args[1]))?;
    if let Some((x1, z1, x2, z2)) = parse_bbox(args) {
        chunks.retain(|&(x, z), _| x >= x1 && x <= x2 && z >= z1 && z <= z2);
    }
    let keep_states = args.iter().any(|s| s == "--states");
    let filter = args
        .iter()
        .position(|s| s == "--match")
        .and_then(|i| args.get(i + 1))
        .cloned();

    // `--positions` prints `x y z name` for every block the filter keeps.
    //
    // `diff --positions` answers "where is every mismatch"; this answers
    // "where is every block of this kind", which is what an attribution to a
    // *structure* needs. Asking whether a set of terrain disagreements sits
    // near a mineshaft means measuring the distance to the nearest mineshaft
    // block, and a census that only counts cannot say.
    //
    // Only useful with `--match`, since without one it would print the whole
    // region.
    let positions_out = args.iter().any(|s| s == "--positions");

    let ybox = parse_ybox(args);
    let in_y = |y: i32| ybox.is_none_or(|(lo, hi)| y >= lo && y <= hi);

    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut total = 0u64;
    for (&(chunk_x, chunk_z), chunk) in &chunks {
        for section in &chunk.sections {
            let section_base_y = i32::from(section.y) * 16;
            if ybox.is_some_and(|(lo, hi)| section_base_y + 15 < lo || section_base_y > hi) {
                continue;
            }
            for i in 0..4096usize {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_possible_wrap,
                    reason = "an index bounded by 4096"
                )]
                let world_y = section_base_y + (i / 256) as i32;
                if !in_y(world_y) {
                    continue;
                }
                let state = &section.palette[section.blocks.get(i).map_or(0, |&v| v as usize)];
                total += 1;
                let name = if keep_states {
                    state.as_str()
                } else {
                    state.split('[').next().unwrap_or(state)
                };
                if let Some(want) = &filter {
                    if !name.contains(want.as_str()) {
                        continue;
                    }
                }
                if positions_out {
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_possible_wrap,
                        reason = "an index bounded by 4096"
                    )]
                    let (bx, by, bz) = (
                        chunk_x * 16 + (i % 16) as i32,
                        section_base_y + (i / 256) as i32,
                        chunk_z * 16 + ((i / 16) % 16) as i32,
                    );
                    println!("{bx} {by} {bz} {name}");
                }
                *counts.entry(name.to_string()).or_insert(0) += 1;
            }
        }
    }

    let mut rows: Vec<(&String, &u64)> = counts.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!("CENSUS chunks={} blocks={total}", chunks.len());
    for (name, n) in rows {
        println!("{n:>10}  {name}");
    }
    Ok(())
}

/// A top-down PNG of one region directory. See `topdown.rs` for why.
///
/// `--mark <file>` overlays positions in magenta; the file is whitespace
/// separated `x y z ...` per line, which is exactly what `diff --positions`
/// and `census --positions` already emit, so a disagreement list can be
/// dropped straight onto the map it came from.
fn cmd_topdown(args: &[String]) -> io::Result<()> {
    let mut chunks = load_dir(Path::new(&args[1]))?;
    if let Some((x1, z1, x2, z2)) = parse_bbox(args) {
        chunks.retain(|&(x, z), _| x >= x1 && x <= x2 && z >= z1 && z <= z2);
    }
    if chunks.is_empty() {
        return Err(io::Error::other(
            "no full chunks in range — REFUSING to write an empty picture",
        ));
    }
    let out = args
        .iter()
        .position(|s| s == "--out")
        .and_then(|i| args.get(i + 1))
        .ok_or_else(|| io::Error::other("topdown needs --out <file.png>"))?;
    let scale = args
        .iter()
        .position(|s| s == "--scale")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, 16);
    let mut marks: Vec<(i32, i32)> = Vec::new();
    if let Some(f) = args
        .iter()
        .position(|s| s == "--mark")
        .and_then(|i| args.get(i + 1))
    {
        let text = std::fs::read_to_string(f)?;
        for line in text.lines() {
            let mut it = line.split_whitespace();
            if let (Some(x), Some(_y), Some(z)) = (it.next(), it.next(), it.next()) {
                if let (Ok(x), Ok(z)) = (x.parse::<i32>(), z.parse::<i32>()) {
                    marks.push((x, z));
                }
            }
        }
    }
    let (w, h) = topdown::render(&chunks, parse_ybox(args), &marks, scale, out)?;
    println!(
        "TOPDOWN {out} {w}x{h} chunks={} marks={}",
        chunks.len(),
        marks.len()
    );
    Ok(())
}

fn cmd_diff(args: &[String]) -> io::Result<()> {
    let a = load_dir(Path::new(&args[1]))?;
    let b = load_dir(Path::new(&args[2]))?;
    // `--twin <dir>` — a second reference for `dir-a`. See the module header.
    let twin = match args.iter().position(|s| s == "--twin") {
        Some(i) => Some(load_dir(Path::new(args.get(i + 1).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "--twin needs a directory")
        })?))?),
        None => None,
    };
    let json_out = args.iter().any(|s| s == "--json");
    // `--positions` prints every mismatch as `x y z vanilla ours`, one per
    // line. The confusion table answers "what is wrong"; this answers "where
    // is every one of them", which is what any attribution — to a structure,
    // a biome, a chunk — has to start from. The examples in that table are
    // capped at four and were never meant to carry that weight.
    let positions_out = args.iter().any(|s| s == "--positions");
    // `--by-biome` decomposes both the numerator and the denominator by the
    // biome the FIRST directory — vanilla — records at each position.
    //
    // # Why an aggregate nether number is close to meaningless without this
    //
    // docs/PARITY.md §5 measures 8,472 mismatches in one r=7 nether window
    // and 39,941 in another, and the second is not worse: it is a different
    // biome. `basalt_deltas` decoration moves 95,240 positions between two
    // *vanilla* runs where soul sand valley moves 48 — three orders of
    // magnitude — so a window's aggregate exactness is mostly a statement
    // about its biome mix, and two windows' aggregates are not comparable
    // numbers at all. Charging every position to the biome it is in makes
    // them comparable, and it is the only form in which a nether figure
    // transfers to a window somebody else chose.
    //
    // Vanilla's biome, not ours, and deliberately: attributing our residual
    // to our own biome grid would score a biome we placed wrongly as though
    // the blocks belonged there. The biome layer is exact (§5) so the two
    // agree today, and `biome_mismatches` in the same output is what would
    // say if that stopped being true.
    let by_biome_out = args.iter().any(|s| s == "--by-biome");
    let bbox = parse_bbox(args);
    let ybox = parse_ybox(args);
    let in_y = |y: i32| ybox.is_none_or(|(lo, hi)| y >= lo && y <= hi);
    let inside = |&(x, z): &(i32, i32)| {
        bbox.is_none_or(|(x1, z1, x2, z2)| x >= x1 && x <= x2 && z >= z1 && z <= z2)
    };

    let keys: Vec<(i32, i32)> = a
        .keys()
        .filter(|k| b.contains_key(*k))
        .filter(|k| twin.as_ref().is_none_or(|t| t.contains_key(*k)))
        .filter(|k| inside(k))
        .copied()
        .collect();
    // How much the twin took off the table, reported rather than hidden: a
    // mask that removes most of the window is a mask nobody should trust.
    let mut masked_out = 0u64;

    let (mut total_blocks, mut block_miss) = (0u64, 0u64);
    let (mut total_biomes, mut biome_miss) = (0u64, 0u64);
    let mut per_chunk: Vec<((i32, i32), u64)> = Vec::new();
    // Every (vanilla, rustmc) pair that disagreed, how often, and where.
    let mut pairs: std::collections::BTreeMap<(String, String), PairStat> =
        std::collections::BTreeMap::new();
    // Every mismatch, by 16-block band, whatever the pair.
    let mut bands: std::collections::BTreeMap<i32, u64> = std::collections::BTreeMap::new();
    // (blocks compared, mismatches) per vanilla biome, for `--by-biome`.
    let mut per_biome: std::collections::BTreeMap<String, (u64, u64)> =
        std::collections::BTreeMap::new();

    for key in &keys {
        let (ca, cb) = (&a[key], &b[key]);
        let ct = twin.as_ref().map(|t| &t[key]);
        let (chunk_x, chunk_z) = *key;
        let mut miss_here = 0u64;

        // Compare the union of section Ys; a section absent on one side
        // counts every position as a mismatch (structural difference).
        let ys: Vec<i8> = {
            let mut v: Vec<i8> = ca
                .sections
                .iter()
                .chain(&cb.sections)
                .map(|s| s.y)
                .collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        for y in ys {
            let section_base_y = i32::from(y) * 16;
            // A section entirely outside the y window contributes nothing —
            // not to the numerator and not to the denominator.
            if ybox.is_some_and(|(lo, hi)| section_base_y + 15 < lo || section_base_y > hi) {
                continue;
            }
            let sa = ca.sections.iter().find(|s| s.y == y);
            let sb = cb.sections.iter().find(|s| s.y == y);
            // A section the twin disagrees about the *existence* of is
            // masked whole. There is no per-position answer to give when the
            // two vanilla runs do not even agree that the section is there.
            let st = ct.map(|c| c.sections.iter().find(|s| s.y == y));
            if let Some(st) = st {
                if st.is_some() != sa.is_some() {
                    continue;
                }
            }
            let st = st.flatten();
            match (sa, sb) {
                (Some(sa), Some(sb)) => {
                    for i in 0..4096usize {
                        #[expect(
                            clippy::cast_possible_truncation,
                            clippy::cast_possible_wrap,
                            reason = "an index bounded by 4096"
                        )]
                        let row_y = section_base_y + (i / 256) as i32;
                        if !in_y(row_y) {
                            continue;
                        }
                        let va = &sa.palette[sa.blocks.get(i).map_or(0, |&v| v as usize)];
                        let vb = &sb.palette[sb.blocks.get(i).map_or(0, |&v| v as usize)];
                        // The mask, and it leaves the denominator too: a
                        // position vanilla spells two ways is not a position
                        // we can be right or wrong at.
                        if let Some(st) = st {
                            let vt = &st.palette[st.blocks.get(i).map_or(0, |&v| v as usize)];
                            if vt != va {
                                masked_out += 1;
                                continue;
                            }
                        }
                        total_blocks += 1;
                        // Anvil stores biomes as 4x4x4 cells, 64 to a
                        // section, indexed y-major then z then x — the same
                        // axis order as the block array one scale coarser.
                        // Derived from the index arithmetic the biome loop
                        // below already uses (`i / 16` is its y cell), not
                        // assumed.
                        let biome_of = |i: usize| -> &str {
                            let cell = (i / 1024) * 16 + ((i / 64) % 4) * 4 + (i % 16) / 4;
                            sa.biome_palette
                                .get(sa.biomes.get(cell).map_or(0, |&v| v as usize))
                                .map_or("<none>", String::as_str)
                        };
                        if by_biome_out {
                            per_biome.entry(biome_of(i).to_owned()).or_default().0 += 1;
                        }
                        if va != vb {
                            block_miss += 1;
                            miss_here += 1;
                            if by_biome_out {
                                per_biome.entry(biome_of(i).to_owned()).or_default().1 += 1;
                            }
                            // Anvil packs a section y-major, so the index's
                            // high bits are the height within it.
                            #[expect(
                                clippy::cast_possible_truncation,
                                clippy::cast_possible_wrap,
                                reason = "an index bounded by 4096"
                            )]
                            let world_y = section_base_y + (i / 256) as i32;
                            *bands.entry(section_base_y).or_insert(0) += 1;
                            // What disagreed, not just how much. A bare
                            // percentage says a generator is 98% right; this
                            // says *which* feature is missing, which is the
                            // difference between a number and a work list.
                            if positions_out {
                                // `x y z vanilla ours`, and with `--by-biome`
                                // a sixth field: the biome vanilla records
                                // there. Appended rather than inserted so
                                // every existing reader — all of which take
                                // the first three fields — is unaffected.
                                // Without it a positional attribution and a
                                // biome decomposition are two tables nobody
                                // can join, and the join is the whole
                                // question: whether a lump sits on a
                                // structure or on the biome whose floor
                                // swallows it.
                                if by_biome_out {
                                    println!(
                                        "{} {} {} {va} {vb} {}",
                                        chunk_x * 16 + (i % 16) as i32,
                                        world_y,
                                        chunk_z * 16 + ((i / 16) % 16) as i32,
                                        biome_of(i)
                                    );
                                } else {
                                    println!(
                                        "{} {} {} {va} {vb}",
                                        chunk_x * 16 + (i % 16) as i32,
                                        world_y,
                                        chunk_z * 16 + ((i / 16) % 16) as i32
                                    );
                                }
                            }
                            pairs.entry((va.clone(), vb.clone())).or_default().record(
                                chunk_x * 16 + (i % 16) as i32,
                                world_y,
                                chunk_z * 16 + ((i / 16) % 16) as i32,
                            );
                        }
                    }
                    for i in 0..64usize {
                        // A biome cell is 4 blocks tall, so a y window that
                        // cuts a section keeps only the cells inside it.
                        #[expect(
                            clippy::cast_possible_truncation,
                            clippy::cast_possible_wrap,
                            reason = "an index bounded by 64"
                        )]
                        let cell_y = section_base_y + (i / 16) as i32 * 4;
                        if !in_y(cell_y) || !in_y(cell_y + 3) {
                            continue;
                        }
                        total_biomes += 1;
                        let ba = sa
                            .biome_palette
                            .get(sa.biomes.get(i).map_or(0, |&v| v as usize));
                        let bb = sb
                            .biome_palette
                            .get(sb.biomes.get(i).map_or(0, |&v| v as usize));
                        if ba != bb {
                            biome_miss += 1;
                        }
                    }
                }
                _ => {
                    // A section one side has and the other does not. Counted
                    // as a whole-section miss rather than skipped, because a
                    // missing section is the loudest possible disagreement.
                    let base = section_base_y;
                    let rows: i32 = (0..16)
                        .filter(|dy| in_y(base + dy))
                        .count()
                        .try_into()
                        .unwrap_or(16);
                    let missed = u64::try_from(rows).unwrap_or(16) * 256;
                    total_blocks += missed;
                    block_miss += missed;
                    miss_here += missed;
                    *bands.entry(base).or_insert(0) += missed;
                    let stat = pairs
                        .entry((
                            "<section present>".to_string(),
                            "<section absent>".to_string(),
                        ))
                        .or_default();
                    for dy in 0..16 {
                        if !in_y(base + dy) {
                            continue;
                        }
                        for _ in 0..256 {
                            stat.record(chunk_x * 16, base + dy, chunk_z * 16);
                        }
                    }
                }
            }
        }
        if miss_here > 0 {
            per_chunk.push((*key, miss_here));
        }
    }

    let exact = if total_blocks == 0 {
        0.0
    } else {
        (total_blocks - block_miss) as f64 / total_blocks as f64 * 100.0
    };
    per_chunk.sort_by_key(|&(_, m)| std::cmp::Reverse(m));
    // Biggest share of the residual first, not biggest biome first: the
    // question this table answers is where the residual lives, and a biome
    // that is 90% of the window and 0% of the residual is the useful null.
    let mut by_biome: Vec<(String, (u64, u64))> = per_biome.into_iter().collect();
    by_biome.sort_by(|x, y| y.1 .1.cmp(&x.1 .1).then_with(|| y.1 .0.cmp(&x.1 .0)));
    let mut by_pair: Vec<((String, String), PairStat)> = pairs.into_iter().collect();
    by_pair.sort_by(|a, b| b.1.count.cmp(&a.1.count).then_with(|| a.0.cmp(&b.0)));

    if json_out {
        let offenders: Vec<serde_json::Value> = per_chunk
            .iter()
            .take(10)
            .map(|((x, z), m)| serde_json::json!({"chunk": [x, z], "mismatches": m}))
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "chunks_compared": keys.len(),
                "full_a": a.len(), "full_b": b.len(),
                "blocks": total_blocks, "block_mismatches": block_miss,
                "biome_cells": total_biomes, "biome_mismatches": biome_miss,
                "exactness_pct": format!("{exact:.6}"),
                "offenders": offenders,
                // Echoed so a reader can tell which way round the call was.
                // Getting this wrong silently inverts every pair while leaving
                // the percentage correct.
                "first_dir": args[1],
                "second_dir": args[2],
                "by_block": by_pair
                    .iter()
                    // Thirty rows is a readable summary, not a complete one:
                    // over a whole-window diff they routinely account for
                    // four fifths of the mismatches and the tail is invisible.
                    // That matters when the question is "does this block
                    // appear *anywhere* in the diff" rather than "what is
                    // biggest" — a truncated table answers the second and
                    // gets quoted for the first.
                    //
                    // So the JSON emits **every** pair by default. The text
                    // output above stays capped because a human is reading
                    // it; a script is not, and a script that silently sees
                    // four fifths of the data is how a stage got called
                    // order-independent when it was not. `RUSTMC_DIFF_PAIRS`
                    // caps it for anyone who wants the old shape.
                    .take(
                        std::env::var("RUSTMC_DIFF_PAIRS")
                            .ok()
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(usize::MAX),
                    )
                    .map(|((va, vb), stat)| {
                        serde_json::json!({
                            // `va` is whatever directory was passed FIRST.
                            // The convention (WORKLIST) is vanilla first, and
                            // these names assume it — but the exactness figure
                            // is symmetric, so a reversed call produces a
                            // perfectly plausible number with every pair
                            // labelled backwards, which is undetectable from
                            // the output alone. `first_dir`/`second_dir` below
                            // make the orientation checkable.
                            "vanilla": va, "rustmc": vb, "count": stat.count,
                            "min_y": stat.min_y, "max_y": stat.max_y,
                            "busiest_band": stat.busiest_band(),
                            "examples": stat.examples,
                        })
                    })
                    .collect::<Vec<_>>(),
                "bands": bands
                    .iter()
                    .map(|(base, n)| serde_json::json!({"y": base, "mismatches": n}))
                    .collect::<Vec<_>>(),
                "by_biome": by_biome
                    .iter()
                    .map(|(name, (blocks, miss))| {
                        serde_json::json!({
                            "biome": name, "blocks": blocks, "mismatches": miss,
                            "exactness_pct": if *blocks == 0 { "0.000000".to_owned() } else {
                                format!("{:.6}", (blocks - miss) as f64 / *blocks as f64 * 100.0)
                            },
                            // What share of the WINDOW this biome is, beside
                            // what share of the RESIDUAL it carries. The two
                            // together are the finding; either alone is not.
                            "window_share_pct": if total_blocks == 0 { "0.000".to_owned() } else {
                                format!("{:.3}", *blocks as f64 / total_blocks as f64 * 100.0)
                            },
                            "residual_share_pct": if block_miss == 0 { "0.000".to_owned() } else {
                                format!("{:.3}", *miss as f64 / block_miss as f64 * 100.0)
                            },
                        })
                    })
                    .collect::<Vec<_>>(),
            })
        );
    } else {
        println!(
            "DIFF chunks={} (a={} b={})  blocks={total_blocks} mismatches={block_miss}  biomes={total_biomes}/{biome_miss}",
            keys.len(), a.len(), b.len()
        );
        if twin.is_some() {
            println!(
                "  TWIN MASK: {masked_out} positions dropped — the two vanilla runs spell them differently"
            );
        }
        if by_biome_out {
            println!("  BY BIOME (vanilla's biome at each position):");
            println!(
                "    {:<34} {:>12} {:>10} {:>11}  {:>7}  {:>7}",
                "biome", "blocks", "mismatch", "exactness", "win%", "res%"
            );
            for (name, (blocks, miss)) in &by_biome {
                let ex = if *blocks == 0 {
                    0.0
                } else {
                    (blocks - miss) as f64 / *blocks as f64 * 100.0
                };
                let ws = if total_blocks == 0 {
                    0.0
                } else {
                    *blocks as f64 / total_blocks as f64 * 100.0
                };
                let rs = if block_miss == 0 {
                    0.0
                } else {
                    *miss as f64 / block_miss as f64 * 100.0
                };
                println!(
                    "    {:<34} {blocks:>12} {miss:>10} {ex:>10.6}% {ws:>6.2}% {rs:>6.2}%",
                    name.trim_start_matches("minecraft:")
                );
            }
        }
        for ((x, z), m) in per_chunk.iter().take(10) {
            println!("  worst chunk ({x},{z}): {m} mismatched blocks");
        }
        if !by_pair.is_empty() {
            println!(
                "  worst disagreements ({} -> {}), with where they are:",
                args[1], args[2]
            );
            for ((va, vb), stat) in by_pair.iter().take(15) {
                let (band, in_band) = stat.busiest_band();
                println!(
                    "    {:7}  {va}  ->  {vb}   y {}..{}, {in_band} in y{}..{}",
                    stat.count,
                    stat.min_y,
                    stat.max_y,
                    band,
                    band + 15
                );
                let where_ = stat
                    .examples
                    .iter()
                    .map(|(x, y, z)| format!("({x},{y},{z})"))
                    .collect::<Vec<_>>()
                    .join(" ");
                println!("             at {where_}");
            }
        }
        if !bands.is_empty() {
            println!("  mismatches by 16-block band:");
            let peak = bands.values().copied().max().unwrap_or(1).max(1);
            for (base, n) in &bands {
                #[expect(clippy::cast_possible_truncation, reason = "a bar width bounded by 40")]
                let width = ((*n * 40) / peak) as usize;
                println!("    y{base:>5}  {n:7}  {}", "#".repeat(width));
            }
        }
        println!("EXACTNESS {exact:.6}%");
    }

    if keys.is_empty() {
        return Err(io::Error::other("no comparable full chunks"));
    }
    Ok(())
}

/// Where a (vanilla, rustmc) disagreement happens, not just how often.
///
/// A count alone cannot tell a feature that landed in the wrong place from a
/// rule evaluated wrongly; a y-range can. The busiest band is kept as well as
/// the extremes because a handful of outliers would otherwise make a tightly
/// clustered blob look like it spanned the world.
#[derive(Default)]
struct PairStat {
    count: usize,
    min_y: i32,
    max_y: i32,
    bands: BTreeMap<i32, usize>,
    /// A few world positions where this pair happens.
    ///
    /// Without these a whole-world diff says *what* is wrong and never
    /// *where*, so every investigation that needs a block to look at has to
    /// re-derive one. That gap cost this project a full session on
    /// `packed_ice`: the confusion table named the block and the y band, and
    /// finding one position to trace took a separate harness.
    examples: Vec<(i32, i32, i32)>,
}

impl PairStat {
    fn record(&mut self, x: i32, y: i32, z: i32) {
        if self.count == 0 {
            self.min_y = y;
            self.max_y = y;
        } else {
            self.min_y = self.min_y.min(y);
            self.max_y = self.max_y.max(y);
        }
        self.count += 1;
        if self.examples.len() < 4 {
            self.examples.push((x, y, z));
        }
        *self.bands.entry(y.div_euclid(16) * 16).or_insert(0) += 1;
    }

    /// The fullest 16-block band, and how much of the pair is in it.
    fn busiest_band(&self) -> (i32, usize) {
        self.bands
            .iter()
            .max_by_key(|(base, n)| (**n, std::cmp::Reverse(**base)))
            .map_or((0, 0), |(base, n)| (*base, *n))
    }
}

// ---------------------------------------------------------------------------

/// **This target executed zero assertions until 2026-08-16.**
///
/// `region-differ` is the instrument nearly every worldgen figure in
/// `docs/DETERMINISM.md` was taken with, its bin target is selected by
/// `ci/t0.sh`'s `--lib --bins`, and it reported `test result: ok. 0 passed`
/// on every gate this project has ever run — the limiting case of a check
/// that cannot fail, inside the tool the numbers come from.
///
/// The window parsers are what is tested here because a window is a
/// *condition of a measurement*: `docs/AGENT-BRIEF.md` rule 3 says name the
/// region a count was taken over, and a `--bbox` that silently does not apply
/// is a count over a different region than the one in the commit message.
#[cfg(test)]
mod tests {
    use super::{bbox_from, ybox_from};

    fn argv(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn a_well_formed_bbox_parses_in_argv_order() {
        assert_eq!(
            bbox_from(&argv(
                "census dir --bbox -4 -8 12 16 --match minecraft:sulfur"
            )),
            Ok(Some((-4, -8, 12, 16)))
        );
    }

    #[test]
    fn no_bbox_is_no_window_and_that_is_not_an_error() {
        assert_eq!(
            bbox_from(&argv("census dir --match minecraft:sulfur")),
            Ok(None)
        );
    }

    /// The one that was silent. `--bbox 1 2 3 --match ...` used to answer
    /// `None`, which `cmd_census` reads as "no window" and runs over the whole
    /// directory.
    #[test]
    fn a_bbox_that_swallows_the_next_flag_is_refused_not_ignored() {
        let got = bbox_from(&argv("census dir --bbox 1 2 3 --match minecraft:sulfur"));
        assert!(
            got.is_err(),
            "a malformed --bbox must not read as an absent one: {got:?}"
        );
        assert!(
            got.unwrap_err().contains("--match"),
            "the refusal must name the token it choked on"
        );
    }

    #[test]
    fn a_bbox_running_off_the_end_of_argv_is_refused() {
        assert!(bbox_from(&argv("census dir --bbox 1 2 3")).is_err());
        assert!(bbox_from(&argv("census dir --bbox")).is_err());
    }

    /// Negative chunk coordinates are half of every centred window this
    /// project measures, and `-4` is a token a looser parser drops.
    #[test]
    fn negative_coordinates_survive_the_parse() {
        assert_eq!(
            bbox_from(&argv("diff a b --bbox -16 -16 -1 -1")),
            Ok(Some((-16, -16, -1, -1)))
        );
        assert_eq!(
            ybox_from(&argv("diff a b --ybox -64 319")),
            Ok(Some((-64, 319)))
        );
    }

    #[test]
    fn a_malformed_ybox_is_refused_rather_than_becoming_a_whole_column() {
        assert_eq!(ybox_from(&argv("diff a b")), Ok(None));
        assert_eq!(
            ybox_from(&argv("diff a b --ybox 0 255")),
            Ok(Some((0, 255)))
        );
        assert!(ybox_from(&argv("diff a b --ybox 0 --positions")).is_err());
    }
}
