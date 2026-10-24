# PRE-REGISTRATION — what the whole-table three-position extract will say about the classes nobody measured

**Written 2026-08-19 on branch `wall-carried-connective`, BEFORE
`ci/shape-three.sh` had produced `data/vanilla/update_shape3.json` on this
tree.** The whole-table run was already under way when this was written; it
emits `byClass` for all 74 refusing classes, so every prediction below is
settled by an artefact this file cannot influence. Appended to, never rewritten:
if a prediction is wrong the wrong text stays and the outcome is written under
it.

## Why pre-register at all

§83.9 measured four classes with `ci/shape-three.sh --only` and §83.10 settled a
fifth from the bytecode. That is 67,212 of the 79,113 refusals. The remaining
**11,901 pairs sit in 69 classes and no one has said anything about them**, and
the whole-table run is about to say something about all of them at once.

A result that arrives with no prediction in front of it cannot embarrass
anybody, and §83.3 is this family's own worked example of the cost: a prediction
of zero new refusals went into a commit message, the regeneration said 2, and
the *finding* was that `discoveryLateRefusal` had been answering a different
question all along. That is only visible because the prediction was on the
record. So these are on the record.

The census's own verdicts are the null hypothesis, and §83.11 already
established they are a bad one: `FenceGateBlock` is 768 `recoverable` and came
back 768 `closed`, while `StairBlock` is 14,400 `recoverable` and came back
19,200 **refused**. `recoverable` says three fillers at one position agreed. It
never said one position was all the rule reads.

## P1 — `WeatheringCopperStairBlock`, 1,280 pairs: ALL REFUSE, on a fourth position

`javap` on the pinned jar: `WeatheringCopperStairBlock extends StairBlock` and
**does not override `updateShape`**. It inherits `StairBlock.updateShape`
verbatim, and that method's only shape decision is `getStairsShape`, which is
what §83.9 measured as reading *both* horizontal neighbours on an axis to pick
inner corner, outer corner or straight.

So this class is `StairBlock`'s rule wearing a different name, and §83.9 says
`StairBlock` refuses all 19,200 on a fourth position, 4,800 symmetric on each
horizontal.

**Predicted:** `refused` 1,280, `closed` 0, `connective` 0, `inert` 0. The
`byOffset` split should be symmetric across the four horizontals — 320 each —
by the same argument that made `StairBlock`'s split exactly 4,800 apiece.

**The census disagrees**, which is the point of naming it: it calls 960 of
these `recoverable` and only 96 `world`.

## P2 — `RedStoneWireBlock`, 6,480 pairs: ALL REFUSE, and NOT on a fourth position

The largest class §83 never named, and the third largest overall.

`RedStoneWireBlock.updateShape` reads no position itself — zero `getBlockState`,
zero `BlockPos` navigation in its own body. It delegates: `getConnectionState`
twice, `getConnectingSide` once directly, and `getConnectionState` reaches
`getMissingConnections` which reaches `getConnectingSide` again.

`getConnectingSide` is where the world is read, and it is not one read:

```text
  4  BlockGetter.getBlockState
  2  BlockPos.above
  1  BlockPos.below
  1  BlockPos.relative
```

Four `getBlockState` call sites against `WallBlock.sideUpdate`'s one, over at
least three distinct offsets (`above`, `below`, `relative`), and
`getConnectingSide` is itself called from three places on the path.

**Predicted:** `refused` 6,480, `closed` 0, `connective` 0, `inert` 0 — and the
refusal cause should be a *position*, at an offset the extract names, rather
than a level method. **A three-position extract is not one short here; it is
several short**, which is a different statement from `StairBlock`'s and the
reason this class is worth predicting separately rather than lumping with it.

**The census disagrees more sharply than for P1**: 4,272 `recoverable`, 2,176
`connective`, 32 `inert`, and **zero `world`** — the only large class in the
census with no `world` verdict at all, which would read as the *easiest* of
them if `recoverable` meant what it looks like it means.

## P3 — the aggregate: a third position will close FEWER than half of the 11,901

P1 and P2 alone are 7,760 of the 11,901 predicted to refuse, which is 65% before
any of the remaining 67 classes is considered. `MossyCarpetBlock` (972) reaches
the world through `getUpdatedState` rather than in `updateShape`, and 551 of its
pairs are already `world` in the census.

