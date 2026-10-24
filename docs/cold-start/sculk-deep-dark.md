# Cold start — branch `sculk-deep-dark`

---

### COLD START — branch `sculk-deep-dark` off `2a50b80f` (`overworld-biome-census`), 2026-08-18

**The deep dark was never unported. It was switched off, behind a default
whose stated reason four later commits in the same file had already
overturned.** Worktree `~/grit/work-sculk`, remote dir `~/rustmc-sculk`.
`docs/PARITY-ROADMAP.md` §66 and §67.

## The thing to know before anything else

The census ranked `sculk` the largest unported family in the overworld by
blocks — 30,184 in window A against our 10 — and made
`sculk_patch_deep_dark` + `sculk_vein` the single next item. **Nothing needed
porting.** `sculk_patch.rs`, `sculk_spreader.rs`, the vein behaviour, the
catalyst and the discharge were all written on 2026-08-09 and sat behind
`RUSTMC_SCULK_VEIN`, defaulted off.

**The refusal tally could not have told you.** Window A declines 1,142
features across 24 kinds and not one is sculk: a feature switched off at a
boolean is not a feature refused, so it shows up in no tally while its block
census looks exactly like an unported family. That is the transferable part —
before believing a family is unported, grep the source for its name.

## What moved

```text
                    floor(N=2)   census   spread on   + air fix      x
  deep_dark A           17,695   47,290      59,796      44,180    2.50
  deep_dark B           21,900   34,408      54,413      33,499    1.53
  jungle J              10,414   41,024          --      44,460    4.27
```

Both `deep_dark` figures improved and both remain far above their floors.
**The census's closing criterion — below 17,695 in A and below 21,900 in B —
is NOT met**, and the deep dark is still the largest single defect in the
overworld. Block census, window A: `sculk` 10 → 25,278 of 30,184,
`sculk_vein` 412 → 8,705 of 9,053, `sculk_catalyst` **exact** at 47,
`sculk_sensor` 96 of 164, `sculk_shrieker` 12 against 10.

## The biggest single defect the switch was hiding was not sculk

Turning the spread on made `deep_dark` **worse** in both windows while the
block census got much better. That is the census's `cocoa` shape and it was
ready to be believed. Decomposing the change by block pair instead of
accepting the story:

```text
  van=air        ours=cave_air        129 -> 15,720   +15,591
  van=sculk      ours=deepslate    19,317 ->  7,389   -11,928
  van=deepslate  ours=sculk             0 ->  5,763    +5,763
  van=air        ours=sculk_vein      166 ->  4,847    +4,681
  van=sculk_vein ours=air           4,180 ->  1,161    -3,019
```

`SculkVeinBlock.onDischarged` replaces a vein that has lost every face with
`Blocks.AIR`, or `Blocks.WATER` where a fluid is present. This port wrote
`cave_air`. One wrong block name, worth more mismatches in window A than every
misplaced sculk block put together — and reachable only once the spread runs,
so it was invisible for exactly as long as the feature was.

> **Decompose a wrong-way movement by block pair before explaining it.** The
> explanation that fits the shape of the number is not the same as the one
> that fits its contents, and here the plausible one accounted for a third of
> the change.

## The control that makes the sculk numbers attributable

Switching the spread on moved **nine more registered rows** across A, B and J
— `dripstone_caves`, `birch_forest`, `lush_caves`, `grove`, `meadow`,
`sparse_jungle` twice, `plains` — none of them `deep_dark`. A residual is
attributed to **vanilla's** biome at each position, and a sculk patch
spreading through a cave system writes across biome boundaries.

**Window E did not move by a single block.** Five rows, all identical to the
census. E holds no `deep_dark`, so nothing spills into it — and a control that
moves by exactly zero is what makes the other nine attributable to the switch
rather than to drift. `plains` in B *improving* while its neighbours worsen is
the second tell.

## Jungle: the wrong way for the third time

`mega_jungle_trunk_placer` was the one item whose closing result was already
measured — beat 41,024. It is ported, together with
`jungle_foliage_placer` (`MegaJungleFoliagePlacer`, which was `Unimplemented`
too; `TreeConfig::is_supported` is an AND, so the trunk placer alone would
have changed nothing) and `GiantTrunkPlacer`. `minecraft:bamboo` is ported as
well — the last refusal in the jungle's step-9 list.

The block census says all three are right: `jungle_leaves` 8,984 → 11,798 of
12,001, `jungle_log` 1,670 → 2,593 of 2,655, `vine` 9,388 → 11,400 of 11,569,
bamboo 137 against 131. **`jungle` went 41,024 → 44,460.**

