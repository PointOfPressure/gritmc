# Golden RNG vectors (generated from the vanilla jar)

Byte-for-byte reference streams for the worldgen RNGs, dumped from **real
vanilla 26.2** (see docs/ORACLE.md §3–5). The Rust ports must reproduce every
value exactly; `tests/golden_rng.rs` replays all of them on every `ci/t1.sh`.

## Provenance

- Source: `net.minecraft.{util.Mth, util.RandomSource,
  world.level.levelgen.*}` from `server-26.2.jar`, called directly.
- Generator: `tools/goldengen` (Rust driver) + `tools/goldengen/vanilla/
  GoldenDump.java`. Both are committed; regeneration is one command.
- Jar: the version pinned by `GAME_VERSION` in
  `crates/rustmc-proto/src/lib.rs`, i.e. what `ci/vanilla-server.sh fetch`
  downloads.

## Regeneration

```
ci/vanilla-server.sh fetch     # once, if oracle-work/ is empty
ci/vanilla-server.sh start     # once — unpacks the bundler jar
cargo run -p goldengen         # writes straight into this directory
```

The whole-chunk and feature vectors have their own scripts, because their
dumpers have to live inside vanilla's own package to reach protected members
and so compile against a signature-stripped copy of the jar:

```
ci/surface-chunk-golden.sh     # surface_chunk_vectors.json (surface + carvers)
ci/temperature-golden.sh       # temperature_vectors.json
ci/mth-sin-golden.sh           # mth_sin_vectors.json
ci/feature-order-golden.sh     # feature_order_vectors.json
ci/biome-zoom-golden.sh        # biome_zoom_vectors.json
ci/ore-golden.sh               # ore_vectors.json
ci/decorate-golden.sh          # decorate_vectors.json
ci/hash-order-golden.sh        # data/vanilla/hash_order.json
ci/fancy-math-golden.sh        # fancy_math_vectors.json
ci/gaussian-golden.sh          # gaussian_vectors.json
ci/structure-placement-golden.sh # structure_placement_vectors.json
ci/block-age-golden.sh         # block_age_tape.json
```

`block_age_tape.json` is a different kind of golden from every other file
here and the difference is the point. The rest are **values**; this one is a
**draw sequence** — per position, the ordered list of calls vanilla's
`BlockAgeProcessor` makes and the state it returns, captured by injecting a
recording `RandomSource` through `StructurePlaceSettings.setRandom`.

It exists because `maybeReplaceFullStoneBlock` builds both candidate arrays
before `getRandomBlock` picks one, so the draws spent on the losing array
never reach the world. A value test cannot see them: the block they were
spent on is correct and the *next* block is wrong. On 2026-08-09 that was
14 blocks in the r=16 overworld portal box, and the tape put the first
divergent draw index at 2 in one command.

`ci/ore-golden.sh`, `ci/hash-order-golden.sh`, `ci/gaussian-golden.sh` and
`ci/structure-placement-golden.sh` run
their dumper **twice and refuse to write unless the two agree** (ORACLE.md
rule 10) — see the note in them. The others do not yet, and should.

Deliberate only: a version bump, or a vector shown to be wrong. Never part of
a normal build, and never run to make a failing test pass — a mismatch means
the port is wrong until proven otherwise.

`gaussian_vectors.json` is the odd one out and worth knowing about before
reading a failure: it is the only file whose values this port is **not**
expected to reproduce bit-for-bit for all time. `Math.log` is a HotSpot
intrinsic that is within 1 ulp of the true logarithm and not correctly
rounded, so the gate asserts draw counts exactly and values to within one
ulp with an exact count of the differences. Currently that count is zero.
ORACLE.md §5c has the four measurements behind that.

## Correction record — 2026-07-24

These vectors previously came from **Minecom**, not from vanilla. Regenerating
them against the jar changed exactly three cases, all in
`xoroshiro128pp_derived.json` and all downstream of one function:

- `blockSeed` (`Mth.getSeed`)
- `forkPositional.at(123456, 80, -654321)`
- `forkPositional.at(-30000000, -64, 30000000)`

**Cause.** Minecom widens `x` to 64 bits before multiplying; vanilla
multiplies in 32 and widens after. The bytecode is unambiguous —
`javap -c net.minecraft.util.Mth` gives `iload_0; ldc 3129871; imul; i2l`
for `getSeed`, and only `z`'s multiply is `lmul`. The two agree while
`x * 3129871` fits in an `i32` and diverge past |x| ≈ 686 blocks, so a
spawn-centred region diff cannot see it. Confirmed by calling the real
method: `getSeed(123456, 80, -654321)` = `-6447583314939` (vanilla) vs
`-22410935387131` (Minecom's export).

Everything else was already exact — both 10,000-draw streams for all five
seeds, the whole seeding ladder (`setDecorationSeed`, `setFeatureSeed`,
`setLargeFeatureSeed`, `setLargeFeatureWithSalt`) and every per-op vector
matched byte-for-byte. The `repr` fields moved because this dumper formats
floats with `Double.toString`/`Float.toString`; `output_bits` are unchanged,
and the tests compare bits only.

This is why the from-scratch amendment requires a first-party oracle: a
second-hand one inherits the other project's bugs, silently, at exactly the
places a near-origin test cannot reach. Worth reporting upstream — Minecom
carries an open "ore placement-origin drift" item whose symptom matches.

## Format

All i64 values are JSON **strings** (a JSON number cannot hold i64 exactly).
Floats/doubles appear as raw IEEE-754 bit patterns (`output_bits`) with a
human-readable `repr` alongside — **compare the bits, not the repr**.

- `xoroshiro128pp_seed_<seed>.json`, `legacy_lcg_seed_<seed>.json`
  (`m1` = seed −1): `{rng, seed, count, outputs}` — the first 10,000
  `nextLong()` draws.
- `*_derived.json`: `{rng, cases: [...]}`, each case naming an operation and
  its parameters. 100 draws per case, each from a fresh instance seeded
  20260708 unless the case says otherwise:
  - `xoroshiro128pp_derived.json`: `mixStafford13` and `blockSeed`
    (input→output vectors); `fork`; `forkPositional.at(x,y,z)` /
    `.fromHashOf(name)` (MD5 path) / `.fromSeed(s)`; per-op vectors
    `nextInt.bounded` (bounds 1, 2, 3, 16, 17, 100, `i32::MAX`),
    `nextDouble`/`nextFloat` (bits), `nextBoolean`,
    `nextIntBetweenInclusive`.
  - `legacy_lcg_derived.json`: the `WorldgenRandom`-over-LCG seeding ladder
    plus the same per-op vectors. Note `setDecorationSeed` takes the chunk's
    **minimum block** coordinates while `setLargeFeatureSeed` takes **chunk**
    coordinates — the pairing (`-208`, `432`) with (`-13`, `27`) is the same
    chunk, and the decompiler names both parameter pairs `chunkX`/`chunkZ`.
  - `xwr_derived.json`: `WorldgenRandom` over Xoroshiro. Its `nextLong` is
    composed from two 32-bit draws (`BitRandomSource`'s default), unlike raw
    Xoroshiro — the two therefore agree on `nextFloat` and must disagree on
    `nextDouble`.
