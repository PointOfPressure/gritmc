# Cold start — branch `feat-worldgen-refused`

---

### COLD START — branch `feat-worldgen-refused` off `7547ce61e` (`sculk-deep-dark`), 2026-08-18

Three refused worldgen features, and an instrument that was lying about a
fourth. Worktree `~/grit/work-feat`, base worktree `~/grit/work-featbase`.
`docs/PARITY-ROADMAP.md` §76; `ci/figures.toml` ids `*-lgdrip`.

## The thing to know before anything else

**`ci/decorate-once.sh -Ddecorate.draws=<substring>` prints VANILLA's draw
sequence, and it had been aborting since the day the deep dark started
generating.** `docs/cold-start/sculk-deep-dark.md` records the jungle's
remaining defect as unresolvable because "our own draw count is readable
through `RUSTMC_TRACE_DRAWS` and vanilla's is not". It is readable. It was
dying on `WorldGenLevel.playSound/6` and then `levelEvent/3`, both of which
`SculkSpreader$ChargeCursor.update` calls, in `GenLevel` — **our own harness,
not vanilla**.

**That failure does not look like one**, which is why it survived. The run
writes no JSON, but the draw trace streams to stderr as it goes, so what
survives is a complete-looking log of every chunk *before* the sculk patch and
no sign at all of the ones after it. A five-centre run of `trees_jungle`
reported three chunks and read as "only three chunks here have jungle". After
the fix the same command reports **fifteen** and writes its JSON.

> **Before believing a harness that reports less than you expected, check
> whether it finished.** A partial run and a small answer are the same
> printout.

Both methods are `0: return` in `WorldGenRegion` under `javap`, so the fix is
the reference rather than a stub. `SculkSpreader`, `ChargeCursor`, `SculkBlock`
and `SculkVeinBlock` make five distinct `LevelAccessor` calls between them and
the other three were already handled — enumerated, so there is no third gap
waiting on that path.

## What moved

```text
                        floor(N=2)   before    after      x
  dripstone_caves A          1,482   11,122    9,421    6.36
  dripstone_caves B         11,616   22,106    8,027    0.69  below floor
  dripstone_caves E          6,874   24,970   14,912    2.17
  sunflower_plains E           668    1,435      841    1.26
  windswept_hills E             25      141       33    1.32
  deep_dark B               21,900   33,499   33,350    1.52
  river A                      239    2,396    2,398   10.03  WORSE
  jungle J                  10,414   44,460   44,460    4.27  unchanged
```

Thirteen registered rows moved. **Windows J and O did not move by a single
block** — neither square holds a dripstone cave — and that is what makes the
other thirteen attributable rather than drift.

`large_dripstone`'s **4,569 declines are 0**: 543 in A, 2,049 in B, 1,977 in E.
It was the largest feature refusal by **count** anywhere and only **fifth by
blocks** at 1,593. Keep both halves in one sentence: a tally ranked by count
ranks how often something is *attempted*, and this one is attempted 10..48
times a chunk over the whole vertical column, so most attempts land in rock and
place nothing.

**Two results are not good news and are published anyway.** `river` in A is two
blocks worse — a port that improves twelve rows and worsens one has worsened
one. And `dripstone_caves` in **B fell from 1.90x its floor to 0.69x**, so that
square has stopped adjudicating the biome under ORACLE rule 6. An improvement
that removes a window is still an improvement; A and E still resolve it.

## WITHDRAWN: the jungle gap is not two draws a tree

§67 localised the jungle residual to "about two draws per mega jungle tree",
from a padding sweep with a clean minimum at pad=2 (41,671 against a target of
41,024). With the oracle working, that is refuted.

Sixteen chunk-runs of `trees_jungle` compared line for line, each aligned to
the 3x3 neighbourhood `DecorateGolden` actually decorates. **Two are
byte-identical.** In every other, the first divergence is a tree that one side
places and the other does not — **and it goes both ways**, which no constant
per-tree draw deficit can produce.

The first divergence in chunk (960,-192) decomposes completely and none of it
is a draw count:

```text
  draw 0-2   count, then in_square -> (15374, ?, -3069)     IDENTICAL
  draw 3-4   two random_selector floats, 2nd entry passes   IDENTICAL
             -> minecraft:jungle_bush selected on both sides
  vanilla    heightmap OCEAN_FLOOR -> y = 87, places it, 1,645 draws follow
  ours       heightmap OCEAN_FLOOR -> y = 114, would_survive fails, 0 draws
```

Read out of both worlds with `anvil-probe`:

```text
  y     ours                          vanilla
  86    grass_block                   dirt
  87    air                           jungle_log[axis=y]      <- the bush
  88    air                           oak_leaves[distance=1]
  113   jungle_leaves[distance=6]     air
```

