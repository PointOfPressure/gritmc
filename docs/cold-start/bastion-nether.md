# Cold start — branch `bastion-nether`

---

### COLD START — branch `bastion-nether` off `structures-surface` (`02039a69`), 2026-08-19

**The nether's decoration-stage structure hook lands, `bastion_remnant` with
it, and `soul_sand_valley` at r=16 goes 70,953 -> 2,548 — which is below
thirteen of the fifteen pairwise floors, so the window has stopped
adjudicating.** Worktree `~/grit/work-bastion`, remote dir `~/rustmc-bastion`.
Full account in `docs/PARITY-ROADMAP.md` §98.

## The thing to know before anything else

**Read the window off the reference, not off this file.** Every number here is
seed 20260722, centre (64,64), r=16, nether, `--bbox 48 48 80 80 --ybox 0 127`,
biome `soul_sand_valley` as *vanilla* records it, twin-masked, vanilla first,
dump order `zx` **read off `gen-wide-20260722-r16-nether-a`'s `HARVEST-ORDER`**.
The r=7 nether windows and the aggregate nether exactness are different
measurements and §98 says why neither may be quoted beside these.

## What moved

```text
                           before       hook      +walls   floor, N=6
  soul_sand_valley         70,953      2,548       2,455   1,709..6,133
  basalt_deltas           109,518    109,518     109,518   313,045..398,660
  nether_wastes                 5          5           5

  bastion_remnant (55,63)  46,858         98          23   105 pieces, margin 0
  bastion_remnant (70,49)  21,592         22           4    89 pieces, margin 0
```

As an exactness 2,455 is **99.991841%**, registered `quotable = false`,
disqualifier **below floor** — it is under the median of the fifteen pairwise
floors, so it is corroboration and not a level. The delta is the measurement.

The layout is gated separately and far more sharply than the block count:
`golden_bastion_start` compares against vanilla's own saved `Children[]`
**piece for piece over 279 pieces in three starts**, and `worldgen-dump
bastion-starts` prints ours in `jigsaw-starts`' own format so the 105-piece
start diffs to zero lines.

## Four things that are NOT what they look like

* **The published before figure is 70,130 and this branch measures 70,953.**
  The 823 is `soul_fire -> air`/`cave_air`: `SoulFireBlock.canSurvive` is on
  `nether-biome-census` and is not on `main` or on `structures-surface`.
  `70,953 - 823 = 70,130` to the block. Do not read the gap as drift.
* **The "3,442 biome floor" is one pair of fifteen** (`b` against `c`). The
  ensemble floor across all six r=16 references is **1,709..6,133, median
  4,048**, so the closing target everything was aimed at was set 1.8x too low.
  2,455 is below thirteen of the fifteen.
* **The third start at (45,43) is NOT a spurious one.** Vanilla saved a
  bastion there with 85 pieces and this port finds it with 85 pieces, every
  field equal. It is outside the r=16 diff square, not outside vanilla.
* **The fortress attempt has to be RUN even though no fortress block is
  written.** `NetherFortressStructure.findGenerationPoint` is unconditional and
  its biome tag is `#minecraft:is_nether`, so it always wins an attempt it is
  given. Running the bastion alone finds **four** starts inside the r=16
  square where vanilla saved **two**.

## What moved the WRONG way

Seventeen families were zero before the port and non-zero after it, because
before the port there was no bastion for them to be wrong in: **93
`blackstone_wall` rotations and 6 `fire -> air`**.

The 93 are **not a bastion defect**. `CROSS_COLLISION_BLOCKS` is 39 fences,
panes and bars and **a `WallBlock` is not one** — it extends `Block` directly
and its connections are `WallSide` enums. This port had never rotated a wall
and it had never cost a block, because no structure it placed had ever written
a *rotated* wall. `WALL_BLOCKS` and `permutes_connections` close it exactly:
**-93 and 0 families worse**. After both commits **one** family in this window
is worse than before the branch, the 6 `fire -> air`, and it is the same
unported family as the 823 above.

Published rather than folded into the final number, because a lane that hides
the intermediate cannot explain the final one.

## What the residual is made of now, and none of it is a structure

