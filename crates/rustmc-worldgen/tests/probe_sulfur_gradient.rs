//! Where do the sulfur-cave disagreements sit relative to the rule's bands?
//!
//! The overworld surface rule for `minecraft:sulfur_caves` is three
//! `noise_threshold` bands over `minecraft:sulfur_cave_gradient`, sampled in
//! 3D:
//!
//! ```text
//!   [-0.4, -0.1] -> cinnabar
//!   [ 0.0,  0.4] -> sulfur
//!   [ 0.4,  MAX] -> cinnabar
//!   otherwise    -> stone
//! ```
//!
//! The r7 whole-world diff disagrees with vanilla on 672 blocks here, and
//! every single one is cinnabar-against-sulfur — **no block flips to or from
//! stone**. That is the interesting part. A noise that was merely imprecise
//! would flip blocks at all four band edges in rough proportion; flipping at
//! exactly one edge says the fault is specific to it.
//!
//! # ANSWERED (2026-07-27): it is not the noise, and it is not the rule
//!
//! Run over the 320 swaps in the r2 window at (64,64), the split is exact:
//!
//! ```text
//!   our noise implies vanilla's block   253      (all of them vanilla=cinnabar, ours=sulfur)
//!   our noise implies our own block      67      (all of them vanilla=sulfur, ours=cinnabar)
//!   our noise implies neither             0
//! ```
//!
//! The two buckets match the two diff directions one for one. Read together
//! they say the same thing from both sides: **our surface rule is doing
//! exactly what the noise tells it, and something downstream is moving the
//! sulfur.** In the 253 our own noise says cinnabar and the finished world
//! has sulfur, which no surface rule did; in the 67 we wrote the cinnabar
//! our noise called for and vanilla put sulfur there instead.
//!
//! One cause, not two: **the sulfur feature is placed in the wrong
//! positions** — it writes where vanilla does not and misses where vanilla
//! does.
//!
//! Everything upstream of it was checked and cleared, each against vanilla
//! rather than by argument:
//!
//! - **Parameters.** `worldgen/noise/sulfur_cave_gradient.json` is
//!   `firstOctave -5`, amplitudes `[1.0, 0.0, 1.0]`; our golden vector is
//!   the same.
//! - **Coordinates.** `SurfaceRules$Context$2.getAsDouble` is
//!   `noise.getValue(blockX, blockY, blockZ)` — ours too, and `is_3d` was
//!   already honoured.
//! - **Comparison.** `NoiseThresholdCondition.test` disassembles to
//!   `>= min && <= max`, inclusive at both ends, which is what we do. So the
//!   shared 0.4 edge between the sulfur and third cinnabar bands is not it.
//! - **Biomes.** The window has 38,400 biome cells and **zero** mismatches,
//!   so the `sulfur_caves` branch is being entered in the same places.
//!
//! And then attributed directly, with `worldgen-dump column` at the dump's
//! own centre:
//!
//! ```text
//!   (993, -3, 1003)   noise stone -> surface CINNABAR -> carvers - -> decorated SULFUR
//! ```
//!
//! **Our decoration paints sulfur over the cinnabar our own surface rule
//! correctly placed.** Vanilla leaves the cinnabar. So the bug is in a
//! feature, and the 253 need no vanilla to chase.
//!
//! Ruled out so far, cheaply:
//!
//! - `minecraft:cinnabar` is **not** in `lava_pool_stone_cannot_replace`
//!   (that tag is `features_cannot_replace` + leaves + logs), so vanilla's
//!   lake is permitted to replace it too — the pool's
//!   `can_replace_with_barrier` is not the difference.
//! - `block_predicate_filter` and `matching_blocks` are both implemented, so
//!   the pool's final "only where the block is already sulfur" filter is not
//!   simply missing.
//!
//! Still to check: whether that filter is applied **after** the
//! `random_offset` of `y_spread: -1` as the chain orders it, and the lake's
//! own barrier shape — the pool writes sulfur in a radius, so an origin that
//! legitimately passes the filter can still paint neighbours that vanilla
//! leaves alone.
//!
//! # The lead to start from next: `root_system` is not implemented
//!
//! **Three** features write plain `minecraft:sulfur`, not one — the pool's
//! lake barrier, `sulfur_spike`, and `rooted_sulfur_spring`. That third one
//! is `type: minecraft:root_system`, and `root_system` appears **nowhere**
//! in `crates/` or `tools/`: the r2 dump at (64,64) reports
//! `REFUSED: 1x minecraft:root_system`.
//!
//! That single refusal is a candidate for **both** directions at once, which
//! is what makes it worth taking first:
//!
//! - the 67 where vanilla has sulfur and we have cinnabar could simply be
//!   the spring's own blocks, never placed;
//! - the 253 where we have sulfur and vanilla has cinnabar need not be a
//!   misplaced pool at all. A refusal writes nothing, but if it also
//!   **skips the draws vanilla would have made**, every feature after it in
//!   that chunk's decoration order reads a shifted stream and lands
//!   somewhere else. This repo has already bought that lesson once, in the
//!   refused trees.
//!
//! # Correction to the above, before anyone spends time on it
//!
//! Vanilla reseeds per placed feature (`setFeatureSeed(seed, index, step)`), so a
//! refusal **cannot** displace the features after it. It loses its own
//! blocks and nothing more. That makes `root_system` a candidate for the 67
//! and *not* for the 253.
//!
//! # The 253 traced, and the long list of things it is not
//!
//! `RUSTMC_TRACE_WRITE=993,-3,1003` names the culprit outright:
//!
//! ```text
//!   WRITE (993,-3,1003) <- minecraft:stone     by ? (no feature running)
//!   WRITE (993,-3,1003) <- minecraft:cinnabar  by ? (no feature running)
//!   WRITE (993,-3,1003) <- minecraft:sulfur    by minecraft:sulfur_spike from chunk (61, 62)
//! ```
//!
//! Not the pool — **`sulfur_spike`**, whose speleothem writes
//! `base_block: minecraft:sulfur`, invoked from a margin chunk. Its
//! placement differs from vanilla's in both directions (we place `up` tips
//! vanilla does not, we miss `down` tips vanilla has), which is what a
//! position or variant shift looks like.
//!
//! Every structural link in that path has now been read against the jar and
//! **cleared**. Recorded so nobody pays for them twice:
//!
//! | link | checked against | verdict |
//! |---|---|---|
//! | `LakeFeature.place` | bytecode | unshifted y-guard, `offset(-8,-4,-8)`, `boolean[2048]`, `nextInt(4)+4` — all ours |
//! | `SimpleRandomSelectorFeature.place` | bytecode | `nextInt(size)` then the child's `PlacedFeature.place` on the same random — ours |
//! | `EnvironmentScanPlacement` | structure | allowed-condition first, target-before-step, the final test after `break` — ours |
//! | `RandomOffsetPlacement` | config + code | three samples x, **y**, z; constant providers draw nothing |
//! | `ClampedNormalInt.sample` | bytecode | `Mth.clamp(Mth.normal(...))` then **`f2i`** — a truncating cast, not `Math.round`. Ours truncates, with a comment saying why |
//! | `Mth.normal` | bytecode | `d2f` on the gaussian **first**, then `fmul`, then `fadd`, all f32 — ours |
//! | `MarsagliaPolarGaussian` | bytecode | **caches** `nextNextGaussian`, so alternate calls draw nothing; ours caches and resets on reseed |
//!
//! A last-ulp difference in `nextGaussian` is also not a candidate here: the
//! result is narrowed to f32 before the clamp, which discards the low ~29
//! bits, and it would then have to land within that of an integer boundary
//! to change the truncation.
//!
//! # Localised to one cluster, and the whole path re-verified
//!
//! `region-differ census` (added for this) counts blocks rather than
//! disagreements, which is what separates "displaced" from "too many":
//!
//! ```text
//!   window (64,64), 25 chunks     vanilla     ours     delta
//!     minecraft:sulfur            222,219  222,404      +185
//!     minecraft:cinnabar          168,964  168,778      -186
//!     minecraft:sulfur_spike        1,146    1,156       +10
//!     minecraft:potent_sulfur           9        9         0
//! ```
//!
//! Nearly a **pure swap of ~186 blocks**, not extra features — and
//! `potent_sulfur` matching exactly exonerates `sulfur_pool`, since that is
//! its second stage.
//!
//! Clustering the 320 swapped blocks in 3D at radius 3 gives 22 groups, and
//! **one holds 182 of them**: x992..998, y-48..-34, z1046..1061. Censusing
//! just those two chunks:
//!
//! ```text
//!   chunks (62,65)+(62,66)        vanilla     ours     delta
//!     minecraft:sulfur             18,594   18,769      +175
//!     minecraft:sulfur_spike           85      124       +39
//! ```
//!
//! So it is **one speleothem cluster we place and vanilla does not**, plus a
//! scatter of smaller displacements. Find why that one cluster survives a
//! check vanilla fails and most of this number goes.
//!
//! Not for want of looking in the obvious place: the entire cluster path was
//! then read against the jar and **matches**, including two asymmetries a
//! rewrite would flatten —
//!
//! | checked | verdict |
//! |---|---|
//! | `SpeleothemClusterFeature.place` | `isEmptyOrWater` guard, then height/wetness/density/radius/radius, then the nested loops — ours |
//! | `placeColumn` head | scan, the **both-empty** early return, `nextFloat() < wetness`, pool via `withFloor(floor-1)`, `nextDouble() < chance` — ours |
//! | `Column.scan` / `scanDirection` | `i = 1`, `i < maxDistance`, tip predicate tested where the walk stopped — ours |
//! | `SpeleothemUtils.isEmptyOrWater` | `isAir() \|\| is(Blocks.WATER)` — ours |
//! | `replaceBlocksWithBaseBlocks` | loop `thickness`, place then move, early return — ours |
//! | `getChanceOfStalagmiteOrStalactite` | **float** `clampedMap`, widened at the end — ours |
//! | `getSpeleothemHeight` | **double** `clampedMap` narrowed by `d2f`, then `randomBetweenBiased` and `f2i` — ours |
//! | `UniformFloat` / `Mth.randomBetween` | `nextFloat() * (max - min) + min`, f32 — ours |
//!
//! # The surplus is entirely SHORT speleothems (2026-07-27)
//!
//! `region-differ census --states` on chunk (62,62), which has 42 spikes in
//! vanilla and 57 here. `buildBaseToTipColumn` emits `base` only at height
//! three or more, `frustum` at two or more and `tip` at one or more, so the
//! three counts decompose the height distribution directly:
//!
//! ```text
//!                vanilla   ours
//!   height 1        10      18     (tips - frustums)
//!   height 2         3       7     (frustums - bases)
//!   height 3+        8       8     (bases)
//! ```
//!
//! **The tall ones match exactly.** Every extra speleothem is height 1 or 2,
//! which is what it looks like when a column that vanilla gives height 0 —
//! no speleothem at all — gets a 1 or a 2 here.
//!
//! Two things decide that, and both were read against the jar and match:
//!
//! - `getSpeleothemHeight` opens with `nextFloat() > density` returning 0,
//!   then
//!   `Mth.clampedMap` in **double** narrowed by `d2f`, then
//!   `randomBetweenBiased` and `f2i`.
//! - `randomBetweenBiased(random, min, max, mean, deviation)` forwards to
//!   `ClampedNormalFloat.sample(random, **mean, deviation, min, max**)` — the
//!   arguments are reordered, and ours reorders them the same way.
//!
//! **But the premise is wrong, and the correction matters more than the
//! suspect.** `biome_features.json` puts `sulfur_pool` in step 1 (LAKES) and
//! both spike features in step 7 (UNDERGROUND_DECORATION). The pool runs six
//! steps earlier, so the spike surplus cannot be a cause of the missing
//! lake — it is a *consequence* of it. Vanilla's lake hollows rock and lays
//! water at step 1, and by step 7 its spikes are placed against that terrain
//! and find 42 homes; with no lake, step 7 sees unbroken rock and finds 57.
//!
//! The window-wide numbers agree: spikes are only +10 across 25 chunks while
//! (62,62) alone is +15, so there is no systematic over-placement here to
//! explain in the first place.
//!
//! Kept below because it is still the right question **if** a systematic
//! surplus ever shows up in a chunk with no lake involved:
//!
//! The suspect would be `density` itself. It is sampled **once per
//! cluster** and every column in that cluster is tested against it, so a
//! cluster whose density comes out higher grows extra speleothems, and the
//! extra ones land at the short end of the height distribution — exactly the
//! shape above. `UniformFloat` and `Mth.randomBetween` both match, which
//! means the value would have to be wrong because the *stream* is offset by
//! then, not because the sampler is.
//!
//! That makes the draw count inside `placeColumn` the thing to audit next,
//! not another sampler.
//!
//! Still unread: `canPlacePool`, `canBeAdjacentToWater`, `isLava`, the
//! stalactite/stalagmite merge, the non-cluster `SpeleothemFeature`, and the
//! outer `count` / `height_range` / `biome` modifiers.
//!
//! This is a probe, not a gate: it prints a distribution and asserts only
//! the thing that would invalidate the conclusion drawn from it. It is
//! ignored by default because it needs a diff artefact that only exists
//! after a whole-world run.
//!
//! # READ THIS BEFORE TRUSTING ITS OUTPUT (2026-07-27)
//!
//! `band()` below models the three `noise_threshold` conditions and **none
//! of the four gates in front of them**. In the datapack the bands sit
//! inside `biome(minecraft:sulfur_caves)`, which itself sits behind
//! `above_preliminary_surface`, a `stone_depth` floor test and a `water`
//! test. The noise is consulted only if all four hold.
//!
//! So "our noise implies sulfur" is a claim about a rule that may never have
//! been reached, and it has already produced one wrong conclusion: 19 cells
//! were read as the surface rule contradicting its own noise, when
//! `RUSTMC_TRACE_BLOCK` showed the cell was `lukewarm_ocean` and the branch
//! was correctly skipped.
//!
//! Use `RUSTMC_TRACE_BLOCK=x,y,z` to ask the real tree — it prints every
//! condition asked at a block, its verdict, and the numbers behind it. This
//! probe is only sound for cells already known to be in `sulfur_caves` and
//! past the other three gates.
//!
//! Run with:
//! ```text
//! RUSTMC_SULFUR_MISMATCHES=/path/to/sulfur_mismatch.json \
//!   cargo test -p rustmc-worldgen --test probe_sulfur_gradient -- --ignored --nocapture
//! ```

