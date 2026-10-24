# Pre-registration — `ruined_portal`'s blocks

**Written and committed before any measurement was taken on this branch.**
Branch `structures-ruined-portal-blocks`, off `structures-ruined-portal`
(`ca661b1`), itself off `structures-trial`. Purpose: fix the numbers this work
will be judged by *before* seeing them, so a partial close cannot be
re-described after the fact as the intended one.

---

## The baseline being predicted against

From `structures-ruined-portal`'s own measurement, quoted from its commit
message and its `## NOW` block, not from recall:

```text
  r=16 overworld, seed 20260722, centre (64,64), 1,089 chunks,
  RUSTMC_WORKERS=1, VANILLA FIRST, twin-masked N=2 against
  gen-wide-20260722-r16-ow-a / -b

  mismatches                29,927
  blocks (denominator) 107,017,525
  twin-mask floor (N=2)     35,531
```

**The floor (35,531) exceeds the residual (29,927).** By the project's Rule 6
this window is therefore **corroboration only**: no absolute exactness
percentage from it is quotable, and neither is 29,927 as a defect count. A
*delta* taken against the same reference, mask and denominator remains
meaningful. This document predicts a delta and a per-piece count; it
deliberately predicts no window percentage.

The item: **`ruined_portal` at (48,64) — 554 blocks, per-piece attribution,
the largest single item in the overworld residual.** The structure is not
built at all today; nothing calls `structure::ruined_portal`.

---

## What is being ported

`RuinedPortalPiece.postProcess` and everything under it, derived from 26.2
bytecode:

* `makeSettings` — the six-processor chain, in order: `BlockIgnoreProcessor`
  (`STRUCTURE_BLOCK` when `air_pocket`, else `STRUCTURE_AND_AIR`),
  `RuleProcessor`, `BlockAgeProcessor(mossiness)`, `ProtectedBlockProcessor`,
  `LavaSubmergedBlockProcessor`, and `BlackstoneReplaceProcessor` only when
  `replace_with_blackstone`;
* the rule list — gold-block@0.3 to air, the lava rule
  (`getLavaProcessorRule`), and netherrack@0.07 to magma only when not cold;
* `spreadNetherrack`, `addNetherrackDripColumnsBelowPortal`,
  `addNetherrackDripColumn`, `placeNetherrackOrMagma`,
  `canBlockBeReplacedByNetherrackOrMagma`, `getSurfaceY`;
* `maybeAddVines` / `maybeAddLeavesAbove`, which for the measured portal
  **never run** — `ruined_portal.json`'s underground setup has `vines: false`
  and `overgrown: false`, and `postProcess` guards the whole per-position pass
  on `vines || overgrown`.

Three pieces of shared machinery have to move for this, and two of them are
shared with `end_city`:

1. **`place_piece` currently hardcodes `Mirror::None`.** The measured portal
   is `FRONT_BACK`. Mirror has to reach both `transform` and the state, since
   vanilla is `info.state().mirror(m).rotate(r)` — mirror first, then rotate.
2. **`rotate_state` gains a mirror sibling.** `Mirror.getRotation(dir)` is a
   180 or nothing depending on the facing's axis.
3. Three new `Processor` variants: `BlockAge`, `LavaSubmerged`,
   `BlackstoneReplace`.

Both (1) and (2) are `end_city`'s code. **The End window is therefore re-run
and reported either way, pass or fail** — it is the project's strongest parity
result (0 mismatches over 14,745,600 blocks, both twins) and no gate in
`ci/t0.sh` would catch a regression in it.

---

## THE PREDICTION

### Primary — the per-piece count

**`ruined_portal` at (48,64): 554 -> 0.**

I predict **0 remaining**, and I commit to the weaker claim that it will be
**at most 40** for this to count as a close. Anything above 40 is a partial
result and will be reported as one.

### Secondary — the window delta