**A mega jungle canopy stands twenty-seven blocks above the ground in our world
and not in vanilla's**, placed by a chunk that decorated earlier, and it lifts
`OCEAN_FLOOR` from 87 to 114. The tree we then fail to place is a
`jungle_bush`; `trees_jungle`'s selector has five children and the mega jungle
tree is only one of them.

So the fault is a **cascade whose head is in a neighbouring chunk**, and the
pad sweep's minimum at 2 is what a compensating desync looks like from
downstream: it puts following trees back in step on average while correcting
nothing, which is exactly why 41,671 never beat 41,024.

## The cheapest form of the comparison, which should have been run first

**Both sides already report a per-feature draw count for a whole chunk.**
`DecorateGolden`'s vectors carry `"draws": N` per feature and
`RUSTMC_TRACE_ATTEMPT=minecraft:` makes our decorator print
`ATTEMPT <feature> chunk (x, z) … draws=N` for every one. No draw log, no
alignment, one run each.

```text
  chunk (960,-192)   69 features   67 identical   2 differ
    step 9  idx  7   minecraft:trees_jungle        van   1645   ours   1182
    step 9  idx 11   minecraft:patch_grass_jungle  van   5128   ours   5107

  chunk (957,-195)   70 features   67 identical   3 differ
    step 9  idx  0   minecraft:glow_lichen         van    478   ours    475
    step 9  idx  7   minecraft:trees_jungle        van   1147   ours   1430
    step 9  idx 11   minecraft:patch_grass_jungle  van   5214   ours   5198
```

**Every feature in steps 0 through 8 spends exactly vanilla's draws, in both
chunks** — ores, lakes, springs, geodes, the ruined portal, the whole
underground. So the jungle residual is not a chain of upstream refusals paying
forward; there is nothing upstream in these chunks to pay forward.

Three features differ and all three read the surface. `glow_lichen` is
`multiface_growth`, which §66 named as its own single next item, and it is
*upstream of `trees_jungle` in the same step*. `patch_grass_jungle` runs
**after** `trees_jungle` and reads what it wrote, so it is a consequence.

> **Take the coarse table before the fine one.** It ruled out sixty-seven
> features in one run a side, before a single `DRAW` line was read.

The same table on the pale-garden/dark-forest chunk at window A's centre finds
this branch's own open defect:

```text
  chunk (-1056,-672)   77 features   74 identical   3 differ
    step 9  idx 17   minecraft:dark_forest_vegetation  van   2425   ours   3981
    step 9  idx 24   minecraft:trees_birch             van     61   ours     81
    step 9  idx 77   minecraft:patch_leaf_litter       van    199   ours    197
```

The `DRAW` diff on that feature names the cause, and **it is not the new
placers**. The first 793 draws are identical; then vanilla spends
`nextInt(3)` and `nextInt(12)` that we do not, and the streams carry the same
values two apart from there. That pair is
`AbstractHugeMushroomFeature.getTreeHeight` — `nextInt(3) + 4`, then a
`nextInt(12)` — off the pinned jar. **`huge_brown_mushroom` and
`huge_red_mushroom` are the only members of that selector this port still
declines**, and this is one of them in the act: vanilla spends the mushroom's
first two draws and abandons it on `isValidPosition`; we spend nothing.

The total goes the other way, 3,981 against 2,425, and the decorators
reconcile it: a placed `dark_oak_leaf_litter` runs two `place_on_ground`
decorators at 96 and 150 tries, three draws a try — **738 draws before any
provider draw**. So +1,556 in one chunk is about **two extra placed trees**,
which over the ~15 dark-forest chunks in this square is the same order as the
+466 `dark_oak_log` the census reports.

> **Read the decorator's `tries` before converting draws into trees.** This
> file said "sixty to ninety" until they were read, on a twenty-draw estimate
> that ignored 738.

So the open item under the overshoot is **the huge mushrooms**, not the dark
oak placers.


## T0, and a remote-only failure that is not the branch's

**T0 is GREEN in the worktree.** `./ci/t0.sh` run in `~/grit/work-feat`, all
825 lib tests included.

**A remote T0 on this base reports four `check-cited-audits` failures and they
are an artefact of where it ran, not of the branch.** `ci/t0-remote.sh` rsyncs
the tree without `.git`, so `check-cited-audits` cannot resolve the commits it
cites and reports `CITATION UNREACHABLE` for all four. The same tree with git
present reports **0 failing** — verified by running the check in the untouched
base worktree. `origin/main` carries `gatefix-gitless-checks`, which makes the
check skip rather than fail when there is no repository, so the failures
disappear on rebase.

