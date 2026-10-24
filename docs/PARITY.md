# What the parity numbers mean, and what they do not

**Read this before quoting a number from this project anywhere it will be
read by someone who cannot run the measurement themselves.**

Every figure below is real and was produced by a command anyone can re-run.
None of them is the sentence "the generator is exact", and the gap between
the two is the whole subject of this file. It exists because the numbers are
good enough now that the temptation to round them up has arrived, and because
a claim that overstates itself once is not believed again.

Written 2026-08-02, immediately after the overworld's order-invariant core
reached zero on its second window, while the measurements were fresh.

> ## 🚫 2026-08-10 — THIS WINDOW NO LONGER CARRIES AN EXACTNESS HEADLINE.
> ## THE r=16 OVERWORLD LEVEL WAS TAKEN WITH AN **UNCONTROLLED CHUNK ORDER**,
> ## AND RE-TAKEN WITH THE ORDER SET IT SITS INSIDE ITS OWN NOISE FLOOR.
>
> **Both halves of that are needed and neither is a retraction.** The figure
> below is an accurate measurement of what it measured. It is not a measure of
> our fidelity, and re-taking it correctly does not produce one either — it
> produces a window that has run out of the ability to tell us from vanilla.
>
> **What was wrong with it.** Our dump walks chunks x-major
> (`Area::chunks()`); every reference it was scored against was harvested
> z-major (`ci/gen-vanilla-region.sh GEN_ORDER=rows` forceloads one z row at a
> time). Chunk order is a *condition* of a comparison in the way the seed is,
> not a detail of how a reference was made: the same instrument charges an
> **unmodified vanilla run 117,628 blocks** for having the wrong chunk order,
> against **3,804** for merely being a different run of the right one. ORACLE
> rule 29; `docs/DETERMINISM.md` §15.
>
> **Re-taken with the order matched — the dump generated in the order the
> references' own `HARVEST-ORDER` names.** Nine unmodified `rows` references,
> same seed, same window, vanilla first, nothing masked:
>
> ```text
>   |ours x-major ∉ V| over 8 references        14,270      the condition unset
>   |ours `zx`    ∉ V| over the same 8           2,450      the condition set
>   an INNOCENT vanilla run over the same 8   2,202..5,263  median 3,281, n=9
> ```
>
> **2,450 of 107,053,056 is 99.997711%, and it is registered `quotable =
> false` with the disqualifier `below floor`.** It sits *inside* the range this
> instrument charges a candidate that is genuinely unmodified vanilla — above
> the minimum, below the median, which is what "below floor" means here. An
> exactness quoted against a floor it sits under is **corroboration, not a
> measurement**: what it says is that over 107 million blocks the instrument
> can no longer separate this port from a vanilla run, which is a real and
> checkable statement and is not a percentage.
>
> **It is not a fidelity win either, and must never be quoted as one.** A
> deterministic generator locked to the references' own harvest order beats a
> vanilla run carrying scheduler jitter. Two properties, neither of which is
> fidelity, are enough to produce that ordering.
>
> **What the window still supports:** deltas, exactly, to one block, against a
> fixed reference set and a fixed pair of orders — including every delta ever
> taken against the figure below, because a delta holds the condition fixed on
> both sides. And the order-invariant core, ~850 blocks, which none of this
> explains away. See `docs/DETERMINISM.md` §16.
>
> ---
>
> ## ✅ 2026-08-10 — **99.992583% AT SEED 12345, AFTER `minecraft:iceberg`
> ## WAS PORTED. THE RESIDUAL FELL BY 88.5% AND THE FLOOR DID NOT MOVE.**
>
> **Same window, same eight references, same matched dump order as the entry
> below — only the generator changed.** Seed 12345, r=16, centre chunk
> (64,64), 1,089 chunks, 107,053,056 blocks, nothing masked, vanilla first.
>
> ```text
>   admissible charge, N=5, harvest rows, dump zx      7,940    99.992583%
>   the same charge before this change                69,288    99.935277%
>   an INNOCENT vanilla run over the same ensemble  2,577..4,835  median 3,387, n=6
>   separation, median charge - median floor               +4,526   (was +65,864)
>   p_hide(5) = 45.1%
> ```
>
> **The floor is identical to the digit, and that is the point.** A floor
> belongs to the reference pair that produced it and this change did not touch
> the references, so an improvement that moved the charge by 61,348 blocks
> while leaving the floor at 2,577..4,835 cannot be a luckier subset of
> references. It is the generator.
>
> **READ THE MARGIN, NOT ONLY THE PERCENTAGE.** The charge now clears the
> worst innocent reading by **3,105 blocks** where it used to clear it by
> 64,453 — 1.6x its floor against 14x. This window is close to spent as an
> adjudicator, and the honest reading of a bigger percentage here is *less*
> resolving power, not more. The next repair on it will need a floor taken
> against more references, or another seed.
>
> **Chunk order is now 41.8% of what is left, and it was 6.7% before.** In
> absolute blocks order barely moved — 4,936 then, 5,705 now — but the
> residual it is a fraction *of* fell by an order of magnitude. This is the
> sharpest demonstration this project has that **order is not a constant
> fraction of a residual**, and it is the same lesson §17 drew from comparing
> two windows, now visible inside one. The order-invariant core is 5,378,
> down from 65,119.
>
> **What closed it, and it was one defect where the worklist said two.**
> `minecraft:iceberg` had no arm in the configured-feature parser at all. That
> single feature is wired up twice in the extract — `iceberg_packed`
> (packed_ice, rarity 16) and `iceberg_blue` (blue_ice, rarity 200) — so the
> `blue_ice` absence and the `packed_ice` shortfall were never separate items.
> The positional census settles it:
>
> ```text
>   block         vanilla-c   vanilla-d       ours     gap      was
>   packed_ice      947,800     947,821    947,787      -13   -26,753
>   blue_ice         17,718      17,741     17,741      +23   -17,718
>   snow_block       14,727      14,727     14,719       -8    -1,682
>   ice             104,854     104,852    105,888   +1,034    +2,960
> ```
>
> **And the delta stated exactly, against a fixed reference set.** Over the
> 17,566 positions where vanilla-c and vanilla-d *both* hold `blue_ice`: we
> hit 0 before and **17,522** after, missing **44** — against an innocent
> floor of **0..151** misses (median 54, n=6) for a real vanilla run scored
> the same way. The miss count is inside the floor. The surplus is not: 439
> positions outside the six-way unanimous set against 272..397 for an innocent
> run, which is the piece left over and is named as such rather than folded
> into the headline.
>
> Registered `quotable = true` as `ow-r16-12345-admissible-iceberg`.
> `docs/DETERMINISM.md` §17.
>
> ---
>
> ## ⚠ 2026-08-10, **superseded** BY THE ENTRY ABOVE AND KEPT AS THE READING
> ## THE WINDOW WAS STOOD UP ON — **99.935277% AT SEED 12345.**
>
> **This number is not wrong and was not withdrawn; it is superseded.** It is
> what the same instrument read before `minecraft:iceberg` was ported, and the
> defect it names is what the port closed. Nothing under its figure id has
> been changed.
>
> **Seed 12345, r=16, centre chunk (64,64), 1,089 chunks, 107,053,056 blocks,
> nothing masked, vanilla first, and the dump order matched to the references'
> own `HARVEST-ORDER` from the very first measurement.** Against five
> independently generated unmodified vanilla runs this port writes **69,288**
> blocks that match none of them:
>
> ```text
>   admissible charge, N=5, harvest rows, dump zx     69,288    99.935277%
>   an INNOCENT vanilla run over the same ensemble  2,577..4,835  median 3,387, n=6
>   separation, median charge - median floor              +65,864
>   p_hide(5) = 45.1%
> ```
>
> **The charge is fourteen times the worst reading the same instrument gives a
> candidate that is genuinely unmodified vanilla.** That is what a window with
> resolving power looks like, and it is the exact opposite of the one above.
> Registered `quotable = true`. `docs/DETERMINISM.md` §17.
>
> **Why this one survives and 20260722 does not, in one line:** chunk order was
> worth 4,936 blocks here — 6.7% of the residual — against 83% there. **Order
> is not a constant fraction of a residual and must never be estimated from
> another window's.** The order-invariant core is 65,119 blocks against ~850.
>
> **What the 69,288 is made of, and it is one nameable thing.** A positional
> census: vanilla writes 17,718 and 17,741 `blue_ice` in two independent runs
> and **this port writes none at all**; `packed_ice` is 26,753 short and
> `snow_block` 1,682 short. That is not run-to-run noise — two vanilla runs
> agree on the `blue_ice` count to within 23 blocks — and it is not an artefact
> of the instrument. It is a block this port never places.
>
> **Corrected 2026-08-10:** this block originally went on to call the
> `packed_ice` shortfall "a separate item". It was not — both blocks come out
> of the one `minecraft:iceberg` feature, and porting it took `packed_ice`
> from 26,753 short to 13 short in the same commit. The line is left standing
> above and corrected here rather than edited away, because a wrong split
> between two items is exactly the kind of thing that gets re-derived.
>
> **So this is not a better result than the window above; it is a working
> instrument pointed at an unported feature.** The 20260722 window could not
> see this, because seed 20260722's window has no frozen ocean in it.
>
> ---
>
> ## ⚠ 2026-08-09, KEPT AS THE RECORD OF WHAT WAS PUBLISHED — **99.972345%**,
> ## MEASURED WITH NO MASK AND WITH AN **uncontrolled chunk order**.
>
> **Over the 33x33-chunk overworld window at seed 20260722, centre chunk
> (64,64), 107,053,056 blocks, this port writes 29,605 blocks that match
> neither of two independently generated runs of the unmodified
> `server-26.2.jar`.** Vanilla first, no twin mask, no positions excluded, full
> height, block states compared as full states. Measured at `main` 8e283f57.
> **Do not quote it as a level** — see the banner above for the condition it
> was measured under. Deltas against it remain exact.
>
> **WHY THIS REPLACED THE 2026-08-07 BANNER, AND WHY IT IS NOT AN
> IMPROVEMENT.** The previous headline — 30,279 of 107,053,056, 99.971716% —
> was taken against a pair of vanilla references that did not survive the
> 2026-08-07 disk failure. A floor belongs to the pair of runs that produced
> it, so when the pair was regenerated the entire trio had to be re-measured
> rather than re-quoted, and nothing from the old pair was carried across.
> **The two numbers are not comparable** — different references, and a tree
> that has moved several worldgen commits since. Nobody should read
> 99.971716% → 99.972345% as progress; it is a change of instrument, and the
> honest summary is that the port's exactness on this window was re-measured
> and came out about where it was.
>
> ~~**Quote it with its floor, because the floor is now the whole story.** Those
> same two unmodified runs disagree with *each other* by **29,560 blocks**
> over this same window, so our 29,605 clears vanilla's own run-to-run noise
> by **45 blocks**.~~ **THAT SUBTRACTION IS NOT A QUANTITY** — it takes the
> width of a twin mask away from a charge that uses no mask, and
> `docs/DETERMINISM.md` §14 killed it on 2026-08-09, the day after this banner
> was written. The rest of the paragraph was right: this window is very nearly
> exhausted as an instrument, a future change worth a few hundred blocks could
> not be told apart from noise on it, and a sharper claim needs a wider window
> or more references. 2026-08-10 finished the job — with the chunk order
> controlled the window cannot tell us from vanilla at all.
>
> **Rule 6 is not overridden here — its premise is absent.** The disqualifier
> exists because the twin mask *removes* the positions two vanilla runs spell
> differently, and a defect of ours can hide in what was removed. This figure
> removes nothing. Every position in the window is adjudicated against the set
> of answers vanilla is known to give there, so there is no blind spot to be
> larger or smaller than the residual. `ci/admissible-residual.py` computes it;
> `docs/DETERMINISM.md` §12 is the argument; `ci/figures.toml` registers it
> with the floor of the *masked* instrument still recorded beside it.
>
> **It is a lower bound on our exactness, never an over-claim.** Two references
> under-sample vanilla's answers, so a position charged to us here might be one
> a third vanilla run would clear. Adding references can only remove charges,
> never add them — the opposite of the twin mask, whose floor grows with every
> reference (`docs/DETERMINISM.md` §4).
>
> **And it is worse than the unquotable number it sits beside**, by 115
> blocks. That is the point. A re-instrumentation that improved the figure
> would be one nobody should believe.
>
> **What it settles.** The blind spot the banner below is about is 29,560
> positions wide on the regenerated pair. Measured, it is **115 blocks deep**:
> at 99.61% of the positions vanilla could not reproduce, we produced
> something vanilla itself produced. **73 of the 115 — 63.5% — involve
> `sulfur`, `sulfur_spike` or `cinnabar`**, so what is left of the blind spot
> is still mostly the sulfur cave, which is also the largest single family in
> the floor itself: 11,246 of the 29,560 floor positions, 38.0%
> (`docs/DETERMINISM.md` §11). The equivalent figures on the destroyed pair
> were a 35,531-wide blind spot, 352 deep, of which 89.5% were sulfur; they
> are not comparable to these and are kept only as history.
>
> **Everything in the banner below still stands.** The twin-masked figures are
> still unquotable and the 2026-08-06 N=6 route is still stale (the N=6 ladder
> itself was re-taken on fresh references on 2026-08-10 — `docs/DETERMINISM.md`
> §4.1 — and is unquotable for its own reasons). **No overworld r=16 exactness
> percentage in this file may be lifted out of it, including the one above.**
>
> ---
>
> ## 🚫 AUDITED 2026-08-07, RE-TAKEN 2026-08-09 — THE TWIN-MASKED r=16
> ## OVERWORLD FIGURES ARE NOT QUOTABLE, AND 99.959781% IS NOT ONE EITHER
>
> Every r=16 overworld percentage in this file was measured **before** two
> merges that are now in `main`, and the figure that replaces them has fallen
> **below its own window's noise floor**. Both halves have to be said together
> or the correction reads as an improvement. **The `reference pair` column is
> the 2026-08-09 addition and it is load-bearing**: the first three rows were
> taken against references that no longer exist, so their residuals, floors
> and denominators cannot be reproduced from this tree and cannot be compared
> with the last row.
>
> | | residual | floor (N=2) | reference pair | quotable? |
> |---|---|---|---|---|
> | published here, N=2 | 43,041 (99.959781%) | 35,531 | destroyed | **stale** — pre-village-savanna |
> | after `village-savanna` | 29,964 (99.972001%) | 35,531 | destroyed | **no** — floor exceeds residual |
> | after `final_state` + `CrossCollisionBlock.rotate` (`main`, `df30334`) | **29,927** (99.972035%) | 35,531 | destroyed | **no** — floor exceeds residual |
> | `main` 8e283f57, re-taken 2026-08-09 | **29,490** | 29,560 | regenerated | **no** — floor exceeds residual |
>
> **ORACLE rule 6's disqualifier fires on our own headline window, and it was
> re-checked rather than inherited.** A window whose vanilla-A-vs-vanilla-B
> floor is larger than our disagreement with vanilla is corroboration only and
> never a headline — that is the rule this file applies to the nether at r=16
> and to seed 31415926, and it applies here. When the references were
> regenerated **both** numbers fell: the floor by 5,971 to 29,560 and the
> masked residual by 437 to 29,490. The verdict held only because the floor
> fell further, and it is close — 70 blocks. So **no exactness percentage from
> the masked route is quotable and 29,490 is not quotable as a defect count**,
> exactly as 99.972035% and 29,927 were not on the pair before it. Neither is
> 99.959781%, which additionally describes a world two merges ago.
>
> **What survives, and the distinction is the whole point.** *Deltas* on this
> window are still sound: the −37 was pre-registered, taken against the same
> reference, the same mask and the same denominator, and a mask that hides
> the same positions equally in both arms cannot manufacture a 37-block move
> in the predicted direction. That argument is about a delta's arms matching,
> so it survives the change of pair intact — what it does not survive is being
> read across pairs. What this window can no longer do is say what the
> remaining residual *is*. That needs a sharper instrument, not this one again.
>
> **The N=6 ensemble figures in this file are stale too, and have not been
> re-taken.** `docs/DETERMINISM.md` §4 measured 37,471 at N=6; `village-savanna`
> moved that to **24,402**, and the −37 landed after *that* and was never
> re-measured at N=6. So there is no current N=6 number either. **Do not
> subtract 37 from 24,402 and publish the result** — an arithmetic inference is
> not a measurement, and this project has already been burned once by treating
> a computed suspect list as a measured count.
>
> Everything below is left standing rather than rewritten, because it was
> published. Read it as history from here down wherever an r=16 overworld
> percentage appears. `ci/check-figures.py` now fails if any of these figures
> is quoted without its disqualifier.

