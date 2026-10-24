# Task: Chunk generator — density to blocks, and the first parity number

## Standing rules (stable prefix — do not edit per-task)

You are one of 3 worker agents building rustmc, a Minecraft-compatible
server in Rust. Non-negotiable rules:

1. You work ONLY in this worktree, on the current branch. Never touch
   `main`, other worktrees, `~/minecom`, or `vanilla-src/`.
2. Before every commit: `ci/t0.sh` and `ci/t1.sh` must pass. Never commit red.
3. Frozen interfaces (marked FROZEN in crate docs) are read-only. If your
   task cannot be done without changing one, STOP and write an escalation
   (see report format) — a half-done interface edit costs more than a stall.
4. Port behavior from the cited reference classes; never copy code. Cite the
   class in a comment at the porting site, Minecom-style.
5. No `static mut`, no ambient singletons, no HashMap iteration at
   behavior-visible sites (ordered containers there), no tolerance-widening
   in tests, no fast-math.
6. Assert invariants, never instantaneous cross-thread state.
7. If a test fails twice for the same reason you don't understand, stop and
   escalate with the evidence. Do not attempt a third blind fix.
8. Deliverables must be cherry-pickable: docs/log entries committed alone;
   code in separate commits.

## Report format (final message — this is all the orchestrator reads)

- LANDED: commits + one-line what/why each
- EVIDENCE: gate output tails proving green (T0/T1 + task-specific)
- ESCALATED: blocked items, each with mechanism/evidence/blocked-on
- NOT TOUCHED: confirm main/other-trees/minecom untouched

## ⚠️ Check the jar before porting anything

`~/minecom/vanilla-src/` mixes versions — `NoiseRouterData.java` there is a
pre-26.2 decompile referencing a `WeirdScaledSampler` class that **does not
exist in 26.2**. Before porting a class:

```
unzip -l ~/mc-26.2/versions/26.2/server-26.2.jar | grep <ClassName>
java -jar ~/minecom/vanilla-src/tools/vineflower.jar <classdir> <tempdir>
```

Decompile to a TEMP dir. Never write into `~/minecom`.

## What exists below you (all bit-exact vs vanilla — do not modify)

- `rng/`, `noise/` — random sources, all noise primitives, and
  `noise/registry.rs` (world seed -> named noise).
- `density/` — the operator framework, splines, terrain shaper, and the
  **full 15-field NoiseRouter**: terrain, caves, slide, post-processing,
  the four aquifer noises, climate, `preliminary_surface_level` and the
  ore-vein trio. All verified against vanilla's own router.
- `density/chunk.rs` — `ChunkSampler`: multiple roots over one shared cell
  lattice, each declaring whether its interpolated markers are read with
  `lerp3` (inside a cell fill) or the staged Y-X-Z path (outside one).
- `density/aquifer.rs` — `NoiseBasedAquifer`, the full state machine.
- `density/ore_vein.rs` — `OreVeinifier`.
- `chunk_gen.rs` — `NoiseFill`, i.e. `doFill`.

**Groups G and H are DONE.** The noise stage is closed:

```
NOISE-STAGE EXACTNESS 100.000000% over 1769472 blocks (0 wrong)
```

18 chunks, 3 seeds, negative coordinates on both axes, one chunk 1000
chunks from origin, palette covering stone/air/water/lava and five of the
six ore-vein blocks. Gate: `tests/golden_noise_chunk.rs`, oracle is
vanilla's public `getBaseColumn`.

## Your slice

**Group I — biomes, surfaces, and the Anvil write.** In that order,
because each needs the one before it.

1. ~~`MultiNoiseBiomeSource` + `Climate.ParameterList`~~ **DONE** — 100%
   over 3,375 ordered climate lookups and 2,304 world positions.
2. ~~Anvil write~~ **DONE** — `rustmc-world::anvil::write_region` plus
   `rustmc-worldgen::anvil_bridge`, proved by rewriting 240 real vanilla
   chunks unchanged.
3. **`SurfaceSystem` + `SurfaceRules`** — the remaining piece.

### Do not transcribe `SurfaceRuleData`

