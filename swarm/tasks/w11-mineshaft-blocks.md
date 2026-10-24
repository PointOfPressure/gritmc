# w11 — gate the blocks a mineshaft writes, not just where its pieces are

**Status: the draw counter exists, and it found four faults.** The mineshaft
was ~1,370 blocks of the r7 diff and is now the residue named at the bottom
of this file. What follows is the instrument, the four faults, and what is
left.

Read this before touching the corridor again: seven hypotheses were
eliminated by *reading* the bytecode and every one of them was correct to
eliminate. Reading was not the missing thing. **Counting was.**

## The instrument: `worldgen-dump mineshaft-ceiling`

A corridor's first drawing step is

```java
generateMaybeBox(random, 0.8F, 0, 2, 0, 2, 2, length, CAVE_AIR, CAVE_AIR, false, false)
```

and it reads **nothing**: one `nextFloat` per position, taken before every
test, nesting y then x then z. So the ceiling row is a verbatim printout of
`3 * (length + 1)` consecutive stream values thresholded at 0.8 — and vanilla
saved the result into a region file.

That makes a saved region a draw counter. A ceiling position holding natural
rock proves the draw at its index **failed**, because the pass writes
`cave_air` wherever it passes and nothing else in a mineshaft puts stone
back. Slide the stream against the rock and the offset the corridor entered
on falls out. `cave_air` positions are the soft constraint — a carver could
have hollowed them first — so they only break ties.

```text
worldgen-dump mineshaft-ceiling <seed> <vanilla-region-dir> [our-region-dir]
```

With a second directory it recovers ours the same way and subtracts.
`RUSTMC_TRACE_MINESHAFT=1` prints the same number from the port itself
through a `DrawCounter`, so the two are directly comparable, and
`RUSTMC_TRACE_MINESHAFT_PHASES=1` breaks one corridor down into the
conditionals that spent the values.