**29,927 -> 29,373, a delta of -554**, with a tolerance of ±40 in the same
direction. The twin-mask floor is unchanged at **35,531** (the mask is a
property of the two vanilla references, not of our tree). No percentage is
predicted, for the Rule 6 reason above.

I predict the delta will **not** exceed 554 by more than the tolerance —
i.e. I do not expect closing the portal to also close residual currently
attributed elsewhere.

### Tertiary — the End

**0 mismatches, both twins, unchanged**, over seed 20260722, centre (83,62),
r=7, y 0..255, denominator 14,745,600, vanilla first. A single mismatch here
outranks the entire overworld result and blocks the landing.

### What I expect to remain, and where

If the primary lands short, I predict the residual will be in
**`BlockAgeProcessor`**, and specifically in `maybeReplaceFullStoneBlock`.
Reason, registered now: it has the deepest unconditional draw sequence of
anything here — after the 0.5 gate it builds **both** candidate arrays before
choosing, and each array costs a `getRandomFacingStairs` draw
(`nextInt(4)` into `HORIZONTAL = [NORTH, EAST, SOUTH, WEST]`), then
`getRandomBlock` spends a `nextFloat` against `mossiness` and a `nextInt(2)`.
Four draws are spent per qualifying block and two of them are for the array
that loses. A port that builds the arrays lazily is correct in its output and
wrong in its stream from the second block onward.

My second candidate is the `cold` property, which gates three separate things
(the lava rule, the magma rule's presence, and `placeNetherrackOrMagma`'s
draw) and is the one input here that is not derivable from the piece alone —
it needs `Biome.coldEnoughToSnow` at the stub.

### Named non-predictions

These are **not** measured by this window and no number about them will be
claimed:

* `partly_buried`, `in_mountain`, `in_nether` placements, and the giant-portal
  templates. The r=16 window holds exactly one portal, `underground`.
* `BlackstoneReplaceProcessor` — the standard overworld portal has
  `replace_with_blackstone: false`, so it is ported but unexercised.
* `maybeAddVines` / `maybeAddLeavesAbove` — dead for this setup, as above.
* `on_ocean_floor`'s lava rule branch.

A gate a window cannot exercise proves nothing, and these are declared here
rather than left to be inferred from a green number.

---

## How it will be falsified

A falsification sweep over the new code: bugs injected one at a time, the
tests that should catch each run against them, the source restored. Reported
as *n of m caught*, with **every uncaught injection named**, plus a control
run proving the suite is green before and after.

The sweep's own instrument failed on this branch's parent — it was killed
mid-run and left a one-character injection in the source that `rsync -az`
could not restore, because a same-length edit preserves file size and the
sweep had touched the mtime. Restoration is by explicit copy verified with
**md5 on both sides**, not by rsync.

---

## OUTCOME — appended 2026-08-09, after the measurement

Written after the fact and marked as such; nothing above this line was edited.

**The window this document predicts against no longer existed when the
prediction was tested.** `gen-wide-20260722-r16-ow-a` and `-b` did not survive
the laptop-death restore and are absent from the whole box. They were
regenerated with `GEN_ORDER=rows ci/gen-vanilla-region.sh 20260722 16 … 64 64`,
1089/1089 window chunks, `-a` holding vanilla's saved `ruined_portal` start at
(48,64) with `TP=768,42,1024`. It is a **different pair of runs**, so the
baseline in this document — 29,927 masked, floor 35,531 — is not comparable to
what follows and was not used. Everything below is a before/after taken
entirely within the regenerated pair, before = main `05c9a2d3`.

```text
                                  before      after     change   predicted
  per-piece ruined_portal (48,64)    554         27       -527    0, close if <= 40
  twin-masked residual            30,331     29,833       -498    -554 +/- 40
  admissible (no mask)            30,446     29,948       -498
  admissible exactness       99.971560% 99.972025%
  twin-mask floor (N=2)           29,560     29,560          =    (new pair)
  End c8362 r=7, both twins            0          0          =    0 unchanged
```