Get the rule tree as **data**, from Mojang's own generator:

```
ci/vanilla-server.sh data
# -> oracle-work/vanilla-<v>/generated-data/data/minecraft/worldgen/
#      noise_settings/overworld.json   (the `surface_rule` key)
```

Worldgen stopped shipping as JSON in the jar years ago — it is built in
code by `VanillaRegistries` — but `net.minecraft.data.Main --server`
still emits it. The overworld surface rule is **35 KB of JSON, 22 levels
deep, 149 conditions**. Hand-copying `SurfaceRuleData`'s builder calls is
149 chances to be quietly wrong somewhere no near-origin test looks, and
this project has already lost days to exactly that class of error twice
(the version-mixed decompile, the `mappedNoise` overload).

The same command also emits `biome/`, `density_function/`, `noise/`,
`multi_noise_biome_source_parameter_list/`, `configured_carver/` and
`configured_feature/` — i.e. the inputs for everything after this task
too.

### Status: parsing and evaluation DONE, rule evaluation UNPROVEN

Landed in `crates/rustmc-worldgen/src/surface/`:

| module | what | proved against vanilla? |
|---|---|---|
| `rule.rs` | AST + parser for all 15 operators | yes — real tree, all 7 dimensions |
| `context.rs` | `SurfaceRules.Context` + its two cache generations | invariants only |
| `eval.rs` | condition/rule interpreter, `SurfaceOracle` seam | invariants only |
| `system.rs` | the column walk, `getSurfaceDepth` | **yes, 100% / 768 columns** |
| `conditions.rs` | `vertical_gradient`, `hole`, `steep` | invariants only |
| `bands.rs` | the 192-entry badlands palette | **yes, 100% / 495 positions** |
| `world.rs` | seed binding, noise + factory derivation | partly (via the two above) |

Regenerate the surface vectors with `ci/surface-golden.sh`.

### The remaining job: prove rule evaluation against `buildSurface`

The two golden tests cover what is a pure function of the seed. The
**conditions and the column walk are still unproven** — they are careful
reads of the decompile, which is exactly the standard that produced two
56%-error bugs in the density layer.

The oracle to build is `SurfaceSystem.buildSurface`, which is reachable
**without** a `StructureManager` — unlike `NoiseBasedChunkGenerator`'s
wrapper:

```java
public void buildSurface(RandomState, BiomeManager, boolean useLegacyRandom,
                         WorldGenerationContext, ChunkAccess, NoiseChunk,
                         SurfaceRules.RuleSource, @Nullable Set<Holder<Biome>>)
```

Signatures already checked, so nobody repeats the lookup:

- `new ProtoChunk(ChunkPos, UpgradeData, LevelHeightAccessor,
  PalettedContainerFactory, BlendingData)`
- `new BiomeManager(BiomeManager.NoiseBiomeSource, long seed)` —
  `NoiseBiomeSource` is a one-method interface, so a lambda over our
  `MultiNoiseBiomeSource` works
- `new WorldGenerationContext(ChunkGenerator, LevelHeightAccessor)`
- `LevelHeightAccessor.create(minY, height)`
- Populate the chunk by reusing `VanillaBench`'s fill loop and writing
  through `setBlockState`, priming `WORLD_SURFACE_WG` as `doFill` does

**The one real blocker**, and where the next attempt should start:
`PalettedContainerFactory.create(RegistryAccess)` needs a `RegistryAccess`,
but `VanillaRegistries.createLookup()` returns a `HolderLookup.Provider`.
`RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY)`
covers the built-in registries but **not** `worldgen/biome`, which is
dynamic and which the factory needs for its default biome holder. Find the
adapter from a `HolderLookup.Provider` to a `RegistryAccess` (look at
`RegistryDataLoader` / `RegistryLayer`) before writing any of the rest.

Like the other in-package dumpers, this needs the signature-stripped jar.

### The whole operator set is 15 types

Counted from the real tree, so this list is complete for the overworld:

