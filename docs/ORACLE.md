# ORACLE — verification architecture

The single load-bearing idea: **rustmc is a port with an oracle, not a
rewrite.** Minecom (private JVM project, ~/minecom) spent its life pinning
vanilla behavior into machine-checkable gates. Those gates — not Minecom's
code — are what transfer. A swarm without a gate produces 200k lines of
plausible Rust; a swarm with these gates produces a server.

> **AMENDED 2026-07-22 (from-scratch decision — human call, supersedes
> minecom-hosted tooling below):** rustmc is fully self-contained. Oracle
> *tooling* is rebuilt in-repo: wire harness on rustmc-proto (tests run
> against the real vanilla server AND the rustmc binary), region differ on
> rustmc-world's Anvil code, golden dumper as a standalone JVM tool in
> tools/. Nothing in CI checks out Minecom. What transfers from Minecom is
> knowledge, not code: the behavior inventory (playtest-manifest as a
> checklist), the flake/determinism discipline (§5–§6, verbatim), and the
> tiebreaker role (§1 last row, read-only). Ground truth was never Minecom
> — it is the real vanilla server + JVM dumps; that stays, because parity
> is *defined* against vanilla. The three minecom-side Phase-0 branches
> (playtest-external, regiondiff-command, goldengen-dumps) are demoted to
> non-blocking reference implementations. Public headline metric: region
> exactness ≥99.9% + externalized wire checks green, denominator
> published. §2–§3 below describe the minecom-hosted v0 and remain as
> spec/prior art for the in-repo rebuild.

## 1. Asset inventory: what transfers, what doesn't

| Minecom asset | Transfers? | How |
|---|---|---|
| PlayTest suite (~1019 checks, protocol-driven scenarios) | **Mostly** | Bot clients speak the wire protocol to a server on a port — language-blind *in principle*; needs the §2 adapter work |
| `scripts/worldgen_region_diff.py` (region-file diff vs real vanilla) | **Yes, as-is in spirit** | Compares Anvil region files on disk; neither side cares who wrote them (§3) |
| SelfTest suite (258 checks pre-security-merge; ~271 after) | **No** | In-process Java assertions against Java internals (§4) |
| Extracted vanilla data (1,486 jar-derived files + extractor `extract_vanilla_data.py --validate`) | **Yes** | Data files are JSON; the extractor is Python; rustmc consumes the same files |
| Decompiled reference cache (`vanilla-src/`, 26.x unobfuscated) | **Yes (read-only)** | Same rules as Minecom: port behavior, cite class, never copy; never commit |
| Benchmark harness (5 scenarios × server matrix, 15-bot load) | **Yes** | Drives servers over the protocol; add rustmc as a third column next to vanilla/Paper |
| Minecom source | **Reference only** | It is the executable spec: when vanilla's decompile is ambiguous, Minecom's tested port is the tiebreaker — but rustmc passes the *gates*, it does not transcribe the *code* |

## 2. PlayTest as cross-server gate (REWRITTEN 2026-07-21 — original premise
falsified by the Phase-0 audit)

**Corrected current state:** PlayTest's actor is an **in-process
`TestPlayer` over a `FakeConnection` packet sink** — there are no wire
bots; the port goes unused by scenarios; setups use `EventDispatcher`/
direct mutation and many assertions read server-side Java state. The
original "language-blind in principle" claim was wrong (escalation E1,
evidence: `playtest/FakeConnection.java`, `PlayTest.run()`).

**What Phase 0 actually delivered** (branch `playtest-external`,
~/minecom-w-external, unmerged):

- `--playtest-external <host:port>` + `playtest/external/WireClient` — a
  REAL TCP client built on Minestom's own PacketWriting/PacketReading (no
  hand-rolled packet ids): status ping, full login→config→play, command
  round-trips. 10 external checks green against a cold-booted Minecom on a
  port, twice. This is the seed of the wire transport, proven end-to-end.
- **Full audit, 1081 checks → A=6, B=611, C=464** (scenario level 221 →
  A=5/B=150/C=66). 3 B→A conversions landed as the pattern proof; the
  remaining B scenarios carry proposed command equivalents in the manifest.
- `playtest-manifest.json` (repo root, regenerable via
  `scripts/playtest_manifest.py`), meta carries the transport caveat so CI
  can never mistake "Class A" for "already wire-tested".

**Revised plan — the wire gate is built, not flipped on:** growing G2 is
wave-1+ work on BOTH sides: re-express scenario verbs as packets via
WireClient, add client-side mirrors for wire-observable assertions, convert
B setups per the manifest's proposed commands. The gate's denominator is
the manifest; the burn-down metric is **externalized checks green**, from
10 upward. Class C (464) is not chased over the wire — those families are
covered by §4 golden vectors/unit tests or documented in GAPS.md.

**Gate definition (amended):** rustmc "passes PlayTest" = every
externalized check green against the rustmc binary on a port, same seeds,
same tick-counted waits — with the externalized set's size public in the
manifest (honest coverage, no silent shrinkage).

**Minecom-side findings for the main-tree owner** (logged in the worktree
run log; unfixed, out of Phase-0 scope): `/setblock` bypasses neighbor
updates (blocks redstone-setup conversions against Minecom itself);
cold-fresh-world first join can keepalive-kick during spawn gen on slow
storage (vanilla pregenerates spawn); trident-riptide joins crossbow in
the known-fragile family.

## 3. Region-diff as worldgen gate

> **IN-REPO SINCE 2026-07-22 (per the top amendment):** the measuring
> instrument is now `tools/region-differ` (Rust: NBT + Anvil parser in
> rustmc-world, full-state + biome compare, `--bbox`, `--json`) plus
> `ci/gen-vanilla-region.sh` (drives the real vanilla server via console
> forceload; ≤ r7 single-shot, r18 needs row-batching — wave-2 work).
> `ci/t2-regiondiff.sh` used to be described here as: same seed generated
> twice by vanilla, inner 3×3 diffed, gate demands bit-exact 100.000000% —
> **and passes**. **That reading was one lucky pair and the gate had stopped
> re-taking it (2026-08-16).** Eight harvests of that window produce five
> distinct worlds, 0..536 blocks apart of 884,736, and only 4 of 28 pairs are
> zero; the 100.000000% is reachable — a fresh harvest still lands on the
> frozen pair exactly — but it is a draw, not the property. It IS the property
> under `-Dmax.bg.threads=1`, which the gate now sets: four runs, six pairs,
> all zero. `docs/DETERMINISM.md` §36 and §37. The gate is now three named
> parts — a parser known-answer test, a differ control mutated by
> `ci/regiondiff-drop-chunk.py`, and the live harvest half — with
> `ci/regiondiff-falsify.sh` as its red-when-broken evidence.
> Hard-won reference-generation protocol (2026-07-22, three live rounds):
> a naively harvested vanilla world is NOT deterministic — forceloaded
> chunks tick (round 1: 359 drifted blocks — random ticks/weather near
> spawn; round 2 after gamerules+away-from-spawn+30s settle: 27 blocks —
> fluids still mid-flow). The fix that closed it: **`/tick freeze` before
> any target chunk loads** (chunk gen runs off-tick), making the harvest
> the pure generator output — which is also the correct semantic for G3.
> Every future reference world MUST be generated through
> ci/gen-vanilla-region.sh for exactly this reason.
> **26.x layout finding:** vanilla now saves the overworld under
> `world/dimensions/minecraft/overworld/region` — NOT the classic
> `world/region` this section's original text assumed; the genregions
> contract below inherits the corrected layout when the writer lands.
> The Python-script description below is the Minecom-side prior art.

`worldgen_region_diff.py` (1) generates a chunk square on a **real vanilla
server**, (2) generates the same square with the candidate server, (3)
compares every block, full block-state, on disk. Nothing in step (3) is
JVM-specific.

**Contract for the rustmc binary (AMENDED 2026-07-21 to match the script's
real invocation, verified by the --command patch):**

```
rustmc --genregions <seed> <radius> <cx> <cz> <overworld|nether>
```

positional form, cwd = the candidate work dir, writing standard Anvil
`region/*.mca` under the same layout the script already reads
(`world/region`, `world/dimensions/minecraft/the_nether/region`).
Script change: DONE — `--command "<exe> ..."` accepted as an alternative to
`--jar` (branch `regiondiff-command`, ~/minecom-w-regiondiff); proven by
identical bit-exact r3 numbers (99.049603%, seed 20260708) via --jar and
--command wrapping the same jar.