`sparse_jungle`, which has no mega jungle trees, is **1.13x** its floor in the
same square, so the selector, the decorators, `cocoa` and the rest of the tree
machinery are all right. What is wrong is confined to the mega jungle tree,
and a padding sweep localises it:

```text
    extra draws        0        1        2        3        4
    jungle J      44,239   43,858   41,671   44,661   43,281

(the sweep was taken before the sculk switch, at 44,239; the tip reads 44,460)
```

A clean minimum at **two draws per mega jungle tree** and nothing like it
elsewhere. The pad was not committed — draws spent at the end of a tree put
the *following* trees back in step while leaving that tree's own blocks
displaced, which is why 41,671 is still above 41,024.

**Do not re-read the class.** Every part of both placers has been checked
against `javap -p -c -constants` on the pinned jar and matches: two draws in
`getTreeHeight`; drawless constant `foliageHeight`/`foliageRadius`;
`placeTrunk` receiving the **clipped** height `k1` and not the drawn `i`;
`placeBelowTrunkBlock` four times with no guard; the branch loop's `nextFloat`
and `nextInt(4)`; the crown carrying `doubleTrunk = true` so it skips
`MegaJungleFoliagePlacer`'s `1 + nextInt(2)` while every branch takes it; the
decorator lists already sorted stably by y. What is missing is an
**instrument**: our own draw count is readable through `RUSTMC_TRACE_DRAWS`
and vanilla's is not.

## A correction to the mechanism, and it narrows it

I found `minecraft:bamboo` by noticing `bamboo_light` sits immediately before
`trees_jungle` in `minecraft:jungle` and is absent from
`minecraft:sparse_jungle`, and read that as the head of the desync chain.
**Wrong.** `setFeatureSeed(decorationSeed, index, step)` gives one stream per
feature index, so bamboo cannot displace a jungle tree by way of the random;
the only coupling is world state. The census's "a refused tree spends no
randomness" mechanism is real and operates **strictly within one placed
feature's own stream**, which is where `trees_jungle` places many trees from
one stream. Bamboo is worth porting and is not the jungle's problem.

## Nulls — do not re-run these

* **Window B cannot adjudicate `jungle`.** 8,823 against a floor of 12,788 is
  0.69x — under its floor, not a measurement. The census said 0.53x and one
  more port has not changed the verdict. Window J is the only square that sees
  jungle.
* **Do not port more jungle decorations hoping the number falls.** Three
  consecutive wrong-way movements — `cocoa`, then the trunk placer, then
  bamboo — are one cause seen three times, not three causes. Correct
  displaced content counts twice.
* **Do not re-derive the sculk cursor logic.** `dadbff41` traced a patch's
  cursors and found them correct; the fault is which face a spreading vein
  occupies, and that lives in `multiface_growth`, upstream of every line of
  sculk code and shared with glow lichen.

## Which window to work in, which is not the one the census named

```text
                    residual   sculk-involved   NOT sculk   floor
  deep_dark A         44,180           24,018      20,162  17,695
  deep_dark B         33,499           33,345         154  21,900
```

**Window A cannot close.** Its biggest single term is `vanilla=air,
ours=deepslate` at 17,706, and every one of those positions is also mismatched
with the spread off (17,755 there, and the 17,706 are a subset — the spread
removed 49 and added none). It is terrain, not sculk: vanilla's deep dark has
open cavern where ours has solid rock. Perfect sculk would
leave A at 20,162 against a criterion of 17,695.

**Window B is the one that adjudicates sculk**: 99.5% of its residual has a
sculk block on one side. Work there, and check A only for corroboration.

## THE SINGLE NEXT ITEM

**`multiface_growth`'s choice of which face a spreading vein occupies.** Ours
converts UPWARD where vanilla converts DOWNWARD from a vein with `down=true`,
which converts the wrong block and leaves a vein where vanilla discharged one
— the pair of residuals now measured at ~7,400 missing and ~5,800 extra sculk
blocks in window A. Four hypotheses have died on this feature and each death
moved the fault one layer up; this is the first pointing outside sculk
entirely, and **glow lichen in a cave exercises the same face choice with none
of the cursor machinery in the way.**

Second: `large_dripstone`. Still the largest feature refusal **by count**
anywhere in this port — 4,569 declines, 543/1,977/2,049 across the three
squares — and only **fifth by blocks** at 1,593. Both halves belong in one
sentence: a tally ranked by count ranks how often something is *attempted*.
Rank by blocks, corroborate by count.

Third: the mega jungle tree's two missing draws, once someone builds the
draw-count oracle rather than re-reading the bytecode.