**Predicted:** `closed` over the 69 unnamed classes is **under 5,950** — under
half — and the whole-table `closed` figure is therefore under `6,718`
(5,950 + `FenceGateBlock`'s 768), before whatever `WallBlock` contributes.

**This is the prediction most likely to be wrong**, and it is here because the
one that is easiest to be vague about is the one worth writing down. The 69
classes have a long tail of 60-to-200-pair entries that nothing has looked at,
and the tail is where the estimate is soft.

## What would falsify the method, not just the number

If a class the bytecode says reads three positions comes back `closed`, then
counting `getBlockState` call sites is not a bound on positions read — most
likely because two call sites resolve to the same offset, which the count
cannot see and the sweep can. That would matter well beyond these three
predictions: it is the method §83.10 used to settle `WallBlock`'s 41,472 pairs
without running anything, and `ci/wall-reads-check.sh` now runs it every gate.

## Corroboration for P2, from the census rather than the bytecode — still before the run

Appended 2026-08-19, still with no `update_shape3.json` on this tree. P2 was
derived by counting call sites in `getConnectingSide`. The **census already
contained the same answer by a completely different route**, and nobody read it
that way.

`ci/checks/shape-refusal-census.json`'s `byExtraPositionCount` buckets every
refused pair by *how many distinct extra positions* its rule reads:

```text
  positions   pairs
      1       54724     <- §71's "69%"
      2       13016
      9        5184
     12        1296
```

`minecraft:redstone_wire` has exactly **1,296 states** in this tree's own
`block_props.json`. So:

    5,184 = 1,296 x 4 directions   reading 9 positions
    1,296 = 1,296 x 1 direction    reading 12 positions
    ------------------------------
    6,480 = 1,296 x 5 directions   = RedStoneWireBlock's whole census count

to the pair, and 6,480 / 1,296 = 5 of the 6 directions. No other class in the
census has a pair count that lands on those two buckets.

**The census says `RedStoneWireBlock` reads nine or twelve positions.** A
three-position extract is not one short of that; it is six to nine short. And
the same artefact, in a different field, calls 4,272 of those pairs
`recoverable`.

That is the sharpest available statement of what §83.11 found on `StairBlock`,
and it was sitting in the census the whole time: **`byExtraPositionCount` and
the per-class verdicts are in the same file and they disagree**, because they
answer different questions — one is "how many positions does the rule read",
the other is "did three fillers at one position agree". Only the first bounds
what an extract can close.

So P2 now has two independent derivations, neither of them a sweep: the
bytecode says four `getBlockState` call sites over three offsets, and the
census says nine-to-twelve positions. If the extract closes or connects a
single `RedStoneWireBlock` pair, both are wrong together and the method in
§83.10 is what needs revisiting, not the number.

## P4 — the census's own position count is NOT a bound on the extract's, and the arithmetic already says so

Registered 2026-08-19, before the whole-table run, and it is a claim about the
**instrument** rather than about a class, which makes it the most useful one
here to be wrong about.

`byExtraPositionCount` looks like it bounds what a three-position extract can
answer: a pair whose rule reads one extra position is answerable, a pair that
reads two is not. Read that way the census sets a ceiling of
`340 + 54,724 = 55,064` of 79,113, which is **69.6%** — and that is almost
exactly §71's "reaches 69%", from `byExtraPositionCount.1` alone.

**The already-published numbers do not fit inside it.** Buckets ≥ 2 hold 24,049
pairs. `RedStoneWireBlock` accounts for 6,480 of them (§ above), leaving 17,569
for everything else. But §83.9 measured `StairBlock` (19,200) and `FireBlock`
(3,072) as refusing **every** pair, and 22,272 does not fit in 17,569.

    22,272 - 17,569 = 4,703

So **at least 4,703 pairs that the extract refuses sit in bucket 0 or 1** — the
census says their rule reads at most one extra position, and the extract, which
answers exactly one extra position, still could not answer them.

**Predicted mechanism, and this is the falsifiable part.** The two instruments
probe with different levels. The census's level **answers a filler for every
position**, so the rule runs to completion and the census counts the positions
that execution *touched*. The extract's level **throws for a fourth**, so the
rule takes different branches: a read the census's level satisfied silently
becomes a throw, and a branch the census never entered gets entered. A position
count taken against an answering level is not a bound on what a throwing level
will be asked for.

**Predicted:** the whole-table run's `byOffset` will show refusals at offsets
for pairs the census puts in bucket 1, and the count of such pairs will be at
least 4,703.

**If this is wrong** — if every refused pair turns out to sit in a bucket ≥ 2 —
then §83.9's `StairBlock` or `FireBlock` figure is wrong, because the
arithmetic above leaves nowhere else for them to be. Either way something
already published moves, which is why it is worth writing down before the run
rather than discovering afterwards which of the two it was.

**And it retires "69%" properly.** §83.11 corrected §71 by saying reaching is
not closing. The stronger correction is that 69.6% is not even a ceiling on
*reaching*, because the field it comes from was measured by a different
instrument than the one that does the reaching.

<!-- OUTCOME: appended below when data/vanilla/update_shape3.json exists. -->