**Numbers to match (the ratchet inherits):** overworld r18 seed 20260708 =
**99.381792%** (Minecom's current cut line — rustmc's worldgen ratchet starts
wherever Minecom's stands at port time); nether via the adopted path =
**99.419259% deterministic**. Same up-only ratchet rule, same
radius-3-then-r18 protocol.

> **Both figures are INHERITED, not measured here. Labelled 2026-08-07.**
> They were produced by a different project on a different window (**r18, seed
> 20260708**), no artefact for them exists in this repo, and **this project has
> never taken an r18 measurement at all** — every parity figure it publishes is
> r=7 or r=16 at seed 20260722/12345/31415926. They are a target to clear, not
> a claim about this server, and they must never be quoted as one or compared
> against an r=7/r=16 number. `docs/CI.md` repeats 99.381792% under the same
> label.

## 4. SelfTest does NOT transfer — gap ledger

The ~271 in-process checks assert against Java objects (loot-table streams,
attribute math, fixed tables, pure decision functions). Strategy, per check
family:

- **Fixed-table families** (map colors, archetype stats, RNG providers):
  re-express as **golden-file tests** — dump the table from Minecom once,
  commit the JSON, assert the Rust implementation reproduces it byte-for-byte.
  Cheapest, highest-value transfer.
- **Pure-function families** (admission control decisions, explosion math,
  enchant costs): port as native Rust unit tests with **test vectors exported
  from the Java implementation** (same inputs → same outputs, generated once
  by a Minecom dump run).
- **Behavioral families already shadowed by PlayTest:** accept SelfTest loss,
  note the PlayTest check ids that cover them.
- **True gaps:** documented in `docs/GAPS.md` at port time. A documented gap
  is acceptable; a silent one is not (Minecom AUDIT.md discipline carries
  over verbatim).

## 5. Float/RNG cross-language exactness

This is the classic cross-language killer; it is **solved by discipline, not
hope**, and there is an existence proof in hand: the ore-RNG diagnosis
(2026-07-19) included a from-scratch **Python** port of Xoroshiro128++ /
XWorldgenRandom (Java int32/int64 wraparound semantics included) that
replayed **bit-exact** against a live JVM placement dump — validating the RNG,
the count/in_square/height_range providers, and the feature-sort index. If
Python can do it, Rust (with explicit wrapping arithmetic) does it more
naturally.

Rules:

1. **Every RNG (Xoroshiro128++, legacy LCG) ships with golden-stream tests**:
   first 10k outputs from known seeds, exported from the JVM once, committed.
2. **Java quirks are ported explicitly**: `Math.sin` lookup-table (not libm),
   int overflow via `wrapping_*`, float→double widening at the exact sites the
   reference widens (Minecom hit this: `0.9f as f64 ≠ 0.9`), `hashCode`-driven
   iteration orders reproduced only where behavior-visible (Minecom's
   leaf-BFS case documents one).
3. **f32/f64 order-of-operations is copied from the decompile**, no algebraic
   "simplifications" — Rust's optimizer is not allowed to reassociate
   (no `ffast-math` anywhere, ever).
4. Divergence hunting is mechanical: region-diff names the chunk, the
   instrumented-origin-dump + replay method from the ore diagnosis names the
   feature. That workflow is the debugging loop, documented and repeatable.

## 5b. The oracle is the thing most likely to be wrong (2026-07-25)

Four of this project's parity investigations ended at a fault in the
*harness*, not the port: a missing `noiseChunk.swapSlices()`, a
`VanillaBench` measuring that same wrong computation, a `SurfaceChunkGolden`
in which every `biome` condition was silently false because it resolved
biomes through a rebuilt registry while the surface rule had been resolved
through the lookup (holder equality is identity), and — the same day — a
carver sweep in which every block tag resolved to an **empty** holder set,
because tags are datapack content and `Bootstrap.bootStrap()` loads no
datapack. 6,936 carvers considered, 567 started, 567 ran to completion, zero
blocks changed, no error anywhere.

Those last two are the dangerous shape: **the oracle produced output that
looked like a world, and in the biome case the port agreed with it, because
both were answering an easier question than the one being asked.** No amount
of staring at the port would have found either.

The shared cause is worth naming: *vanilla resolves a lot of things lazily
against global state that a real server populates and a harness does not.*
Registries, tags and holder identity are all in that category. A harness that
skips the population step does not fail — it returns empty, and empty is
usually indistinguishable from "the feature did not apply here".

Rules that follow, and that the gates now enforce rather than merely record:

1. **A harness that builds a registry, lookup or `RandomState` threads it
   from one place.** Two objects that claim to be "the same registry" must be
   the same object; anything vanilla compares by identity will otherwise fail
   silently and plausibly.
2. **A gate asserts that it reached what it claims to cover.** A percentage
   next to a condition that was never evaluated is not evidence about that
   condition. `golden_surface_chunk` counts `temperature` evaluations and
   fails at zero; `golden_temperature` asserts the vectors contain a biome
   carrying the `frozen` modifier and that vanilla answered both ways.
3. **Where an oracle reconstructs something private, it audits the
   reconstruction against a public equivalent and ships the result as a
   flag** — `TemperatureGolden` rebuilds three private `PerlinSimplexNoise`
   statics and compares its rebuild of the one that *is* public, and the Rust
   test refuses vectors where that flag is false.
4. **Search for the interesting case in the oracle, not the port.** A search
   run through the port can only find cases the port already handles, and
   reports "none found" for exactly the condition it has broken.
5. **Cover a registry, not a hand-picked subset of it.** The named-noise gate
   sampled six noises chosen when only the density router mattered; a surface
   rule then named a seventh, unproven one. Any noise a datapack can name is
   a noise that has to be right, so the gate enumerates the registry.
6. **Populate the global state a real server would.** Block tags, biome
   tags and anything else datapack-loaded resolve to empty in a bare
   `Bootstrap.bootStrap()`, and empty reads as "did not apply". If a harness
   touches a `#tag`, it binds the tags first — `prepareTagReload` with the
   datagen output is enough — and asserts something is in them.
7. **Re-decompile before porting, every time.** The `is_3d` field on
   `noise_threshold` is new in 26.2; the cached decompile it was written
   against did not have it, and a field that is absent from the reference is
   a field nobody thinks to look for.
8. **A harness that has to stand in for a live object implements it as a
   proxy that throws, not a stub that answers.** Rule 6 says populate the
   global state; this is what to do when you cannot. `OreGolden` needs a
   `WorldGenLevel`, which on a real server is a `WorldGenRegion` over a
   `ServerLevel`. `javap -c` over `OreFeature` and `BulkSectionAccess` names
   the **six** methods between them that the feature actually reaches, so the
   harness is a `java.lang.reflect.Proxy` implementing those six over nine
   real `ProtoChunk`s and throwing for everything else — inherited default
   methods included. A hand-written stub returning `Blocks.AIR` from a
   method nobody foresaw would place no ore and report no problem, which is
   fault shape #4 again. A proxy that throws turns an incomplete analysis
   into a crash naming the method.

   The five delegating methods are not reimplemented either: four forward to
   the same `LevelHeightAccessor` the chunks were built with, and the
   heightmap lookup copies `WorldGenRegion.getHeight` including its `+ 1`.

### The thing that finding produced

`OreFeature` reads `Heightmap.Types.OCEAN_FLOOR_WG`, and
`ChunkStatusTasks.generateFeatures` calls `Heightmap.primeHeightmaps` on the
line before it runs features — with `EnumSet.of(MOTION_BLOCKING,
MOTION_BLOCKING_NO_LEAVES, OCEAN_FLOOR, WORLD_SURFACE)`. **Every heightmap
except the one the ore feature reads.**

So `OCEAN_FLOOR_WG` at feature time is a fossil: it is filled during the
noise fill and never touched again. The surface rules write grass and sand
over it without updating it; the carvers cut caves through it without
updating it. The height an ore vein tests itself against is the height of
terrain that no longer exists.

A port that recomputed that heightmap from the blocks in front of it — the
obvious implementation — disagrees with it on **69.07% of the 6,912 columns
in the ore gate's worlds, by up to 25 blocks**. `ore_vectors.json` therefore
ships the heightmap alongside the blocks rather than inviting the Rust side
to derive it.

### The gate could not see that, and had to be told to

Substituting the recomputation into the ore gate as a deliberate mutation
changed **nothing**: 1,440 cases, still 100.000000%. Two other mutations —
dropping the swallowed-sphere deletion pass, dropping the tested-bitset —
failed 2 and 20 cases, so the gate worked; this one it simply could not see.

The reason is that `OreFeature.place` uses the heightmap in one place, as a
loose inequality: `y - 2 - i <= level.getHeight(OCEAN_FLOOR_WG, x, z)` for
any column in the vein's box. Every origin in the case set sat either well
below the terrain, where that passes whatever the height is, or well above
it, where it fails whatever the height is. A 25-block error in the input
changed no output.

`SURFACE_OFFSETS` in the dumper is the fix: origins placed at the origin
column's own height plus a few blocks either way, so the inequality is
actually being decided. **A rule falls out of this, and it is not the same
as "assert you reached the condition" (rule 2):**

9. **A gate that reads an input must contain cases where that input decides
   the answer.** Reaching a condition is not exercising it. Every case in
   this gate called `getHeight` and used its result, and the result could
   have been off by 25 blocks in either direction without one block moving.
   Coverage counted it; sensitivity did not exist. The way this was found was
   a mutation test, and the way it stays found is a case set chosen so the
   mutation fails.

### Fault #5: the feature order was sorted from the wrong list

**Found 2026-07-25 by the regenerate-twice check that fault #4 put in
`ci/ore-golden.sh`.** `DecorateGolden` produced different vectors on two runs
of the same JVM, and the difference was that one feature had index 27 in one
run and 28 in the other — the *global feature ordering itself* was moving.

Every feature in the world is seeded `setFeatureSeed(decorationSeed,
indexWithinStep, step)`. That index is a position in one ordering computed by
`FeatureSorter.buildFeaturesPerStep`, which numbers features in
**first-encounter order across a list of biomes**. So both which biomes are
in that list and what order they come in decide the seed of every tree, ore
and lake in the game.

`FeatureOrderGolden` fed it `lookup.lookupOrThrow(Registries.BIOME)
.listElements()`. `javap -c` of `ChunkGenerator`'s constructor says vanilla
feeds it

    FeatureSorter.buildFeaturesPerStep(
        List.copyOf(biomeSource.possibleBiomes()), ..., true)

which is wrong twice over. It is the **biome source's** possible set, not the
registry — per dimension, so the overworld's list excludes every nether and
end biome. And it is ordered by first appearance in the climate parameter
list, not by registration. (It is nonetheless deterministic:
`BiomeSource.possibleBiomes` collects through `ImmutableSet.toImmutableSet()`
and Guava's ImmutableSet iterates in insertion order. `listElements()` offers
no such guarantee, and was observed changing between two runs.)

Corrected, the input goes from 66 biomes to **55** and the output from 209
features to **168**:

| step | before | after | positions that differ |
|---|---|---|---|
| 2 LOCAL_MODIFICATIONS | 6 | 5 | 4 of 5 |
| 4 SURFACE_STRUCTURES | 9 | 4 | 4 of 4 |
| 6 UNDERGROUND_ORES | 34 | 34 | **0** |
| 7 UNDERGROUND_DECORATION | 30 | 7 | 7 of 7 |
| 9 VEGETAL_DECORATION | 115 | 106 | **105 of 106** |
| 10 TOP_LAYER_MODIFICATION | 3 | 1 | 1 of 1 |

So `golden_feature_order`'s **`100.000000%` over 209 features is retracted**:
it was agreement on a computation vanilla does not perform. Every tree and
vegetation feature in the world would have been seeded from the wrong index.

Two things are worth keeping about how this went. The **port was right** —
`build_features_per_step` reproduces vanilla's sorter exactly on the
corrected input, and the only Rust change needed was a ratchet pinned to the
wrong biome count. The oracle was asking it the wrong question, which is
fault shape #4 in a new place. And it was caught by a **cheap mechanical
check added for an unrelated reason**: regenerating twice and running `cmp`
costs one extra JVM run and found a fault that reading could not, because the
harness's comment confidently asserted the thing that was false.

10. **Every golden dumper regenerates twice and refuses to write unless the
    two agree.** Determinism is not a property to assume of a harness that
    walks registries, and an unstable oracle hides a real regression inside
    its own churn. This is now the only reason fault #5 was found.

### Fault #6: the world dumps threw away block properties

**Found 2026-07-25 by following a dependency, not a failure.** `disk_sand`'s
placement chain asks `matching_fluids`; a fluid state needs block properties;
the golden world dumps did not have any.

`SurfaceChunkGolden.encode` recorded
`BuiltInRegistries.BLOCK.getKey(state.getBlock())` — the **block name**. So
`golden_surface_chunk`, `golden_carver_chunk` and `golden_ore` had been
comparing names, and a port that placed the right block in the wrong state
would have read 100.000000%.

It is not a small distinction:

- `minecraft:water` is **sixteen states**. Level 0 is fluid
  `minecraft:water`; levels 1-15 are `minecraft:flowing_water`. A name-level
  dump cannot tell them apart, which is exactly why `matching_fluids` was
  unanswerable.
- `grass_block` carries a `snowy` property the surface rules set.
- 31,170 of the game's 32,366 states are non-default.

Fixed by dumping `BlockStateParser.serialize(state)`. Six of the thirty-five
distinct states in the surface gate's chunks carry properties, including
`deepslate[axis=y]`, and the gate now reads **100.000000% over 2,555,904
blocks at full state level** — with **no change to the port**, which had been
parsing and emitting properties correctly all along.

That is the sixth fault of the same family and the fourth in which the port
was already right. The pattern is now strong enough to state as a prior:
*when a parity number is wrong, look at the harness first.*

**And it immediately found a real one.** Tightening the carver gate to states
took it from `100.000000%` to **`99.991275%` — 223 wrong of 2,555,904**.
Vanilla writes `Blocks.LAVA.defaultBlockState()`, which serialises as
`minecraft:lava[level=0]`; `WorldCarver#getCarveState`'s flood-below-lava-level
path in the port wrote a bare `"minecraft:lava"`.

Two things make that worth more than 223 blocks. It was the *only* place that
got it wrong — `anvil_bridge::block_name` had been emitting
`minecraft:lava[level=0]` and `minecraft:water[level=0]` correctly since the
noise stage was written, and this was one string literal that did not match
its neighbours. And `[level=0]` is what goes into the Anvil file, so the
region-diff headline metric would have found it eventually, as a mystery in a
whole-world diff rather than as one line in a carver test.

Of the six faults up to here, four vindicated the port, one was in the
reference, and this is the first where fixing the harness surfaced a genuine
defect. It was not the last — see fault 7.

10b. **Regenerating twice is not enough if both runs used the same extent.**
    Rule 10 catches a nondeterministic generator. It does not catch an oracle
    whose answer depends on how the harness asked. Vanilla generated at two
    different region centres produces two different worlds at the same seed,
    because a feature writes one chunk out and which of two adjacent chunks
    decorates first decides the overlap — measured at 134 blocks in one
    chunk, with same-centre reruns bit-identical. Where an oracle is a
    generated world, regenerate at a **different centre** too, and treat
    whatever differs as a floor rather than as a fault to chase.

11. **A gate compares the most specific thing both sides can express.** Not
    the most convenient. Where the port carries more information than the
    oracle dumps, the oracle is the one that is lossy, and the number is
    weaker than it reads.

    The exception has to be argued in the source, not assumed: the surface
    gate's *coverage* count stays name-shaped, because the rule spells
    properties as the datapack does (which the port sorts alphabetically)
    while the palette spells them as `BlockStateParser` does (property
    definition order). Comparing those would measure the spelling. The
    reason is written where the comparison is.

12. **A measurement that cannot distinguish between two explanations is
    not evidence for either.** The whole-world diff spent four ports
    reporting a confusion matrix — *which* blocks disagreed — and no
    positions. That number supported three different stories about its
    largest row at once (unported features, a carver difference, a surface
    rule evaluated wrongly) and gave no way to choose, so the story that
    got written down was whichever one seemed likeliest at the time. One
    of them was written into BENCH.md and was wrong.

    Adding a y-range per disagreement chose in a single run: every
    `cave_air -> solid` row, every `oak_planks` row and `cave_air -> tuff`
    all sat in the same fifteen-block band, which is one mineshaft rather
    than three causes. 45% of the remaining diff, reattributed by an hour's
    work on the instrument.

    `golden_surface_chunk` had already learned this and says so in its own
    module docs: the one real port bug it caught needed the matrix *and*
    the band histogram together. The lesson did not travel to the tool that
    measures the whole world until it cost something.

    The general form: when a report is ambiguous, extend the report. Do not
    resolve the ambiguity by argument, and do not record the argument as
    though it were a result.

13. **Stage gates cannot see seams, and every large fix so far has been in
    one.** Noise reads 100% over 1.77M blocks, surface 100% over 2.56M,
    carvers and decoration have their own numbers — and the composed world
    reads 99.39%. Both of the big whole-world fixes (the carver's aquifer,
    the biome manager's fuzz) lived in the gap, and neither was findable by
    any gate that measures one stage.

    So the composition needs its own gate, and it must reuse the *stage's
    own vectors* rather than get new ones: a disagreement is then not a new
    fact about vanilla but the assembly disagreeing with something already
    proved, which localises it to one boundary.
    `golden_assembly_chunk` is that gate.

14. **A margin that covers writes does not cover reads (2026-07-27).** The
    pipeline generated one ring of chunks around the requested area, on the
    reasoning that decoration writes at most one chunk outwards, so one ring
    covers everything that can write inward. That is true and it is not
    enough. The ring has to decorate *correctly* too, and decoration reads
    its neighbours — which, for the outermost ring, do not exist. So it
    scans into void, finds floors that are not there, and writes the mistake
    **inward**, into the area the margin exists to protect.

    Measured on our own output, seed 20260722, the 25-chunk window at
    (64,64):

    ```text
      1 chunk of context vs 2    604 blocks differ
      2 chunks vs 3                0 blocks differ   <- converged
    ```

    One ring was wrong by **more blocks than the window's entire
    disagreement with vanilla** (557). Every parity number taken before this
    was partly measuring its own margin.

    The general form, worth applying to any generator with neighbour-reading
    stages: **widen the context until the answer stops changing, and quote
    the converged one.** The width is not something to reason out from what
    a stage writes; it is something to measure from what it reads. A single
    ring is the natural guess and it is the one that is wrong.

### Fault 7 — a name is not a state, again (2026-07-25)

`DecorateGolden.diff` compared block states by **id** — so a write that only
changed a property counted as a change — and then recorded the block's
*registry key*. Rule 11 was written for exactly this shape and the decoration
gate still had it. Two things fell out of fixing it:

- Six ore features were writing `minecraft:redstone_ore` where vanilla writes
  `minecraft:redstone_ore[lit=false]`. A datapack names a block; vanilla's
  codec resolves it through `defaultBlockState()`, which carries every
  property the block has. The port had been carrying the bare name from the
  JSON straight into the world. `crates/rustmc-worldgen/src/feature/state_spec.rs`
  now does the resolution, from a `default` column added to the block-props
  extract, and refuses rather than guessing when a block is unknown.
- The gate's own `changes()` had to learn that several writes to one position
  collapse to one row. Vanilla diffs the world once per feature, so it cannot
  see an intermediate state; glow lichen paints a face, spreads back onto the
  same block, and paints a second, and vanilla reports one block with two
  faces. Reporting two rows would have failed a port that agreed with it.

`OreGolden` had the same lossy encoding and was fixed the same way, which
meant teaching its `setAt` to parse a state back rather than a name — the
round trip is the check that the two halves agree.

Eight faults now. Five vindicated the port, one was in the reference, and two
surfaced genuine defects once the harness stopped throwing information away.

### Fault 8 — the fluid tags were never bound (2026-07-25)

`bindBlockTags` loads every block tag out of the datagen extract and binds it
to `BuiltInRegistries.BLOCK`. Nothing did the same for fluids, so
`FluidState.is(FluidTags.WATER)` answered **false for every fluid in the
world**.

It showed up as a feature that generated nothing.
`TallSeagrassBlock.canSurvive` ends with
`fluidState.is(FluidTags.WATER) && fluidState.isFull()`, so no tall seagrass
could survive anywhere; vanilla's own dump contained not one across three
worlds, and the port — which reads the fluid from the block-props extract and
so was right — looked like it was placing 24 blocks too many.

The tell was statistical before it was structural: the config's probability is
0.3, the port chose tall 12 times in 30, and the oracle chose it zero times.
A fair coin does not come up tails thirty times, so the disagreement was in
the harness rather than in the draw.

This is the third time an unloaded tag registry has produced a plausible world
rather than an error — empty block tags made 567 carvers touch zero blocks,
an unfilled biome container made a chunk decorate nothing, and now this. The
shape is always the same: **a tag registry that was never loaded does not
throw, it says no.** Bound now in all four harnesses, from the same folder
reader.

### And one in the harness's own house

The first two runs of `ci/ore-golden.sh` produced different vectors.
`HolderLookup.listElements()` has no stable order between JVM runs, and the
position of a config in that list becomes the feature index handed to
`setFeatureSeed` — so every case drew from a different stream each time the
file was rebuilt. Sorted by name now. The rule this project already had
("no map iteration at behaviour-visible sites") applies to the oracle too,
and the way it was caught was regenerating twice and running `cmp`, which is
now what the gate's regeneration step does.

### Fault 9 — the structure registry has no stable order (2026-07-26)

`ChunkGenerator.applyBiomeDecoration` seeds each structure with
`setFeatureSeed(decorationSeed, index, step)`, where `index` is its position
in `registryAccess.lookupOrThrow(STRUCTURE).stream().collect(groupingBy(step))`.
So a structure's whole block stream depends on where it sits in a registry
listing.

Run twice against `VanillaRegistries.createLookup()`, the
`underground_structures` group comes back as

    [trial_chambers, mineshaft, buried_treasure, mineshaft_mesa, trail_ruins]
    [buried_treasure, mineshaft, mineshaft_mesa, trail_ruins, trial_chambers]

**Two runs of the same JVM on the same jar, two different orders.** This is
rule 10's fault a third time — after `HolderLookup.listElements()` churned
the feature-order vectors, and after `java.util.HashSet` iteration turned
out to be behaviour. The rule earns its place again: *every dumper
regenerates twice and `cmp`s*, and this one was caught by doing exactly
that before writing a line of port.

What it means, precisely: **`createLookup()` cannot answer this question at
all.** A real server builds its registries through `RegistryDataLoader`
rather than the datagen lookup, so the order a running game uses is not the
order this dump produces — and no amount of re-running gets it, because the
instability is in the source, not the sampling.

So the mineshaft port stops one step short of the world. Layout is done and
proved (100% over 659 real pieces); `postProcess` is written; the pipeline
hook exists. What is missing is one integer, and guessing it would put a
mineshaft's blocks on the wrong stream while leaving them looking almost
right — corridors in the right places with the wrong cobwebs. That is the
exact failure mode this project exists to refuse, so the hook is wired as a
**named no-op** and `Generated::caveats` says so.

The way out is a measurement, not a better guess: with the pipeline wired,
place the mineshaft under each candidate index and diff the blocks against
the real region. Exactly one should match. That settles the index *and*
verifies `postProcess` in the same run, which is worth more than reading the
answer off a registry would have been.

## 5d. The JVM's own data structures are part of the spec (2026-07-25)

`TreeFeature.place` collects its logs, foliage and roots into
`Sets.newHashSet()`. `TreeDecorator.Context` then sorts the logs by y with a
**stable** sort, so everything at one height keeps the set's own order, and a
beehive attaches to the first entry that qualifies. The iteration order of a
`java.util.HashSet` is therefore part of the world, in the same way a draw
count is.

It is not arbitrary — it is a pure function of the keys' hash codes and the
insertion sequence — so `crates/rustmc-worldgen/src/java_hash_set.rs`
reproduces it. What made that worth a gate rather than a unit test is that
several *wrong* reproductions are self-consistent:

- `h ^ (h >> 16)` instead of `h ^ (h >>> 16)`. Identical for every
  non-negative hash, and `BlockPos.hashCode` is negative for most of the
  world.
- A resize that rebuilds by re-inserting instead of splitting each bucket into
  a low and a high half with relative order preserved.
- `size >= threshold` instead of `size > threshold`, which resizes one entry
  early and reorders everything.

Each produces a plausible hash-table order. `ci/hash-order-golden.sh` asks the
JVM instead: six insertion sequences, 360 positions, including one at negative
coordinates specifically to catch the shift. The port matched all of them.

Treeification is refused rather than guessed at. A bucket becomes a red-black
tree at eight collisions in a table of 64 or more, and the resulting order is
a different algorithm; `insert` returns an error there instead of carrying on
with the wrong one.

## 5c. Where Rust and the JVM genuinely disagree (2026-07-25)

One measured, non-hypothetical gap, found by dumping `Math.sin`/`Math.cos`
at the angles `OreFeature.place` can produce and comparing bit for bit:

    PLATFORM SIN/COS: 22/4097 angles differ (0.537%), worst 1 ulp,
    0 of which would move a floored ore coordinate

`Mth.sin` is a lookup table and is bit-exact (all 65,536 entries). But the
ore feature calls **`Math.sin`**, the platform transcendental, on a float
angle widened to double — and the result feeds straight into doubles and is
never narrowed, so the 1-ulp slack in the specification is not absorbed the
way the table's is.

Both Rust's `f64::sin` and Java's `Math.sin` are specified to within 1 ulp
and they are not the same implementation. HotSpot uses an x86 stub rather
than fdlibm, so even matching `StrictMath` would not close it — closing it
means reproducing the HotSpot intrinsic.

The gap is pinned by `golden_mth_sin`, which fails if it grows past 1 ulp,
if it spreads beyond the tail, **or if it disappears** — the last because
that would mean the special handling is no longer needed and the note should
go. The "0 of which would move a floored ore coordinate" figure is a probe at
a spread of origins and radii, not a proof: it says the difference did not
reach the world in what was sampled, not that it cannot.

Worth stating why this was found before ore parity was measured rather than
after: a 1-ulp trig difference surfaces downstream as "ore placement is
99.99% and nobody knows why". Naming it first turns that into a checked
expectation.

### `Math.log` — the same shape, and the fix that being *more accurate* buys

`MarsagliaPolarGaussian` calls `Math.log` once per accepted sample, so every
gaussian in worldgen depends on its last bit. `golden_gaussian` (new) found
Rust's `f64::ln` disagreeing with the JVM on **231 of 300,000** inputs over
`(0, 1)`, always by exactly 1 ulp.

Four measurements, in order, because each one changed what the fix should be:

1. `Math.log` and `StrictMath.log` **disagree on 7.3%** of inputs in `(0, 1)`
   — 365,590 of 5,000,000. They are different functions, so "port fdlibm" is
   a choice about *which* answer to reproduce, not a way to be correct.
2. `Math.log` is **stable across 2,000,000 calls** to one site and differs
   from `StrictMath.log` from the first call. The intrinsic is in play
   immediately and does not switch as the JIT warms up — so vanilla is not
   quietly non-reproducible within a run, which was the first thing worth
   ruling out and is not obvious: `Math.log` is *declared* as a delegation to
   `StrictMath.log`, and only the intrinsic makes it something else.
3. Over 4,000 inputs where the two Java logs disagree, `Math.log` was the
   correctly-rounded one **every time**. That said "implement correct
   rounding and you match the JVM" — and it was **wrong**. The sample was
   drawn from the disagreeing subset, which is precisely where the better
   implementation wins. A biased sample answering a question it was not
   drawn to answer.
4. Over 300,000 *unfiltered* inputs, a correctly-rounded logarithm differs
   from `Math.log` on four — and on all four an 80-digit reference says the
   JVM is the one that is wrong.

So `Math.log` is within 1 ulp of the truth, deterministic, and not correctly
rounded. Bit-for-bit agreement is not reachable by being more accurate; it
needs HotSpot's x86 `dlog` stub ported as such, the same conclusion `Math.sin`
reached above.

**What was done instead, and why it is not a compromise.**
`crates/rustmc-worldgen/src/precise_log.rs` is a correctly-rounded `ln` in
double-double arithmetic. It is a target that does not move with the JVM, the
architecture or the JIT state, and it is strictly closer to the reference than
the standard library: 299,996 of 300,000 against 299,769. On the gate's own
2,730 values it now reads **100.000000%**, where `f64::ln` read two wrong.

The residual cannot desynchronise anything, and that is the part worth being
precise about rather than reassuring about: the rejection loop draws only
`nextDouble` and never touches the logarithm, so a 1-ulp difference changes a
*value* and never a *draw count*. It reaches the world only through
`ClampedNormalFloat` -> a cast to `int`, which it can move only when the
value lands within 1 ulp of an integer boundary.

**A note on the gate's shape.** `golden_gaussian` asserts everything that
decides draw counts — the stream position after 256 gaussians, the
interleaved longs and doubles, `setSeed` discarding the cached half of a pair
— with no latitude at all, and separately asserts a 1-ulp ceiling on every
value plus an exact count of how many differ. That is not a widened
tolerance: the two kinds of failure have different meanings and the gate
distinguishes them, so a structural fault cannot hide inside a rounding
allowance.

## 5e. The fancy tree placer's arithmetic, measured before it was ported
(2026-07-25)

`FancyTrunkPlacer` is the only worldgen code that calls `Math.sin`,
`Math.cos`, `Math.pow` and `Math.sqrt` on values it computed itself and then
**floors** the result into a block coordinate. §5c measured sin and cos at the
*ore* feature's angles; the fancy placer's are a different set with larger
multipliers, so the conclusion had to be re-earned rather than inherited.

    FANCY MATH: sqrt 1089/1089 exact; pow 83/83 exact;
                sin 4093/4096, cos 4089/4096 exact, worst 1 ulp

Read in order of how much each matters:

- **`sqrt` is exact and had to be.** IEEE 754 requires a correctly rounded
  square root, so a difference there would be a bug in this port or in the
  harness and never in the platform. The gate asserts it outright, with no
  tolerance — it is the fixed point the other two are judged against.
- **`Math.pow(x, 2.0)` is exact** on every height the codec's ranges allow.
  Not guaranteed by anything, but an exponent of two is a single multiply in
  any sane implementation, and now it is measured rather than assumed.
- **sin and cos differ on 10 of 8,192 values, by one ulp.** That is the same
  shape as §5c and it is the reason the gate reports rather than only
  asserting: the number is a standing measurement of how much of a fancy
  oak's shape is at the mercy of libm.

What is *not* yet measured is whether a one-ulp difference survives the floor.
The placer computes `length * sin(angle) + 0.5` with `length` up to about 17,
so a one-ulp error arrives at the floor around 2e-15 wide — it moves a block
only if the value sits that close to an integer. §5c could rule that out by
enumerating the ore feature's whole input set; the fancy placer's lengths come
from the stream and cannot be enumerated the same way, so the check belongs
with the placer and is written down here as owed rather than done.

The gate exists before the code it guards, deliberately. A one-ulp divergence
discovered *after* the placer lands looks like a placer bug for as long as it
takes to suspect the platform.

## 6. Determinism contract (lesson bought today, 2026-07-19)

The vibenilla adopt was measured five times and produced five numbers
(±0.04pp): its cross-chunk decoration **spill model branched on chunk
completion order** under parallel generation — later found to be silently
*losing* border writes, not just reordering them. Sequential defined-order
generation made it bit-identical (99.419259%, twice) and *raised* the number
by 0.67pp. Conclusions, baked into rustmc's worldgen contract from day 0:

1. **Generation output is a pure function of (seed, chunk pos, datapack
   version).** Never of scheduling, load, thread count, or map iteration
   order (`HashMap` iteration leaking into world bytes is a bug by
   definition; use ordered containers at behavior-visible sites).
2. **Cross-chunk decoration runs on a shared canvas in a defined chunk order**
   (Minecom measured z-major scan winning for the overworld; the order is a
   named constant, A/B-able, per-dimension).
3. **Parallel worldgen must be provably order-equivalent**: parallelize only
   stages proven independent (noise/terrain per-chunk), decorate in canonical
   order, and CI proves it — **two identical-seed full runs, byte-compared,
   under load** is a standing merge-train check, not a one-off.
4. Any nondeterminism found later is a **stop-the-line bug** (it invalidates
   every measurement taken on top of it — today's near-miss: an order "win"
   almost got wired in off two runs of a nondeterministic measure).

## 7. Gate summary (what CI enforces, see CI.md)

- **G1** per-crate unit + golden-vector tests (fast, every change). Includes
  the two **seam** gates, which are not stage gates and are the ones that
  catch what §5b rule 13 is about: `golden_assembly_chunk` (the assembly's
  own output against the surface stage's vectors) and `golden_gaussian` (the
  RNG primitive whose *cost* is variable).
- **G2** PlayTest Class A+B shards vs rustmc binary (merge gate)
- **G3** region-diff overworld r3 (cheap cut) / r18 (merge-train, ratchet
  up-only from the inherited number)
- **G4** determinism: double-run byte-compare (merge-train)
- **G5** bench smoke vs Paper baseline (weekly, not per-merge)

## 15. Count what the oracle counts, not what looks countable

`DecorateGolden` reports `draws`, and the obvious reading — "how far the
generator advanced" — is wrong twice. It counts `RandomSource` **method
calls**: `nextInt(bound)` is one call however many times its rejection loop
retries, and `nextGaussian` is one call however many `nextDouble`s Marsaglia
spends, because Java's gaussian holds the *inner* source and its draws never
reach the wrapper. Ours calls back into the same object, so a naive counter
would score a gaussian as 2 or more and every comparison would be noise.

Match the oracle's semantics before comparing against its number, and prove
you did: three of four sulfur features matched vanilla's count exactly on the
first run, which is what told us the counter was right and the fourth
feature's `+7` was real rather than an artefact of our own counting.

## 16. A regenerated oracle the gate does not read is not a gate

`ci/gaussian-golden.sh` writes `data/vanilla/gaussian.json` by default.
`tests/golden_gaussian.rs` reads `crates/rustmc-worldgen/golden/gaussian_vectors.json`.
A new case was added to the dumper, the script was run, the file was
regenerated — and the test went green **without ever executing the new case**,
both with the fix and with the bug deliberately restored.

The only reason this was caught is that the new gate was checked for being
load-bearing: the defect it exists to detect was reintroduced on purpose and
the gate was required to fail. It did not, which is what exposed the path
mismatch rather than the behaviour.

Regenerate into the path the test opens, and prove every new gate fails
without its fix. A gate that has never been seen to fail has not been shown
to test anything.

## 17. One vanilla world is not the answer key

Vanilla does not generate one world per seed. Generated as a square centred at
(64,64) and as rows, seed 20260722, the same 225 chunks, two **vanilla** worlds
differ by **21,556 blocks** — and about 3,550 of those are kelp and seagrass,
in both directions.

So a mismatch against a single reference is not evidence of port error. It was
taken as evidence here: the kelp column at (977, 48..58, 955) survived all six
of our decoration orders, matched our isolated `DecorateGolden` replay exactly
(57 attempts, 4,982 blocks), and still disagreed with `vanilla-r7`. The
resolution was not in our code at all — `van-rows-c64` has kelp_plant in that
exact column, which is what we produce.

**The test is not "does it survive our orders" but "does it differ from every
vanilla world we have".** Applied to the 121-chunk window: of a 226-block
order-invariant core, **122** hold a value that matches the other vanilla world
exactly, and only **104** differ from both.

Subtracting whole *positions* where vanilla is unstable is not good enough
either — at such a position a port can still produce a third value neither
vanilla world has. Compare the values.

## 18. Post-processing IS generation (2026-08-02)

A saved region file is not "what the generator wrote". It is what the generator
wrote **plus one synchronous pass of block and fluid logic**, and that pass is
part of chunk generation, not part of the game loop. Anything that assumes
"the server did not tick, therefore only the generator ran" is wrong by exactly
this pass.

`LevelChunk.postProcessGeneration(ServerLevel)` — called from `ChunkMap` when a
chunk is promoted to FULL — walks the chunk's `postProcessing` short-lists and,
for every marked position, does:

```text
  FluidState fs = state.getFluidState()
  if !fs.isEmpty()             ->  fs.tick(level, pos, state)          SYNCHRONOUS
  if block instanceof LiquidBlock ->  state.tick(level, pos, random)   SYNCHRONOUS
  else                         ->  Block.updateFromNeighbourShapes(...)
                                   level.setBlock(pos, ..., 276)
  finally                      ->  ShortList.clear()
```

Positions get marked by `ChunkAccess.markPosForPostProcessing`, whose callers in
the 26.2 jar are `NoiseBasedChunkGenerator` (aquifer fluid), `WorldCarver`,
`SurfaceSystem`, `Feature`, `StructurePiece`, `MultifaceGrowthFeature`,
`Blender` and `WorldGenRegion`.

**Consequences for the oracle.**

* `tick freeze` does **not** suppress it, and must not. It is off the game loop.
  A frozen harvest is still a post-processed harvest, and that is the correct
  reference — every vanilla world that has ever loaded that chunk has it.
* Flowing water in a reference is therefore **generation output**, and it counts
  against us. It is not simulation drift and it is not harness contamination.
  This was measured, not assumed: see below.
* A residual made of non-source fluid, of `CrossCollisionBlock` connection
  states, or of multiface (`glow_lichen`) faces should be read as a missing
  post-processing pass **before** it is read as a missing feature. Those are
  exactly the three things this pass produces.

**How to tell generation output from simulation drift**, since a block diff
cannot: `ci/chunk-ticks-probe.py`. Two readings decide it, and they are
independent.

1. **Did the loop turn?** Look at the pending tick lists' `t`, which is the
   delay still remaining at save time. A tick sitting at `t=0` was **due** and
   never executed; one turn of the loop consumes every one of them. If ticks
   are queued at `t=0`, the loop never turned.
2. **Did post-processing run?** Look at `PostProcessing`. Clearing those lists
   is the last thing the method does, so *present but empty* means it ran.

Measured on `oracle-work/vanilla-r7` (seed 20260722, r=7, centre (64,64),
chunks (57,57)..(71,71), 225 chunks, all `minecraft:full`):

```text
  block_ticks cave_air     13852  t=0     due, never executed
  fluid_ticks water          426  t=0     due, never executed
  fluid_ticks lava            27  t=0     due, never executed
  fluid_ticks water         5522  t=5
  block_ticks sand          1115  t=2
  fluid_ticks flowing_water   26  t=5     freshly rescheduled by the spread
  PostProcessing entries       0          cleared => the pass ran
  non-source fluid            26  = 21 water[level=1] + 5 water[level=8]
```

The loop never turned and the pass still ran. The run's own log agrees: "The
game is frozen" at 02:32:46, the forceload at 02:33:16. Corroborating shape —
all 26 sit exactly one spread step from a source (`level=8` directly under a
`water[level=0]`, `level=1` beside one) and each carries its own freshly
scheduled `t=5` tick, which is what `FlowingFluid.spread` leaves behind. No
feature writes those levels; flow logic computed them.

**Rule 6 still applies to them.** Against `vanilla-r7-b` the twins agree on
**24** of the 26, not all of them: `(991,-55,1136)` and `(991,-54,1133)` are
r7-only and `(991,-55,1134)` is r7-b-only — one cinnabar pocket at
x=991, y=-55..-54 whose carved shape vanilla does not reproduce run to run.
Zero state disagreements at any shared position. So the honest figure for that
window is **24 twin-agreed non-source fluid blocks**, and they are ours to
close.

**Read the `fluid_ticks lava 27` line above with the r=16 window beside it —
2026-08-10.** Every non-source block in this r=7 table is water, and
`stage/fluid.rs` was scoped to water only on exactly that evidence, saying so
and instrumenting the omission. On the r=16 window that scope is wrong: it
holds **24 `lava[level=2]` blocks, all eight references unanimous**, sitting
one overworld spread step (`getDropOff` 2, so `8 - 2 = 6`, so `level=2`)
outside lava lakes whose source sets we already reproduced exactly. The pass
declined **5,736** marked lava positions over that window while doing it, so
the omission was live and counted rather than hidden — the instrument was
right and the window it was read on was too small. Both fluids are ticked
since `2b8b8848`. **The general lesson is the cheaper one: a scope decision
justified by a measurement inherits that measurement's window, and needs
re-taking when the window grows.**

## 19. A post-processing mark is not a promise about the block (2026-08-02)

§18 establishes *that* `postProcessGeneration` runs and counts against us.
This is *how* it behaves, because porting it turned up one property that no
reading of `FlowingFluid` alone would have suggested and that a port will get
wrong by default.

**The mark and the block are decoupled.** `NoiseBasedChunkGenerator.doFill`
marks a position only when it holds a fluid — offsets 455-492 are
`aquifer.shouldScheduleFluidUpdate() && !state.getFluidState().isEmpty()`, and
there is no second condition. But the mark is stored on the `ProtoChunk` and
the pass does not run until FULL, which is **after** carvers, surface,
structures and features have all rewritten the same blocks. Measured on our
own r=7 generation at seed 20260722 with `RUSTMC_DUMP_POST_MARKS`:

```text
  marks       54001
  cave_air    32323   fluid when marked, carved away afterwards
  water[0]    14478
  oak_planks   2168
  lava[0]       653
```

**Three-fifths of the marks stand on a block that is not a fluid.** A pass
that assumes a mark implies a fluid is not wrong on those — it skips them —
but it is wrong about the ones an *earlier* mark refills.

**The order is behaviour, and so are the duplicates.** `ProtoChunk` keeps
`ShortList[] postProcessing`, one list per section;
`markPosForPostProcessing` is a bare `ShortArrayList.add` with no dedup; and
`LevelChunk.postProcessGeneration` (offsets 5-180) walks the sections in
ascending index, each list through its own iterator, **applying every write
before it reads the next position**. Three consequences:

* A position marked twice ticks twice.
* A set — which is what `Canvas::post_processing` was — cannot express either
  the order or the duplicates.
* A marked position that an earlier mark's spread has **refilled** ticks with
  the refilled state, and spreads again from there.

That last one is the mechanism behind vanilla's two-deep `water[level=8]`
column at (947,10,1082) and (947,11,1082) in `oracle-work/vanilla-r7`. No
single synchronous spread reaches it: a source can never be spread *into*
(`canMaybePassThrough` refuses `isSourceBlockOfThisType`) and a source can
never demote itself (`tick` only calls `getNewLiquid` when `!isSource`), so
the middle block was air when the pass began, was filled by the mark above it,
and then ticked on its own mark.

**Two hypotheses this killed, both plausible and both false:**

* *"vanilla must mark air positions too."* It does not. The condition is
  quoted above.
* *"the fluid ticks twice per mark, since the method calls `FluidState.tick`
  and then `BlockState.tick`."* It does not. `LiquidBlock.tick` in 26.2 is a
  single `shouldBubbleColumnOccupy` test and never reaches the fluid.

**The reading for the oracle.** A residual made of fluid is not evidence about
the flow rules until the mark *set* has been read. The marks are invisible in
a region file — a marked-and-skipped position writes exactly what an unmarked
one writes — so `RUSTMC_DUMP_POST_MARKS=<path>` exists to make the set itself
readable before anything acts on it, and it is what turned a two-hour
contradiction into a one-line answer.

## 20. Vanilla's decoration read bound is chunk radius 8 (2026-08-02)

Settled from `net/minecraft/world/level/chunk/status/` bytecode, because this
had been an open hazard with two unevidenced numbers attached to it.

**Where the radius lives.** Not on `ChunkStatus`. It is an argument to
`ChunkStep$Builder.addRequirement(ChunkStatus, int)`, called from the
`UnaryOperator` lambdas that `ChunkPyramid`'s static initialiser passes to
`Builder.step`. For the FEATURES step, `ChunkPyramid.lambda$static$7`:

```text
   1: getstatic  ChunkStatus.STRUCTURE_STARTS
   4: bipush 8
   6: invokevirtual ChunkStep$Builder.addRequirement:(...;I)
   9: getstatic  ChunkStatus.CARVERS
  12: iconst_1
  13: invokevirtual addRequirement
  16: iconst_1
  17: invokevirtual blockStateWriteRadius:(I)
```

`addRequirement` grows a `ChunkStatus[] directDependenciesByRadius` to
`radius + 1` and max-merges, so FEATURES' direct dependencies are
`[CARVERS, CARVERS, STRUCTURE_STARTS x7]` — **size 9, radius 8**, write radius
1.

**The 8 is `MAX_STRUCTURE_DISTANCE`.** `ChunkStatus` declares
`public static final int MAX_STRUCTURE_DISTANCE = 8` as a `ConstantValue`,
which is why it appears inlined as a bare `bipush 8` in six of the twelve
generation steps. Structure starts are the reason for the radius —
**but the structure-starts step's own radius is 0**; `lambda$static$1` is
`setTask` and nothing else. "Decoration is bounded at 8 by structure starts"
is true; "structure starts have radius 8" is not.

**Where it is enforced.** `WorldGenRegion.getChunk(int, int, ChunkStatus,
boolean)` computes `ChunkPos.getChessboardDistance` and compares it against
`generatingStep.directDependencies().size()`:

```text
   9: invokevirtual ChunkPos.getChessboardDistance:(II)I
  26: if_icmplt  ...            d < size(9) ? continue : fall through
  99: new java/lang/IllegalStateException
 102: ldc "Requested chunk unavailable during world generation"
 299: athrow
```

`getBlockState(BlockPos)` routes through `getChunk(x, z, ChunkStatus.EMPTY)`,
so an ordinary block read during decoration is subject to it. **A decoration
read at Chebyshev chunk distance >= 9 throws.** Between 2 and 8 it succeeds
but is capped at `STRUCTURE_STARTS`-level content; at 0..1 at `CARVERS`-level.
Reads past the write radius but within 8 only log.

**The accumulated radius is 10 and is a different question.**
`ChunkStep$Builder.buildAccumulatedDependencies` shifts the parent's array
outward by the parent's own radius and max-merges, giving FEATURES
`[CARVERS, CARVERS, BIOMES, STRUCTURE_STARTS x8]`, size 11, radius 10. That is
how far out chunks must *exist* before FEATURES may run, not how far it may
read. Do not quote it as the read bound.

**Our side, and the shape of the mistake.** Our reported reach of 39 was never
a constant — `RUSTMC_READ_REACH` measures it and nothing sets it. It came from
two structure placers running a heightmap walk *before* testing whether the
piece touches the chunk, so every chunk on the canvas sampled every piece
wherever it was and the number tracked the square rather than the features.
`StructureStart.placeInChunk` tests `piece.getBoundingBox().intersects(box)`
at offset 105 and calls `piece.postProcess(..)` at 124; every heightmap read
lives inside `postProcess`. Fixed; the reach is 1 at r=7 and r=16.

**The generic lesson, which is rule 7 again.** A maximum tells you how far
something reached and never what reached. `RUSTMC_READ_REACH_TRAP=<n>` panics
on the first access beyond `n` so the stack names the call site, and it took
two thresholds at two radii to find that there were **two** offenders and not
one — the second was invisible at r=7 because the first dominated there.

**And the correction to a prediction.** Closing this bought **no** generation
time: decoration CPU at r=16 over three runs each is 15.13 CPU-s before and
15.02 after, flat inside the spread. A parity hazard is not automatically a
cost worth reclaiming, and the block output was byte-identical before and
after — so this was a latent hazard, never an active defect on these windows.

## 21. The aquifer answers the block; the marks answer the water (2026-08-02)

§19 says a post-processing mark is not a promise about the block. This is the
converse, and it is the one that costs turns: **a block of water is not
evidence about whatever placed water.**

**The case.** Six blocks at y=16 in a cave at x=971..973 — three
`water[level=0]` sources vanilla has and we did not, three `water[level=1]`
beside them. Sources at a fixed height, on a cave floor, one block apart. Every
reading of that shape says aquifer: a fluid level is exactly a "water below
this y, air above it" constant, and three sources sharing a y is what a wrong
one looks like. The brief said aquifer, and the aquifer is perfect.

**What it actually was.** `WorldCarver.carveBlock`'s
`markPosForPostProcessing`, unported. The carver had already placed the water
body correctly — a connected sheet of sources thirty blocks long, matching
vanilla row for row — and vanilla's `postProcessGeneration` then extended it
one block west off marks the carver set. We never set them, so the sheet
stopped one block short along its whole western fringe.

**The tell was the shape, and it was visible before any code was read.**
Printing the y=16 slice of both worlds side by side took one script and showed
a *rim*. A wrong fluid level loses a whole lattice cell of water — sixteen
blocks square — because the level is a property of the cell. Losing the
boundary blocks of a body you otherwise reproduce exactly is a property of a
*propagation*, not of a threshold. Slice before you sample: a residual listed
as coordinates hides its own shape, and the shape is the diagnosis.

**Two vanilla facts worth keeping.**

* `getCarveState` (offsets 0-23) returns lava below `config.lavaLevel`
  **without calling `computeSubstance`**, so `shouldScheduleFluidUpdate` is
  stale in that branch and `carveBlock` marks on the *previous* position's
  flag. It is a field, not a return value, and reproducing that staleness is
  the reason `CarveTarget` has a separate `should_schedule_fluid_update()`.
* `FlowingFluid.getSpread` calls `getNewLiquid` **at each neighbour**, and
  `getNewLiquid` returns a source on two source neighbours over a solid block.
  So a `water[level=0]` source can appear at a position that was never marked
  and never held fluid. "It is a source, therefore something placed it as a
  source" is false.

**The instrument gap this exposed.** The aquifer had no vanilla-side probe at
all. Our `RUSTMC_TRACE_AQUIFER` printed twelve candidate centres and their
levels; vanilla's side could only be inferred from finished region files,
which show the winning centre and never the numbers behind it. That asymmetry
is what let an aquifer hypothesis survive three briefs. `ci/aquifer-probe.sh`
closes it: `centre:gx,gy,gz` for the jittered lattice centre, `spread:cx,cy,cz`
for the `fluid_level_spread` waterline with its `* 10` and
`Mth.quantize(..., 3)`, `flood:x,y,z` for floodedness, erosion and depth. At
seed 20260722 every one of the twelve contending centres and both fluid levels
agreed with our trace to the digit — which is what turned "probably the
aquifer" into a fact, in one run.

**The rule.** When a residual is made of fluid, the order of suspicion is:
the mark set, then the propagation, then the thing that placed the fluid. It
has now been the first of those three times running.

## 22. A filter that hides a defect is worth more than the defect (2026-08-02)

Two defects sat on top of each other in `postProcessGeneration`'s shape half,
and the outer one made the inner one unobservable. Getting both took one
measurement and then one bytecode read; getting either alone was impossible.

**The outer one.** `post_process::run` tested
`tags.contains("minecraft:fences", block)` before folding
`updateFromNeighbourShapes`. Vanilla's only test is `!(block instanceof
LiquidBlock)`. The fold already returned every unmodelled family untouched, so
the family gate could never change an answer the fold would have given — it
was pure suppression. It made the stairs, door and speleothem rules
unreachable from the pass no matter how many positions were marked, and
`shapes=221` out of **65,741** marks was the number that should have been read
as a symptom and was not.

**The inner one.** `StructurePiece.CAVE_AIR` is a `BlockState` field holding
the fill block. `SHAPE_CHECK_BLOCKS` is a twelve-member `ImmutableSet` built
three instructions later in the same static initialiser. Our
`needs_shape_check` had `cave_air` as a thirteenth member. A mineshaft writes
cave air by the thousand, so the post-processing list carried thousands of
positions vanilla never marks.

**Neither is visible alone.** With the family gate in place, a spurious
cave_air mark is skipped and writes nothing — the world is byte-identical
either way. Remove the gate and the spurious marks become wrong blocks
immediately: three `sulfur_spike` tips demoted that vanilla leaves merged. The
first measurement after widening the pass therefore *looked like a
regression*, 5 fixed and 3 broken, and the temptation was to narrow the gate
again. Narrowing would have restored the number and kept both defects.

**The reading for the oracle.** When a change that is right by the bytecode
makes a measurement worse, the new damage is evidence about something else,
not an argument against the change. Both here were provable from the jar in
minutes once the question was "what did widening expose", and the pair fixed
eight blocks: the five spikes it was aimed at and all three `oak_fence` blocks
of a cluster nobody was working on.

**The generalisation, which is rule 7 turned around.** Rule 7 says: when a
measurement clears every candidate, suspect the measurement. This is the
mirror: **when an instrument has never reported a family, suspect the filter
in front of it, not the absence of the family.** A conservative gate written
as "we have no rule for this yet" ages into a claim that there is nothing to
find, and it stops being conservative the moment a rule is added behind it.
Count what a pass *skips* as well as what it does; `shapes=221 / marks=65,741`
was the whole finding, printed every run, for weeks.

**Two vanilla facts worth keeping.**

* `UPDATE_SHAPE_ORDER` is `{WEST, EAST, NORTH, SOUTH, DOWN, UP}`, from
  `BlockBehaviour`'s static initialiser. It had never mattered because every
  family modelled here folds order-free. `SpeleothemBlock` does not:
  `calculateSpeleothemThickness` reads `isTipMerge` off the state it was
  handed, so DOWN's answer is UP's input. A fold order that "does not matter"
  is a property of the families in it, not of the fold.
* 26.2 merged `PointedDripstoneBlock` into `SpeleothemBlock` and
  `calculateSpeleothemThickness` gained `&& blockState.is(this)`: two tips
  meeting head-on only merge if they are the **same block**. With
  `#minecraft:speleothems` now holding both `pointed_dripstone` and
  `sulfur_spike`, that test is reachable.


## 23. Vanilla's post-processing mark set is readable after all (2026-08-02)

§19 says a mark is invisible in a region file, and this file has repeated that
three times. It is true of the window we measure and false of the reference
that contains it, and nobody looked.

**`postProcessGeneration` clears the lists and nothing else does.** So a chunk
saved at FULL whose pass never ran keeps them. In a forceloaded square the
interior is post-processed and the border ring is not, which makes the border
ring vanilla's own mark set — position by position, duplicates included.
`oracle-work/vanilla-r7`, which has been the overworld reference for weeks:

```text
  chunks 57..71   the measured window     PostProcessing entries left:     0
  chunks 55..56   west border strip                                     5383
  chunks 72..73   east border strip                                     2436
```

The zero is what made §18's "the pass ran" conclusion right and simultaneously
hid the rest. One command over a wider box would have shown both.

**The decode.** `ProtoChunk.packOffsetCoordinates(BlockPos)` is
`x&15 | (y&15) << 4 | (z&15) << 8`; the lists are one `ShortList` per section
in ascending section order, so the list index carries the section and the
short carries only the offset within it. `ci/chunk-ticks-probe.py marks`
prints `x y z state` — deliberately the same shape as
`RUSTMC_DUMP_POST_MARKS`, so `comm` compares the two sets directly.

**What it said the first time it was run.** Ours generated at r=12 so the
border ring sits well inside our own decoration margin, seed 20260722, same
x-range:

Restricted to **chunk column 55**, which is where the lists actually survive.
Chunk 56 *was* post-processed and cleared, so a box spanning both compares an
uncleared chunk against a cleared one and manufactures differences in both
directions. That mistake was made first, published, and corrected, and it is
the same shape as the finding itself: the box was chosen before the question
was.

```text
  vanilla  5154 entries over 4296 distinct positions
  ours      304 distinct positions
  in vanilla and not ours  3992     in ours and not vanilla  0
```

The zero in the other direction is what makes the rest usable: nothing we mark
is wrong, so every one of the 3992 is a missing call rather than a
disagreement. The 858 entries vanilla has beyond its distinct positions are duplicates, and
that is §19's "`markPosForPostProcessing` is a bare `add` with no dedup"
turned from a bytecode reading into a count.

**Our mark set is a strict subset of vanilla's and it is 7% of it.** All three parity items closed
this turn were a missing mark rather than a wrong rule — the carver's, the
structure's over-marking of cave air, and the pass's own family filter — and
each was found by working backwards from one wrong block. There are four
thousand more marks unaccounted for in one strip.

**The reading for the oracle, and it is the expensive one.** "Invisible" was a
property of the *box being measured*, not of the format. The residual window
was chosen for the twin and the order sweep, and every instrument since has
been pointed at it; the reference around it was never asked a question. When a
quantity is declared unobservable, check whether it is unobservable *here* or
unobservable *at all* — the second is rare, and the first is a box you chose.

**The caveat, stated so it is not overclaimed.** A border chunk is at the edge
of vanilla's generated area, so features from chunks beyond it never reached
in and its mark set is if anything an *under*-count. That biases the 3992 the
safe way: it is a floor on what is missing, not a ceiling. It showed up
exactly where predicted the moment the gap started closing: of 2063 marks we
make in chunk column 55, 28 are ones vanilla does not have, and all 28 sit on
that column's west and east edges.

**And the trap, which is the same fact wearing the opposite sign.** The border
ring is a valid oracle for **marks** and an invalid one for **blocks**. Its
`postProcessGeneration` never ran — that is the whole reason the lists
survive — so its blocks are the *pre-pass* world: no fluid has flowed, no
shape has been folded, no lava has turned to obsidian. Diffing our finished
chunks against it reports **6,636** twin-masked mismatches in one chunk
column, against 1,083 across the entire fifteen-by-fifteen measured window,
and every one of them is the pass we ran and vanilla did not.

Both halves of that are usable and they must not be mixed:

```text
  marks   pre-pass on both sides    valid    this is the oracle
  blocks  pre-pass vs post-pass     invalid  6636 phantom mismatches
```

The state printed beside each mark is likewise vanilla's *pre-pass* block, so
"the block standing on a mark" means something slightly different on the two
sides. It is still the right way to sort the missing set by cause — §19 says
the block never identifies the caller anyway — but it is not a block
comparison and must not be read as one.

## 24. The border ring is only a mark oracle where it is also a block oracle (2026-08-02)

§23 found that vanilla's post-processing mark set is readable off the border
ring, and that is right. What it then did with the finding was not, and the
error cost this project a number it quoted for two turns: **"our mark set is
7% of vanilla's"**, later 48%, both measured on a chunk column whose world we
do not reproduce.

**The mistake, exactly.** §23 picked chunk column 55, decoded both mark sets,
found 3,992 positions vanilla marks and we do not, and read every one of them
as a missing call. The column's **blocks** were never compared. They differ by
6,688:

```text
  989  tuff_bricks  ->  cinnabar        vanilla builds a trial chamber there
  662  air          ->  sulfur          we build a sulfur cave
  547  tuff_bricks  ->  sulfur
  450  air          ->  cinnabar
  226  andesite     ->  diorite         and the ore blobs disagree too
```

Vanilla's own twin differs from it by **444** over the same column, with **no
`tuff_bricks` disagreement at all**. So the structure is reproducible in
vanilla and absent from ours, and the two mark sets being compared belonged to
two different worlds. §23 attributed the 6,636 it saw to "the pass we ran and
vanilla did not". A fluid pass does not turn tuff bricks into cinnabar.

**Why the ring is the worst place to compare and the only place to compare.**
Both at once, and that is the whole trap. Marks survive only where
`postProcessGeneration` never ran, which is the outermost chunks that reached
FULL. Those are also exactly the chunks where the two runs' generated extents
disagree: vanilla's ring has no decorated neighbour beyond it, and ours does.
Regenerating ours at r=7 so the extents match does **not** fix it — measured,
missing 979 and extra 332, against 2,261 and 28 at r=12. The ring is not one
world seen twice.

**The instrument.** `ci/mark-oracle.py` keeps only the border chunks that are
uncleared **and** twin-masked byte-identical to us, and throws the rest away
with the count of what it threw:

```text
  oracle chunks   24 clean, 47 rejected for 11,410 marks in blocks we
                  do not reproduce
  vanilla         4,034 positions BOTH vanilla runs mark
  MISSING 85 -> 11        EXTRA 0
```

Fewer marks and every one a fact. 85 of 4,034 is a 97.9% mark port, not 7%
and not 48%, and 74 of the 85 were one feature.

**And the mask has to cover the marks, not just the blocks.** The first
version of this script masked the blocks against the twin and compared the
marks raw. It reported two marks as ours-and-not-vanilla's, at (1179,26,1022)
and (1180,6,1017). Vanilla's first run has `air` at both; vanilla's second has
a glow lichen at both. A position vanilla does not agree with itself about is
not evidence about us in **either** channel. Rule 6 is not a property of the
block comparison; it is a property of every comparison.

**The generalisation.** §23's own closing line was "when a quantity is
declared unobservable, check whether it is unobservable *here* or unobservable
*at all* — the second is rare, and the first is a box you chose." The box it
then chose was wrong for the opposite reason: not too small, but not
*validated*. **An oracle has to be shown to be an oracle.** Before a reference
window is allowed to accuse the port of anything, ask it the question you
already know the answer to — here, "do the blocks match?" — and throw the
window away if it fails. That check costs one command and it was never run.

## 25. Three placeholders of one shape, and what they cost (2026-08-02)

Worth its own section because it is now a pattern with four instances, and the
fourth was found by looking for the pattern rather than by measurement.

```text
  DiskWorld::mark_above_for_post_processing   no-op default body on the trait
  post_process::run                           tags.contains("fences", ..) gate
  LakeFeature's two mark sites                "neither writes a block, so
                                               neither is here"
  MultifaceGrowthFeature's two mark sites     "markPosForPostProcessing
                                               changes no block"
```

Every one is a true statement about the call and a false statement about the
system. A mark writes no block; the **pass** it feeds writes blocks. A default
that does nothing is not neutral once a caller depends on it. Each of the four
was invisible in every block diff for as long as it stood, because a mark that
is never made and a mark that is made and skipped produce the same region
file.

**The fix is structural, not textual.** `mark_above_for_post_processing` on
`DiskWorld` and `LakeWorld`, and `mark_pos_for_post_processing` on
`MultifaceWorld`, are now **required** trait methods with no default. An
implementor with nowhere to put a mark has to write that down in its own body
— `golden_decorate`'s counts them into `marks_seen` and drops them, which is
the honest version of the same decision and is greppable.

**And the one that is not yet closed.** `spread_all`, `spread_from_face_toward
_random_direction` and `spread_from_face_toward_direction` now carry vanilla's
`markForPostprocessing` boolean rather than assuming it. Every call site in
this port passes `true`, because every one of them is world generation —
`SculkVeinBlock.attemptUseCharge` passes `spreader.isWorldGeneration()` — but
the argument exists so that the first caller that is not generation cannot get
it silently wrong.

**The reading for the oracle.** A comment that explains why something is
absent is a claim, and claims get measured. When one says "this cannot matter
because it changes no block", the question to ask is *what reads it* — and if
the answer is a pass that runs later, the comment is describing the call and
not the consequence.

## 26. Take the randomness out of the projection, and make something else claim the mechanism (2026-08-05)

A check whose reference cannot reproduce itself is not a check. There are four
things you can do about one and only the last is honest.

The corpus reached this through the chest lid.
`ContainerOpenersCounter.recheckOpeners` re-arms every `CHECK_TICK_DELAY` = 5
ticks and calls `openerCountChanged` **unconditionally**, so an open chest
emits the same `block_event(1, 1)` every 250 ms for ever. `verdict_for` is
exact sequence equality, so how many of those fall inside `settle_ms` *was* the
check — and vanilla, recorded twice, disagrees with itself about it. The two
chest checks were declared unstable at 1/25 and 3/25 and one of them had
already turned a merge gate red at random.

**What does not work, and why each fails on its own terms:**

* **Widen the accepted verdict set.** Nothing links the widening to a measured
  rate, so nothing stops it being widened to make a red go away — and it cannot
  distinguish the cause: a wrong `b1` lands on `diverge` too.
* **Suppress a difference that falls on the declared field.** For these two
  the declared field is the *entire* projection, so suppression converts the
  check into a no-op precisely for the checks it was added to protect.
* **Re-run and take the modal verdict.** Cost, and worse: the corpus is one
  connection through one world, so replaying a scenario advances
  `ServerPlayer.containerCounter` and shifts every later menu check. The
  mechanism corrupts exactly what it was added to stabilise.
* **Declare it unstable and move on.** This is the status quo, and its cost is
  that the check is capped below `match` for ever — the ratchet can express a
  floor, not a ceiling.

**What works: remove the dimension no check claims to test.** `trace::decode`
already did it once, decoding `ClientboundSoundPacket` purely to throw away the
`long seed` `Level.playSound` draws from `soundSeedGenerator.nextLong()`.
`collapse_repeats = ["block_event"]` is the same move: a run of identical
consecutive observations compares as one, every field still compared, and the
reference becomes deterministic — so `match` becomes a floor one run can
support.

**THE CONDITION, WHICH IS THE WHOLE RULE.** An exemption that removes a signal
without something else claiming the mechanism manufactures a check that cannot
fail. This corpus has found fifteen of those and every one was an accident;
inventing the sixteenth on purpose would be worse. So the cadence coverage
moved to a unit test that measures the **gap** rather than the count — timing,
which the corpus excludes from comparison deliberately — and the two shipped in
one commit. What is *still* unmeasured (this server's cadence against
vanilla's, on the wire) is written on both check files rather than left to be
assumed closed.

**And prove it, on a control.** Rule 7 applies to the fix as much as to the
defect. `ci/selfcheck-recompare.sh` re-compares saved recordings under a chosen
projection in seconds, so the same 26 vanilla recordings can be run through the
old projection and the new one with the same binary. The old arm must
reproduce the *already published* rates — 1/25 and 3/25 — or the new arm's 0/25
means nothing. It did, and the per-run projections then showed the residue
directly: across 26 cold-booted servers the sequences differed **only** in the
length of a trailing run of byte-identical observations.

That last step is what turns "the reference should now be deterministic" from
an argument into a measurement, and it is cheap enough that there is no excuse
for skipping it.

**AND THE CONTROL DOES NOT REACH THE FIX THIS RULE RECOMMENDS. Found
2026-08-05, applying the rule to `add_entity`'s movement.**

Read the two halves of this rule together and they do not compose. The
prescribed remedy is a change to `trace::decode` — it is named twice, and the
sound seed is its worked precedent. The prescribed control is
`ci/selfcheck-recompare.sh`, "cheap enough that there is no excuse for
skipping it". But `trace::observe` runs at **record** time (`client.rs:714`),
so a saved trace holds decoded fields and a digest of everything the decoder
did not consume — never the packet's bytes. **A decoder change cannot be
re-applied to a saved recording**, because the digest it would have to undo
has already been taken. `selfcheck-recompare.sh`'s own header says it "says
nothing about a change to the recorder"; what it does not say, and what cost a
run to work out, is that this rule's headline remedy is exactly such a change.

So the rule splits in two and the cost is not the same on both sides:

* **projection options** — `observe`, `within`, `timing`,
  `collapse_repeats` — are compare-time. The saved-recording control applies,
  it is seconds, and it is not optional. This is what `collapse_repeats` was.
* **decoder exemptions** — consuming a field so it leaves `rest`, which is
  what the uuid, the sound seed and now `add_entity`'s movement are — are
  record-time. They need **real re-recording** at a real N. There is no cheap
  arm and no way to run the old projection against the new recordings.

What replaces the control in that case is a **control set inside the same
runs**: checks whose instability has a *different* named cause must keep
their published rates while the targeted one falls. For the movement
exemption that set was `spawn-egg-makes-a-mob` (same packet, different field —
the spawn rotation, which stays in `rest`) and
`a-dropped-item-goes-into-your-inventory` (same packet, the item's position,
which is decoded and still compared). If a decoder exemption cures those too,
it is wider than it claims and the exemption is wrong — which is the same
question the two-arm control was asking, answered from one arm.

Write the predictions down before the run. They are in this branch's commits.

## 27. A harness that guesses the answer right cannot see the answer (2026-08-05)

Some serverbound packets carry the client's **prediction of the result**, and
the server answers only where the prediction is wrong. For any such packet the
harness's own accuracy is a dial on how much of the server's behaviour is
observable — and the honest setting is the one that makes the server say the
most, not the one that imitates a real client best.

`ServerboundContainerClickPacket` is the first one this corpus met.
`handleContainerClick` runs `menu.clicked(...)`, then writes the packet's
`changedSlots` and `carriedItem` into the menu's remote-belief table with
`setRemoteSlotUnsafe` / `setRemoteCarried`, then calls `broadcastChanges` —
which sends a `container_set_slot` **only for slots whose truth differs from
what the client is believed to think**.

So a harness that predicted correctly would be answered with **silence**. Two
servers would then say nothing about a click, `verdict_for` would compare two
empty projections, and every storing and crafting check in the corpus would
report `match` against a server that has no containers at all. The check would
be measuring the harness.

This is not the same failure as the sixteen before it. Those were checks whose
*assertion* could not fail. This one is a check whose **reference is silenced
by the instrument**, and it would have arrived looking like success: a new verb
lands, the checks go green immediately, and the green is the bug.

**The rule.** When a serverbound packet carries a claim about state the server
also knows, send the *weakest true claim the harness can make* — here, an empty
map and an empty carried item, which is honest because the harness models no
inventory — and write down that the projection depends on it. Never send a
prediction the harness has not actually computed, and never "fix" a check by
making the harness predict better.

**The tell, for the next one.** Ask of any new serverbound verb: *does the
server's reply depend on what I claimed?* If yes, a green on the first run is
evidence about the claim, not about the server. `Step::ContainerClick`'s
documentation carries this for the packet it belongs to; `stateId` in the same
packet is the same shape with the opposite sign — get *that* wrong and the
server answers with `broadcastFullState()`, the whole menu, which drowns the
targeted correction in the player's entire accumulated inventory rather than
suppressing it.

## 28. Rule 6 is not a worldgen rule, and the corpus went without it for a week (2026-08-09)

**Rule 17 says one vanilla world is not the answer key. One vanilla *recording*
is not the answer key either, and nothing in the gameplay corpus said so.**

`--twin` has been in the region differ for months and `docs/PARITY.md` quotes
every worldgen figure beside the floor it could not have resolved. The corpus
had `ci/checks/gameplay-unstable.toml` — the right idea, the wrong process. It
was populated ad hoc by whichever agent last tripped over an entry, and in one
night three separate agents each spent part of a run rediscovering an
instability by hand from recordings that happened to be lying around.

Measured with `ci/corpus-floor.sh` over six cold boots of one recorder:
**9 of 88 scenarios do not reproduce between two vanilla runs**, and the same
nine names with the same classes come out of a second, independent ensemble of
six recordings taken by three different recorders. That is the
number a corpus score is quoted beside, on the same terms and for the same
reason as a block residual. It is in `ci/checks/gameplay-floor.toml`, it is
printed by `ci/t2-gameplay.sh` under every score, and the full account is
`docs/PARITY-ROADMAP.md` §13.

> ⚠ **THE LIVE FLOOR IS 7 OF 88, NOT 9 — corrected 2026-08-17.** The `9 of 88`
> above is `ci/figures.toml`'s `corpus-floor-n6`, which is
> `quotable = false, disqualifier = "destroyed"`: its six recordings all lived
> at one path in one tree, each `ci/t2-gameplay.sh` run overwriting the last, and
> no surviving file's sha256 matches any of them. The successor
> `corpus-floor-n6-gpfloors` is **7 of 88** and is `quotable = true`, re-measured
> 2026-08-10 with bit-identical shas recorded. **Quote 7 of 88.**
>
> The paragraph above is otherwise sound and the reasoning it draws is
> unaffected — the independent second ensemble really did reproduce the same
> names and classes, which is why the floor survived its recordings being lost.
> Only the *level* moved. This was found on 2026-08-17 by putting this file into
> `ci/figures.toml`'s coverage scan, which it had never been in; it had been
> publishing a withdrawn figure as current with nothing able to notice.

Three lessons generalise past this corpus.

**A floor is a lower bound and the sample size is part of the figure.** `1 -
alpha^(1/N)` is the largest minority rate that can hide behind N identical
runs. At N = 6 and alpha = 0.05 that is 39.3%, and the corpus's own smallest
measured rate is 1 in 26, which needs N = 77. So stability here is not
declarable at any affordable N, and the honest instrument publishes what it
could not have seen rather than claiming what it did not measure. The End is
the same lesson in the other dimension: two references gave a floor of 0 and
six gave 252.

**"The reference disagreed with itself" and "we sampled the reference badly"
read identically and have opposite fixes.** A projected observation carries
`who`, `step`, `kind`, `fields`, `when`; `step` and `when` are the harness's
own bookkeeping, so re-running the comparison with those two blanked separates
them mechanically. A disagreement that survives is vanilla's, to be recorded; a
disagreement that vanishes is ours, to be repaired. `Lens::BoundaryBlind`, and
it is a diagnostic that no gate may use — it deletes real discriminating power
on purpose.

**But a lens is only sound in the direction it can prove.** `boundary` proves
the disagreement is ours. `reference` proves only that it is *not* the boundary
class, and reading it as "therefore vanilla's" is the rule-7 mistake in a new
costume. It cost a wrong entry immediately:
`540-a-player-that-leaves-the-ground-is-resynced` was reported as newly
unstable on 2026-08-08, on the way into the file that means "the reference
cannot reproduce this", and it is not one — four of six runs are byte-identical
and two produced nothing at all, which is our observer not watching rather than
vanilla changing its mind. Pairwise verdicts cannot see that; only the
projections can. Every class this instrument can name is a
measurement; the *cause* is still a human reading bytecode, and the entry's
`why` is what makes the claim.

**Corollary, and it is the one that keeps this from being a way to make
scenarios stop failing.** A declared instability must never be able to change a
verdict. The floor is printed beside a corpus score and never subtracted from
it; `ci/t2-gameplay.sh` does not read the declared list at all; and the
promoter refuses to mark a scenario the reference reproduced, because the rate
on an entry is a declaration and can say anything while the distinct-projection
count is a measurement and cannot. `ci/corpus-floor-falsify.sh` injects all
fifteen ways round that, and runs in T0. The two that matter most are the two
that are hardest to state as arithmetic: a scenario the reference reproduced
cannot be marked however maximal a rate is written on it, and an entry with no
mechanism is not an entry — the promoter fills in every number and refuses to
guess the cause.

## 29. A reference carries a chunk order, and a comparison that does not match it is measuring the order (2026-08-10)

**Rule 6 says a residual is a defect only if two independent vanilla runs agree
on it. That is a rule about *runs*. Chunk order is not a run, it is a
condition — and it is worth thirty times more than the run-to-run difference
the rule was written for.**

Seed 20260722, r=16, centre (64,64), bbox `48 48 80 80`, 107,053,056 blocks,
vanilla first, Threadripper. Four unmodified vanilla references, two per chunk
order:

```text
  rows-a    vs rows-i        32,127     same order, different run
  rowsrev-a vs rowsrev-b     36,810     same order, different run
  rows-a    vs rowsrev-a    146,169     different order
  rows-b    vs rowsrev-b    152,538     different order
```

Now run the admissible instrument with an **innocent** candidate — unmodified
vanilla, same seed, same window, differing from the eight references only in
which direction the chunk rows were walked:

```text
  |innocent vanilla, rows_rev, ∉ V| over eight rows references   117,628
  |innocent vanilla, rows,     ∉ V| over the same eight            3,804
```

**The instrument charges a vanilla run 117,628 blocks for having the wrong
chunk order.** Our own r=16 residual, the one an entire brief was written
about, was 14,270 — eight times smaller than the price of that one
uncontrolled condition. It fell to **2,450** when the dump was generated in the
order the references were harvested in, and to **2,583** when instead the
ensemble was widened to contain both orders. Either way the same fact:
`ci/order-charge-sweep.py`, `docs/DETERMINISM.md` §15.

So, three things that are now rules rather than observations:

* **A reference has a harvest order and it must be recorded.**
  `ci/gen-vanilla-region.sh` stamps `HARVEST-ORDER` into every reference it
  makes, naming the `GEN_ORDER` and the `RUSTMC_DECORATE_ORDER` that reproduces
  it: `rows` → `zx`, `rows_rev` → `zx_zrev`, anything else → `unknown` rather
  than a guess. `region-differ` filters on the `.mca` extension, so the file is
  invisible to every existing reader.
* **An N-reference ensemble sampled along one order is not an N-reference
  ensemble.** Adding a ninth reference in the *same* order bought 307 blocks;
  adding two in the *other* order bought 11,687. The admissible set was
  sampling vanilla's scheduler and never sampling vanilla's order, and the
  residual lived in the gap. This is rule 7 in its usual clothing — when a
  measurement clears every candidate, suspect the measurement — except here the
  measurement was *convicting*, which is the harder direction to doubt.
* **Do not assume vanilla is the stable one.** The brief that produced this
  said so and it was right to: vanilla is 4.6x more sensitive to chunk order
  than to its own scheduler, and the whole investigation was opened on the
  premise that our order sensitivity might be a defect. It is ordinary.

**And the falsification is the part worth copying, not the numbers.** The
winning dump order, `zx`, is exactly the order `GEN_ORDER=rows` harvests in, so
"we found a better order" and "we finally set the control" predict the same
table. The prediction that separates them was written into the script header
before the run: against `rows_rev` references, `zx_zrev` must win and `zx` must
lose by about as much. It did — 6,025 against 126,053, with `zx` winning the
mirror arm at 6,323. A result that can only be explained one way is worth more
than a result ten times its size that can be explained two ways.

## 30. An assertion is not a stamp, and an unrecognised knob is not a turned one (2026-08-10)

**Rule 29 established that a reference carries a chunk order and that a
comparison must match it. This is the follow-on that rule 29 does not cover:
what to do when the reference does not say, and what to do when the thing you
set to match it silently does nothing.**

Both were found in one afternoon standing up the seed-12345 r=16 window, and
neither would have surfaced as a wrong answer. Both would have surfaced as a
*confident* one.

### An order asserted in a header is not evidence

`gen-wide-12345-r16-ow-{a,b}` carry no `HARVEST-ORDER` file — they predate the
stamp. `ci/ow-r16-window-12345.sh`'s header said they were made with
`GEN_ORDER=rows`, and scoring against them would have produced a number under a
condition that is written in a comment and recorded nowhere in the artefact.

The order does not have to be asserted, because rule 29's own measurement is
the classifier: two vanilla runs of one seed and window differ by ~17,000
blocks in the same chunk order and ~93,000 across orders on this seed. So diff
each unknown against one *stamped* reference of each order and read it off,
with the same-order and cross-order distances measured in the same run as
controls:

```text
  control  c vs d      (rows    vs rows    )     17,779
  control  c vs rr-a   (rows    vs rows_rev)     93,821

  a  vs c 16,670   vs rr-a 89,271   -> rows
  b  vs c 18,074   vs rr-a 91,416   -> rows
```

**The header was right.** That is the point rather than an anticlimax: it was
not evidence until it was measured, and the cost of measuring it was four
diffs. `ci/classify-12345-ab.sh`. **A classification with no controls under it
is a guess with a number next to it** — the controls are what make 16,670 mean
`rows` rather than merely "smaller than the other one".

So: **the anchor of an ensemble must be stamped, and an unstamped member is
carried only if the matrix classifies it.** `ci/order-level-retake.sh` refuses
an unstamped anchor and prints the classification for the rest. Refusing every
unstamped reference outright would have retired the 20260722 ensemble, where
only `i`, `rr-a` and `rr-b` postdate the stamp — and an order corroborated by a
5x gap in the matrix is *measured*, where one asserted in a script header is
not.

### An unrecognised order name printed a digest that read as proof

`decorate_order_for`'s match on `RUSTMC_DECORATE_ORDER` ended in a bare
`_ => {}`. So `RUSTMC_DECORATE_ORDER=zx_zev` produced the **default raster**
— and then printed

```text
  decorate-order: zx_zev chunks=1369 first=... last=... digest=...
```

a traversal digest, naming the order it had not used, emitted by the path where
the knob did not turn. Two of this project's order names differ by one
character (`zx_rev`, `zx_zrev`), and the charge for the wrong chunk order is
117,628 blocks on an innocent vanilla run. It now exits 2 and says so; `default`
is an explicit no-op name so a caller can *state* the condition, and the empty
string stays silent so no existing caller changes behaviour.

Its neighbour was the same defect in the checker rather than the knob.
`ci/order-level-retake.sh` verified "the knob turned" partly by printing a
digest from an unanchored `grep -m1 digest=`, which matched
`BIOME_PALETTE order=Forward len=66 digest=...` on line 1 of every dump log.
That value is identical for every order by construction, so the column printed
the same digest for two demonstrably different traversals — and would have
printed the same for two identical ones. Measured: both dumps reported
`4462c36f462d1ad9` while the `zx` dump's own traversal digest was
`1afc539f71b91805`. **A check whose output cannot differ between the pass case
and the fail case is not a check.**

### The general form

Three questions to ask of any condition a measurement depends on:

* **Is it recorded in the artefact, or only in a comment?** A comment travels
  with the file it is in and nowhere else.
* **If it is not recorded, can it be measured?** Usually yes, and usually
  cheaply, and the measurement needs its own controls.
* **When I set it, what would tell me it did not take?** If the answer is "the
  same output as when it did", you have not set it — you have hoped.

The positive control costs one command and this one was run:
`RUSTMC_DECORATE_ORDER=zx_zev` exits 2, `zx` exits 0, unset exits 0.

## 31. A null result and an empty result must be different words (2026-08-19)

This rule is here rather than in a checker because **no string comparison finds
it.** `ci/figure-units.py` catches a number whose prose noun disagrees with its
registry unit, and it says in its own header that this class is outside its
reach. It is worth stating because it has now produced two separate defects on
its own.

### The two

**`frozen` counted checks with an empty history beside checks that agreed every
build.** A check that has never run and a check that ran sixty-four times and
never moved are opposite findings. One says the instrument has no reading; the
other says the instrument has a reading and it is flat. They were reported under
one word, and the consequence was `108/108 falsifiable` — a figure measuring
**declarations** — while 61 of the 108 had never moved across 64 recordings.
Nothing about the label was inaccurate. `frozen` is a fair word for both.

**`union_N` was scored as a null.** It measured the width of the region a mask
removes. The instrument had no reading to give about the candidate at all, and
"no reading" was written into the same slot as "a reading of zero". A headline
of **14x** went out that was **3.53x**.

### Why a checker cannot reach it

In both cases the label was accurate, the arithmetic was correct, and every
field in the ledger agreed with every other. The defect is in the DEFINITION of
the quantity — two distinguishable states collapsed into one bucket — and a
definition is not a string a checker can compare against anything. A check can
ask whether a number's noun matches its unit. It cannot ask whether the unit
should have been two units.

### The rule

**Every instrument this project writes must use a different word for "I have no
reading" and for "my reading is zero, or empty, or unchanged."** Not a different
value in the same field. A different word.

Concretely, when you write a counter, a census, a coverage table or a floor:

* `unmeasured` / `no-reading` / `absent` — the instrument did not run, could not
  run, or had nothing to run against. **This is not a result.** It must never be
  summed, averaged, ranked, or compared against a floor, and a total that
  includes it is a total of two different things.
* `zero` / `empty` / `unchanged` — the instrument ran and this is what it found.
  This is a result and behaves like one.

The test to apply before publishing any aggregate: **take one item out of each
bucket and ask whether swapping them changes the headline.** If it does, and the
report cannot tell them apart, the report is wrong by exactly that swap. Both
defects above fail that test in one line, and both were published.

`unrecorded` is already an accepted answer in `ci/figures.toml`'s window fields,
for the same reason and against the same failure — "`unrecorded` is an accepted
answer; silence is not". This rule is that convention made general.
