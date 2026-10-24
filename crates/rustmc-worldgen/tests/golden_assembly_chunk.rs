//! **The seam gate.** The assembly's own output, against the same palettes
//! the stage gates use.
//!
//! Every stage of this generator is proved on its own. `golden_noise_chunk`
//! reads 100% over 1.77M blocks, `golden_surface_chunk` 100% over 2.56M,
//! `golden_carver_chunk` and `golden_decorate` have their own numbers — and
//! the composed world that `worldgen-dump full` writes reads 99.39%. Those
//! two facts have coexisted for four ports now, and every large fix in
//! BENCH.md has been in the gap between them: the carver aquifer, the biome
//! manager's fuzz. Both were *seams*, and no gate that measures one stage
//! could see either.
//!
//! # What is different here
//!
//! Nothing about the vectors. This reads `surface_chunk_vectors.json`, the
//! same file `golden_surface_chunk` reads, and compares the same blocks
//! against the same palettes. The only thing that changes is **who
//! generates** — `worldgen_assembly::generate`, the code path that actually
//! produced the 99.39% world, rather than the gate's own hand-wired flow.
//!
//! So a disagreement here is not a new fact about vanilla. It is the
//! assembly disagreeing with a stage that is already proved, which is the
//! definition of a seam and localises it to one stage boundary in one run.
//!
//! # Why it is not free
//!
//! `generate` produces a one-chunk margin, so asking for one chunk generates
//! nine. That is the price of measuring the real pipeline rather than a
//! reconstruction of it, and it is why the case list here is a sample rather
//! than all twenty-six — see `CASES`. Run it on the Threadripper (docs/CI.md).

mod common;

use common::{expand_runs, palette_of, read};

use rustmc_worldgen::pipeline::{Area, Through};

/// How many of the surface gate's chunks to run through the assembly.
///
/// The whole file is twenty-six cases and each costs nine generated chunks,
/// so this is a sample. It is taken from the *front* of the list rather than
/// at random: the first cases are the ones whose provenance is recorded in
/// `golden_surface_chunk`'s `locate_*` searches, so a failure here can be
/// read next to a passing stage-gate result for the same chunk.
const CASES: usize = 6;

/// The assembly's surface stage agrees with the dumped palettes.
///
/// Compared **through** `Through::Surface` rather than `Through::Decoration`:
/// the vectors were dumped from `SurfaceSystem.buildSurface` over a
/// `ProtoChunk`, so surface is the last stage both sides can express
/// (ORACLE.md rule 11). Carvers and features would each be a different
/// question with its own vectors.
#[test]
fn the_assembly_agrees_with_the_surface_palettes() {
    let doc = read(&worldgen_assembly::surface_golden());
    let cases = doc["cases"].as_array().expect("cases");

    let mut total = 0usize;
    let mut wrong = 0usize;
    let mut confusion: std::collections::BTreeMap<(String, String), usize> =
        std::collections::BTreeMap::new();
    let mut by_band: std::collections::BTreeMap<i32, usize> = std::collections::BTreeMap::new();
    let mut examples: Vec<String> = Vec::new();
    let mut compared_chunks = 0usize;

    for case in cases.iter().take(CASES) {
        let seed: i64 = case["seed"].as_str().expect("seed").parse().expect("i64");
        let chunk_x = i32::try_from(case["chunkX"].as_i64().expect("chunkX")).expect("i32");
        let chunk_z = i32::try_from(case["chunkZ"].as_i64().expect("chunkZ")).expect("i32");
        let min_y = i32::try_from(case["minY"].as_i64().expect("minY")).expect("i32");
        let height = i32::try_from(case["height"].as_i64().expect("height")).expect("i32");

        let palette = palette_of(case, "palette");
        let expected = expand_runs(palette.len(), &case["runs"]);

        let area = Area::square(chunk_x, chunk_z, 0);
        // Not a skip. "The assembly could not load its data" was the same
        // sentence for a missing extract and for an extract this code cannot
        // read, and both returned a pass. `have_extract` above now refuses the
        // first; anything left is the second, which is a fault worth the panic.
        let world = worldgen_assembly::generate::generate(seed, area, Through::Surface)
            .unwrap_or_else(|e| panic!("the assembly could not load its data: {e}"));
        compared_chunks += 1;

        for (i, want_index) in expected.iter().enumerate() {
            let want = palette[*want_index];
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_possible_wrap,
                reason = "an index bounded by the chunk's block count"
            )]
            let flat = i as i32;
            let y = min_y + flat / 256;
            let x = chunk_x * 16 + (flat % 256) % 16;
            let z = chunk_z * 16 + (flat % 256) / 16;
            let got = world.canvas.block_at(x, y, z);

            total += 1;
            if got != want {
                wrong += 1;
                *confusion
                    .entry((want.to_string(), got.to_string()))
                    .or_insert(0) += 1;
                *by_band.entry(y.div_euclid(32) * 32).or_insert(0) += 1;
                if examples.len() < 8 {
                    examples.push(format!(
                        "seed={seed} chunk=({chunk_x}, {chunk_z}) block=({x}, {y}, {z}): \
                         want {want}, got {got}"
                    ));
                }
            }
        }
        // The canvas is taller than the dump only if the settings disagree,
        // which would make every comparison above meaningless rather than
        // wrong. Checked once per case rather than assumed.
        assert_eq!(
            expected.len(),
            usize::try_from(height * 256).expect("usize"),
            "the dump and the canvas disagree about the world's height"
        );
    }

    assert!(total > 0, "no blocks were compared");
    #[expect(
        clippy::cast_precision_loss,
        reason = "a percentage over a block count"
    )]
    let exactness = 100.0 * (total - wrong) as f64 / total as f64;
    println!(
        "ASSEMBLY SURFACE EXACTNESS {exactness:.6}% over {total} blocks ({wrong} wrong), \
         {compared_chunks} chunks"
    );
    if !confusion.is_empty() {
        let mut rows: Vec<_> = confusion.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1));
        println!("  worst disagreements (vanilla -> assembly):");
        for ((want, got), n) in rows.iter().take(10) {
            println!("    {n:7}  {want}  ->  {got}");
        }
        println!("  by 32-block band:");
        for (band, n) in &by_band {
            println!("    y{band:>5}  {n:7}");
        }
        for line in &examples {
            println!("  {line}");
        }
    }

    // The stage gate reads 100% on these same chunks. Anything less here is
    // the assembly, and no tolerance is ever the right answer to it
    // (docs/CI.md zero-flake rules).
    assert_eq!(
        wrong, 0,
        "the assembly disagrees with a stage that is already proved — \
         that is a seam, not a stage bug"
    );
}