---

## 1. The reference, which is the part that actually distinguishes this

Parity is measured against **vanilla as shipped**. The reference worlds in
`oracle-work/` are generated by running the unmodified `server-26.2.jar` — the
same file `ci/vanilla-server.sh fetch` downloads — force-loading a square, and
harvesting the region files it writes. The jar is never patched, the
generator is never substituted, no datapack overrides the noise settings or
the biome lists, and nothing about the reference is derived from this port.

That is a deliberately harder target than measuring against a reference you
also control, and it is the reason the numbers move slowly. It also means a
disagreement is always *ours* until proved otherwise — with one exception,
which is §4's twin mask.

The port itself is clean-room: every rule is derived from the jar's bytecode
via `javap`, and `CLAUDE.md` forbids consulting any other implementation.

---

## 2. Where parity stands

All figures: **vanilla first**, twin-masked, full height unless a `y` range is
given, block states compared as full states and not block names.

**READ THE FLOOR COLUMN BEFORE THE RESIDUAL COLUMN — AND READ WHICH FLOOR IT
IS.** Every figure in this project is quoted beside the floor its window
carries and is never subtracted from it, and until 2026-08-18 this dashboard —
the one table a reader actually lands on — was the only place that dropped it.
Restoring it exposed something worse than a missing column: **the column is two
different quantities, and they move in opposite directions as N grows.**

```
union_N     positions where the N runs are NOT unanimous. The null for a
            TWIN-MASKED residual, whose mask widens with every reference
            added.                                  NON-DECREASING in N.

innocent_N  what the instrument charges a run known to be innocent — an
            unmodified vanilla run substituted for ours. The null for an
            ADMISSIBLE residual |ours not in S|.     NON-INCREASING in N.
```

They are **the same number at N=1**, the pairwise width, which is why the
divergence was invisible in a table where nine rows sat at N=2. On the identical
six references of the identical seed-12345 window they read **4,835 and
37,248** — a factor of 7.7 that grows with N.

Each row is internally coherent: the null is correctly matched to its own
instrument in every case. What is NOT sound is comparing the rows, which is
exactly what the column invites. So the `kind` is now printed with every floor,
and a floor without its kind is no more a number than one without its N.

A floor belongs to the ensemble that produced it, so the N is part of the
number and not a footnote: the End's floor is 0 across two references and 127
across six, on an ensemble whose six members were all harvested in one order.
`N` below is the reference count the floor was measured over, and it is not
always the reference count of the residual beside it.

**Seven of these floors were N=2 or absent on 2026-08-18 and have been re-taken
at N=7 or N=8** on 37 fresh unmodified-vanilla references — `docs/floors/
CAMPAIGN-SUMMARY.md`. Five rows changed verdict, in both directions.

| dimension | seed | window | blocks | residual | floor — kind (N) | order-invariant core |
|---|---|---|---|---|---|---|
| overworld | 20260722 | chunks 57..71 | 22,114,295 | 1,079 🚫 | was *not measured*; **union 11,344 (N=2), innocent 3,862 (N=7) 🚫 UNDER FLOOR / below floor** | **0** |
| overworld | 31415926 | chunks 59..69 | 11,894,185 | 6 🚫 | was *not measured*; **union 1,222 (N=2), innocent 527 (N=7) 🚫 UNDER FLOOR / below floor** | **0** |
| overworld | 20260722 | r=16, 1,089 chunks | 107,017,525 | 43,041 ⚠ | 35,531 union (N=2, destroyed pair); now **86,240 union / 5,263 innocent (N=8)** | — |
| overworld | 12345 | r=16, 1,089 chunks | 107,040,282 | 77,067 ⚠ | 12,774 union (N=2); now **37,248 union (N=5)** | — |
| overworld | 12345 | r=16, 1,089 chunks, **order matched, N=5** | 107,053,056 | **69,288** ✅ | 4,835 innocent (N=5) → **2,241 (N=15) → 362 (N=39)** | 65,119 |
| overworld | 31415926 | r=16, 1,089 chunks | 107,003,486 | 39,143 | was 49,570 union (N=2) on a pair **not on this box**; now **8,859 innocent (N=7) ✅ CLEARS at 12,542** | — |
| the End · `the_end` | 20260722 | c(0,0) r=7, 225 chunks | 14,745,600 | **0** | 0 (N=3) ✅ | — |
| the End · `end_midlands` | 20260722 | c(64,64) r=16 | 25,821,184 | **0** | 4 (N=6) ✅ | — |
| the End · `small_end_islands` | 20260722 | c(64,64) r=16 | 19,464,192 | **0** | 0 (N=6) ✅ | — |
| the End · `end_highlands` | 20260722 | c(64,64) r=16 | 18,415,616 | **0** | 123 (N=6) ✅ | — |
| the End · `end_barrens` | 20260722 | c(64,64) r=16 | 7,667,712 | **0** | 0 (N=6) ✅ | — |
| nether | 20260722 | chunks 57..71, y 0..127 | 7,372,752 | 8,472 | 48 union (N=2); **superseded — 104 on `nether-biome-census`** | **8,459** |
| nether | 20260722 | chunks 93..107, y 0..127 | 7,372,780 | 10,038 | was *not measured*; now **217 union / 38 innocent (N=7) ✅ CLEARS 12.6x** | — |
| nether | 31415926 | chunks 57..71, y 0..127 | 7,277,560 | 39,941 | 95,240 union (N=2); now **303,381 union / 22,377 innocent (N=7) 🚫 UNDER FLOOR** | **6,127** |
| nether | 20260722 | r=16, y 0..127 | 35,366,627 | 254,799 | 317,725 union (N=2); now **766,490 union (N=5) 🚫 UNDER FLOOR** | — |

**Raising N does not rescue a row — it depends which floor the row is quoted
against.** An admissible row's floor SHRINKS as references are added and a
twin-masked row's GROWS, so the same purchase clears one and condemns another.
The three 🚫 rows above could never have been cleared by buying references for
the instrument they were quoted on; the 31415926 r=16 row cleared only because
it was re-taken on the *other* instrument, and the two nether 🚫 verdicts got
more emphatic rather than less.

**What the 🚫 rows mean, and it is not what the residual column looked like it
meant.** A residual under its floor is not a small defect, it is *no
measurement*: two runs of unmodified vanilla disagree with each other more over
that window than we disagree with either. The nether r=16 row read as this
project's worst number at 254,799 and the floor over the same window is
317,725, rising to 766,490 at N=5 — the comparison has no resolving power there
at all. The same is true of the raw half of the 31415926 nether row (39,941
against 95,240), whose **6,127** core survives only because intersecting eight
traversals removes the traversal noise the floor is made of.

**And the two rows at the top of this table are the two with no resolving power
at all.** They carry no warning marker, an order-invariant core of 0 each, and
until 2026-08-18 an empty floor cell. Measured: 1,079 sits about ten times
under its own instrument's floor of 11,344, and 6 sits below even the *minimum*
innocent reading of 31. Both are **below floor**, both are corroboration and
neither is an exactness figure. Their cores of 0 stand — that is the
eight-order intersection and nothing here bears on it. `not measured` is what
hid this: an empty cell reads like a small number and it is not one.

**Two of the residuals above are the BEST OF NINE ORDERS and not a
measurement.** The two nether `chunks 57..71` rows are scored across nine
traversal orders and the column shows the cheapest: soul sand valley spans
8,472 to 16,535, basalt deltas spans 39,941 to 213,864 — 5.4x between the
cheapest order and the dearest. Both are registered in `ci/figures.toml` as
**best of nine orders** and neither is quotable, because a residual whose
orders span 5x is a statement about traversal and not about fidelity. §5 below
records that this project published exactly such a conclusion once, on
2026-08-02, and withdrew it the same day; the wide spread is a stop sign, and
it is visible before the intersection finishes. What survives the intersection
is the core column, and that is the half to read.

**The strongest rows in the table are the two the old shape flattened.** The
End at r=16 is 0 against a floor of 252 — byte-identical to vanilla across
71,368,704 blocks over a window where six vanilla references disagree with each
other in 252 places. And the End at r=7 is 0 against a floor of **0**: five
vanilla references, ten pairs, no pair disagreeing anywhere, so the instrument
has full resolving power and there is nowhere for a defect to hide. Without the
floor column both read as the same kind of zero as an unmeasured window.

**That End row used to say `three windows, r=7 and r=9` and now names one.**
The two r=9 windows are defined **nowhere in this repository, in any commit** —
no script, no log, no figure, no centre chunk. They were never destroyed; they
were never written down, so "exact in three windows" had two-thirds of its
evidence missing. The r=7 window is real, reproducible and now measured.

Every floor here is `ci/figures.toml`'s own `floor` field for the entry that
holds the residual beside it, and its kind is `window.floor_kind`;
`ci/check-doc-claims.py` reads this table.

Biomes are exact in every one of these: `biomes=N/0`.

🚫 **The 20260722 r=16 window has NO quotable exactness figure, in this table
or anywhere else — 2026-08-10.** It used to have one that was not in this table:
29,605 blocks matching neither reference, over the whole 107,053,056-block
window with nothing excluded. That figure was taken with an **uncontrolled
chunk order** and re-taking it with the order matched puts our charge inside
the range the same instrument charges an unmodified vanilla run. See the first
banner at the top of this file and `docs/DETERMINISM.md` §15 and §16. The three
rows below stay as they are.

⚠ **The three r=16 overworld rows are STALE, none of them is quotable, and the
20260722 one was taken against a reference pair that no longer exists.** They
are the state **after** the trial chamber port (2026-08-06) and **before**
`village-savanna` and `df30334`; on `main` 8e283f57, re-measured 2026-08-09 on
the regenerated pair, the 20260722 row is **29,490**, which sits below that
window's 29,560 floor and is therefore disqualified by rule 6 exactly as its
predecessor was. Its 107,017,525 denominator is the destroyed pair's masked
denominator — on the regenerated pair it is 107,023,496 — which is why the row
cannot simply have its residual swapped and be left in place. Seed 31415926's
39,143 was already below its own 49,570 floor. Seed 12345's 77,067 has a floor
of 12,774 and so is the only one of the three that ever had resolving power —
and it has not been re-measured since, on either pair. The banner at the top of
this file is the whole statement; §6 has the before/after and the caveats. Before it was superseded, the standing rule was
already that **the r=16 rows must not be quoted without them**.

✅ **Seed 12345 has now been cashed in, and the row above it is the result —
2026-08-10.** That is the `order matched, N=5` row: **69,288 of 107,053,056,
99.935277%**, taken on a fresh eight-reference ensemble
(`ci/refs-12345-ensemble.sh`) with the dump order matched to the references'
own `HARVEST-ORDER` from the first measurement, against an innocent-vanilla
floor of 2,577..4,835 at the same N. It is the **only quotable exactness
figure any r=16 overworld window carries**, and the paragraph above is now the
history of how it was reached rather than a live caveat on it. The stale
77,067 row is kept beside it, unmodified, because a number is retired by being
labelled and not by being overwritten.

**Two things about that row that must travel with it.** Its denominator is
107,053,056 and not the 107,040,282 above, because nothing is masked — the two
rows are not the same measurement with a better number in it. And its floor
belongs to *its* ensemble: the old 12,774 is not comparable to the new
2,577..4,835, because a floor belongs to the runs that produced it and, since
§15, to their harvest order too.

📈 **THAT ROW HAS BEEN OVERTAKEN FOUR TIMES SINCE AND THE LIVE FIGURE IS
99.998805% — 2026-08-12.** The paragraph above is kept because a number is
retired by being labelled and not by being overwritten, but the sentence in it
that reads *the only quotable exactness figure any r=16 overworld window
carries* is no longer true of 69,288: `ow-r16-12345-admissible-matched` is
`quotable = false`, superseded. The same window, the same instrument and the
same order went 69,288 → 7,940 (`minecraft:iceberg`) → 3,117 → 1,329 → and then
re-anchored on a pool of 40 references at **1,279 of 107,053,056, 99.998805%,
against an innocent-vanilla floor of 0..362 at N=39** — 3.53x, p_hide 7.4%.
That is the project's one quotable worldgen exactness figure and it is
registered as `ow-r16-12345-level-n39-iceberg`. `docs/DETERMINISM.md` §24 is the
reference ladder that bought it and §46 is the full inventory.

🚫 **AND THERE IS STILL NO AGGREGATE WORLDGEN PARITY PERCENTAGE — do not
compose this table into one.** Summing the best figure per dimension gives
99.962214% against a summed floor 9.52x larger than the summed residual, and

(Registered unquotable because the **floor exceeds its residual**, which is rule 6 firing on the aggregate itself.)
correcting the nether's estimator and holding every input at a common N=5 still
only reaches 99.954675% against a floor it sits 1.20x under. Both are
registered as `quotable = false` precisely so that this arithmetic, which is
trivial to redo and produces a respectable-looking number, is refused once in
the ledger rather than re-litigated. The seeds differ, the End's denominator is
ensemble-masked where the overworld's is not, and floors from two different
estimators at two different N are not addable. `docs/DETERMINISM.md` §55.5.

**The End's r=16 row is superseded and the dimension is closed at zero.**
Measured 2026-08-06 against six independently generated unmodified-vanilla
references: our dump is **byte-identical to one of them**
(`det-ens-end-d`, 0 mismatches unmasked over 71,368,704 blocks), its residual
against the six-reference stable set is **0**, and its order-invariant core
over eight distinct orders is **0**. The 125 is what this port differs from
*one particular vanilla run* by, and every one of those 125 positions is a
position vanilla does not reproduce between its own runs. All three routes are
in `docs/DETERMINISM.md` §6.

**The claim that the End's noise floor is exactly zero was wrong**, and it is
left here corrected rather than deleted because it was published. The floor of
that *pair* is zero; across six references it is **252** (pairwise: 0, 46, 81,
125, 171, 206, …). Two samples agreeing is not the same as there being nothing
to disagree about, which is what §4 says in general and what this is a case of.
The conclusion drawn from it — "there is no position vanilla is unsure about,
so there is nowhere for a defect to hide" — does not hold.

The nether's r=16 row is **corroboration only and never a headline**: vanilla's
own two runs of that window disagree by 317,725 blocks, which is *larger than
our whole residual there*. §6 states it.

---

## 3. What "order-invariant core" means, and what it excludes

The residual column is the raw twin-masked disagreement. The core column is
what survives `ci/decorate-order-intersect.sh`: the same window generated
under eight distinct chunk-visitation orders, with the mismatch *positions*
intersected.

The reasoning is that a mismatch which moves when our traversal moves is
noise about traversal, because **vanilla's own answer moves with vanilla's
traversal too** — two vanilla runs of one seed and one extent, harvested in
different orders, differ from each other by 13,366 blocks in the overworld,
253 in the End and 3,621 in the nether. **The End's 253 is an r=7 figure and
does not carry to r=16**, where the same `rows` against `rows_rev` control
moves vanilla by **944**, 100% of it `chorus_plant` and `chorus_flower` in
`end_highlands` (`docs/DETERMINISM.md` §49). Quote the one that belongs to the
window. A mismatch that survives all eight
orders is a candidate port defect.

**A core of zero is not a proof of exactness.** Three limits, all of them
written into the script's own header:

* **Eight orders is a filter.** A defect that the terrain moves under every
  one of the eight perturbations is invisible to it. The intersection is an
  **upper bound** on our exactness.
* **An order can hide a real defect** by changing the terrain underneath it,
  so the count is also a **lower bound** on the defect count.
* **The raw residual is not zero.** On the first overworld window it is still
  1,079 blocks. What is zero is the part of it that survives reordering. Ore
  blobs, the sulfur cave and seagrass are nearly all of the 1,079.

**A wide spread between orders is a stop sign, not a curiosity.** Basalt
deltas spans 39,941 to 213,864 — 5.4x between the cheapest order and the
dearest — where soul sand valley spans 8,472 to 16,535, about 2x. That ratio
is visible *before* the intersection finishes, and it is the signature of a
residual that is about to collapse: basalt deltas' did, by 84.7%. A
conclusion drawn from a raw residual whose orders span 5x has been drawn
about traversal. This project published such a conclusion once, on
2026-08-02, and withdrew it the same day.

And the method lesson that produced all of this, because it is the thing most
likely to be mis-stated: **"all nine orders agree with one vanilla reference"
was never a valid closing condition.** Vanilla fails its own nine-order test.
A 253-block "defect of ours" in the End was convicted by a control that varied
only which *ascending* row went first; adding a descending control showed
vanilla does exactly the same thing, in the same two chunks, with the same
count. Our port lands on **both** of vanilla's answers bit for bit, and making
the End order-invariant would have made it wrong.