```text
   823  soul_fire      canSurvive, ALREADY WRITTEN on nether-biome-census
   403  fire           the same family, no branch anywhere
   314  crimson_roots  nether_forest_vegetation, 1 of §96.7's 17
   915  everything else, of which 313 is the unplaced ruined_portal_nether
        (48,64) and the rest is the basalt-deltas swap signature
  ----
  2,455  of which 27 lie inside the two bastion boxes
```

## THE SINGLE NEXT ITEM

**Merge `nether-biome-census`.** 818 blocks, already written, no port needed.
After that and `FireBlock.canSurvive` this window is under 1,250 against a
median floor of 4,048 and is measuring nothing — so the item after that is a
*window*, not a port. `basalt_deltas` is not it (§47).

## Nulls — do not re-run these

* **The r=16 soul-sand-valley window no longer adjudicates a bastion.** 2,548
  against 1,709..6,133. A further bastion improvement here cannot be
  distinguished from a reference swap.
* **`minecraft:linear_pos` is not worth porting.** It is registered in vanilla
  and reached by **zero** documents in 26.2. `ci/structure-surface.py
  --positions` re-derives that; porting it would be `Processor::BlockAge` in
  reverse.
* **A bastion needs no new element type and no new processor type.** All 176
  elements across 60 pools are `single_pool_element` and all 176 are `rigid`.
  Confirmed by this branch, not just by §96's survey.
* **`RUSTMC_JIGSAW_STREAM=1` was not flipped here.** It is global and the flip
  is the owner's. What this branch adds is one number to price it with: **a
  bastion spends 17 `nextLong` draws** it would not spend with the threading
  off. `region-differ census` counts 18 `minecraft:chest` in vanilla's r=16
  window and 17 in ours; the missing one is the unplaced
  `ruined_portal_nether`'s, and all 17 bastion chests land at the right
  position with the right state. §98.8.
* **The landing note, which matters more than the recommendation:** the knob
  on `worldgen-be-save-arms` gates `place_jigsaw_starts`, and this branch's
  nether hook threads the stream unconditionally at its own call site.
  Whichever lands second must bring the bastion call site under the knob, or
  `RUSTMC_JIGSAW_STREAM=0` is off for the overworld and on for the nether
  while reporting one state.

## One red that is NOT this branch's, controlled rather than assumed

`golden_carver_chunk::carved_chunks_match_vanilla` fails with **106 of
7,077,888 blocks**. The count and not the exactness, deliberately: an
exactness is a published figure and would need a ledger entry, a window and a
floor, and this is a failing test's own output on a tree nobody is quoting.
`ci/check-figures.py` refused the first version of this paragraph for exactly
that, which is the guard working. It is a T1 golden and nothing here can reach
it — this branch touches processors, a rotation table and the nether's
decoration hook, and that test compares carved chunks before any of them run.

Controlled rather than argued: `origin/structures-surface` was checked out
untouched into a second, identically provisioned tree
(`~/rustmc-bastion-base`) and the same test fails there with **the same 106**,
in 474.9 s against this branch's 539.8 s. So it is not attributable to this
work.

What is NOT settled, and is stated rather than guessed: whether that is a real
red on `structures-surface` or an artefact of a freshly provisioned gate
directory. Two trees is enough to exonerate a branch and not enough to
diagnose a test. It is left for whoever owns the carvers.

## Tools added

* **`ci/structure-surface.py --positions`** — every `position_predicate` in the
  jar and which pools reach it, so `PieceOrigin::unthreaded` is falsifiable
  from the datapack. `position_predicate_pools_outside_bastion` joins
  `--check`; four positive controls, one of them a pair, because a zero that
  means "the walk found nothing" is the failure this repo keeps finding.
* **`worldgen-dump bastion-starts`** — ours in the format `jigsaw-starts
  <ref> bastion_remnant` prints vanilla's — and
  **`crates/rustmc-worldgen/tests/golden_bastion_start.rs`**, which gates 279
  pieces across three starts. **Its first version was green while comparing
  nothing**: own `eprintln!` and `return`, pointed at `<ref>/region` when the
  reference IS the region directory. It hid two real things — the rotation
  spelling (`CLOCKWISE90` against vanilla's `CLOCKWISE_90`, one underscore)
  and the fact that the reference holds FIVE bastion starts, not two, because
  its four region files cover chunks 32..95.
* **`RUSTMC_TRACE_BEARD` now names each bastion start** with its chunk and
  union box, because a count cannot tell "one too many" from "one the diff
  window does not reach".