> **Check where a gate ran before you believe what it says about the branch.**
> The four rows name a branch this base predates; nothing in them mentions any
> file this branch touched.

The rebase onto `origin/main` is clean — 66 commits, zero conflicts, the
project's own merge drivers resolving both append regions in `ci/figures.toml`
and `docs/WORKLIST.md` — and is pushed as `feat-worldgen-refused-on-main`
beside the branch itself, so the orchestrator can take either.

## Nulls — do not re-run these

* **Do not re-run the padding sweep and do not add draws to a placer.** See
  above. The `mega_jungle_trunk_placer` and `jungle_foliage_placer` bytecode
  was already checked line by line on `sculk-deep-dark`; nothing on this branch
  contradicts that, and nothing on this branch supports adding to it either.
* **Neither `dark_forest` nor `pale_garden` can adjudicate a dark-oak port.**
  They sit at 0.72x and 0.52x their floors in window A, so the residual is not
  a measurement there under rule 6. The block census is: `dark_oak_leaves`
  6,017 → 0 and `pale_oak_leaves` 6,732 → 0 before this branch. Quote the
  census, not the row.
* **`golden_carver_chunk` is RED on `sculk-deep-dark` already** — 106 of
  7,077,888 blocks, `calcite`/`sand`/`sandstone` confused with
  `dirt`/`grass_block`/`stone` in two y-bands. Verified by running the same
  test in a clean worktree at the base commit: identical figure. It is not
  this branch's and it is not new.

## What is ported here, and what is deliberately not

Ported: `minecraft:large_dripstone` (feature, two inner classes,
configuration); `SpeleothemUtils`' `getSpeleothemHeight`,
`isCircleMostlyEmbeddedInStone` and `isEmptyOrWaterOrLava`; `Column#scan`
generalised over its **end** predicate and shared with the cluster;
`IntProvider::min_value` plus a correction to `max_value` for clamped
providers; `dark_oak_trunk_placer`; `dark_oak_foliage_placer`; the
`creaking_heart` tree decorator.

**Not ported, on purpose: `pale_moss`.** Three of its four passes are ordinary
decorator work; the fourth places a whole configured feature —
`minecraft:pale_moss_patch`, a `vegetation_patch` this port already implements
— from inside a tree decorator. A decorator holds a `DecoratorWorld` (five
block questions); placing a configured feature needs the feature registry and a
`FeatureWorld`, and threading either down changes `TreeConfig::parse`'s
signature and widens the bound on every decorator. Porting the other three and
skipping the patch would be **worse than refusing**: the patch costs the stream
whatever a `vegetation_patch` costs, so everything after it would land
displaced while `pale_oak` reported success. The note lives in
`crates/rustmc-worldgen/src/feature/tree/decorator.rs` beside the enum a future
lane will extend.

So `minecraft:dark_oak` and `dark_oak_leaf_litter` are placeable as of this
branch and `minecraft:pale_oak` and `pale_oak_creaking` are not.

## THE SINGLE NEXT ITEM

**`multiface_growth`'s face choice, which is §66's own next item, and then
re-take the per-feature table above.** It is the only defect implicated
upstream of `trees_jungle` inside a chunk whose other sixty-seven features are
exact, and three commands say whether the jungle's remaining term survives
fixing it. If it does, walk the decoration order for the first chunk whose
`trees_jungle` sequence diverges and decompose that one the way (960,-192) is
decomposed above. The instrument is:

```bash
ci/decorate-once.sh /tmp/j.json -Ddecorate.seed=20260722 \
    -Ddecorate.centers=<cx>,<cz> -Ddecorate.draws=trees_jungle
RUSTMC_DECORATE_ORDER=zx RUSTMC_TRACE_ATTEMPT=trees_jungle \
    RUSTMC_TRACE_DRAWS=trees_jungle RUSTMC_TRACE_ATTEMPT_POS=1 \
    target/release/worldgen-dump full 20260722 1 /tmp/ours <cx> <cz>
```

Grep the vanilla log **unanchored** — log4j prefixes every stderr line — and
align by `DRAWS-BEGIN <feature> chunk x,z`. Where the streams part, read our
`STAGE #3 heightmap` against the vector's `stageTrace` entry `[3, x, y, z]` and
probe the column in both worlds with `anvil-probe get`.

Second: `pale_moss`, which is the whole of `pale_garden`'s 9,129 missing blocks
in window A and needs the parse-time change described above.

Third: `large_dripstone` is closed, but `dripstone_caves` in A is still 6.36x
its floor. Its residual has not been decomposed by block pair the way §66
decomposed the deep dark's, and that decomposition is what turned a plausible
story into the `onDischarged` finding.