---

## 4. What the twin mask excludes

Every diff runs `--twin` against a **second, independently generated vanilla
world** at the same seed. A position is only allowed to count against us if
the two vanilla runs spell it the same way. This is ORACLE rule 6.

It is necessary — vanilla is not deterministic across runs at the block level,
because decoration order depends on the order chunks are requested — and it is
a real weakening of the claim:

```text
  overworld window 1     4,105 positions dropped
  overworld window 2       599 positions dropped   (twin at a different
                                                    centre: overlaps, does
                                                    not cover)
  nether                    48 positions dropped   (nearly deterministic)
```

**A genuine defect of ours that happens to sit on a position vanilla disagrees
with itself about is invisible to these numbers.** It has already happened
once in the other direction: two marks were reported as ours-and-not-vanilla's
at (1179,26,1022) and (1180,6,1017) before the mark comparison was twin-masked
as well as the block comparison, and vanilla's two runs disagree at both.

### 4.1 The gameplay corpus has a floor too, and it is 7 of 88

> ⚠ **A newer ensemble reads 7, and both are correct.**
> `docs/PARITY-ROADMAP.md` §16.2 measured `yrot6-20260809` — six cold vanilla
> boots whose recordings carry `y_rot` — and got **7 of 88**, the two-name
> difference being `605`/`606`, which reproduce 0/15 there against 14/15 and
> 15/15 here. This section's 9 is not superseded by it: **a floor belongs to
> the ensemble that produced it**, and `twin-homog-20260809`'s recordings
> predate the `y_rot` decoder arm, so the exemption that converts those two
> cannot reach a field they do not carry. Quote whichever you like *with its
> ensemble*; do not quote 9 -> 7 as a movement. Noted 2026-08-10 from
> `corpus-coverage-audit`, which was briefed with the 7 and this section's
> citation and had to reconcile them. §20 of the roadmap.

The same rule, the same shape, a different instrument. `ci/corpus-floor.sh`
compares the whole gameplay corpus between independently recorded vanilla runs,
all pairs, and reports the scenarios two runs of the reference answer
differently. Measured over six cold boots of the unmodified jar on the
Threadripper, recorded 2026-08-09 17:23-21:57 and re-verified 2026-08-10 —
ensemble `gpfloors-homog6-20260809`, six trees, one recorder, 15 pairwise
comparisons, vanilla 26.2, protocol 776, superflat, seed 20260803:

```text
  7 of 88 scenarios do not reproduce between two vanilla runs
    7 vanilla's own nondeterminism   0 ours
  N = 6 recordings, 15 pairwise comparisons, alpha = 0.05
  projection proj:58380c9e4cbeae81
```

**This is the second ensemble to hold this floor, not a correction of the
first.** The original measurement — ensemble `twin-homog-20260809`, 2026-08-09
07:35-09:04 — read:

```text
  9 of 88 scenarios do not reproduce between two vanilla runs
    9 vanilla's own nondeterminism   0 ours
  N = 6 recordings, 15 pairwise comparisons, alpha = 0.05
  projection proj:58380c9e4cbeae81
```

Two of those nine — both `container-click-throw-drops-*` — were an artefact of
that ensemble's *decoder*, not of vanilla: `trace::decode` did not yet name
`add_entity`'s rotation fields, so the thrown item's random spawn yaw sat
inside an opaque `rest` digest and every recording disagreed on it by
construction. Once a decoder that names `y_rot` exists, both scenarios measure
**0/15 pairs, 1 distinct projection, stable** — reproduced independently on two
non-overlapping ensembles, `gpfloors-homog6-20260809` here and
`yrot6-20260809` in `docs/PARITY-ROADMAP.md` §16.1 — and the floor is 9 minus
exactly those two names: 7.

**The 9 is destroyed, not superseded, and is kept here for that reason.**
`twin-homog-20260809`'s six recordings lived at one path in one tree, each
`ci/t2-gameplay.sh` run overwriting the last; by the time this floor needed
re-deriving, only the sixth remained on disk and its sha256 no longer matches
any of the six the figure was taken from. So 9 of 88 cannot be re-measured
from this tree, however sound it was when taken, and this is a **destroyed
ensemble**, the corpus-floor analogue of the destroyed r=16 reference pair
above: not reproducible, not quotable, not deleted.

**The split was 8 and 1 until 2026-08-09 and the 1 was the instrument.**
`540-a-player-that-leaves-the-ground-is-resynced` measured `silent` — two of
the six runs "recorded nothing at all" — and both of them hold the packet the
check is about. The comparator was remembering where `530` had announced the
actor 32 blocks earlier and bounding every unpositioned observation out of
`540`. A player is no longer placed; the total is unchanged and the whole of
the floor is now vanilla's. `docs/PARITY-ROADMAP.md` §15.5.

**`projection` is part of the figure, not decoration.** A floor counts
scenarios two runs *project* differently, so it is a number under a comparator,
and this one did not say which until the same day. The staleness test was the
denominator alone — 88 against 88 — and a branch that changed
`compare::project` left the gate printing `9/88` beside a corpus score,
internally consistent and describing a comparator that no longer existed.
`ci/t2-gameplay.sh` now re-derives the identity from the binary it runs and
prints `STALE PROJECTION` beside the score, withdrawing the floor's
quotability, when the two differ. §15.

**It has a control, and the control is a whole second ensemble.** Six
full-corpus recordings taken the previous night by six *different* trees with
three *different* recorders — `nightly6-20260809` — give the same nine names
with the same classes. No recording and no recorder is shared between the two.
The per-scenario rates differ, and that difference is the honest size of what
N = 6 can support: `swapping-to-the-offhand` is 8 of 15 pairs in one and 12 of
15 in the other. What both support is that the nine do not reproduce; what
neither supports is a second decimal place.

A corpus score is quoted beside it and never net of it. `declared 62/88` does
not become `62 of 79`: the floor is an annotation saying which of the failures
could not have gone the other way, and `ci/t2-gameplay.sh` prints it under
every score it produces.

**It is a lower bound, and this section is where that trap is already
recorded.** At N = 6 a scenario that flaps less than 39.3% of the time still
shows six identical runs more often than one time in twenty, so nine is what
six recordings could see and not what is there. That is the End's lesson in the
other dimension — two references gave a floor of 0 and six gave 252 — and the
corpus version is worse, because the smallest rate this corpus has ever
measured is 1 in 26 and resolving *that* would take 77 recordings, about
fifteen hours of vanilla boots. **Stability is therefore not declarable here at
any affordable N**, and the protocol reports the resolving power rather than
pretending to have it.

**One of the nine is ours, and telling that apart is mechanical.** A projected
observation carries `who`, `step`, `kind`, `fields` and `when`; `step` and
`when` are the harness's own bookkeeping. Re-run the comparison with those two
blanked and a disagreement that survives is vanilla saying different things,
while one that vanishes is our step boundary landing inside vanilla's arrival
spread. The two have opposite fixes and used to read identically.
`ci/checks/gameplay-unstable.toml` keeps them in different tables and
`ci/corpus-floor.py` refuses a claim filed under the wrong one.

There is also a warning that fires before anything flaps, and on this ensemble
it fires on nothing. A step boundary with less than one server tick of observed
slack in front of it is not a boundary, it is an unmeasured coin.
`040-stairs-and-furnace-face-apart` was at **35 ms** while `Step::Hold`
returned 60 ms; it had never flapped, then one run lost the toss and read as a
branch regression. The window is now 250 ms and the same measurement on this
ensemble puts 040's tightest boundary at **217 ms**, over four ticks, with the
knife-edge set **empty across all 88 scenarios**. The instrument measured the
repair without being told it had happened — which is also why a margin is only
ever taken inside one recorder, and why the ensemble that mixes the two window
sizes reports 35 ms and 253 ms rather than an average of them.

---

## 5. The nether, stated separately because it is the honest weak spot

### RETAKEN 2026-08-18, and the weak spot is now a named list rather than a concession

**Every reference this section's 2026-08-02 numbers were taken against was
lost in the 2026-08-07 disk failure.** `van-nether-c64`, `van-nether-c100`,
`van-nether-s2-c64` and their `-b` twins exist nowhere on the box. So none of
what follows below could be reproduced, and 99.885% was a figure with no
artefact behind it. Sixteen fresh references were harvested on 2026-08-18 —
three windows, two seeds, `GEN_ORDER=rows` so the harvest order is stamped,
half of them under `-Dmax.bg.threads=1` — and every number in this block is
against those.

Read the dump order off the reference (`HARVEST-ORDER`: `gen_order=rows` →
`dump_order=zx`) before comparing anything. It is worth **335 against 7,638**
on this window, so a nether figure quoted without its dump order is not a
figure.

**The nether is now stated per biome, because an aggregate over a nether
window is a weighted average whose weights are that window's biome areas and
nobody reading it knows those weights.** `docs/DETERMINISM.md` §47 measures
the weights: 15.6% of the r=16 window carries 98.9% of vanilla's own floor.

All figures twin-masked, vanilla first, `--ybox 0 127`, dump order `zx`:

```text
  window                             biome              ours        of        exactness      floor
  20260722 c(64,64)  r=7   57..71    soul_sand_valley     104   7,323,536   99.998580%      75..151
  20260722 c(100,100) r=7 93..107    soul_sand_valley     253   7,372,749   99.996568%       43..71
  31415926 c(64,64)  r=7   57..71    basalt_deltas     33,112   2,052,634   98.386853%   113k..132k
                                     nether_wastes      1,165   3,105,666   99.962488%  1,768..4,284
                                     soul_sand_valley   1,312   2,090,453   99.937238%  1,533..2,603
  20260722 c(64,64)  r=16  48..80    soul_sand_valley  70,130  30,089,264   99.766927%  3,442..4,960
                                     basalt_deltas    109,518   5,188,505   97.889219%   313k..399k
                                     nether_wastes          5      28,634   99.982538%        1..41
```

**In six of those eight rows the floor exceeds residual**, so rule 6 fires and
the exactness may be used for deltas and for nothing else. Only two resolve:
the r=16 `soul_sand_valley` row at 14.1x its floor and the c(100,100) row at
3.6x. Re-take any of them with `ci/nether-biome-window.sh`.

### 2026-10-02, `nether-parity`: every measured window is inside vanilla's own spread but one, and it is decoration order

**Read this before the two sections below, which it supersedes as a statement
of what is ported.** The forest features, the fortress (walk, blocks, chests and
spawners), `ruined_portal_nether`, the dried ghast, nether mushrooms and the
post-process pass are all ported, each with a check that has been shown red.
The figures are taken against **every** same-order reference, not one twin
pair, because a nether harvest is not reproducible (`ci/nether-parity-windows.py`):
`ours` is our median disagreement with each reference, `floor` the median over
every pair of references, run counts beside them. Block counts, box
sayer-desktop, `--ybox 0 127`, dump order `zx` read off each reference.

```text
  window                   biome              ours    floor    runs (ours/pairs)
  31415926 c(0,0) r=7      crimson_forest    36,924   46,965   3/3
  31415926 c(-200,200) r=7 crimson_forest    30,956   35,593   3/3
                           warped_forest      5,914    7,020   3/3
                           nether_wastes         29       13   3/3   decoration order
  31415926 c(12,3) r=7     nether_wastes        109      326   3/3   (a fortress)
  31415926 c(-189,198) r=7 nether_wastes      1,913    1,253   3/3   basalt blob spill
  20260722 c(100,100) r=7  soul_sand_valley      33       61   4/6
  20260722 c(64,64) r=7    soul_sand_valley      89      183   6/15
  20260722 c(64,64) r=16   soul_sand_valley   2,737    4,048   6/15
```

So **no nether window adjudicates this port any more**, and the r=16
`soul_sand_valley` figure below has stopped being a level: 2,737 is under the
median of its own fifteen floors. What a block diff cannot see, the layout
gates do: `golden_fortress_start` holds 198 of 198 fortress pieces and 23
filler seeds equal to vanilla's saved `Children[]`, and
`golden_nether_ruined_portal` the three `in_nether` portals vanilla saved.

The c(-200,200) `nether_wastes` excess is 25 `twisting_vines` blocks from
chunk (-198,193) whose columns vanilla never grew, because a warped fungus hat
and nether sprouts from row z=194 were already there: vanilla decorated that
row's chunks first in all three runs, and ours decorates `zx`. The c(-189,198)
excess is basalt and blackstone blob spill from the neighbouring deltas, the
biome the null below says not to chase. `docs/cold-start/nether-parity.md`.

### All five nether biomes are now measured, and the last two are the largest unported thing in the dimension

The three windows above hold three biomes. `crimson_forest` and
`warped_forest` were unmeasured because no window this project had ever
generated contained either — which is a window-finding problem, and two
windows were found on 2026-08-18 by dumping candidate centres and reading the
per-biome denominators off a self-diff:

```text
  31415926 c(0,0)      r=7   crimson_forest 91.17%  nether_wastes 4.42%  soul_sand_valley 4.42%
  31415926 c(-200,200) r=7   crimson_forest 65.78%  warped_forest 24.42%  nether_wastes 9.81%
```

Three fresh references each, twin-masked, `--ybox 0 127`, dump order `zx`:

```text
  window                biome              ours          of      exactness        floor      x floor
  31415926 c(0,0)       crimson_forest   79,911   6,674,283   98.802703%   45,074..47,253      1.7
                        nether_wastes       448     325,575      —              30..57         7.9
                        soul_sand_valley    118     325,632      —               0..11        10.7
  31415926 c(-200,200)  crimson_forest   58,229   4,819,195   98.791728%   30,469..37,114      1.6
                        warped_forest    13,795   1,795,823   99.231829%    4,369..7,816       1.8
                        nether_wastes     5,053     722,927   99.301036%          4..17      297.2
```

**`crimson_forest` reads 98.802703% and 98.791728% on two independently
harvested windows a thousand blocks apart — agreeing to 0.011 percentage
points.** That agreement is what makes it a figure rather than a reading.

**And `worldgen-dump` names the cause without being asked.** Its REFUSED block
on the two windows:

```text
  crimson window        warped window
        3,871                 4,103    minecraft:huge_fungus
        2,870                 3,425    minecraft:nether_forest_vegetation
        2,378                 1,913    minecraft:weeping_vines
```

**Three feature families, none of them ported, and nothing else refuses.**
They are worth ~152,000 blocks across the two windows, which is **more than
twice the r=16 window's entire adjudicable residual** and makes them, not
`bastion_remnant`, the largest unported thing in the nether by area. They were
invisible for as long as they were, because every nether window this project
had measured was soul sand valley or basalt deltas, where none of the three
generates at all.

`nether_wastes` at c(-200,200) is the sharpest instrument in the dimension —
**297x its floor of 4..17** — and it reads 99.301036% because it borders the
forests and those three features spill across the boundary. A biome that is
below its floor on three windows and at 297x on a fourth is the clearest
possible demonstration that a floor belongs to a window and not to a biome.

```text
```

**`99.766927%` at r=16 in soul sand valley is the nether's one quotable
figure**, at 14.1x its floor — the same resolving power as the project's only
quotable overworld window. Everything else in the table is corroboration: the
biome's own vanilla-against-vanilla floor is larger than our disagreement with
it, so rule 6 fires and no exactness may be quoted.

**And the quotable one decomposes into a list of names.** Every residual
position charged to vanilla's own saved starts at margin 0
(`ci/attribute-residual.py`; margin 0 because `bastion_remnant` declares
`terrain_adaptation: none` and beards nothing):

```text
  46,856   66.8%   bastion_remnant (55,63)      105 pieces, NOT IMPLEMENTED
  21,591   30.8%   bastion_remnant (70,49)       89 pieces, NOT IMPLEMENTED
   1,261    1.8%   outside every structure box
     313    0.4%   ruined_portal_nether (48,64) portable today, never placed here
      75    0.1%   two nether_fossil boxes      out of ~137 fossil starts
```

**68,447 of 70,130 — 97.6% — is two structures that are not ported.** The
1,261 blocks that sit on no structure are *below* this biome's own floor of
3,442. So the honest statement of the nether's weak spot is not "something is
unmatched": it is

> two `bastion_remnant` starts, one `ruined_portal_nether` start, and
> `FireBlock.canSurvive`. Outside those, soul sand valley is inside vanilla's
> own noise at every window measured, and basalt deltas cannot be measured at
> all because vanilla disagrees with itself there 500x more densely than it
> does in soul sand valley.