* **Primary: CLOSED on its own criterion, not on its headline.** 27, not 0.
  The document commits to "at most 40 for this to count as a close"; 27 is
  inside it.
* **Secondary: MISSED.** -498 against -554 ± 40, i.e. 16 outside the band.
* **Tertiary: HELD.** The End is unchanged at 0 of 14,745,600, both twins.

**The registered shortfall prediction was the right region.** This document
named `BlockAgeProcessor.maybeReplaceFullStoneBlock` and the draw *stream* as
where a shortfall would sit. What is left is `spreadNetherrack`'s apron:
netherrack present where vanilla has none and absent where vanilla has some,
scattered through the same volume, total 464 against vanilla's 406. That is a
stream offset, not a geometric one, and `int i1 = random.nextInt(max(1, 8 -
l/2))` sets the apron radius directly from it.

**A defect this document did not anticipate, and it was the larger one.** The
generation stub was taken at the template box's **centre** rather than at the
chunk origin, putting every block of the portal 4 east and 12 south. The first
measurement of the primary came back `554 -> 553` with the window 991 blocks
worse. `RuinedPortalStructure.findGenerationPoint` spends the box centre only
on `getBaseHeight` and builds its stub from `ctx.chunkPos().getWorldPosition()`;
vanilla's own saved `TPX/TPY/TPZ` for this portal is the chunk origin. The
position was the one saved field with no gate on it —
`the_four_saved_portals_reproduce_field_for_field` pins the plan and passed
throughout. `worldgen-dump ruined-portal-starts` now gates it line for line
against `jigsaw-starts <ref> ruined_portal`.

**The falsification sweep, first ever run** (its commit says "Not yet run"):
33/48 with **3 injections dead** because `f7254194`'s `!(x < 0.5)` -> `x >= 0.5`
rewrite stranded their find-patterns; the harness reported them as NO-OP rather
than as catches. Re-pointed: **34/48 caught, 14 uncaught, 0 NO-OP**, control
green either side, restore md5-verified. Nine of the fourteen uncaught are the
apron, which has no unit coverage at all — the same code the window blames.

---

## FOLLOW-UP — appended 2026-08-09, branch `worldgen-portal-apron`

Written after the apron item closed; nothing above this line was edited.

**The registered shortfall prediction named the right code and the wrong
mechanism, and the difference is worth recording.** This document predicted
that a shortfall would sit in `BlockAgeProcessor.maybeReplaceFullStoneBlock`
and in the *draw stream*. Both turned out to be live, but they are two
separate defects and the apron was neither of them:

* the apron was **one `nextLong`** — `StructureTemplate.placeInWorld` stamps
  `LootTableSeed` into every placed `RandomizableContainer` off the piece's
  own stream, every portal template holds a chest, and the very next draw is
  `spreadNetherrack`'s `nextInt(max(1, 8 - avgSpan/2))`. Vanilla entered that
  pass at draw 1 and this port at draw 0, so it drew offset 2 against
  vanilla's 3 and spread one ring too far;
* plus `TemplateStructurePiece.postProcess`'s **second pass**, which
  overwrites every placed jigsaw block with its `final_state`. `portal_3`'s
  jigsaw lands on the box floor inside the apron's inner disk, and vanilla's
  air there is the one thing `spreadNetherrack` will not build over;
* `maybeReplaceFullStoneBlock` **is** the remaining 14 blocks, all at y 43..44
  and all stone-brick spellings — which is the prediction landing, one item
  later than it was written for.

Measured against the regenerated `gen-wide-20260722-r16-ow-{a,b}` pair, before
= main `cd521254`:

```text
  window netherrack (vanilla 406)   464  ->  406
  per-piece box, margin 0            27  ->   14
  per-piece attribution, margin 16  361  ->   18
  admissible residual            29,948  ->  29,605
  End c8362 r=7, both twins           0  ->   0
```