| count | operator |
|---|---|
| 149 | `condition` (rule: if condition then rule) |
| 111 | `block` (rule: place this state) |
| 53 | `sequence` (rule: first rule that returns wins) |
| 42 | `biome` (condition) |
| 42 | `noise_threshold` (condition) |
| 23 | `stone_depth` (condition) |
| 20 | `water` (condition) |
| 10 | `y_above` (condition) |
| 5 | `steep` (condition) |
| 4 | `not` (condition) |
| 3 | `hole` (condition) |
| 2 | `vertical_gradient` (condition) |
| 2 | `bandlands` (rule — the badlands colour banding) |
| 1 | `above_preliminary_surface` (condition) |
| 1 | `temperature` (condition) |

Three rule types and eleven condition types. That is a small interpreter,
and a JSON-driven one is also what a datapack needs later.

**The trap to expect here**: `SurfaceRules.Context` computes several of
these **lazily and caches per column or per position** (`stone_depth`
walks down from the surface; `hole`/`steep` read the preliminary surface
level). Cache semantics that look like optimisation are behaviour — the
same lesson `cacheOnce` taught in `density/`. Port the laziness, do not
"simplify" it into eager evaluation.

## The gate that matters

When I is done, `ci/t2-regiondiff.sh` stops testing the harness on flat
worlds and starts producing a parity percentage against a
vanilla-generated region. Note that number will be **lower** than the
noise-stage one until carvers and features land, and that is expected —
report it with its denominator and its scope, never rounded up.

Two things about it, both non-negotiable:

- **Generate the vanilla side with `/tick freeze` before loading**, exactly
  as `ci/gen-vanilla-region.sh` already does. Forceloaded chunks tick, and
  a ticking world drifts (random ticks, fluids, weather) — the existing
  harness took three rounds to learn this and the protocol is written down
  in ORACLE.md §3. Do not re-derive it.
- **Diff far from origin as well as near it.** The `Mth.getSeed` bug this
  project found in Minecom is invisible within ~686 blocks of spawn, and
  the `spaghetti_2d_elevation` overload bug was invisible outside one
  narrow y band. Near-origin diffs and sparse sampling both let real bugs
  through; the harness now sweeps densely for exactly that reason.

Report the number with its denominator, honestly, whatever it is. A first
number of 94% that is real is worth more than 99% that is measured on a
flat world.

## Traps carried forward

- The decompile cache is version-mixed (see above).
- **Java overload resolution is a parity hazard.** Count the arguments
  against the overload set — `mappedNoise` has three overloads and
  misreading one cost a real bug that only showed in a narrow y band;
  `noise(data, x)` is the *yScale* overload, not xzScale.
- **Float literals with an `F` suffix are `f32` widened to `f64`.** This
  has now bitten three times: `-0.08F` in vein_ridged, `-0.225F`/`0.9F` in
  isDeepDarkRegion, and `0.64F`-adjacent constants in postProcess. Also
  watch for comparisons that stay in f32 entirely — `nextFloat() > 0.7F`
  is an f32 compare, while `nextFloat() < richness` promotes.
- Java's `Math.min`/`max` are not Rust's — `math::java_min`/`java_max`.
- No algebraic simplification; copy the reference's association order.
  Moving a constant multiply outside an `interpolated(..)` marker is
  algebraically free and bit-wrong — that cost four investigations.
- Bounds feed evaluation-time short-circuits in `Min`/`Max`.
- Splines evaluate entirely in `f32`.
- **A rule your sample never exercises is not proven.** The first
  noise-stage sample hit 100% over 4 chunks while containing zero ore
  veins; widening it to 18 chunks immediately found a real bug. Check the
  palette, not just the percentage.
- When a test fails, **check the test's premise before the code's**. Three
  scenarios in `density/chunk.rs` were wrong before any line of code was:
  a linear function is reproduced exactly by interpolation, a `min` whose
  other branch always wins hides its input, and the Y-X-Z lerp order
  usually agrees with X-Y-Z bit-for-bit.
- **When an aggregate diverges, observe its inputs rather than bisecting
  hypotheses about it.** `min`/`max`/`rangeChoice` all discard a branch, so
  a badly wrong intermediate can stay masked. Dumping each interpolated
  marker's wrapped value directly localised a 56% error in one run after
  four rounds of reasoning had failed.