`fire` is the fourth: `worldgen-dump`'s REFUSED block reports
`483x canSurvive(minecraft:fire[...])` at r=16.
`FireBlock.canSurvive` is `below.isFaceSturdy(UP) || isValidFireLocation(pos)`
and the second clause needs the 177-entry ignite-odds table `FireBlock.bootStrap`
builds. `soul_fire` was the same shape and is ported (2026-08-18): one tag
test, `#soul_fire_base_blocks`, worth **327 -> 104 and 477 -> 253** on the two
20260722 r=7 windows.

### What this replaced, kept because the reasoning still stands

Everything from here to the end of §5 is the 2026-08-02 record. Its
*conclusions* — that the nether's residual is systematic rather than order
noise, that the beard is worth forty-six times the bones, that a raw residual
must be decomposed by order before anything is concluded from it — all
survive. Its *numbers* do not: they were taken against references that no
longer exist, with a dump order that was never read off the reference, and its
two biome labels are corrected in place below.

8,472 of 7,372,752 was **99.885%**, down from 22,014 and 99.701% on
2026-08-02, and it is not order noise. The nine-order intersection takes it
from 8,472 to **8,459** — thirteen blocks, the same thirteen as before — where
the same decomposition removes 95.5% of the overworld's residual:

```text
  default 8472  diag 8472  spawn 8825  zx 8837  spiral 8913
  zx_rev 13364  spiral_in 15605  reverse 15959  zx_zrev 16535
  intersection  8459
```

That inversion is still the most useful single fact about the nether: it is
systematic, not a haze, and the order sweep has nothing further to say about
it.

**What closed the 13,542 was one structure.** `nether_fossil` — the only
bearding structure with a start in this window — plus the `Beardifier` it
finally had something to feed:

```text
  no nether structures     22,014    99.701%
  fossil placed, beard fed  8,756    99.881%     -13,258
  fossil bones written      8,472    99.885%       -284
```

The beard is worth forty-six times the bones. The terrain a structure raises
around itself is a much larger fact about a world than the structure.

**And it reproduces on a second, independently referenced window.** Centre
(100,100), chunks 93..107, its own vanilla pair
(`van-nether-c100`, `van-nether-c100-b`):

```text
  window 1, chunks 57..71     22,014 -> 8,472     -13,542   -61.5%
  window 2, chunks 93..107    25,641 -> 10,038    -15,603   -60.9%
```

Two windows a third of the dimension apart, two independently generated
vanilla pairs, the same effect to within half a percent. And the placement
oracle holds on both: **30 of 30 and 33 of 33** starts identical to vanilla's
own saved `Children[]` — chunk, template, rotation, position and box.

Window 2 has no order decomposition yet, so its core is unmeasured and the
table says so rather than borrowing window 1's.

### The second seed, and the caveat it forces on every nether number here

Seed **31415926**, chunks 57..71, its own vanilla pair
(`van-nether-s2-c64`, `van-nether-s2-c64-b`):

```text
  before  46,146 / 7,277,560   99.366%
  after   39,941 / 7,277,560   99.451%     -6,205   -13.4%
```

**13.4%, against 61.5% and 60.9%. That is not a regression and it is not the
port behaving differently — it is a different biome.** Normalised per fossil,
which is what the beard is actually a function of:

```text
  window                          closed   fossils   per fossil
  20260722, chunks 57..71         13,542        30        451.4
  20260722, chunks 93..107        15,603        33        472.8
  31415926, chunks 57..71          6,205        14        443.2
                                        mean 455.8, spread +/-3.7%
```

Three windows, two seeds, the same number to within 4%. The *percentage*
varies because the denominator does, and the denominator is whatever else is
wrong in that window.

**And what else is wrong depends on the biome, which is the caveat.**

**Those biome labels were asserted and are wrong, corrected 2026-08-18 by
measuring them** (`region-differ diff --by-biome`, which charges every
position to the biome vanilla records there; `docs/DETERMINISM.md` §47). Over
`--ybox 0 127`, 7,372,800 blocks each:

```text
  window                        soul_sand_valley   basalt_deltas   nether_wastes
  20260722 c(64,64)  57..71               99.33%           0.67%            —
  20260722 c(100,100) 93..107               100%               —            —
  31415926 c(64,64)  57..71               28.39%          29.44%       42.17%
```

The second window is 100% soul sand valley; **the first is not**, and its
0.67% is 49,152 blocks that carry **55–58% of that window's own
vanilla-against-vanilla disagreement**. And the 31415926 window's largest
biome is `nether_wastes`, not basalt deltas: basalt is its second by area, and
the section below calls `nether_wastes` unmeasured while 42% of this window
is made of it.

The claim the old labels were reaching for survives the correction and is
sharper without them: the 31415926 window holds 904,491 `basalt` and 167,026
`blackstone` against **zero of either** in the first, so it is the window with
basalt in it and the other is not.

**But its raw residual is the wrong number to compare, and the order
decomposition says so loudly.** §3's instrument, run on both:

```text
  window                          residual    core   order share   core exactness
  20260722 soul sand valley           8,472   8,459         0.2%       99.8853%
  31415926 basalt deltas             39,941   6,127        84.7%       99.9158%

  31415926 by order:
    default 39941  diag 39941  spawn 51235  zx 51319  spiral_in 108668
    spiral 129498  zx_zrev 155949  zx_rev 173073  reverse 213864
```

**Basalt deltas is not worse than soul sand valley. By the order-invariant
measure it is better.** 84.7% of its residual is chunk-visitation order — the
overworld's pattern, where reordering removes 95.5% — while soul sand valley's
residual is 99.8% order-*invariant* and therefore almost entirely real.

An earlier version of this section read the raw 39,941 as "this port is
99.451% in basalt deltas", and that was published before the decomposition was
run. It is withdrawn. **The standing rule exists precisely for this: decompose
by order BEFORE drawing a conclusion from a residual**, and it was not
followed here.

**So `the nether is 99.885%` is a soul-sand-valley number, and it is that
because soul sand valley is the HARDER biome.** Stated plainly, because it is
the sentence a release claim rests on:

* soul sand valley's residual is **99.8% order-invariant** — 8,472 raw against
  a core of 8,459. Unlike almost every other residual in this project it
  **cannot be argued away as traversal noise**. What is left is real.
* basalt deltas' residual is 84.7% order noise — 39,941 raw against a core of
  6,127 — so its larger raw number describes vanilla's traversal, not ours.
* **vanilla's own variance tracks the biome too**: the twin mask drops 95,240
  positions in basalt deltas against **48** and **20** in the two soul sand
  valley windows. (Re-measured 2026-08-18 on the replacement references:
  **124,047** against **151** and **51**. Same three orders of magnitude, same
  ranking, different pairs — a floor belongs to the pair of runs that produced
  it, and the pair that produced 95,240/48/20 no longer exists.)

Quoting 99.885% is therefore conservative rather than flattering: it is the
biome where the measurement is sharpest and the remaining defect is real.

**"Three of the five nether biomes remain unmeasured" was true when written
and is now NONE, measured 2026-08-18.** `nether_wastes` is 42.17% of the
31415926 r=7 window and 0.08% of the 20260722 r=16 one, and carries a
published figure in both (99.962488% and 99.982538%, both below their own
floors and therefore corroboration). `crimson_forest` and `warped_forest` had
never been measured because **no window this project had ever generated
contained either** — not the three r=7 windows, not the r=16 one. That was a
window-finding problem rather than a measurement one, and two windows were
found the same day; both biomes now carry figures. See the block above.

### Vanilla's own variance is biome-dependent too, by three orders of magnitude

The twin mask on the 31415926 window drops **95,240 positions** — 1.3% of it —
against **48** and **20** on the two soul sand valley windows. Diffing the two
vanilla runs against each other directly gives the reason:

```text
  22,083  basalt      -> netherrack
  20,761  netherrack  -> basalt
  16,772  basalt      -> blackstone
```

`basalt_deltas` decoration is enormously sensitive to the order vanilla
requests chunks in, far beyond anything the soul sand valley does. That makes
this window a **much weaker instrument**: a real defect of ours sitting on any
of those 95,240 positions is invisible to it (§4).

It is also the strongest argument in this file for why the twin mask is not
optional — **measured, not inferred**:

```text
  unmasked   102,727
  masked      39,941      the mask removes 62,786, or 61%
```

**A mask's size is not the number of your mismatches it removes.** The mask
drops 95,240 positions and removes 62,786 mismatches, because we agree with
reference A at 32,454 of the places the two vanilla runs disagree. The two
numbers are only equal if every position vanilla is unsure about is also one
you got wrong, which is a strong claim about your own port and never a safe
default. Adding a mask size to a masked count to recover an unmasked one is
wrong by that difference — this file said 135,181 before the measurement was
taken, against a true 102,727.

What has been *proved* about it, so nobody re-litigates it:

* the nether **density function** is bit-for-bit identical to vanilla's at
  every point sampled, including all eight lattice corners of a disputed cell
  (`ci/nether-density-probe.sh` against `worldgen-dump nether-density`);
* the nether **fill** is byte-identical over two whole chunks, 65,536 blocks,
  including the chunk that is worst in the finished diff
  (`ci/nether-fill-probe.sh --chunk`) — with the caveat in the next paragraph;
* our nether **carving** matches vanilla's to within four blocks in seven of
  nine chunks measured, on counts up to 3,354, and our nether ore features
  produce block censuses identical to vanilla's per chunk;
* the **biome layer** is exact.

**The fill probe has been checked against vanilla's own `doFill`, and both
were wrong in the same way.** `ci/nether-fill-probe.sh` and
`ci/nether-real-fill-probe.sh` agree position for position on both chunks tried
— 14,382 and 23,038 non-air, zero differing lines — and over the entire cell
lattice of chunk (57,64) the fill's block matches the sign of the density
function at 256 of 256 corners. All of that is true and none of it caught the
defect, because **every one of those checks passed
`DensityFunctions$BeardifierMarker.INSTANCE`, which computes zero.**

`NoiseChunk` injects that marker in its own constructor — it is never in the
datapack, in any version — and `wrap` substitutes a real `Beardifier` for it.
Measured on nether chunk (58,67) with a real `Beardifier` built from the
reference's own piece box: the beardless fill is 699 blocks short of vanilla's
finished world and the bearded fill is 75 short.

**The beardifier is ported as of 2026-08-02, so read the fill claim as: our
fill matched vanilla's where nothing bearded, and now beards.** The density
term is gated bit-for-bit against vanilla's own `Beardifier` —
`ci/beard-golden.sh` and `tests/golden_beard.rs` — over the 13,824-entry
kernel, both contribution functions, and `compute` for all five
`TerrainAdjustment` values.