**One clause of the closing criterion this document's successor inherited was
wrong.** "The window netherrack census reaches vanilla's 406 (obsidian 46)"
charged the obsidian gap to the portal. It is not the portal's: all 28
vanilla-only obsidian sit at y = -55 around (1036,-55,1188), chunk (64,74),
forty chunks away, at the top of a lava aquifer with water above it. A
separate item.

---

## The ledger caught up — appended 2026-08-09, later

Until this point every figure above lived only in this document and in
`docs/WORKLIST.md`: `ci/figures.toml` still registered the destroyed pair's
trio (30,279 / 29,927 / 35,531), so the published headline and the numbers
measured here disagreed inside one repository. That is now closed. The ledger
carries the regenerated pair, re-measured independently at `main` 8e283f57:

```text
  admissible (no mask)   29,605 of 107,053,056   99.972345%   quotable
  twin-masked            29,490 of 107,023,496   —            rule 6 refuses it
  floor (A vs B)         29,560
```

The 29,605 above reproduces exactly, from a fresh dump and a fresh pair of
unmasked diffs, which is the strongest thing that can be said for it: two
independent runs on two different trees agreeing on the same instrument.

**The masked residual is the one to watch.** This document's after-figure was
29,833 against the 29,560 floor — the floor *below* the residual, so rule 6's
premise was marginal rather than firing, and `docs/WORKLIST.md` recorded it
that way. At `main` 8e283f57 the masked residual is 29,490 and the floor
exceeds it again by 70. The verdict flips on a margin of tens of blocks, which
is the practical meaning of a window with 45 blocks of headroom: **re-check
rule 6 on this window every time, and never inherit its verdict.**

---

## Two corrections to the section above — appended 2026-08-17

**Appended rather than edited, on purpose.** This is a pre-registration: its
value is that it was written before the measurement and cannot be re-described
afterwards, so nothing above this line is touched, including the two errors
named here. Both are in the `2026-08-09, later` append, not in the predictions.

**1. `29,605 … quotable` in that table is wrong, and the number carries no
exactness claim today.** `ci/figures.toml`'s `ow-r16-admissible-n2` has read
`quotable = false, disqualifier = "uncontrolled chunk order"` since 2026-08-10.
The dump walked chunks x-major against references harvested z-major, and the
same instrument charges an **unmodified vanilla run 117,628 blocks** for that
condition alone against 3,804 for being a different run of the right one
(`docs/DETERMINISM.md` §15, ORACLE rule 29). **The seed-20260722 r=16 overworld
window has no quotable exactness figure at all.** Withdrawn as a *level* and not
as a *measurement*: 29,605 is still an accurate `|ours ∉ {A,B}|`, and every
delta taken against a fixed reference set is still exact, because a delta holds
the condition fixed on both sides.

**2. "a window with 45 blocks of headroom" is not a quantity.** It subtracts
29,560 — the width of the **twin mask** — from 29,605, a charge under a rule
that excludes nothing. Two different instruments.
`docs/DETERMINISM.md` §14 killed that subtraction on **2026-08-09**, the same
day this section was appended, by measuring the admissible instrument's *own*
floor against six new references: an innocent unmodified vanilla run is itself
charged **16,932** on this window at N=2. The same strike was applied to
`docs/PARITY.md`'s banner and to `docs/WORKLIST.md`.

**What survives, and it is the sentence worth keeping:** *re-check rule 6 on
this window every time, and never inherit its verdict.* That was right, and the
reason is stronger than the one given — not a thin margin, but a condition
nobody had controlled.

**The two percentages in the prediction table above (99.971560%, 99.972025%)
are deliberately not registered in `ci/figures.toml`.** They are *predictions*
made before measurement, which is a different kind of number from a published
measurement, and registering them as figures would give them an artefact and a
window they never had. That is why this file is not in the ledger's coverage
scan; the reason is recorded in `ci/figures.toml` beside the scan list.