Knobs: `RUSTMC_CEILING_MIN_ROCK` (default 6 — four was not enough and the
tool's first wrong verdict came from four), `RUSTMC_CEILING_FROM` and
`RUSTMC_CEILING_SEARCH` to narrow the window once the corridors either side
are pinned, `RUSTMC_CEILING_SHOW`.

## The second instrument: `worldgen-dump mineshaft-refs`

`structures.References` is a `LongArray` per structure in every chunk, and
`LongOpenHashSet#toLongArray` fills it **through the iterator** — so the file
records the set's iteration order, not just its membership. Since
`startsForStructure` walks exactly that set and every start shares one
`RandomSource`, this gates both halves of "what enters a chunk's pass".

```text
  vanilla-s2-r7   362 chunks, 375 references   0 wrong membership, 0 wrong order
  van-s2-c62      362 chunks, 339 references   0 wrong
  vanilla-r7      362 chunks, 215 references   0 wrong
  vanilla-r7-b    362 chunks, 215 references   0 wrong
  van-c6458        82 chunks,  26 references   0 wrong   (seed 31415926, not 20260722)
```

That closed the last hypothesis about the *entry* to a chunk's pass and moved
the search inside `postProcess`, where the four faults were.

## The four faults, in the order they were worth

### 1. `createChest` returned before every chest in the game — and a chest costs two values

Vanilla tests `level.getBlockState(pos).isAir()`. The port compared the name
against `minecraft:air`. A chest sits at local y 0, inside the tunnel the
corridor has just carved, so the block there is **`cave_air`**: `isAir()`
says yes and a name comparison says no. The function returned before the
`nextBoolean`, every time, everywhere.

And the chest spends **two** values, not one. `MinecartChest.setLootTable`
takes `random.nextLong()`, inside the non-null branch of an
`EntityType.create` that never returns null on a real level. The entity is
still not written — this port has none — but the draw is not optional.

### 2. `hasPlacedSpider` is piece state, not call state

A corridor is post-processed once per chunk it overlaps and the field carries
over. Vanilla checks it **before** the `nextInt(3)` that picks the spawner's
z, so a spider corridor that already placed its spawner spends nothing on it
in the next chunk it reaches. The port reset it per call and redrew.

Vanilla saves the flag as `hps` in the piece NBT, which settles that the
persistence is intended and not an accident of object reuse — and makes it a
free oracle. Over `vanilla-s2-r7`: 11 spider corridors, 10 placed a spawner,
**7 of those reach more than one chunk**.

### 3. `isSolidRender` and `isFaceSturdy(UP)` were name lists

The rail pass draws only where `getBlock(1, -1, z)` is non-air *and*
`isSolidRender`. That was answered from a hand-written list of block names,
which is wrong for `oak_fence`, `iron_chain`, `spawner` and
`pointed_dripstone` — all things a mine or a cave puts under a corridor floor,
and none of which occlude. `setPlanksBlock`'s `isFaceSturdy(UP)` had the same
shape of guess. Both are columns of the props table and now come from it.

### 4. The writable area starts at `getMinY() + 1`

`ChunkGenerator.getWritableArea` builds its box from `getMinY() + 1`, not
`getMinY()`. Ours was one row too tall at the bottom. `getBlock` answers air
outside the box, `isInterior` false, `placeBlock` writes nothing — so the row
is behaviour. It moved nothing at this seed (no corridor here reaches y −64)
and is kept because it is what the reference says.

## What it measured

Over the r7 window at seed 31415926, against `oracle-work/vanilla-s2-r7`:

```text
                          all 225 chunks            inner 169 chunks
  branch baseline         3,389   99.984678%        1,785   99.989256%
  clip + props table      3,346   99.984872%        1,770   99.989346%
  createChest             2,721   99.987698%        1,386   99.991657%
  hasPlacedSpider         2,460   99.988878%        1,125   99.993228%
```

**929 blocks over the wide window and 660 over the inner one: 27% and 37% of
everything that was still wrong.** And in the units the tool measures:

```text
  before        88 of 102 offsets agreed; deltas -9, -7, -6, -5, -3 x5, -1, +1 x4
  createChest   101 of 105 agreed; deltas +1 x4
  spider        106 of 106 agreed
```

Every corridor whose ceiling row is readable enters on exactly the draw
vanilla entered on. Over the 36-chunk window where the mineshaft dominates,
the diff fell from 804 blocks to 254, and `cobweb`, `cave_air`, `rail` and
`oak_planks` have left it entirely — the 254 are 184 blocks of
`diorite <-> granite` plus the ore rows, neither of which is this stage.

**Cross-seed.** None of the four faults was found at seed 20260722, so it is
a clean check, and it agrees: **62 of 62 offsets**, 9 corridors unexplainable
on both sides. The r7 window there stands at 4,204 / 99.980993% over 225
chunks and 2,860 / 99.982785% over the inner 169, dominated by rows this
stage does not write.

## It also settles the feature seed index

`MEASURED_MINESHAFT_INDEX = 1` was fitted to the block diff and flagged as
ORACLE.md fault 9. The ceiling tool tests it at a different level: the first
corridor of a chunk's pass must enter at offset 0, because nothing before it
draws.

```text
  index   first corridors pinned   at offset 0   unexplainable
  0       3                        0             118
  1       27                       25            8
  2       3                        0             122
  3       6                        0             115
  4       2                        0             120
```

## What is left

- **A real block gate.** `mineshaft-ceiling` is a probe, not a CI check. The
  ceiling row alone is gateable today: it needs no world state, so a test
  could assert every readable corridor's offset against vanilla's on a
  committed vector.
- **`hps` as a gate.** Vanilla saves the spawner-placement outcome per
  corridor and nothing *compares* it — `mineshaft-refs` only counts vanilla's
  side. Ours is already available: the driver keys a
  `BTreeSet<(start_cx, start_cz, piece_index)>` and the piece index is gated
  at 100% by the layout test, so the two are directly comparable. Note the
  draw *consequence* of `hps` is already covered — a wrong flag suppresses or
  spends a `nextInt(3)` and the next corridor's ceiling offset moves, which
  `mineshaft-ceiling` measures at 106 of 106. What a gate would add is the
  spawner **block** itself.
- **The mesa mineshaft is not generated at all** — see below. This is now the
  largest mineshaft-attributable cluster in either window.

## Two things that were open are now measured

### The eight unexplainable corridors are refusals, on both sides

They were *assumed* to be pieces `isInInvalidLocation` turned away, because
they failed on both sides. `mineshaft-ceiling` now decides it instead of
inferring it. The ceiling pass writes `cave_air` wherever `nextFloat() < 0.8`,
so a corridor that ran and left all `n` of its in-clip ceiling cells as
untouched natural rock did so with probability `0.2^n` — 3e-32 at the
smallest of these. An all-rock row with no hollow cell and no foreign block
is a corridor that never drew, and the tool now says `REFUSED`:

```text
  offsets pinned on both sides: 106, agreeing: 106, disagreeing: 0
  corridors vanilla refused: 8, ours refused too: 8, ours placed: 0
  ... 25 entered at offset 0, 120 pinned, 1 ambiguous, 8 refused,
      0 unexplainable, 626 unreadable
```

**0 unexplainable, down from 8.** And `RUSTMC_TRACE_MINESHAFT=1` names the
same eight boxes from the other side — our code path, not our blocks — so two
independent instruments agree piece for piece.

### The refusal trace could only report half its reasons

Reconciling those two is what exposed it. `is_in_invalid_location` refuses
four ways — a blocking biome, and liquid on the y, z or x faces — and **only
the first two printed anything**; the z-face and x-face loops returned `true`
silently. The blocks said eight refusals and the trace said six.

So the earlier entry in this file — "it reports nothing at all, so we refuse
no piece" — was never a claim those two `println!`s could support. The
verdict survives on the ceiling tool's evidence, but it survived on better
evidence than was quoted for it. All four paths now report.

## Retraction: mineshaft blocks are **not** order-independent

This file and `docs/BENCH.md` both said the mineshaft pairs "score zero
between the two vanilla worlds", on the reasoning that
`StructurePiece#getBlock` answers air outside the clip. **That is wrong**, and
it was wrong because it was read off a truncated table.

`region-differ --json` caps `by_block` at the thirty largest pairs, which over
an r7 window is about four fifths of the mismatches — mineshaft rows are in
the tail, not absent. `RUSTMC_DIFF_PAIRS` now lifts the cap. With the whole
table, two **vanilla** worlds of seed 20260722 over the same 225 chunks,
differing only in whether the square was forceloaded as a square or a row at
a time:

```text
  vanilla-r7 vs van-rows-c64   10,040 mismatches, 263 pairs
  of which mineshaft-only blocks   52, in 17 pairs:
      cobweb 26   oak_planks 11   rail 5   oak_fence 5
      wall_torch 3   chest 1   spawner 1
```

**52 mineshaft blocks move with generation order alone.** That is the floor
for this stage: a mineshaft difference smaller than it cannot be told from a
change of extent. It does not overturn any of the four fixes above — those
were measured in draw offsets, which are order-free — but it does mean the
block diff cannot be pushed to zero here, and nobody should try.

## What the mineshaft is actually still worth

Measured exhaustively (`RUSTMC_DIFF_PAIRS=100000`), counting every pair
touching a block only a mineshaft places, over the r7 window at centre
(64, 64). **Argument order matters: `region-differ` labels its first
directory `vanilla` and its second `rustmc` positionally**, so pass vanilla
first or read every row backwards.

```text
  seed 31415926   2,460 mismatches total,   19 mineshaft-only ->  3 after monster_room
  seed 20260722   4,204 mismatches total,   30 mineshaft-only -> ~18 after monster_room
  (order-only floor, vanilla vs vanilla:    52)
```

A caution the second row cost: **`oak_planks` is a substring of
`dark_oak_planks`.** Filtering block names by substring silently pulled a
shipwreck into a mineshaft total and turned it into a phantom task. Match
whole names, or exclude the prefixes explicitly.

Seed 31415926's 19 are almost none of them this stage:

| blocks | pair | whose |
|---|---|---|
| 9 | `chest[facing=east/south/west]` -> `chest[facing=north]` | **`monster_room`** — `StructurePiece.reorient` is deliberately not ported and the feature hardcodes `facing=north`. Same position on both sides. |
| 6 | `cobblestone` -> `oak_fence`/`oak_planks` | a vanilla dungeon overwrote a mineshaft support and ours kept it — `monster_room` again |
| 1 | `chest[facing=north,waterlogged=true]` -> `water` | `monster_room` a third time |
| 2 | `oak_fence[all false]` -> `oak_fence[south=true]` / `[east=true]` | ours connects a fence vanilla leaves free-standing |
| 1 | `cave_air` -> `cobweb` | one cobweb we place and vanilla does not |

So **at seed 31415926 the mineshaft is worth 3 blocks out of 2,460** — two
fence connection states and one cobweb — against a 52-block order floor. The
other 16 are `monster_room`. It is finished as a source of diff.

### RETRACTED: seed 20260722's dark oak is a **shipwreck**, not a mesa mineshaft

An earlier revision of this file said 76 of seed 20260722's 106
mineshaft-only blocks were `dark_oak_planks`/`dark_oak_fence` from an
unbuilt mesa mineshaft, and called it "the largest mineshaft-attributable
cluster anywhere". **That was wrong**, and the error was a substring match:
the filter looked for `oak_planks` and `oak_fence`, which are substrings of
`dark_oak_planks` and `dark_oak_fence`.

`worldgen-dump mineshaft-mesa` settles it by reading the starts vanilla
saved:

```text
  vanilla mesa mineshafts in oracle-work/vanilla-r7: 0
  structure starts saved in this world, by id:
        3x minecraft:mineshaft
        1x minecraft:shipwreck
    minecraft:shipwreck start (57, 65) 1 pieces, union 912,54,1040..920,62,1063
```

**47 of 47 sampled dark-oak positions lie inside that shipwreck's union
box.** There are **zero** mesa mineshafts in any of the five oracle worlds,
at either seed. The cluster is a shipwreck — one of the nineteen structure
sets this project does not generate at all, exactly as the `full` CAVEATS
say — and it is not this stage's to fix.

Corrected residue at seed 20260722, excluding dark oak:

```text
  4,204 mismatches total
     96 dark_oak, all inside the shipwreck box
     30 mineshaft-only, of which ~12 are monster_room chests and a spawner
```

So the honest numbers are **3 blocks at seed 31415926 and about 18 at seed
20260722**, both far below the 52-block order-only floor. Nothing
mineshaft-shaped is left at either seed.

### The mesa rule is ported, and is UNVERIFIED

The reading was done before the retraction and is kept, because it is right
and because it removes a refusal — but it is **not gated against anything**,
and nothing calls it.

- `MineshaftStructure$Type` carries exactly three Blocks: NORMAL is
  `(OAK_LOG, OAK_PLANKS, OAK_FENCE)`, MESA is `(DARK_OAK_LOG,
  DARK_OAK_PLANKS, DARK_OAK_FENCE)`. Every reference to the `type` field in
  `MineShaftPiece`, `MineShaftCorridor`, `MineShaftCrossing` and
  `MineShaftRoom` is `getWoodState`, `getPlanksState`, `getFenceState` or
  `ordinal()` for the `MST` tag. **No piece compares it against MESA**, so
  inside a piece it is a palette and nothing more.
- The single behavioural branch is `generatePiecesAndAdjust`:
  `surface = getBaseHeight(centre.x, centre.z, WORLD_SURFACE_WG)`, then
  `target = surface <= seaLevel ? seaLevel : randomBetweenInclusive(random,
  seaLevel, surface)`, then `offsetPiecesVertically(target - centre.y)`.
  `randomBetweenInclusive` is `nextInt(max - min + 1) + min` — **one draw,
  and only on the above-sea-level branch**. `getCenter` is `min + span / 2`,
  not the `(min + max) / 2` midpoint used by `isInInvalidLocation`.
  `iterateNoiseColumn` returns `OptionalInt.of(y + 1)`.
- Unit tests pin the arithmetic and the draw counts. Nothing pins it against
  vanilla, because no oracle world contains a mesa mineshaft.

**What it still needs, in order:**

1. **A badlands oracle.** `ci/gen-vanilla-region.sh` at a seed and centre in
   `#minecraft:is_badlands` with a structure chunk in it. Without one,
   `mineshaft-mesa` prints "nothing to compare" and the rule is unmeasured.
2. **The selection and the biome gate**, which are missing for *both* types.
   `mineshaft` and `mineshaft_mesa` sit in **one** structure set with equal
   weight, so `ChunkGenerator.createStructures` draws `nextInt(2)` on a
   separate `WorldgenRandom` (`setLargeFeatureSeed(seed, cx, cz)`), tries the
   winner, and on failure removes it and retries with the rest. A structure
   fails when `isValidBiome` rejects it — sampled **after** the layout, at
   `getNoiseBiome(QuartPos.fromBlock(x), fromBlock(50 + dy), fromBlock(z))`
   with `x = cx*16 + 8` and `z = cz*16` (min, not middle), with no biome-manager
   fuzz.

   Our `mineshaft_starts` does **no biome check at all** and always passes
   `MineshaftType::Normal`. The two biome tags are disjoint
   (`#is_badlands` against a list containing no badlands), but the two checks
   sample *different y* — a normal mine's is deep, a mesa mine's near the
   surface — so both can pass in badlands with a cave biome below, and then
   the `nextInt(2)` decides. The selection must be ported, not skipped.
   The resolved biome sets are already extracted to
   `data/vanilla/structure_biomes/{mineshaft,mineshaft_mesa}.json`.
3. `getBaseHeight` on our side would come from `canvas.world_surface_wg`,
   which is the same convention (one above the highest non-air block) but is
   **not proven equal** to vanilla's standalone `iterateNoiseColumn` — that
   runs a one-cell-wide `NoiseChunk` with `Blender.empty()` and the global
   fluid picker rather than the chunk's aquifer. `mineshaft-mesa` recovers
   vanilla's value from its own piece boxes precisely so the two can be
   compared the moment a badlands world exists.

## Eliminated, and do not redo

| candidate | verdict |
|---|---|
| piece **order** within a start | gated by `golden_structure_starts`, by index |
| a non-corridor piece draws | room, crossing and stairs make **zero** `RandomSource` calls in `postProcess` — checked in the bytecode |
| the wrong set of pieces is post-processed | `bounding_box.intersects(clip)` matches `placeInChunk` |
| `is_in_invalid_location` disagrees | trace reports our refusals; the unexplainable corridors fail on both sides |
| the feature seed index | swept, and now pinned independently — see above |
| the search reach | `createReferences` scans ±8; the reference gate passes at 100% on four worlds |
| the reference set's membership and order | gated, 100% on 1,168 chunks and 955 references |
| `maybePlaceCobWeb` order and short-circuit | matches; the neighbour count is `iconst_2` |
| `generateMaybeBox` loop nesting and `<=` vs `<` | matches, both comparisons |
| `isSupportingBox` | identical loop, reads local y + 1 |
| `StructurePiece#getBlock` outside the clip | returns **air** on both sides |
| `canBeReplaced`'s protected set | planks, wood, fence, `minecraft:iron_chain` |
| OCEAN_FLOOR_WG frozen at decoration | confirmed: `SURFACE` carries the `_WG` pair, `CARVERS` carries the final four, and the persisted status lags by one |

---

# Fancy trees — landed, measurable, NOT finished

`fancy_trunk_placer` + `fancy_foliage_placer` are ported from the bytecode and
now run. Village window, seed 20260722, centre chunk (-53,-33), inner 169
chunks, against `oracle-work/van-village`:

```text
  before   99.525364%   (~78,853 mismatches)
  after    99.542610%     75,988 mismatches      -2,865 blocks
```

Refusals over the same generation:

```text
  fancy_trunk_placer / fancy_foliage_placer   92 -> 0
  java_hash_set treeify                      199 -> 175
```

**The treeify refusals were partly caused by the missing trees, not
independent of them** — they fell by 24 without anything touching the hash
set. The remaining 175 are a real limit and still want their own answer.

## What is NOT fixed, and the evidence

The leaves rows are still the top of the diff and they are **two-sided**,
which is displacement rather than absence:

```text
  4276  air -> oak_leaves[distance=3]      3853  oak_leaves[distance=3] -> air
  4042  air -> oak_leaves[distance=2]      3517  oak_leaves[distance=2] -> air
  2871  air -> oak_leaves[distance=4]      2625  oak_leaves[distance=4] -> air
  1620  air -> oak_leaves[distance=1]      1360  oak_leaves[distance=1] -> air
```

Near-symmetric counts at every distance: we place about as many leaves as
vanilla and put them somewhere else. That is the signature of a wrong branch
geometry or a draw-count difference inside the cluster loop, not of a missing
feature.

Two rows say the residue is **not all** fancy oak, and should be split off
before anyone chases the geometry:

```text
  2374  spruce_leaves[distance=2] -> air          (one-sided: we place none)
  1346  spruce_leaves[distance=2] -> spruce_leaves[distance=7]
```

`spruce_leaves` is not this feature. The second row is a **distance property**
difference at the same position, which is the leaves post-pass recomputing
`distance`, not placement at all. Whoever takes this next should measure the
oak rows alone before attributing the whole cluster to the fancy placer.

## Verified, and not

Verified: it builds, it generates, the refusals go to zero, and the window
number moves the right way by 2,865 blocks.

**Not verified: any individual tree.** There is no per-tree gate — nothing
compares one fancy oak's logs and leaves against vanilla's for a known
position, so "the geometry is right" is not a claim this work can make. The
draw counts are read from the bytecode and argued in the module docs; they are
not measured. That gate is the next increment, and it is what the ceiling tool
was for the mineshaft: without it this is guesswork with a number attached.

---

# The leaves rows are the treeify limit, not the leaves pass

The suggestion was that `spruce_leaves[distance=2] -> [distance=7]` (1,346
blocks) is a self-contained fix in the leaves post-pass, worth taking before
any geometry work. **It is not self-contained, and it is not the leaves pass.**

## What the numbers say

Over the village window, every leaf this port leaves at the undecayed default
is spruce — not one oak, not one birch, in a plains village full of both:

```text
  ours has distance=7 leaves: 4,049 in 14 rows, 100% spruce_leaves
    1346  spruce_leaves[distance=2] -> spruce_leaves[distance=7]
    1046  spruce_leaves[distance=3] -> spruce_leaves[distance=7]
     796  air                       -> spruce_leaves[distance=7]
     389  spruce_leaves[distance=1] -> spruce_leaves[distance=7]
```

A leaves pass that were simply wrong would be wrong for every species. This
is wrong for exactly one, so the cause is upstream of it.

## The cause, measured

`TrunkPlacer.name()` and `FoliagePlacer.name()` are now attached to any
refusal escaping `do_place`, and the window answers immediately:

```text
  174x  treeify (straight trunk, spruce foliage)
    1x  treeify (straight trunk, pine foliage)
    0x  anything else
```

**The `java_hash_set` treeify limit is, in this window, a spruce-tree limit.**
A spruce carries enough foliage positions to drive a bucket to eight entries;
oak and birch do not.

And a refusal is not a no-op. `do_place` writes straight to the world as it
goes, so when the refusal propagates:

- the logs and leaves already written **stay in the world**, and
- `update_leaves` — which runs after `do_place` in `place` — **never runs**.

So each of those 174 trees leaves a half-built spruce behind whose leaves keep
the provider's `distance=7`. That is where all 4,049 come from, and it is why
the row looked like a leaves-pass bug.

The same refusal explains a share of the one-sided loss: 7,261 blocks where
vanilla has spruce leaves and we have air is a tree that stopped partway.
Total attributable to this one limit is of order 11,000 of the window's
75,988 — around 14%.

## This is the port's floor, said loudly

Per the standing instruction: the spruce tree **legitimately needs a treeified
bucket**. This is not a bug to work around, it is
`java.util.HashMap` treeification that this port does not reproduce, and it is
now measured rather than suspected — 174 trees in one 225-chunk window.

Two things follow, and neither is a geometry question:

1. **Reproducing treeification** is the real fix and the only one that makes
   these trees exist. Until then the window has a floor around 14% of its
   current diff that no tree work can reach.
2. **A refusal should not leave a half-built tree.** Whatever happens to the
   limit, writing part of a tree and skipping its leaves pass is strictly
   worse than writing none: it pollutes the diff in two directions at once
   (undecayed leaves *and* missing ones). Fixing that is independent of
   reproducing treeification, but it changes what `do_place` may write, so it
   needs care — trees read their own blocks while placing.

## Not done: the per-tree gate

The gate that would settle the fancy-oak geometry — one oak recovered from
vanilla's own logs and leaves at a known position, compared against ours —
**was not built.** The draw counts in `trunk.rs` remain read from the bytecode
and argued, not measured, exactly as before. What this increment establishes
is only that a large part of the leaves cluster is not the fancy oak's fault,
so building that gate against the current window would have been measuring
through 11,000 blocks of unrelated noise.

---

# All-or-nothing trees: the fix works, and the window got worse

`place` now wraps the world in an undo log and a refusal puts it back. The
intended effect landed exactly:

```text
  leaves left at the undecayed default   4,049  ->  1
```

And the window number moved the **wrong** way:

```text
  partial trees      75,988   99.542610%
  all-or-nothing     83,697   99.496207%      +7,709 worse
```

**That is the finding, and the fix stays.** The old number was flattered: a
half-built spruce overlaps a real spruce, so its logs and leaves were scoring
against vanilla's tree while being a shape that exists in no vanilla world.
Deleting the fabrication removes the coincidental credit with it.

Picking the better-scoring variant here would have been fitting the mechanism
to the metric — the same error as reading a stage's health off a table
truncated to thirty rows.

## The successful path is provably unchanged

Oak and birch are **identical to the block** across the change:

```text
                          before    after
  oak_leaves   missing     12,840   12,840
  oak_leaves   extra       14,300   14,300
  birch_leaves missing      2,356    2,356
  birch_leaves extra        3,023    3,023
```

Not "close" — the same numbers. That is what the undo log was chosen to
guarantee: writes still go straight through, so every read during placement is
served by the real world exactly as before, and a tree that succeeds never
learns the log existed. A write **buffer** could not have made this claim,
because `isFree` accepts an existing log and `validTreePos` reads air and
replaceability — buffering changes what the placement sees, not just what it
leaves.

Only spruce moved, which is the population the refusal touches.

## The 7,261 split, now readable

With partial trees gone, `spruce_leaves -> air` is entirely "no tree here":

```text
  vanilla spruce leaves we do not place    7,261  ->  10,874
  spruce leaves we place and vanilla does not  2,549 -> 3,478
```

So of the original 7,261, **all 7,261 were genuinely missing**, and a further
**3,613 were only ever supplied by partial trees** — present in the old world
solely because a refused tree had already written them before giving up.

The true cost of the treeify limit on this window is therefore larger than the
~11,000 estimated last time: 10,874 spruce leaves absent, 3,478 misplaced,
plus the logs, against a total diff of 83,697.

Refusals also rose, 174 -> 188 spruce: a refused tree no longer leaves logs
for the *next* tree's `isFree` to find, so the trees after it take different
paths. That is a consequence of no longer polluting the world, not a
regression in the placer.

## Not verified

- The **decomposition of the +7,709 is not complete.** Undecayed leaves
  removed (-4,048), spruce leaves now missing (+3,613) and now misplaced
  (+929) account for about +494; the remainder is spruce logs and blocks the
  leaf-only accounting above does not cover, and I did not break it down.
- **Rollback and heightmaps.** `motion_blocking_no_leaves` is answered by the
  underlying world. Restoring a block through `set_block` should move the
  heightmap back, but whether every `TreeWorld` implementation does that on a
  *lowering* write was not checked, and no test covers it.
- The one remaining `distance=7` leaf is unexplained.
- The single **pine** refusal from the previous run is gone, unremarked; I did
  not chase why.

## Next, and it needs a decision rather than a commit

Reproducing `java.util.HashMap` treeification is now the only thing that makes
these trees exist, and it is a real subsystem. **Not started deliberately** —
it wants to be chosen, not drifted into. The alternative is to document the
floor: on this window it is of order 14,000 blocks, 17% of the current diff,
all spruce, all from one limit.