What it changes is bounded by which bearding structures this port actually
places. That was **villages only** when this paragraph was written; as of
2026-08-06 it is villages and **trial chambers**, which are `encapsulate`
where the villages are `beard_thin`, so two different adjustments are now live
at once. Of the eleven structures in 26.2 whose `terrain_adaptation` is not
`none`, those six are the ones generated here. (`nether_fossil` was added
separately and is covered in this section's own measurements above.) On a window chosen to contain a village —
seed 20260722, chunks 73..87 by 69..83, twin-masked — the term is worth
**73,912 -> 54,882 blocks, a 25.7% reduction**. On a second village window at
centre (45,76) it is worth **123,027 -> 107,931, a 12.3% reduction**. Two
windows, two independently generated vanilla pairs, 34,126 blocks in total,
neither made worse. The piece list it is fed is identical to vanilla's own
saved `Children[]` on both — 73 of 73 and 98 of 98.

**None of the numbers in §2 moved**, and that was checked with the beard live
rather than assumed: all twelve region files across the three dimensions are
byte-identical to a control built from the pre-beard commit, because no
bearding start exists in any of those windows.

The other numbers are unaffected and that was checked, not assumed. The
overworld's r=7 window has one `shipwreck` and two `mineshaft` starts, neither
declaring `terrain_adaptation`; the End reference has one `end_city`, likewise;
and seed 31415926's only bearding structures are three `trial_chambers`
(`encapsulate`) whose starts are at chunks (5,10), (45,76) and (82,45) — far
outside the measured window of 59..69, and unreachable at a beard radius of 12.

**Check this for every new window before quoting its number.** One pass over
the reference's `structures.starts` against each structure's
`terrain_adaptation` is enough, and it is the check that would have caught the
nether a week earlier.

And the converse check, which is newer and cost a wasted measurement to learn:
**before believing that a term changed nothing, confirm it could have.** The
first run of the beard's own regression gate reported every window unchanged
while `RUSTMC_TRACE_BEARD=1` reported `starts=0` — there was no village in the
area, so the term could not have fired however wrong it was. "Unchanged" is
the same observation whether a term correctly declines or is wired to nothing.
A null result from an instrument that cannot fire is not evidence.

What is missing is named. `nether_fossil` is ported and gated — all 30 of
vanilla's saved starts reproduced exactly, chunk, template, rotation, position
and box — but fortresses, bastions and ruined portals are not. **This window
does not exercise them**, and that was checked rather than assumed: reading
`structures.starts` out of the reference gives 30 `nether_fossil` and one
`bastion_remnant`, and the bastion's pieces span x 833..880, chunks 52..55,
outside the measured 57..71. There is no fortress and no ruined portal start
in the reference at all. So the remaining 8,472 is **not** missing structures,
and a port of them will not move this number.

It is still the same positional signature it was — `soul_sand`/`soul_soil`
against `netherrack`/`air` in both directions, terrain sitting a few blocks
off — at a third of the amplitude. Plus two `canSurvive` rules. The measured
window is 100% `soul_sand_valley`, all 57,344 biome cells, so it exercises one
biome's rules and says nothing about the other four.

---

## 6. What none of these numbers covers

* **Other seeds.** Two for the overworld, one for the nether, and **none for
  the End that can be reproduced** — its 12345 and 31415926 references were
  destroyed on 2026-08-07 and never replaced, so the End is one seed until they
  are re-harvested (`docs/DETERMINISM.md` §51).
* **Other windows.** The overworld's zeros are r=7 windows — 15x15 chunks.
  **The overworld has now been measured at r=16 as well, and it is not zero.**
  That measurement is the subsection below, because what it limits is the scope
  of the zeros rather than their correctness.
* **Anything that is not a block state.** Block entities are not written, so no
  chest contents, no spawner contents, no sign text. Entities are not spawned.
  The scheduled `block_ticks` and `fluid_ticks` lists are not reproduced, and
  `PostProcessing` marks are measured separately by `ci/mark-oracle.py` (11
  missing of 4,034 on 24 validated chunks, 0 spurious).
* **Anything after generation.** No simulation, no random ticks, no fluid flow
  beyond the one `postProcessGeneration` pass, no weather, no growth.
* **Non-default world settings.** Everything here is a default world with
  `generate_structures = 1` and no datapacks.

### What the GAMEPLAY corpus does not cover either, measured 2026-08-18

The section above is about worldgen. Three audits finished on their own
branches this week and between them they bound what a green corpus run is
evidence of. Every number here is **cited, not measured by this document** —
`ci/checks/cited-audits.toml` pins each to the commit it was taken on and
`ci/check-cited-audits.py` re-reads that blob on every T0, so these cannot
quietly become descriptions of a measurement that no longer exists.

* **The projection removes 16 fields before any comparison happens** — 10
  discards and 6 normalisations. Two of the sixteen `decide` a live claim:
  its green is not evidence, it is the projection agreeing with itself.
  `informative.block_update_allowance` left `420` with one observation, a
  sound, whose pitch a different line in a different file had already dropped;
  and `project.observe_from` throws away a whole recorded client for 102 of the
  108 checks. Eight more are `armed` — real server behaviour that no check
  compares today and none would be told about.
* **12 of the 37 packet kinds `decode` can emit never enter a recording at
  all.** Not "no check reads them": nothing produces them, so no check could.
* **`crates/rustmc-server` carries 47 distinct unported gaps, and 41 of them
  are reached by no scenario.** 41 unported paths sit behind a green corpus.
  **47 is a lower bound and knowing why matters**: it is what the crate's own
  refusal vocabulary catches, and `item.rs`'s `DEFAULT_MAX_STACK_SIZE` is a
  carefully written extraction gap that no marker regex reaches because it says
  "a named gap" rather than "unported". The gaps written up most carefully are
  the ones least likely to use a keyword.

The unit is load-bearing on the last of those and the audit says so: the same
surface is 47 gaps, 85 markers or 29 files, and only the first is a quantity
anyone can work from.

**One retraction propagated here, because it changes what a corpus green
means.** `551-attacking-from-out-of-range` was first reported as a plain
divergence — vanilla four step sounds walking z=20 to z=3, ours five — read
from a single recording pair. Counted over every recording on this box it is
not one:

```text
  vanilla   4 in 32 runs, 5 in 39 runs   (n = 71)
  ours      5 in 66 runs, 4 in 0         (n = 66)
```

Vanilla is near a coin flip and we are pinned at five. **"We emit one too many"
is a bug; "we are deterministic where vanilla is not" is a class this corpus
cannot see at all** — every single-run comparison passes, because our constant
is a value vanilla does produce, and a check reading that stream would flap on
vanilla's variance and be declared unstable. That is the right treatment of the
reference and the wrong conclusion about the subject. What stands unchanged is
the axis: `551` keeps the actor, a player is not sent their own step sounds, and
no distribution of them can reach `551`'s comparison. Footstep parity remains
unmeasured by the corpus.

### The overworld at r=16, and the caveat that must travel with it

Measured 2026-08-03, re-derived and then improved 2026-08-06, and **re-taken
from scratch 2026-08-09 on a regenerated reference pair**. Seed 20260722,
centre chunk (64,64), **r=16 — 33x33 = 1,089 chunks**, `GEN_ORDER=rows`,
against two independently generated vanilla references
(`gen-wide-20260722-r16-ow-a` and `-b`), vanilla first.

**The `pair` column is the one to read first.** The two directories keep those
names, but the pair that produced the first five rows was lost in the
2026-08-07 disk failure and the pair that produced the last three was
generated on 2026-08-09. A floor belongs to the pair of runs that produced it,
so **no row may be compared with a row of the other pair** — not the
residuals, not the floors, and not the masked denominators, which differ
because each is its own window minus its own floor.

| comparison | blocks | mismatches | exactness | pair |
|---|---|---|---|---|
| vanilla A vs vanilla B — the noise floor, unmasked | 107,053,056 | 35,531 | — | destroyed |
| vanilla A vs ours, twin-masked, **before** trial chambers | 107,017,525 | 396,762 | 99.629255% | destroyed |
| vanilla A vs ours, twin-masked, **after** | 107,017,525 | 43,041 | 99.959781% ⚠ stale | destroyed |
| …after `village-savanna` | 107,017,525 | 29,964 | 99.972001% 🚫 below floor | destroyed |
| …after `df30334` | 107,017,525 | **29,927** | 99.972035% 🚫 below floor | destroyed |
| vanilla A vs vanilla B — the noise floor, unmasked | 107,053,056 | **29,560** | **99.972388%** | regenerated |
| vanilla A vs ours, twin-masked, `main` 8e283f57 | 107,023,496 | **29,490** | 🚫 below floor | regenerated |
| ours matching NEITHER reference, no mask, `main` 8e283f57, **x-major dump against rows references** | 107,053,056 | 29,605 | 99.972345% 🚫 uncontrolled chunk order | regenerated |
| ours matching NEITHER of **eight** references, no mask, **dump order MATCHED** | 107,053,056 | **2,450** | 🚫 below floor — an innocent vanilla run is charged 2,202..5,263 | 8 refs, 2026-08-10 |

🚫 **Every twin-masked row is measured against a denominator that has its own
pair's floor cut out of it, and the ones after `village-savanna` sit below
that floor, so none of their percentages may be quoted.** That is why the
2026-08-09 masked row carries no percentage at all: printing one would invite
exactly the lift this file exists to prevent. See the second banner at the top
of this file. The rows are printed because the *deltas within one pair* are
sound and the absolute values are not, and hiding the numbers would make that
distinction impossible to state.

✅ **The last row is the quotable one and it is a different instrument, not a
different window.** It charges a position to us only when our block matches
neither reference — so nothing is masked, the denominator is the whole window,
and there is no blind spot for rule 6 to be about. It is 115 blocks worse than
the masked row above it, and those 115 are exactly the positions inside the
mask where we are wrong by any vanilla answer. `docs/DETERMINISM.md` §12 has the
method and the monotonicity argument; `ci/admissible-residual.py` computes it
from two unmasked `region-differ --positions` runs and refuses if its parse
disagrees with the differ's own counter.

**The old caveat is retired because the thing it warned about was fixed, and it
is recorded here because it was published and it was wrong.** It read: *"~73% of
that residual is a single trial chamber this port does not generate at all."*
There were **three** chambers, at chunks (53,74), (74,78) and (75,55), and
attributing every mismatch to a structure's union bounding box by position put
them at **364,330 of 396,762 — 91.8%**, not 73%. A caveat written from one
inspected cluster under-counted a family with three.

Trial chambers are ported as of 2026-08-06 and gated piece for piece against
vanilla's own saved `Children[]` — three seeds, eight chambers, **1,906 of
1,906 pieces** identical in element, rotation, position, ground level delta and
bounding box (`worldgen-dump trial-chamber-oracle`).

**Three caveats travelled with 99.959781% while it stood. Caveat 3 has since
become the disqualifier — the window crossed over on `df30334` and the figure
is now unquotable outright rather than heavily qualified:**

1. **A chamber's block entities are not written.** Its trial spawners, vaults,
   chests and decorated pots are bare blocks with no NBT. The block states
   match; the contents do not exist.
2. **Sixteen of the twenty structure sets are still not generated**, including
   the `ruined_portal` that has a start inside this very window and is worth
   855 blocks of the remainder.
3. **This window is close to the end of its resolving power.** Its noise floor
   is 35,531 against a residual of 43,041 — 83%. The twin mask removes those
   positions, so every one of the 43,041 sits on ground both vanilla runs
   agree about and genuinely counts; but §4's weakening is no longer a small
   correction, because the ground a defect could hide on is now the same order
   as the residual itself. The next real information comes from a sharper
   instrument, not a wider one.

   **It crossed over on 2026-08-06/07.** `village-savanna` took the residual
   to 29,964 and `df30334` to **29,927**, both against the same unchanged
   35,531 floor. The prediction in the paragraph above — "the next real
   information comes from a sharper instrument, not a wider one" — is now the
   binding constraint rather than a warning.

   **And then the floor itself moved, because the references did —
   2026-08-09.** Every number in this caveat belongs to the reference pair
   lost in the 2026-08-07 disk failure. On the regenerated pair this same
   window's floor is **29,560**, not 35,531, and `main` 8e283f57's masked
   residual is **29,490**. The crossing-over verdict survives — the floor
   still exceeds the residual, now by 70 blocks rather than 5,604 — but it
   survives because it was re-measured, not because it was carried across. The
   caveat's lesson got sharper rather than weaker: on the quotable no-mask
   instrument the residual is 29,605 against that same 29,560 floor. **Both
   halves of that sentence were later refuted**: the subtraction is a category
   error (§14, 2026-08-09 — a mask's width against a charge that uses no mask),
   and the 29,605 was itself taken under an uncontrolled chunk order (§15,
   2026-08-10). The caveat's conclusion holds and neither of its stated reasons
   does.

   **Sharpened, 2026-08-06, by four more references.** Against six
   independently generated unmodified-vanilla references instead of two, the
   residual is **37,471** ⚠ **— stale; `village-savanna` moved it to 24,402
   and nothing has re-measured N=6 since `df30334`.** 5,570 of the 43,041 were
   positions where the first two runs happened to agree and a third does not,
   and were never ours. It
   has converged: the last reference moved it by 1.1%. Two things do **not**
   follow, and both matter. The floor does not shrink with more references —
   it is the size of the set of positions vanilla fails to reproduce, so it
   only grows, 35,531 → 73,820 at N=6 — **destroyed pair, 2026-08-09: that
   ladder was built on the references lost in the disk failure and no N=6
   ensemble has been re-taken on the regenerated pair, so its rungs are
   history and not an available instrument.** And the ratio of the two
   therefore gets *worse*, which means the floor/residual ratio was never the
   right measure of resolving power; that conclusion is about the shape of a
   union and does not depend on which pair seeded it.
   `docs/DETERMINISM.md` §4 has the full argument.

**And it is three seeds, both arms built from two commits of this branch**
(destroyed pair throughout, 2026-08-06; the 20260722 row's floor of 35,531 is
that pair's and is 29,560 on the regenerated one — the deltas stand because
both arms of each are inside a single pair, which is precisely what an
exactness figure cannot claim)**:**

| seed | floor | before | after | change |
|---|---|---|---|---|
| 20260722 | 35,531 | 396,762 | 43,041 | -353,721 (-89.2%) |
| 12345 | 12,774 | 164,143 | 77,067 | -87,076 (-53.0%) |
| 31415926 | **49,570** | 64,756 | 39,143 | -25,613 (-39.6%) |

**Seed 31415926's r=16 window is corroboration only** — its floor of 49,570 is
larger than its residual, which is rule 6's own disqualifier. It is quoted as
support for the other two, never on its own.

**What is left at 43,041 is still 61% structures**, and reading it off the top
block families would say otherwise — the same mistake the retired caveat made.
Attributed by position:

```text
    16795   39.0%  outside every structure box (terrain, carvers, ores)
    13659   31.8%  two village_savanna starts this port does not place at all
    11623   27.0%  still inside a trial chamber's own box
      855    2.0%  ruined_portal (48,64)
       109    0.3%  shipwreck + ocean ruin
```

**The 13,659 row is CLOSED — see the paragraph below and the village section
further down.** The row is left standing rather than edited away because the
attribution above is the N=2 one that goes with the 43,041 in this table, and
silently rewriting a published breakdown to match a later measurement is how
a caveat list starts lying.

* ~~**The chambers are not finished.** ... 11,623 blocks inside their boxes
  still differ.~~ **CORRECTED 2026-08-06. The chambers write 24 wrong
  blocks.**

  **`ci/attribute-residual.py` was right and this sentence misread it.** The
  tool adds a **16-block margin to every side of a start's union box** and
  says in its own docstring that it "narrows the suspect list; it does not
  close it". 11,623 was therefore a *suspect-list size*, correctly computed,
  and the prose above turned it into a defect count. Re-run on the rebased
  tree (residual 29,964 at N=2 twin-masked, seed 20260722, r=16, centre
  (64,64)), the three chambers come to **11,577** at margin 16 — the
  published method reproduces, so nothing about the tool is in question:

  ```text
    margin 16, union box   11,577   what could be the chambers' fault
    margin  0, union box    1,570   inside the hull of their pieces
    per-PIECE box              24   inside a block vanilla's saved
                                    Children[] actually covers
  ```

  A chamber's union hull is ~600,000 blocks of mostly untouched stone
  (start (53,74) spans x 796..895, y -43..24, z 1104..1195), and a 16-block
  margin on every side of that is where the other ~11,000 come from.

  **Corroborated by a census, which shares no mechanism with the positional
  diff**: over the chamber window every marker a chamber cannot avoid is
  exact — `chiseled_tuff_bricks` 2,797 = 2,797, `trial_spawner` 13 = 13,
  `vault` 18 = 18, `waxed_copper_bulb` 161/90/81 identical, `copper_grate`
  602/112 identical; `tuff_bricks` 41,486 vs 41,472, exactly the +14 the
  per-piece attribution blames on `corridor/end_2`. Two y-slices of chamber
  (53,74) are visually indistinguishable (`grit/chamber-renders/`).

  So `random_block_match` is **cleared, not untested** — the copper-bulb
  degradation splits 90 weathered / 81 exposed in both worlds, and that is a
  random outcome reproduced exactly. `liquid_settings` and waterlogging are
  **unmeasured**, because nothing in this window makes them observable; that
  is a different statement from "suspected".

* **What the per-piece attribution puts in front instead.** Of 29,964, only
  690 sit inside a piece box vanilla actually saved, and they are not
  chambers:

  ```text
     554  ruined_portal (48,64)  — portal_3: air->diorite, air->stone,
                                   netherrack->stone. Not built at all.
     110  village_savanna (81,77) — hay_block->air, acacia_leaves->air:
                                   the feature pool elements, 188 blocks,
                                   now unblocked (see the index below)
                                   [NOT CLOSED. Read the whole entry.
                                   village-pool-elements measured five
                                   elements of this village, 99 order-stable
                                   blocks, 0 newly charged, and corrected the
                                   188: it was a block-kind count and half of
                                   it was natural trees_savanna. The COUNT
                                   correction stands. The CLOSE does not —
                                   landing that branch beside the jigsaw
                                   stream work (2026-08-18, worldgen-land)
                                   left the elements ported but not run,
                                   because running one spends draws on a
                                   piece stream that is off by default.
                                   Closing this row is downstream of the
                                   owner's RUSTMC_JIGSAW_STREAM decision.]
      24  trial_chambers          — 14 of them -> tuff_bricks in
                                   corridor/end_2
  ```

  And two defect *families* fall out of the same pass, both definite rather
  than order noise (96 positions are the same block in a different state):

  ```text
      18  connection-only differences — 9 glass_pane, 9 acacia_fence.
          This is CrossCollisionBlock.rotate, which AGENT-BRIEF already
          lists as known-open. It is now measured rather than predicted.
      19  property-SET differences — ours is missing a property the block
          has: grass_block `snowy` x16, waxed_copper_bulb `powered`,
          waxed_oxidized_copper_grate `waterlogged`, acacia_stairs
          `half`/`shape`/`waterlogged`. These are jigsaw `final_state`
          strings written verbatim where vanilla runs
          BlockStateParser.parseForBlock against the block's DEFAULT state.
          place.rs's comment names that parser; the code does not call it.
  ```
* **The two villages were a pre-existing gap, not chamber fallout, and they
  are CLOSED as of 2026-08-06** (branch `village-savanna`). Both start outside
  the generated square, and `village_starts` skipped a village whose walk
  asked for a height outside it. There is no vanilla counterpart to that
  clamp: `ChunkGenerator.getFirstFreeHeight` is `getBaseHeight` and nothing
  else, a pure beardless noise column that needs no chunk, which is why
  vanilla can lay structures out at STRUCTURE_STARTS before any neighbour
  exists. Unclamping it took the r=16 overworld residual from **37,471 to
  24,402 blocks at N=6** and the two village boxes from 13,309 to 720. The
  village walk is gated piece for piece against vanilla's own saved
  `Children[]` — `worldgen-dump village-oracle`, 98/98 at (46,77) and 73/73 at
  (81,77). See the section below.

The true terrain share is 16,795 blocks in 107 million, **0.0157%**.

### The overworld is measured in eighteen of its fifty-five biomes, and the other thirty-seven now have three windows — 2026-08-18

**Every published overworld figure is the same map location.** All 41 entries
in `ci/figures.toml` with `dimension = "overworld"` are centre chunk (64,64) at
r=16, on seeds 20260722, 12345 and 31415926, and the two narrower rows in §2 —
chunks 57..71 and chunks 59..69 — are sub-squares of that identical centre.
Three points on the map. Every one of them was chosen by centre and radius; not
one was chosen by biome.

`region-differ diff X X --by-biome` — a reference against itself, zero
mismatches and a complete decomposition for free — says what is in them:

| window | biomes |
|---|---|
| 20260722 r=16 | savanna 42.74%, lukewarm_ocean 39.34%, river 7.44%, beach 6.48%, sulfur_caves 3.49%, windswept_savanna 0.51% |
| 12345 r=16 | frozen_ocean 78.70%, snowy_plains 8.43%, snowy_beach 8.30%, frozen_river 1.88%, snowy_taiga 1.84%, sulfur_caves 0.84% |
| 12345 r=18 | as above, plus ice_spikes 0.10% |
| 31415926 r=16 | lukewarm_ocean 61.74%, beach 22.17%, savanna 5.92%, river 3.77%, sulfur_caves 2.20%, jungle 1.72%, lush_caves 0.94%, plains 0.73%, deep_lukewarm_ocean 0.34%, forest 0.25%, sparse_jungle 0.23% |

⚠️ **The third row was added by correction and the count in this heading was
published wrong first.** Seed 31415926's references are named
`floors-ow31415926r16-*` under `~/rustmc-floors/oracle-work`, not `gen-wide-*`
like every other window's, and two searches keyed on the other naming family
each returned nothing and each looked conclusive. This section first said
twelve biomes and forty-three unmeasured. `docs/DETERMINISM.md` §48.5 is the
correction; the six it recovers are `jungle`, `plains`, `forest`,
`lush_caves`, `sparse_jungle` and `deep_lukewarm_ocean`, all of which sit in
the window §2 calls corroboration only and never quotable on its own.

**Eighteen of the fifty-five** the pinned jar's own
`biome_parameters_overworld.json` lists. So "the overworld at r=16" is a
savanna-and-warm-ocean reading on the flagship seed, and on the seed carrying
the only quotable r=16 overworld exactness this project has it is **78.7% deep
frozen water**. `desert`, `swamp`, `taiga`, `badlands`, `deep_dark`,
`dripstone_caves`, `mangrove_swamp`, `cherry_grove` and `pale_garden` are in
none of them, and `deep_dark` and `dripstone_caves` are the two this branch
went on to measure.

🚫 **And decomposed against its own pair's floor, one biome out of the
twenty-two readings across all three windows is above it** — `lush_caves` on
seed 31415926 at 1.02x over 0.94% of the square, which is as thin as this
project quotes anything. On the 20260722 and 12345 squares nothing clears its
floor at all.** `docs/DETERMINISM.md` §48 has both tables. That is a
harder statement than the aggregate disqualifier already on those rows: an
aggregate under its floor leaves open that some sub-window inside it still
resolves — which is exactly what the nether turned out to be doing — and here
no sub-window does. **The r=16 overworld square is exhausted per biome and a
sharper reading of it cannot produce anything.**

**Three squares were harvested to fix that**, chosen by
`ci/ow-biome-scout.sh` — which finds them from our own biome-stage dumps and
harvests nothing — and ranked by `ci/ow-biome-coverage.py`. Seed 20260722,
r=7, `GEN_ORDER=rows`, fresh twin pairs, dump order read off each reference:

| window | centre | biomes |
|---|---|---|
| A | (-1056,-672) | 12 |
| E | (-960,672) | 7 |
| B | (-480,-1440) | 9 |

**Twenty-three distinct biomes, against eighteen for the whole published corpus,
in three squares each one fifth the area.** Sixteen of them sit above their
floor and are measurements; the published squares have none. Twin-masked,
vanilla first, N=2, `--ybox` full:

```text
  window  biome                     ours     floor      x    exactness
  A       river                     2396       239  10.03    99.824140%
  A       old_growth_birch_forest  16252      1849   8.79    99.450163%
  A       dripstone_caves          11105      1482   7.49    98.924657%  SUPERSEDED
  E       windswept_hills            141        25   5.64    99.981378%
  E       dripstone_caves          24970      6874   3.63    99.462110%
  A       meadow                     546       155   3.52    99.925554%  SUPERSEDED
  A       deep_dark                47290     17695   2.67    97.499346%  SUPERSEDED
  B       sparse_jungle             7547      3108   2.43    99.768900%  SUPERSEDED
  E       sunflower_plains          1435       668   2.15    99.933333%
  A       birch_forest              9164      4555   2.01    99.768735%  SUPERSEDED
  B       dripstone_caves          21761     11616   1.87    99.518504%  SUPERSEDED
  B       deep_dark                34408     21900   1.57    98.683737%  SUPERSEDED
  A       grove                     3160      2604   1.21    99.909724%  SUPERSEDED
  B       plains                     382       336   1.14    99.979962%  SUPERSEDED
  E       swamp                     4506      4228   1.07    99.917763%
  A       lush_caves                4392      4229   1.04    99.659783%  SUPERSEDED
```

✅ **`deep_dark` and `dripstone_caves` are corroborated across independent
windows**, which is what makes them results rather than readings. `deep_dark`
resolves in A and in B, squares 900 chunks apart, at 2.67x and 1.57x.

⚠️ **Both `deep_dark` rows above were re-measured on 2026-08-18 and the reason
was not a port.** They were taken with the sculk vein spread switched off, so
the deep dark contained no sculk at all — 10 blocks against vanilla's 30,184
in window A. The whole family was already written and sitting behind a default
that four later commits in the same file had made obsolete. With the spread on,
and with `SculkVeinBlock.onDischarged` corrected to write `AIR` rather than
`cave_air`:

```text
  window  biome         ours     floor      x    exactness
  A       deep_dark    44180     17695   2.50    97.663800%
  B       deep_dark    33499     21900   1.53    98.718511%
```

The rows above are retired rather than overwritten — `ow-deepdark-A` and
`-B` — because a figure id names a reading. **Both improved and both remain
far above their floors, so the deep dark is still the largest single defect
in the overworld** and the closing criterion of 17,695 / 21,900 is not met.
`docs/PARITY-ROADMAP.md` §66.

⚠️ **Every square above was re-measured at the tip of `sculk-deep-dark`, and
nine more rows moved.** Not a re-harvest: the same reference pairs, the same
N=2 floors, the same dump order. Switching the sculk vein spread on writes
about 25,000 sculk blocks and 8,700 veins into the deep dark of these squares,
and **a sculk patch does not respect a biome boundary** — so a residual
attributed to vanilla's biome at each position moves for caves that merely
neighbour the deep dark.

```text
  win  biome                      ours    exactness
  A  dripstone_caves            11122  98.923011%
  A  birch_forest                9385  99.763158%
  A  lush_caves                  4669  99.638325%
  A  grove                       3362  99.903953%
  A  meadow                       616  99.916009%
  B  dripstone_caves            22106  99.510870%
  B  sparse_jungle               7589  99.767614%
  B  plains                       271  99.985785%   UNQUOTABLE: floor exceeds residual (336)
  J  sparse_jungle              11995  99.826747%
```

The rows they replace are marked `SUPERSEDED` above and retired in
`ci/figures.toml` under their own ids; each new reading is registered as
`<id>-sculk`. **Window E is byte-identical to the census** — no `deep_dark`
in it — and is deliberately not re-registered. `docs/PARITY-ROADMAP.md` §66.
`dripstone_caves` resolves in all three, at 7.49x, 3.63x and 1.87x. Neither
appears in any published window at any radius on any seed.

✅ **`minecraft:large_dripstone` is ported, and every one of those squares was
re-measured again on 2026-08-18 at the tip of `feat-worldgen-refused`.** Same
reference pairs, same N=2 floors, same dump order `zx`, same
`ci/ow-new-windows.sh`. It was this port's largest feature refusal **by
count** anywhere — **4,569 declines** across these squares, 543 in A, 2,049 in
B, 1,977 in E — and only **fifth by blocks** at 1,593. Both halves belong in
any sentence quoting either: a tally ranked by count ranks how often something
is *attempted*, and this feature is attempted 10..48 times per chunk over the
whole vertical column wherever the dripstone-cave placement fires, so most
attempts land in solid rock and place nothing. **The 4,569 declines are now
0.**

```text
  win  biome                      ours    floor      x    exactness
  A  dripstone_caves             9421     1482   6.36  99.087726%
  B  dripstone_caves             8027    11616   0.69  99.822390%   below floor
  E  dripstone_caves            14912     6874   2.17  99.678774%
  A  deep_dark                  44178    17695   2.50  97.663905%
  B  deep_dark                  33350    21900   1.52  98.724211%
  A  birch_forest                9078     4555   1.99  99.770905%
  A  old_growth_birch_forest    16218     1849   8.77  99.451313%
  A  river                       2398      239  10.03  99.823993%   WORSE
  A  meadow                       576      155   3.72  99.921463%
  B  sparse_jungle               7352     3108   2.37  99.774872%
  E  sunflower_plains             841      668   1.26  99.960929%
  E  windswept_hills               33       25   1.32  99.995642%
  E  swamp                       4488     4228   1.06  99.918092%
```

⚠️ **Windows J and O did not move by a single block**, and that is what makes
the thirteen rows above attributable. Neither square holds a dripstone cave, so
nothing spills into either; a control that moves by exactly zero separates a
port's effect from drift. It is the argument window E made for the sculk
switch, made here by two squares instead of one.

🚫 **Two of the thirteen are not good news and are published anyway.** `river`
in A went **2,396 → 2,398**: a port that improves twelve rows and worsens one
has worsened one. And `dripstone_caves` in **B fell from 1.90x its floor to
0.69x**, so window B has *stopped adjudicating* that biome — under ORACLE rule
6 the row is no longer a measurement at all. An improvement that removes a
window is still an improvement, and the next reader needs to be told the
window is gone rather than rediscover it. Windows A and E still resolve it, at
6.36x and 2.17x.

Each new reading is registered as `<id>-lgdrip` and the rows it replaces are
retired under their own ids. `docs/PARITY-ROADMAP.md` §76.

✅ **`dark_oak_trunk_placer` and `dark_oak_foliage_placer` are ported on the
same branch, and window A moved a third time.** One pair of placers is five
trees — `minecraft:dark_oak`, `dark_oak_leaf_litter`, `pale_oak`,
`pale_oak_bonemeal`, `pale_oak_creaking` — because `TreeConfig::is_supported`
is an AND over every part of a config. The first two are now placeable; the
pale oaks are still refused on the `pale_moss` decorator.

```text
  win  biome            ours    floor      x    exactness      before
  A  birch_forest       8614     4555   1.89  99.782615%        9078
  A  meadow              390      155   2.52  99.946824%         576
  A  grove              2563     2604   0.98  99.926779%        3362   below floor
```

⚠️ **Windows B, E, J and O did not move by a single block.** None of them holds
a `dark_forest` or a `pale_garden`, so four squares of five are the control.

🚫 **Neither `dark_forest` nor `pale_garden` can adjudicate this port.** They
sit at 0.67x and 0.52x their floors in A, so `dark_forest` 7,846 → 7,277 and
`pale_garden` 6,435 → 6,523 are movements without a verdict under ORACLE rule
6. The block census is the instrument here, and it says the trees place and
**overshoot**:

```text
  minecraft:dark_oak_leaves     0 -> 6784   vanilla 6017   +767
  minecraft:dark_oak_log        0 -> 2651   vanilla 2185   +466
  minecraft:pale_oak_leaves     0 ->    0   vanilla 6732   still refused
  minecraft:pale_oak_log        0 ->    0   vanilla 2397   still refused
```

✅ **The pale garden closes on branch `pale-census`, and window A moved a
fourth time.** `pale_moss` lands both pale oaks, and three defects underneath
it were then ported: `pale_moss_carpet`'s survival rule, then
`MossyCarpetBlock.placeAt`, then `WorldGenRegion.getRandom()`, which is a
**positional** stream at the generating chunk's origin rather than the
unmodellable server one. `leaf_litter`'s survival rule went with them.
**Window A's decline count across all features is 441 → 23.**

```text
  win  biome            ours    floor      x    exactness      before
  A  birch_forest       7902     4555   1.74  99.800583%        8614
  A  meadow              353      155   2.28  99.951869%         390
  A  grove              2596     2604   1.00  99.925836%        2563   below floor
```

⚠️ **Windows B, E, J and O are untouched, and seven of A's own twelve biomes
did not move by a single block across all six runs** — `deep_dark`,
`old_growth_birch_forest`, `dripstone_caves`, `lush_caves`, `river`,
`jagged_peaks`, `forest`. Two rows adjudicate and both improve.

🚫 **`pale_garden` and `dark_forest` still cannot adjudicate their own port**,
at 0.67x and 0.49x their floors. `pale_garden` 6,542 → 8,309 went *up* while
its contents became almost exactly right, which is the `cocoa` shape. The
block census is the instrument:

```text
  minecraft:pale_oak_leaves     0 -> 7191   vanilla 6732   +459   floor 356
  minecraft:pale_oak_log        0 -> 2744   vanilla 2397   +347   floor  28
  minecraft:pale_moss_block   723 -> 2902   vanilla 2836    +66   floor 213
  minecraft:pale_moss_carpet    0 ->  492   vanilla  512    -20   floor  30
  minecraft:pale_hanging_moss   0 -> 1022   vanilla  964    +58   floor  58
  minecraft:dark_oak_leaves  6255 -> 5775   vanilla 6017   -242   floor 107
  minecraft:dark_oak_log     2367 -> 2157   vanilla 2185    -28   floor  13
```

`pale_garden`'s 9,129 missing blocks — `pale_oak_leaves` plus `pale_oak_log`,
both at zero — are **806**, and both remaining rows are overshoots. Four of the
seven rows above are inside their own floor. `docs/PARITY-ROADMAP.md` §89.

⚠️ **Three rows moved the wrong way mid-port and all three came back**:
`dark_oak_log` reached 2,415, `leaf_litter` 3,153, and `grove` crossed above
its floor to 2,988. Each still-refusing pale oak desynchronised everything
decorated after it in its chunk, which is §58.6's mechanism. **A wrong-way
number from a partly-ported family is evidence about the incompleteness, not
about the port** — and publishing them was still right, because the
`leaf_litter` overshoot is what kept the last refusals worth chasing.

⚠️ **`large_dripstone`'s own census disagrees with its residual in A and agrees
in E**, which is the `cocoa` shape and has to be stated both ways.
`dripstone_block` in A goes 10,448 → 10,305 *away* from vanilla's 12,033 while
that window's residual falls 15%; in E, which holds 4.5x the dripstone-cave
volume, it goes 36,076 → **39,655** toward vanilla's 46,215 and closes 35% of
the gap. In A the few large dripstones the port adds are outweighed by the
pointed-dripstone clusters its draws displace.

🚫 **Under their floors in these squares, and therefore not measured here
either:** `mangrove_swamp` 0.38 — vanilla disagrees with itself over 214,991
blocks of it, 2.4% of the biome, which is the `basalt_deltas` shape in the
overworld — `pale_garden` 0.50, `stony_peaks` 0.51, `jungle` 0.53,
`jagged_peaks` 0.60, `dark_forest` 0.72, `forest` 0.89, `savanna` 0.16. They
are named so the next reader does not spend a harvest rediscovering it.

**What is in the residual, by name.** A block-family census of window A against
its own reference, sharing no mechanism with the positional diff:

```text
  minecraft:sculk           30184 -> 10       deep_dark, not generated at all
  minecraft:sculk_vein       9053 -> 412
  minecraft:pale_oak_leaves  6732 -> 0        pale_garden, no tree placed
  minecraft:pale_oak_log     2397 -> 0
  minecraft:dark_oak_leaves  6017 -> 0        dark_forest, no tree placed
  minecraft:dark_oak_log     2185 -> 0
  minecraft:dripstone_block 12033 -> 10440    large_dripstone, 543 declines
```

`minecraft:large_dripstone` is declined 543, 1977 and 2049 times in the three
squares — this port's largest feature refusal by count anywhere. The dark oak
and pale oak trees are one cause with two names:
`minecraft:dark_oak_trunk_placer` is not a supported trunk placer, and
`TreeConfig::is_supported` is an AND over every part of a config.

**One repair was made on this branch and it is the `swamp` row.**
`leave_vine` — 160 declines of `straight trunk, blob foliage, decorators
["minecraft:leave_vine"]`, where both placers were already supported and one
decorator refused the whole tree. Ported from `LeaveVineDecorator`:
**21,030 → 4,506, from 4.97x its floor to 1.07x**, and 16,532 blocks off that
window's residual. The identical shape is still open in jungle —
`["minecraft:cocoa", "trunk_vine", "leave_vine"]`, 186 declines, now waiting
only on `cocoa` — but `jungle` sits at 0.53 of its floor in window B, so
**porting `cocoa` could not be validated on any square this project owns.**
That is a reason to harvest a jungle window before porting it.

**Two further squares were harvested to answer questions the first three
raised**, and both did:

| window | centre | biomes | what it settles |
|---|---|---|---|
| J | (960,-192) | 9 | `jungle` is **3.83x** its floor here against 0.53x in B — the square that makes `minecraft:cocoa` validatable |
| O | (-1248,-672) | 4 | four ocean biomes, **zero feature refusals in the whole square** |

```text
  window  biome                     ours     floor      x    exactness
  J       jungle                   41024     10414   3.94    98.802055%  SUPERSEDED
  O       deep_ocean                 555       226   2.46    99.983868%
  J       sparse_jungle            11793     10363   1.14    99.829665%  SUPERSEDED
```

✅ **`cocoa` was ported against window J and the two halves of the result
disagree, which is the informative part.** Before the port, `jungle` read
**98.835636%** (39,874). After it, 98.802055% (superseded) — *worse* — while
`sparse_jungle` improved from 13,116 to 11,793 and the block census moved
decisively the other way: `cocoa` 0 → 65 against vanilla's 67,
`jungle_leaves` 0 → 8,984 against 12,001, `vine` 532 → 9,388 against 11,569,
and the `oak_leaves` overshoot fell from +7,234 to +1,265. Roughly twenty
thousand blocks of jungle canopy that did not exist now exist.

⚠️ **And it happened a third time.** `mega_jungle_trunk_placer` and
`jungle_foliage_placer` were both ported on 2026-08-18 — two placers, not one,
because `TreeConfig::is_supported` is an AND — along with the `bamboo`
feature:

```text
  window  biome         ours     floor      x    exactness
  J       jungle       44460     10414   4.27    98.701720%
```

The block census again says the port is right — `jungle_leaves` 8,984 →
11,798 of 12,001, `jungle_log` 1,670 → 2,593 of 2,655, `vine` 9,388 → 11,400
of 11,569 — and `sparse_jungle`, which has no mega jungle trees, sits at
**1.13x** its floor in the same square, so nothing in the surrounding tree
machinery is at fault. A padding sweep puts the remaining gap at about **two
draws per mega jungle tree**. The closing result the census named — beat
41,024 — is **not met**, and window B cannot adjudicate jungle at all at 0.69x
its floor. `docs/PARITY-ROADMAP.md` §67.

The two are consistent. `mega_jungle_trunk_placer` was unported when the
paragraph above was written — 93
declines in this square — and **a refused tree spends no randomness at all
while vanilla spends the whole tree's draws**, so every mega tree
desynchronises the stream for what follows it in that chunk and the small trees
this port now places land displaced. `sparse_jungle` has no mega trees, which
is exactly why it is the biome that improved cleanly and the biome that
validates the port. `jungle` cannot improve until the mega trunk placer lands
beside `cocoa`.

**Window B moved the same way, and since B and J share no chunks that is
independent corroboration**: `sparse_jungle` **7,976 → 7,547** (2.57x its floor
to 2.43x) and `jungle` 6,818 → 7,498, both under the same port. Two squares a
long way apart, the same split, the same cause.

🚫 **`cold_ocean` is 0 of 1,253,376 — byte-identical to vanilla over the whole
biome — and is NOT quotable**, because its floor over the same pair is also 0.
Two runs agreeing is not the same as there being nothing to disagree about;
that is this file's own argument about the End, made after the End's "floor is
exactly zero" claim was withdrawn, and it applies here unchanged. `ocean` at
0.15x and `deep_cold_ocean` at 0.77x are under their floors too.

`ci/ow-biome-census.sh` reproduces the composition and per-biome floor of the
published windows; `ci/ow-new-windows.sh` reproduces all sixteen figures above.

### The nether at r=16, which is corroboration only in aggregate and quotable in one biome

**RETAKEN AND SPLIT 2026-08-18.** This section's verdict — corroboration only
— was right about the window and wrong about why, and it was right for a
reason that does not apply to 84% of the window. Decomposed by biome
(`region-differ diff --by-biome`), twin-masked, vanilla first,
`--ybox 0 127`, dump order `zx` read off `HARVEST-ORDER`:

| biome | ours | of | exactness | floor, N=6 same-order pairs | verdict |
|---|---|---|---|---|---|
| `soul_sand_valley` | **70,130** | 30,089,264 | **99.766927%** | 3,442..4,960 | **14.1x — quotable** |
| `basalt_deltas` | 109,518 | 5,188,505 | 97.889219% | 313,045..398,660 | floor is 2.9x the residual |
| `nether_wastes` | 5 | 28,634 | 99.982538% | 1..41 | floor exceeds the residual |
| aggregate | 179,653 | 35,306,403 | — | 318,043..402,103 | **not quotable** |

**The aggregate is unquotable because 15.6% of the window is `basalt_deltas`
and that 15.6% carries 98.9% of vanilla's own floor.** Averaging a biome whose
floor is 313,045 with one whose floor is 3,442 produces a number that
describes neither. The quotable figure was there the whole time and the
aggregate was hiding it.

And `soul_sand_valley`'s 70,130 is 97.6% two structures — see §5's list. The
part of it that sits on no structure at all is **1,261 blocks against a floor
of 3,442**.

The original measurement and its reasoning follow, with one correction: **its
floor row belongs to a reference pair that no longer exists.** The `a`/`b`
pair on the box today gives **377,949**, not 317,725, over the same window and
denominator. A floor belongs to the pair of runs that produced it.
### The clamped harvest, and the choice this file has not made yet

**This section states two measured numbers and deliberately does not choose
between them.** Which one this file should publish is a judgement about what a
published number ought to mean, not a measurement, and it belongs to whoever
owns the claim rather than to whoever took it.

#### What changed

Vanilla's own harvest is not reproducible. Eight harvests of one small window
give five distinct worlds (`docs/DETERMINISM.md` §36), and the cause is
concurrency: `ChunkMap` dispatches chunk status upgrades onto a background pool
sized `availableProcessors() - 1`, so which chunk decorates first is a race
(§37). **Every reference ensemble this file has ever quoted was harvested
without controlling that.** So every floor here — and a floor is what decides
whether a residual is quotable at all — has been partly a measurement of
vanilla disagreeing with itself.

The unmodified server reads `-Dmax.bg.threads=1` for itself; nothing is patched,
and on a two-core box vanilla already runs at parallelism 1, so a clamped world
is a world the stock server produces unaided rather than a laboratory
construct. §38 and §39 measure what clamping the reference side is worth:

* At r=16 the raw twin floor falls **45.1%**, median 16,302.5 → 8,951, but not
  to zero: 15 of 15 pairs remain non-zero, 6,829..9,738 of 107,053,056. **There

  (That range is a floor and is registered **not a level**: it is a spread over fifteen pairs and nothing may be scored against it.)
  is no exact-match route to a level on this window** (§38).
* The clamp *is* exact over the interior of a **small** harvest, at two seeds,
  and that is what `ci/t2-regiondiff.sh` relies on. It is exact nowhere once
  1,089 chunks are generated at once (§39).

#### The two columns

Same window, same instrument, same tree, same afternoon: seed 12345, r=16,
centre chunk (64,64), 1,089 chunks, 107,053,056 blocks, `GEN_ORDER=rows`,
dump order `zx` read off the references, vanilla first, `ci/order-level-retake.sh`.
The admissible instrument charges us at a position only when our block matches
**neither** of the references there, so nothing is masked and the denominator is
the whole window.

```text
  N=5          our charge                    innocent vanilla floor         sep
  UNCLAMPED    1,330..1,884 (median 1,342)   2,577..4,835 (median 3,387)   -2,045
  CLAMPED      1,917..2,333 (median 1,988)   2,006..3,265 (median 2,643)     -655
```

Each range is over the six ways of choosing which five of the six references to
use; `sep` is median(charge) − median(floor).

**Both columns are negative, and that is the finding**: our residual on this
window now sits *below* what an innocent unmodified vanilla run is charged by
the same instrument, in both arms. Rule 6's disqualifier fires on both, so
**neither column yields a quotable exactness** — but it fires in the good
direction, because what the residual has fallen below is the noise rather than
a threshold.

**Read the two rows carefully, because the obvious reading is backwards.** Our
charge is *higher* against the clamped ensemble (1,988) than against the
unclamped one (1,342), and that is not the generator doing worse. The
admissible instrument charges us only where our block matches **neither**
reference, so a reference set that disagrees with itself more is a set that
forgives us more: the unclamped ensemble's admissible sets are wider, and some
of what they admit is vanilla's scheduler rather than vanilla. Clamping
narrows them, which is why the *floor* falls too — 3,387 to 2,643 — and the
clamped column is the harder test of the two. **We are below floor in the
harder one.**

Note also what these numbers are not: `docs/PARITY.md`'s standing r=16
headline of 69,288 predates the work that has landed on this window since, and
neither column above is comparable to it. Both were taken on the same tree on
2026-08-17; the 69,288 was not. Re-basing the published headline is part of the
choice below, not separate from it.

#### The options, and what each one costs

**A — keep publishing the unclamped figure.** Nothing to re-take, and every
historical number in this file stays comparable to the current one. The cost is
that the floor beside it is inflated by vanilla's scheduler by roughly the 45%
§38 measured, so the figure understates how well the generator is doing and
will keep doing so.

**B — publish the clamped figure.** The floor is the tightest honest one
available and the reference is closer to being reproducible. The cost is that
every earlier figure in this file was taken the other way, so a reader comparing
across dates is comparing across instruments unless every one of them is
re-taken; and a clamped *wide* reference is still not reproducible from its
stamp (§39), so the gain is resolving power, not repeatability.

**C — publish both, as a pair.** Honest and complete, and it is what this
section does today. The cost is that two numbers for one window is exactly the
shape a reader rounds down to whichever one they prefer, which is the failure
this whole file exists to prevent.

**No option makes the r=16 overworld window quotable.** That was the hope §38
was opened to test and it did not survive; the choice above is about which floor
travels beside an unquotable residual, not about unlocking a headline.

#### One thing here is not a judgement call

**For attribution — deciding whether a block pair is our defect or vanilla's
noise — the clamped ensemble is strictly better and should be used regardless of
what this file publishes.** The per-pair floors fall with the aggregate, and
that is what separates an item from noise. On the clamped ensemble at N=6,
`sulfur -> cinnabar` is charged 350 against a per-pair floor of 9 — an item,
unambiguously — while `diorite -> andesite` is charged 87 against a floor of 428
and is nothing at all. Read those two rows against an unclamped floor and the
first is far less clear and the second is still noise. Attribution is where the
45% is actually worth something.

### The nether at r=16, which is corroboration only

Measured 2026-08-06 — the first measurement of the nether at this window.
Same seed, centre and radius; `--ybox 0 127` is mandatory here.

| comparison | blocks | mismatches | exactness |
|---|---|---|---|
| vanilla A vs vanilla B — the noise floor, unmasked | 35,684,352 | **317,725** | 99.109624% |
| vanilla A vs ours, twin-masked with B | 35,366,627 | 254,799 | 99.279550% |

**Sharpened, 2026-08-06, and still corroboration only.** Against six
unmodified-vanilla references the residual is **151,434** rather than 254,799 —
40.6% of the published figure was positions a third vanilla run disagrees
about. It has **not** converged; the sixth reference still moved it by 4.7%, so
151,434 is an upper bound that will keep falling. The floor over the same six
is 668,532, so the window does not become quotable and **no N makes it so**:
the floor is a union and grows with every reference added. See
`docs/DETERMINISM.md` §4.

**The floor is larger than our entire disagreement with vanilla.** Rule 6
names exactly this shape for the overworld's centre-(64,64) r=7 window —
*"larger than our whole disagreement with vanilla there, so that window is
corroboration only and never a headline"* — and the nether at r=16 is the
same shape at 1.25x. The mask drops 317,725 positions, 0.89% of the window,
and a genuine defect of ours on any of them is invisible.

Why the floor is that large is already in §5: `basalt_deltas` decoration is
enormously order-sensitive, and a window this wide holds a great deal of it
where the r=7 windows are 100% soul sand valley with floors of 48 and 20.

Ranked, by the same positional attribution:

```text
     47456   18.6%  bastion_remnant (55,63)   — 105 pieces, not implemented
     22020    8.6%  bastion_remnant (70,49)   —  89 pieces, not implemented
    185323   72.7%  everything else
```

The 185,323 is led by `basalt <-> blackstone <-> netherrack` in both
directions. It has **not** been decomposed by chunk-visitation order at this
window, so no conclusion is drawn from it; that run is the named next item.
There is no fortress start in the window at all.

> **BOTH BASTION ROWS ARE CLOSED, 2026-08-19, branch `bastion-nether`** —
> `docs/PARITY-ROADMAP.md` §98. The two starts are placed and charge **27
> blocks** between them at margin 0 against vanilla's own saved boxes.
>
> **The 47,456 and 22,020 above are NOT re-taken and must not be subtracted
> from the 254,799.** They were measured on a different tree and, per §96.5,
> against a different window from the 46,856/21,591 pair that circulated for
> the same two structures; §98 measures its own window end to end and states
> its own before figure, 70,953 in `soul_sand_valley`, rather than adjusting
> anybody else's. This note is a pointer, not an arithmetic.
>
> The row's own framing is what is now wrong rather than its numbers: "not
> implemented" was true of a decoration-stage hook, not of the bastion, and
> the whole port turned out to be one processor predicate and one threaded
> `BlockPos`.

**Read this before any number under it.** Every reference the table below was
originally taken against — `van-end-20260722-r16-a`, `-b`, `det-ens-end-c`,
`-d`, `-e`, `-f` — was destroyed on 2026-08-07 and none of them was ever
re-harvested, because a dimension reading 100.000000% never looked like it
needed anything. A search across every `oracle-work` on the box, ~150 lane
checkouts, finds none of the six under that name or any other. So from
2026-08-07 to 2026-08-18 **the strongest figure in this project could not be
reproduced**, which is not a claim that it was wrong; it is a claim that nobody
could have found out. `docs/DETERMINISM.md` §51 has the search and the
fourteen order-stamped replacements harvested to close it.

### The End per biome, and the references that were not there

**Read this before any number under it.** Every reference the table below was
originally taken against — `van-end-20260722-r16-a`, `-b`, `det-ens-end-c`,
`-d`, `-e`, `-f` — was destroyed on 2026-08-07 and none of them was ever
re-harvested, because a dimension reading 100.000000% never looked like it
needed anything. A search across every `oracle-work` on the box, ~150 lane
checkouts, finds none of the six under that name or any other. So from
2026-08-07 to 2026-08-18 **the strongest figure in this project could not be
reproduced**, which is not a claim that it was wrong; it is a claim that nobody
could have found out. `docs/DETERMINISM.md` §51 has the search and the
fourteen order-stamped replacements harvested to close it.

**Retaken per biome, 2026-08-18**, on those replacements — `ci/end-biome-window.sh`,
vanilla first, twin-masked, `--ybox 0 255`, `dump_order=zx` read off each
reference's own `HARVEST-ORDER` and never asserted:

| biome | window | blocks | residual | floor (N) | exactness |
|---|---|---|---|---|---|
| `the_end` | c(0,0) r=7 | 14,745,600 | **0** | 0 (N=3) | **100.000000%** |
| `end_midlands` | c(64,64) r=16 | 25,821,184 | **0** | 4 (N=6) | **100.000000%** |
| `small_end_islands` | c(64,64) r=16 | 19,464,192 | **0** | 0 (N=6) | **100.000000%** |
| `end_highlands` | c(64,64) r=16 | 18,415,616 | **0** | 123 (N=6) | **100.000000%** |
| `end_barrens` | c(64,64) r=16 | 7,667,712 | **0** | 0 (N=6) | **100.000000%** |
| `end_highlands` | c(83,62) r=7 | 7,667,712 | **0** | 0 (N=3) | **100.000000%** |
| `end_midlands` | c(83,62) r=7 | 5,636,096 | **0** | 0 (N=3) | **100.000000%** |

**All five biomes, and `the_end` had never been measured at all.** The r=16
window's centre is block (1,024, 1,024), 1,448 from the origin, and
`TheEndBiomeSource` returns `the_end` only inside 1,000 — so the island with
the ten obsidian spikes, the exit portal and the arrival platform was outside
every window in this file. It is 100.000000% too, over 14,745,600 blocks with a
floor of 0 across three references, and the window is not a void: 1,257,395
`end_stone`, 40,524 `obsidian`, 146 `iron_bars`, 10 `bedrock` and 10 `fire`,
every count matched exactly.

**The aggregate at r=16 reproduces exactly and the floor under it does not.**

| comparison | blocks | mismatches | exactness |
|---|---|---|---|
| **ours vs `gen-wide-20260722-r16-end-a`, UNMASKED** | 71,368,704 | **0** | **100.000000%** |
| ours vs the fresh six-reference stable set | 71,368,704 | **0** | **100.000000%** |
| the floor across those six, all harvested in one order | 71,368,704 | **127** | — |
| ours vs a `rows_rev` reference — the order control | 71,368,704 | 944 | 99.998677% |
| vanilla `rows` vs vanilla `rows_rev`, the same control | 71,368,704 | 944 | 99.998677% |
| ~~vanilla A vs ours, twin-masked against B~~ | 71,368,704 | ~~125~~ | ~~99.999825%~~ ⚠ superseded, and the reference is destroyed |
| ~~vanilla A vs ours, older untwinned reference~~ | 71,368,704 | ~~81~~ | ~~99.999887%~~ ⚠ no twin, and the reference is destroyed |
| ~~the floor across the original six~~ | 71,368,704 | ~~252~~ | ⚠ ensemble destroyed |

Our dump is byte-identical to an ordinary unmodified-vanilla harvest of this
seed and window — zero disagreeing blocks, no mask, no order decomposition —
and the twin mask drops **0** positions because there is nothing for it to
drop. That part of the old claim survived its own evidence being destroyed,
which is the best outcome that was available.

The floor did not. 252 was published across six references of unrecorded
harvest order; six references all harvested in one order give **127**, and the
pairwise shape against the primary is 0, 127, 46, 46, 0 rather than the old 0,
46, 81, 125, 171, 206. The two are not two measurements of one quantity — one
is a mixed-order floor and the other a matched-order one — and neither is
subtracted from anything.

**The two 944 rows are the same control read twice, and they are what makes the
zero mean something.** Both are vanilla against vanilla with the knob turned —
`GEN_ORDER=rows` against `GEN_ORDER=rows_rev`, default pool — so neither is a
figure about this port at all. Our dump differs from a reverse-order vanilla by exactly
what a forward-order vanilla differs from it by, to the block, because our dump
*is* a forward-order vanilla. A figure taken against a reference whose order
was not read off its stamp is not a worse figure, it is a measurement of the
knob.

**Where the floor lives, and it is one biome.** `end_highlands` is 25.80% of
the r=16 window and carries **123 of the 127**; `end_midlands` carries 4;
`small_end_islands` and `end_barrens` carry none. Every position is
`chorus_plant` or `chorus_flower` against `air`, in five chunks, in y58..80.
`chorus_plant` is placed in `end_highlands` alone and grows upward across chunk
boundaries off a `MOTION_BLOCKING` heightmap, so it reads a neighbour that may
not have decorated yet — the End's `basalt_deltas`. §49 of
`docs/DETERMINISM.md` has it.

**What the residual is made of: nothing, and that is the finding.** There is no
residual to attribute in any of the five biomes. What is left is a coverage
statement instead, and it is not a comfortable one. The r=16 window returns
`starts=0 pieces=0` and its entire census is `air`, `end_stone`,
`chorus_plant`, `chorus_flower` — the widest End window this project has
exercises two block families. `end_gateway_return`, the one unported End
feature, is in **no** measured window and could not have been: its placement is
`rarity_filter chance=700` filtered to `end_highlands`, and over all three
windows — 1,539 chunks — the expected number of return gateways is **0.57**
against an observed 0. The port could place them perfectly or not at all and
every number above would read the same. §50 has the arithmetic and the same
test applied to the four features that *are* ported, all of which it passes;
`end_platform` passes it outright rather than statistically, because
`fixed_placement` puts it at exactly (100, 49, 0) and that is inside the c(0,0)
window and no other.

The old reading of the pair's zero — *"the floor is exactly zero, so there is
nowhere for a defect to hide"* — was wrong and is corrected in §2. What is
true is the much better statement the retaken rows make: there is no position
on which vanilla is consistent and we are wrong, in any of the five biomes.

**Three seeds, three fresh references each, ours unmasked against every one —
⚠ HISTORY, every reference below is destroyed:**

```text
  seed 31415926     a  33      b   0      c   0     vanilla a:b 33, a:c 33, b:c 0
  seed 12345        a   0      b   0      c 287     vanilla a:b  0, a:c 287, b:c 287
  seed 20260722     a 125      b 125      c 171   d 0   e 46   f 206
```

On every seed we were byte-identical to at least one unmodified vanilla run,
and always to the **majority** answer rather than the odd one out. **Only the
20260722 row has been re-taken** (§51), and it re-takes as 0 against the
primary rather than 125 — because the fresh ensemble is same-order throughout
and the old one demonstrably was not. The other two seeds are **not
reproducible today**: no End reference exists for 31415926 or 12345 at any
window, so those two rows are kept as history and may not be quoted as current
evidence. Re-harvesting them is a named next item and costs about a minute per
reference.

The 81 and the 125 came from a period when the method changed mid-window: the
older reference `van-end-20260722-r16` had no twin, so the 81 was unmasked, and
the two references differed from each other by 206 blocks, which is what a
differently ordered harvest costs. Both are struck above rather than deleted,
because both were published; neither can be recomputed, and the replacement for
both is the unmasked 0 against a reference whose order is stamped on it.

One thing the noise-floor row settles, and note carefully that it is *not*
"permanently": **over 1,089 chunks vanilla disagrees with itself by tens of
thousands of blocks — 35,531 on the destroyed pair, 29,560 on the regenerated
one — so any wide-window residual published without a twin mask is mostly
reporting that.** The qualitative point is robust and the number is not. This
sentence read "settles permanently … by 35,531 blocks" until 2026-08-09, when
regenerating the references moved it by 5,971 and made a claim written as
permanent simply wrong. A floor belongs to the pair of runs that produced it,
and that applies to sentences about floors as much as to the floors.

Two further facts below are about the **pre-port** 396,762 and are kept as
history, because both were used to argue that the residual was structures
rather than a defect, and that argument turned out to be right:

* The pre-port residual was **not diffuse**. The inner r=7 square carved out of
  the same wide generation was 5,545 mismatches over 225 chunks (99.974913%
  — ✅ **the missing denominator is recovered, 2026-08-07**. The obvious one
  does not reproduce it: 5,545 over 225 full-height chunks is 22,118,400
  blocks and 99.974930%. The masked one does, exactly. Over chunks 57..71 the
  two references disagree at **15,060** positions, and 225 x 98,304 - 15,060
  = **22,103,340**; 5,545 of that is 99.974913% to the last digit. So the
  percentage was correctly derived from a twin-masked denominator that was
  simply never written down. Two things follow and the second is the useful
  one: nothing about the published figure was wrong, and **that sub-window's
  own floor is 15,060 against a residual of 5,545**, so it was disqualified by
  rule 6 the day it was published, and still is. The count reproduces today,
  which is why the denominator could be recovered at all: none of the merges
  since touched that inner box, so its residual is still exactly 5,545. Quote
  the count),
  so 98.6% of the total lay in the outer ring — 24.6 mismatches per chunk
  inside against 452 outside, concentrated where the unimplemented structures
  were. (5,545 is not comparable to §2's 1,079 for the same chunks: that
  reference was harvested in a different order, and §7 says why that is a
  different world by a few thousand blocks.)
* It was **order-invariant**. `ci/decorate-order-intersect.sh` over the same
  window, nine names and eight orders, ranged 385,420 to 547,193 and
  intersected to a stable **core of 376,575** — only ~20k was order-dependent,
  and the survivors were the chamber palette led by `tuff_bricks -> deepslate`
  at 82,522. That is what a missing structure looks like and what a
  decoration-order defect does not.

**Neither has been re-run against the 43,041.** The post-port residual has no
order decomposition yet, and until it does, nothing in it may be called a
defect — the standing rule is to decompose before concluding, and §5 records
what ignoring it once cost. That run is the named next item.

So the r=7 zeros still do not generalise to r=16, but the remaining reason is
much smaller than it was: sixteen structure sets are still unimplemented, and
the terrain, carvers and ores hold up over five times the area.

---

## 7. How a stranger reproduces it

Nothing here runs on a laptop; all of it goes on the build machine.

```bash
# 1. Fetch and unpack the unmodified server jar.
ci/vanilla-server.sh fetch

# 2. Harvest a vanilla reference, and a second one for the twin mask.
#    (Never overwrite an existing reference — generate a new directory.)
ci/gen-vanilla-region.sh 20260722 7 oracle-work/my-r7
ci/gen-vanilla-region.sh 20260722 7 oracle-work/my-r7-b

# 3. Generate ours over the same square.
cargo build --release -p worldgen-dump -p region-differ
RUSTMC_WORKERS=1 ./target/release/worldgen-dump full 20260722 7 /tmp/mine 64 64

# 4. Diff — VANILLA FIRST, always.
./target/release/region-differ diff oracle-work/my-r7 /tmp/mine/region \
    --bbox 57 57 71 71 --twin oracle-work/my-r7-b

# 5. Decompose by chunk-visitation order to get the core.
ISECT_SEED=20260722 ISECT_RADIUS=7 ISECT_CENTRE="64 64" \
ISECT_REF=oracle-work/my-r7 ISECT_TWIN=oracle-work/my-r7-b \
ISECT_BBOX="57 57 71 71" ISECT_OUT=/tmp/isect \
    ci/decorate-order-intersect.sh
```

For the nether add `ISECT_DIM=nether` and `ISECT_YBOX="0 127"` — the `ybox` is
mandatory there, because the dimension stores 256 blocks and the generator
fills 128. For the End, `ISECT_DIM=end`.

**Expect your numbers to differ from the table.** A reference harvested in a
different order is a different world by a few thousand blocks, which is the
whole reason §3 and §4 exist. What should reproduce is the *core*, and the
families in the residual.

---

## 8. The one-sentence version, for when only one sentence fits

> On two seeds and two 15x15-chunk windows, rustmc's overworld generation
> disagrees with unmodified vanilla 26.2 in no block that survives being
> regenerated under eight different chunk-visitation orders; the End is exact
> in three windows and **byte-identical over 1,089 chunks to an unmodified
> vanilla run of the same seed**; and the nether's soul sand valley is 99.885%
> and its basalt deltas 99.916% on the order-invariant measure; and over a
> 33x33-chunk overworld window at one seed, 1,089 chunks and 107,053,056 blocks
> with **nothing masked** and the chunk order held fixed on both sides, rustmc
> writes 2,450 blocks that match none of eight independently generated
> unmodified-vanilla runs, which is inside the 2,202..5,263 the same instrument
> charges a run that IS unmodified vanilla — so on this window nothing separates
> this server from vanilla, and that is stated as a comparison rather than as a
> percentage — with sixteen of the twenty structure sets still unbuilt and no
> block entities written. (The clause used to end *"a lower bound of
> **99.972345%**, against a floor of 29,560 blocks by which those same two runs
> disagree with each other"*. That figure was taken with an uncontrolled chunk
> order and is no longer quotable; §15, §16.)

The clause about the 33x33 window was removed on 2026-08-07 and put back the
same day, and both halves are kept because the difference between them is the
lesson. It read *"has run out of resolving power — its residual is now smaller
than vanilla's disagreement with itself there — so it carries no quotable
exactness figure and this sentence no longer contains one"*. That was true of
the **twin-masked** instrument and it is still true of it. What restored the
sentence was not a better window or a quieter reference: it was refusing to
mask. `docs/DETERMINISM.md` §12.

The clause *before* that one read *"over a 33x33-chunk window on three seeds it
is 99.96%, 99.93% and 99.96%, with sixteen of the twenty structure sets still
unbuilt and no block entities written"*. Removed 2026-08-07: the first and
third of those are below their own floors and the second has not been
re-measured since. It is recorded rather than deleted because it was published.

🚫 **WITHDRAWN 2026-08-13 — uncontrolled chunk order. THE PARAGRAPH BELOW
CARRIED A ✅ FOR THREE DAYS AFTER THE FIGURE IN IT WAS WITHDRAWN.** It is kept
rather than deleted, because a number is retired by being labelled and not by
being overwritten — but the ✅ is gone, and that is the correction. §15 and §16
withdrew this figure on 2026-08-10 and the paragraph twenty lines above already
says so; a reader who landed here instead found an endorsement, and the two
sentences did not reach each other. **No number fits.** This window carries no
exactness figure at all: the successor taken with the dump order matched to the
references' own `HARVEST-ORDER` is 2,450, which sits inside the 2,202..5,263 the
same instrument charges a run that IS unmodified vanilla, so it is corroboration
and not a level either. `ci/figures.toml:ow-r16-admissible-n2`, and
`ci/check-figures.py` rule 13 now refuses a withdrawn figure printed on a ✅
line — rule 4 accepted this one, because a disqualifier anywhere within 4,000
characters satisfied it and proximity is not attachment.

**If only one *number* fits, it is 99.972345%, and every word of its window
has to travel with it: 33x33 chunks, seed 20260722, centre (64,64), 1,089
chunks, 107,053,056 blocks, full height, two unmodified references, NO MASK,
and** ~~a floor of 29,560 that the residual clears by 45 blocks.~~ Rewritten
twice on 2026-08-07, and re-taken on a regenerated reference pair on 2026-08-09;
all three are kept.

> **THE STRUCK CLAUSE IS THE SAME SUBTRACTION §1 ALREADY STRIKES, AND IT SURVIVED
> HERE UNTIL 2026-08-18.** 29,605 is a charge under an instrument that excludes
> nothing and 29,560 is the *width of the twin mask that instrument does not
> use*, so their difference is not a quantity and 45 blocks of headroom is not a
> reading. `ci/figures.toml` says it in the entry's own `floor_basis`:
>
> > twin-mask width `|A != B|` on the masked window (denominator 107,023,496),
> > NOT an innocent-candidate floor for this instrument. The comparable floor
> > for the no-mask instrument is 16,932 at N=2 (`docs/DETERMINISM.md` §14),
> > against which 29,605 does NOT clear.
>
> That is why `ow-r16-admissible-n2` carries `floor_comparable = false`, which
> makes rule 4 **skip** its arithmetic rather than pass it — a comparison nobody
> should draw is not one to declare green. The paragraph was rewritten twice for
> its digits and its argument and kept this clause both times, which is the
> case for a checker reading documents rather than a reader re-reading them.

This paragraph used to say "it is **99.959781%**", and that is still wrong on
two counts: it describes a world two merges ago, and its successor on `main`,
29,927 blocks over 107,017,525, sat **below the 35,531 floor of its own
window**, which is rule 6's disqualifier and makes 99.972035% unquotable as an
exactness claim. The N=6 route is stale in the same way: 37,471 became 24,402
under `village-savanna` and has not been re-taken since `df30334`. **No
arithmetic recovers a publishable figure from those three numbers**, and the
second rewrite of this paragraph said so and stopped there. All four of those
numbers belong to the destroyed reference pair; on the regenerated pair the
masked residual is 29,490 against a floor of 29,560 and the verdict is the
same, which is the only reason this paragraph did not have to be rewritten a
third time on its argument as well as its digits.

What it missed is that the twin mask was throwing away the evidence that
settles it. At a masked position the two references give two different answers
and we wrote something; whether we wrote one of *theirs* is a fact the mask
discards. Counted instead of discarded on the regenerated pair at `main`
8e283f57: we match one of the two at 29,445 of the 29,560, and the 115 left
over join the 29,490 to make 29,605 blocks that match no vanilla run of this
seed. Same window, same two unmodified references, nothing excluded. On the
destroyed pair the same arithmetic ran 35,179 of 35,531 matched, leaving
352 + 29,927 = 30,279. It is the identity that matters — the charges inside
the mask plus the charges outside it are the admissible total — and it holds
on both pairs. `docs/DETERMINISM.md` §12.

The rest of what can be said about the overworld, all of it measured:

* the order-invariant core is **0** on both r=7 windows (seeds 20260722 and
  31415926) — no block survives regeneration under eight distinct
  chunk-visitation orders;
* biomes are exact in every window measured — `biomes=N/0`;
* the r=16 window's *deltas* are sound even though its absolute value is not,
  and they run in one direction: 396,762 → 43,041 → 29,964 → 29,927 — all four
  on the destroyed reference pair, which is what makes them a legitimate chain;
  `main` 8e283f57's 29,490 is on the regenerated pair and is **not** the fifth
  term of it.

And the strongest sentence in this file is still not a percentage: **the End is
byte-identical to an unmodified vanilla run over 1,089 chunks, and all five of
its biomes read 100.000000%** (0 mismatches, unmasked — §2 and
`docs/DETERMINISM.md` §§51-50). It carries fewer qualifiers than the overworld
figure, which needs its whole window, but it does not carry none, and the two
it carries were both discovered on 2026-08-18:

* **One seed, not three.** The three-seed claim stood on End references for
  12345 and 31415926 that no longer exist — like every other End reference, they
  went in the 2026-08-07 disk failure and were never re-harvested. Only
  20260722 has been re-taken. The other two rows are history until someone
  spends the two minutes it costs to harvest them.
* **The 1,089-chunk window holds no structure and no spike.** `starts=0
  pieces=0`, and its whole census is `air`, `end_stone`, `chorus_plant`,
  `chorus_flower`. What makes the sentence a statement about the dimension
  rather than about chorus plants is the two r=7 windows beside it — c(0,0),
  which is 100% `the_end` and holds the ten spikes and the arrival platform,
  and c(83,62), which holds a whole end city. `the_end` had never been measured
  before that day, in a dimension this file had published 100.000000% for since
  2026-08-06.
**The nether's r=16 figure is not a headline at all**: vanilla disagrees with
itself there by more than we disagree with vanilla, and no number of references
fixes that — see `docs/DETERMINISM.md` §4.

**And if only one *sentence about a defect* fits, it is not a number at all:**
this port does not place the two `village_savanna` starts in the overworld
window, and a missing village is the one thing in the residual a player would
see. It is 0.013% of the figure. `docs/DETERMINISM.md` §7 has the pictures.

Every qualifier in that sentence is load-bearing. Do not drop one.