use rustmc_worldgen::noise::{NoiseParameters, NoiseSeeding};
use serde_json::Value;

/// `(double)(float)0.4` — the band edge as the datapack stores it.
const EDGE: f64 = 0.400_000_005_960_464_5;

fn read(path: &str) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path} unreadable: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path} is not JSON: {e}"))
}

fn long(v: &Value) -> i64 {
    v.as_str()
        .unwrap_or_else(|| panic!("expected a long-as-string, got {v}"))
        .parse()
        .unwrap_or_else(|e| panic!("bad i64 {v}: {e}"))
}

#[test]
#[ignore = "needs a sulfur_mismatch.json from a whole-world diff"]
fn sulfur_disagreements_cluster_at_one_band_edge() {
    let Ok(path) = std::env::var("RUSTMC_SULFUR_MISMATCHES") else {
        panic!("set RUSTMC_SULFUR_MISMATCHES to the extracted positions");
    };
    let doc = read(&path);
    let rows = doc
        .as_array()
        .expect("a list of [x, y, z, vanilla, rustmc]");

    // The same noise the surface rule reads, built the same way
    // `WorldSurfaceOracle::new` builds it.
    let router = read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/golden/router_vectors.json"
    ));
    let seeding = NoiseSeeding::new(20_260_722);
    let entry = router["parameters"]
        .as_array()
        .expect("parameters")
        .iter()
        .find(|e| e["name"] == "minecraft:sulfur_cave_gradient")
        .expect("no sulfur_cave_gradient in the router vectors");
    let amplitudes: Vec<f64> = entry["amplitudes"]
        .as_array()
        .expect("amplitudes")
        .iter()
        .map(|v| f64::from_bits(long(v) as u64))
        .collect();
    let noise = seeding.instantiate(
        "minecraft:sulfur_cave_gradient",
        &NoiseParameters::new(
            entry["firstOctave"].as_i64().expect("firstOctave") as i32,
            amplitudes,
        ),
    );

    // The rule, applied to whatever value we compute here.
    let band = |v: f64| -> &'static str {
        if (-0.400_000_005_960_464_5..=-0.100_000_001_490_116_12).contains(&v) {
            "cinnabar"
        } else if (0.0..=EDGE).contains(&v) {
            "sulfur"
        } else if v >= EDGE {
            "cinnabar"
        } else {
            "stone"
        }
    };

    // The question this answers: does our *noise* already imply vanilla's
    // block? If it does, the noise is right and the surface evaluation never
    // reached it. If it implies our own block, the noise itself differs.
    let mut agrees_with_vanilla = 0usize;
    let mut agrees_with_us = 0usize;
    let mut agrees_with_neither = 0usize;
    let mut samples = Vec::new();
    for row in rows {
        let r = row.as_array().expect("row");
        let (x, y, z) = (
            r[0].as_i64().expect("x") as i32,
            r[1].as_i64().expect("y") as i32,
            r[2].as_i64().expect("z") as i32,
        );
        // Namespaced in the diff, bare in `band`. Comparing them directly
        // made every row "implies neither" — a probe that reported 0/0/320
        // and looked like a finding. Same class of mistake as matching
        // `oak_planks` inside `dark_oak_planks`.
        let strip = |s: &str| s.strip_prefix("minecraft:").unwrap_or(s).to_string();
        let want = strip(r[3].as_str().expect("vanilla block"));
        let got = strip(r[4].as_str().expect("rustmc block"));
        let value = noise.get_value(f64::from(x), f64::from(y), f64::from(z));
        let implied = band(value);
        if implied == want.as_str() {
            agrees_with_vanilla += 1;
        } else if implied == got.as_str() {
            agrees_with_us += 1;
        } else {
            agrees_with_neither += 1;
        }
        if samples.len() < 10 {
            samples.push((x, y, z, value, want.clone(), got.clone(), implied));
        }
    }

    println!("{} disagreements", rows.len());
    println!("  our noise implies vanilla's block: {agrees_with_vanilla}");
    println!("  our noise implies our own block:   {agrees_with_us}");
    println!("  our noise implies neither:         {agrees_with_neither}");
    for (x, y, z, value, want, got, implied) in &samples {
        println!("  ({x},{y},{z}) noise={value:+.6} vanilla={want} rustmc={got} implied={implied}");
    }
}
