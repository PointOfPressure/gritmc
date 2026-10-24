# The r=16 window, all three dimensions, measured 2026-08-06

> 🚫 **AUDITED 2026-08-07 — THE OVERWORLD HALF OF THIS FILE IS HISTORY.**
> Every overworld figure here (99.959781%, 43,041, 99.629255%, and the three
> seeds' after-column) was measured before `village-savanna` and before
> `df30334`. On `main` the 20260722 residual was **29,927**, which is smaller
> than this window's own 35,531 vanilla-A-vs-vanilla-B floor — rule 6's
> disqualifier — so **no absolute overworld exactness figure from this window
> is quotable**, neither the published one nor its successor. The nether half
> was already corroboration-only and still is. The End half is superseded in
> the better direction and is closed at zero. `docs/PARITY.md`'s banner is the
> full statement; `ci/figures.toml` and `ci/check-figures.py` enforce it.
>
> **The ledger's own words for the figures on this page, so a grep for any of
> them lands here — added 2026-08-17.** `ci/figures.toml` disqualifies
> 99.959781% as **stale**, 29,490 and 29,964 as **not quotable**, and the nether
> 99.279550% as **corroboration only**. The prose above says all of this in
> substance and always did; what it did not do is use the ledger's vocabulary,
> so `ci/check-figures.py`'s echo rules could not see that the qualification was
> present. Vocabulary aligned, no verdict changed.
>
> 🚫🚫 **AND THE WHOLE FILE IS NOW HISTORY IN A SECOND, STRONGER SENSE —
> 2026-08-09.** Every overworld number below was taken against
> `gen-wide-20260722-r16-ow-a` and its `-b` twin as they existed before the
> 2026-08-07 disk failure. Neither directory survived the restore. A floor
> belongs to the pair of runs that produced it, so **nothing in this file can
> be reproduced from this tree** — not the residuals, not the 35,531 floor,
> and not the 107,017,525 masked denominator, which is that floor subtracted
> from the window. The pair was regenerated on 2026-08-09 and the window
> re-measured from scratch; on the new pair the floor is **29,560**, the
> masked residual is **29,490** and the admissible charge is **29,605 of
> 107,053,056**. Those live in `docs/PARITY.md` §6 and `ci/figures.toml`.
> **Do not compare any number in this file with any of those three.**
>
> ✅ **The window itself was not the problem and it carries a figure again —
> from a different instrument, not a different window.** Charge a position
> only when our block matches *neither* reference and mask nothing: on the
> destroyed pair that was **30,279 of 107,053,056**, and on the regenerated
> pair it is 29,605 of the same denominator. That is `docs/PARITY.md`'s first
> banner and `docs/DETERMINISM.md` §12. Every twin-masked overworld number in
> this file stays history.
>
> 🚫 **AND THE 29,605 IS NOT QUOTABLE EITHER — 2026-08-10.** It was taken with
> an **uncontrolled chunk order**: our dump walks chunks x-major and every
> reference in this file was harvested z-major (`GEN_ORDER=rows`). The same
> instrument charges an unmodified vanilla run 117,628 blocks for that
> mismatch. Re-taken with the order matched it is **2,450**, which sits inside
> the 2,202..5,263 an innocent vanilla run is charged, so the window carries no
> exactness figure at all now — only corroboration and deltas.
> `docs/DETERMINISM.md` §15 and §16, ORACLE rule 29. **Every reading in this
> file was taken x-major and none of them states it**, which is the defect this
> banner exists to name: a condition that is not recorded is not controlled.

Seed **20260722**, centre chunk **(64,64)**, **r=16 — 33x33 = 1,089 chunks**,
`GEN_ORDER=rows` on the vanilla side, `RUSTMC_WORKERS=1` on ours, **vanilla
first** in every call, twin-masked against a second independently generated
vanilla world of the same seed. Threadripper 1920X, 24 threads, load average
3.5–7.0 throughout (other agents building).

This file exists because `docs/PARITY.md` §6 carried an r=16 number for the
overworld only. **The nether had never been measured at this window** and the
End's r=16 number had no twin. Both are now measured, and one of the two
answers changes how the number may be quoted.

---

## The three numbers, each with the floor it sits on

The **floor** is vanilla against *itself*: two independently generated worlds
of the same seed, same extent, unmasked. It is the honest limit of what the
window can resolve.

| dimension | blocks compared | floor (vanilla A vs B) | our residual, twin-masked | exactness |
|---|---|---|---|---|
| overworld | 107,017,525 | 35,531 | **43,041** | **99.959781%** |
| nether (y 0..127) | 35,366,627 | **317,725** | 254,799 | **99.279550%** |
| the End | 71,368,704 | **0** | 125 | **99.999825%** |

> **Two of these three rows were superseded on 2026-08-06 and the End row is
> wrong in both columns.** Against six independently generated
> unmodified-vanilla references instead of two:
>
> | dimension | masked out at N=6 | denominator | residual at N=6 | exactness |
> |---|---|---|---|---|
> | overworld | 73,820 | 106,979,236 | 37,471 | 99.964974% |
> | nether | 668,532 | 35,015,820 | 151,434 | 99.567527% |
> | the End | 252 | 71,368,452 | **0** | **100.000000%** |
>
> The denominator is the window minus the positions masked out, which is why
> it moves; **the block count is the honest figure and the percentage is the
> flattering one.**
>
> The End is byte-identical to one of the references, on three seeds — **and
> only the 20260722 seed is reproducible today; see the warning further down,
> the other two seeds' references are gone.** The
> floor does **not** shrink with more references — it is the size of a set and
> only grows — so the floor/residual ratio was never a measure of resolving
> power. `docs/DETERMINISM.md` §4 and §6. The table above is kept because it
> was published.

The overworld row is **after** the trial chamber port landed in this same
branch. It measured 396,762 / 99.629255% when this file was first written a few
hours earlier, which reproduced the 2026-08-03 figure exactly; the section
below carries both numbers and the before/after on three seeds.

Biomes are exact in all three: `biomes=N/0` — 1,672,704, 557,568 and
1,115,136 cells respectively, zero wrong.

**Every one of these three carries a different caveat, and none of them may be
quoted without it.** They are below, one per dimension.

---

## Overworld — 99.959781%, and the caveat that now travels with it

**The caveat is no longer "trial chambers are missing". It is that the window
is running out of resolving power, and that a chamber's block entities are not
written.** Both are below.

### What it was, and what closed it

The starting figure reproduced the 2026-08-03 number to the digit from a fresh
generation on a fresh tree: **396,762 mismatches of 107,017,525, 99.629255%**.

Attributing every mismatch to the union bounding box of a structure vanilla
saved into its own chunks — 16-block margin, read out of `worldgen-dump
jigsaw-starts` on the reference — put **91.8% of it on three trial chambers**:

```text
    133419   33.6%  trial_chambers (53,74)
    121850   30.7%  trial_chambers (75,55)
    109061   27.5%  trial_chambers (74,78)
     17725    4.5%  outside every structure box  (terrain, carvers, ores)
     11089    2.8%  village_savanna (81,77)
      2654    0.7%  village_savanna (46,77)
       855    0.2%  ruined_portal (48,64)
        85    0.0%  shipwreck (57,65)
        24    0.0%  ocean_ruin_warm (82,68)
         0    0.0%  shipwreck (81,57)
    396762   total
                    -----
    364330   91.8%  the three chambers together
```

That is what the trial chamber port then closed:

```text
  before   396,762 / 107,017,525   99.629255%
  after     43,041 / 107,017,525   99.959781%    -353,721   -89.2%
```

Denominator, twin mask and biome count are identical across the pair —
107,017,525 blocks, 35,531 masked positions, `biomes=1,672,704/0` — so the
number moved because the world moved, not because the measurement did.

**And it is three seeds, not one.** Both arms generated from the same two
commits, the vanilla side untouched:

| seed | floor (A vs B) | before | after | change | after |
|---|---|---|---|---|---|
| 20260722 | 35,531 | 396,762 | **43,041** | -353,721 (-89.2%) | 99.959781% |
| 12345 | 12,774 | 164,143 | **77,067** | -87,076 (-53.0%) | 99.928002% |
| 31415926 | 49,570 | 64,756 | **39,143** | -25,613 (-39.6%) | 99.963419% |

The percentages differ because the denominator does — whatever *else* is wrong
in a window — which is the same effect `PARITY.md` §5 documents for the nether.
Every seed improves and none is made worse.

⚠ **All three after-columns are stale and none has been re-measured since
`village-savanna` and `df30334`.** Seed 20260722's successor, 29,927, is below
its own 35,531 floor; seed 31415926's 39,143 was already below its 49,570 floor
when it was taken. **Seed 12345 is the only one of the three that ever had
resolving power** — floor 12,774 against a residual of 77,067 — and it too is
`not re-measured` since either merge, so it cannot carry a headline either.

⚠⚠ **And all three floors belong to reference pairs that no longer exist —
2026-08-09.** Only seed 20260722's pair has been regenerated, where the floor
is 29,560 rather than 35,531 and the masked residual is 29,490. Seed
31415926 has no surviving r=16 references at all, so its floor of 49,570
cannot be re-derived and neither can the residual beside it.
(29,490 is `ow-r16-n2-main`, **not quotable**.)
Seed 12345's "only one that ever had resolving power" therefore could not be
cashed in without generating a fresh ensemble for it — which was a real item,
not a formality, and it was the one place in this table where a re-measurement
could still change a conclusion rather than only its digits.

✅✅ **SEED 12345 HAS BEEN CASHED IN, AND IT PAID — 2026-08-10.** Eight fresh
references (`ci/refs-12345-ensemble.sh`, ~57 s each, six `rows` and two
`rows_rev`, every one stamped with its `HARVEST-ORDER`), the dump order matched
to those stamps from the first measurement, nothing masked:

```text
  admissible charge, N=5, harvest rows, dump zx      7,940 of 107,053,056   99.992583%
  the same charge before minecraft:iceberg was ported   69,288              99.935277%
  an INNOCENT vanilla run over the same ensemble    2,577..4,835  median 3,387
```

**It is the only r=16 overworld window in this project that carries a quotable
exactness figure.** The residual it was standing on was one nameable thing —
this port wrote **no `blue_ice` at all**, where six unmodified vanilla runs of
this seed write 17,618..17,743 — and `minecraft:iceberg` was ported on
2026-08-10, taking the charge down by 61,348 blocks against an **unchanged**
floor. `docs/DETERMINISM.md` §17 and §17.1;
`ci/figures.toml:ow-r16-12345-admissible-iceberg`.

**But it is now 1.6x its floor, not fourteen times it.** The floor did not
move, so all of that ratio was spent by the repair. Quote the margin — 3,105
blocks of clearance — beside the percentage, or the number reads as a stronger
instrument when it is a weaker one.

**The old row above is not superseded arithmetic, it is a different
measurement** — different references, a twin mask, a smaller denominator and an
uncontrolled dump order — so its residual is not subtracted from the new one and
neither floor is comparable to the other. It stays exactly as it was printed.

### The caveat that must travel with 99.959781%

**Seed 31415926's r=16 window is corroboration only.** Its floor, 49,570, is
larger than its residual, 39,143. By rule 6 that window cannot carry a
headline, and its -39.6% is quoted here as support for the other two rather
than on its own.

**And the 20260722 window is close to the same limit.** Its floor is 35,531
against a residual of 43,041 — 83%. The twin mask removes those positions from
the denominator, so the 43,041 sits on ground both vanilla runs agree about and
every one of them counts; but the *hidden* capacity is now the same order as
the measured residual, and §4's weakening is therefore no longer a small
correction. **The next real information about the overworld comes from a
sharper instrument, not from this window.**

**It did not stay close to the limit; it went past it — 2026-08-09.** On the
regenerated pair the floor is 29,560 and the masked residual 29,490, so the
floor now exceeds the residual outright rather than sitting at 83% of it, and
even the unmasked admissible charge of 29,605 clears the floor by only 45
blocks. The prediction in the paragraph above was correct and is now simply
the state of the window.

> ⚠ **Two corrections to the sentence above, both later than it.** The "clears
> the floor by only 45 blocks" arithmetic is a category error — it subtracts
> the width of a twin mask from a charge that uses no mask — and
> `docs/DETERMINISM.md` §14 killed it on 2026-08-09. And on 2026-08-10 the
> charge itself was found to have been taken under an **uncontrolled chunk
> order**. The state of the window is still that it is spent; both of the
> reasons given here for saying so were wrong.

**A chamber's block entities are not written.** Its trial spawners, vaults,
chests and decorated pots are placed as bare blocks with no NBT — no spawner
contents, no vault contents, no loot, no sherds. Block states match; the
things inside them do not exist. `worldgen-dump`'s caveat block says so.

### What is left, attributed the same way — and it is still 61% structures

Reading the top block families would have said "ore blobs and terrain":

```text
   3057  deepslate -> air          2340  stone    -> air
   1771  diorite   -> andesite     1742  diorite  -> granite
   1507  andesite  -> granite
```

**That reading is wrong, and it is the same mistake the retired caveat made.**
Attributing all 43,041 by position gives:

```text
    16795   39.0%  outside every structure box (terrain, carvers, ores)
    11051   25.7%  village_savanna (81,77)
     7950   18.5%  trial_chambers  (53,74)   <- over-attributed, see below
     2608    6.1%  village_savanna (46,77)
     2413    5.6%  trial_chambers  (74,78)
     1260    2.9%  trial_chambers  (75,55)
      855    2.0%  ruined_portal   (48,64)
       85    0.2%  shipwreck       (57,65)
       24    0.1%  ocean_ruin_warm (82,68)
```

Two things follow, and neither is comfortable:

* ~~**The chambers are not finished.** 11,623 blocks — 27.0% of the
  residual — are still inside a chamber's own box.~~ **RETRACTED 2026-08-06.
  The chambers write 24 wrong blocks, not 11,623.** The sentence is kept
  struck through rather than deleted because it is the *third* time this file
  has recorded an over-attribution to trial chambers, and the shape repeats:
  the first said "~73%, a single chamber", the correction said "91.8%, three
  chambers", and this one said "27.0% inside their boxes". All three charged
  the structure for terrain.

  The error this time is the **box**, and — this matters —
  **`ci/attribute-residual.py` was right; the sentence above misread it.**
  The tool adds a **16-block margin to every side of a start's union box**,
  and its own docstring says it "narrows the suspect list; it does not close
  it". 11,623 was a correctly computed *suspect-list size*. The prose turned
  it into a defect count. Re-run on the rebased tree (N=2 residual 29,964
  after the villages landed) the same method gives **11,577**, so the
  published figure reproduces and the tool is not in question (29,964 is
  `ow-r16-n2-village`, **not quotable**, and 11,577 is a suspect-list size
  rather than an exactness figure):

  ```text
    margin 16, union box   11,577   what COULD be the chambers' fault
    margin  0, union box    1,570   inside the hull of their pieces
    per-PIECE box              24   inside a block vanilla's saved
                                    Children[] actually covers
  ```

  A chamber's union hull is ~600,000 blocks of mostly untouched stone —
  start (53,74) alone spans x 796..895, y -43..24, z 1104..1195 — and a
  16-block margin on every side of that is where the other ~11,000 come
  from. **The lesson is not "the tool over-attributes"; it is that an upper
  bound and a defect count are different numbers and this file printed one
  under the other's name.**

  The 24, in full: 14 `-> tuff_bricks` (8 from `sulfur`, 4 from `cave_air`,
  2 from `water`), all in `corridor/end_2`; 6 `granite -> andesite`, which is
  an ore blob that merely happens to lie inside a piece box; and one each of
  `glow_lichen -> air`, `copper_ore -> dirt`, and two block **states** whose
  property lists are short —
  `waxed_copper_bulb[lit=true,powered=false] -> waxed_copper_bulb[lit=true]`
  and `waxed_oxidized_copper_grate[waterlogged=false] -> ...[no properties]`.

  **Corroborated by a census, which shares no mechanism with the diff.**
  Every marker a chamber cannot avoid is exact over the chamber window:
  `chiseled_tuff_bricks` 2,797 = 2,797, `trial_spawner` 13 = 13, `vault`
  18 = 18, `waxed_copper_bulb` 161/90/81 identical, `copper_grate` 602/112
  identical. `tuff_bricks` is 41,486 against 41,472 — exactly the +14 the
  positional attribution blames on `corridor/end_2`.

  **Two of the three named candidates are now cleared rather than untested.**
  The copper-bulb `random_block_match` degradation splits 90 weathered / 81
  exposed in both worlds, which is a *random* outcome reproduced exactly, so
  that processor's draws are right. Nothing this window contains makes
  `liquid_settings` or waterlogging observable, so those stay unmeasured —
  and that is now the honest statement rather than a suspicion.
* **13,659 blocks — 31.8% — are two villages this port does not place at
  all.** `village_savanna` at chunks (46,77) and (81,77) both start *outside*
  the generated square, and `village_starts` skips a village whose walk asks
  the canvas for a height it does not have. `RUSTMC_TRACE_BEARD=1` confirms it
  directly: the beard sees `starts=3 pieces=692 adaptations={"Encapsulate"}` —
  three chambers and no village. This is pre-existing and unrelated to the
  chamber work, and it is now the second-largest single item in the window.

  **CLOSED 2026-08-06, branch `village-savanna`.** The clamp had no vanilla
  counterpart — `getFirstFreeHeight` is `getBaseHeight`, one beardless noise
  column, no chunk required — so removing it is strictly more faithful and
  also removes a way in which our output depended on the size of the window
  being generated. r=16 overworld residual **37,471 -> 24,402 at N=6**; the
  two village boxes **13,309 -> 720**, 94.6% cleared; the residual a player
  can actually see, y >= 62, **11,247 -> 1,087 positions**. What is left
  inside the boxes is 302 `grass_block -> dirt` (a pre-existing family, down
  from 990), 188 blocks of unplaced `feature_pool_element`s (**still
  unplaced** — and that 188 was a block-kind count: only 99 of it was
  village elements, the rest natural `trees_savanna`. The count correction
  holds; the 2026-08-10 close does not, because the port that would have
  closed it runs on a piece stream this server leaves off), 158 of
  underground ore-blob noise and 36 state-only differences on
  `CrossCollisionBlock`s. Details in the `## NOW — worldgen features` block of
  `docs/WORKLIST.md`.

The true terrain share is **16,795 blocks in 107 million, 0.0157%**.

### The correction to the old caveat, kept because it was published

The sentence this file replaces — *"~73% of that residual is a single trial
chamber"* — was **wrong in both halves**:

* it is **three** trial chambers, not one: vanilla's own saved starts put them
  at chunks **(53,74)**, **(74,78)** and **(75,55)**, with 234, 238 and 220
  pieces;
* they are **91.8%** of the residual, not 73%.

It is worth keeping rather than quietly overwriting, because the shape of the
error is the one to expect again: a caveat written from a *sample* of the
disagreement list — one cluster, inspected — under-counted a family that had
three clusters, and the correction only appeared when every mismatch was
attributed to a structure box by position.

`ruined_portal` is the one unimplemented structure with a start left in this
window, and it is worth 855 blocks.

---

## The nether — 99.279550%, and it is CORROBORATION ONLY, never a headline

This is the first measurement of the nether at this window. It is also the
one that must not be quoted as a headline, and the reason is arithmetic
rather than judgement:

```text
  vanilla A vs vanilla B, unmasked   317,725 blocks    (the floor)
  vanilla A vs ours, twin-masked     254,799 blocks    (our residual)
```

**The floor is larger than our entire disagreement with vanilla.** AGENT-BRIEF
rule 6 already names this shape for the overworld's centre-(64,64) r=7 window
— *"larger than our whole disagreement with vanilla there, so that window is
corroboration only and never a headline"* — and the nether at r=16 is the same
shape, at 1.25x.

The mask drops 317,725 positions, 0.89% of the window. §4 of `PARITY.md`
applies at full strength: a genuine defect of ours sitting on any of those
positions is invisible here.

Why the floor is so large is already known and documented: `basalt_deltas`
decoration is enormously sensitive to the order vanilla requests chunks in
(`PARITY.md` §5). A window this wide contains a great deal of it, where the
r=7 windows are 100% soul sand valley and have floors of 48 and 20.

Ranked residual, attributed the same way:

```text
     47456   18.6%  bastion_remnant (55,63)   — 105 pieces, not implemented
     22020    8.6%  bastion_remnant (70,49)   —  89 pieces, not implemented
    185323   72.7%  everything else
    254799   total
```

The 185,323 is led by `basalt <-> blackstone <-> netherrack` swaps in both
directions — 33,800, 30,517, 25,427, 23,788 and 23,354 for the top five — which
is the basalt deltas signature, the same families whose residual `PARITY.md` §5
measured as **84.7% chunk-visitation order** on a narrower window. It has not
been decomposed by order here, so **no conclusion is drawn from it**; the
standing rule is to decompose before concluding, and that run is named as the
next item rather than guessed at.

Also present and unimplemented: one `ruined_portal_nether` at (48,64). There
is **no fortress start in the window at all**, so this measurement exercises no
fortress code and a port of one will not move this number.

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


---

## The End — 100.000000%, closed 2026-08-06

> **SUPERSEDED BELOW.** This section as first written reported 125 blocks
> against a floor of zero and called the window the sharpest instrument in the
> project. Both halves were wrong in the same direction: the floor of that
> *pair* is zero but the floor across six references is 252, and our residual
> against ground vanilla actually reproduces is 0. The original text is kept
> because it was published. `docs/DETERMINISM.md` §5 and §6 have the whole
> derivation.

```text
  ours vs det-ens-end-d, UNMASKED           0 blocks   over 1,089 chunks
  ours vs the six-reference stable set      0 blocks
  order-invariant core, eight orders        0 blocks
  the floor across six references         252 blocks
```

> **⚠ NONE OF THOSE FOUR REFERENCES EXISTS — 2026-08-18.** `det-ens-end-d` and
> its five companions were destroyed in the 2026-08-07 disk failure and, unlike
> the overworld's and the nether's, were never re-harvested. Every End number in
> this file was therefore unreproducible for eleven days. Re-taken on fourteen
> order-stamped replacements: **the two zeros reproduce exactly** — 0 unmasked
> against a fresh reference, 0 against the fresh six-reference set, and the twin
> mask drops nothing — and **the 252 does not**, reading **127** across six
> references all harvested in one order. The old six recorded no harvest order
> at all (`harvest = "unrecorded"` on every End entry in `ci/figures.toml`), and
> on this window `rows` against `rows_rev` moves vanilla by 944, so a
> mixed-order floor and a matched-order one are different quantities rather than
> two readings of one. The whole retake is per biome and shows something this
> table cannot: the r=16 window weights `the_end` at **exactly zero**, so the
> central island is in none of it. `docs/DETERMINISM.md` §§51-50 and
> `ci/end-biome-window.sh`.

**Our dump is byte-identical to an unmodified vanilla run of this seed and
window.** Zero disagreeing blocks in 71,368,704, with no twin mask and no order
decomposition applied. Against the other five references it differs by 46 to
206 blocks, all `chorus_plant`/`chorus_flower`, and every one of those
positions is one the six vanilla runs spell differently among themselves.

The original text follows.

```text
  vanilla A vs vanilla B, unmasked         0 blocks    over 1,089 chunks
  vanilla A vs ours, twin-masked         125 blocks
  TWIN MASK: 0 positions dropped
```

**The floor is exactly zero and the mask therefore hides nothing.** This is the
only wide window in this project where §4's weakening does not apply at all:
there is no position vanilla is unsure about, so there is no position a defect
of ours can hide on. Every one of the 125 counts.

All 125 are one family in three chunks:

```text
   35  chorus_plant   -> air            worst chunk (55,72): 100 blocks
   23  air            -> chorus_plant   then (55,71): 15, (54,72): 10
    7  chorus_flower  -> air
```

**Both numbers, because the method changed.** The pre-existing reference
`van-end-20260722-r16` has no twin, so the published 81 was unmasked. Ours is a
new twinned pair harvested with `GEN_ORDER=rows`. Against the *old* reference
by the *old* method our dump gives **81 mismatches, 99.999887%** — the
published figure, reproduced exactly. Against the new pair it gives 125. The
two references differ from each other by 206 blocks, which is what a
differently ordered harvest costs, so both numbers are honest and neither
supersedes the other:

| comparison | mismatches | exactness |
|---|---|---|
| old reference, no twin (the published method) | 81 | 99.999887% |
| new rows-ordered pair, twin-masked, floor 0 | 125 | 99.999825% |

The new pair is the better instrument because its floor is measured and zero.

---

## What this window still does not cover

* **One seed for the nether and the End.** The overworld now has three.
* **Structure coverage, which is still a quarter of the nether.** Bastions,
  ruined portals, fortresses, ancient cities, strongholds, monuments,
  mansions, temples, igloos, outposts, trail ruins and buried treasure are not
  generated. Trial chambers now are — that is what changed in this branch.
* **Block entities, which a trial chamber has a great many of.** The blocks
  match; the spawner contents, vault contents, chest loot and pot sherds do
  not exist.
* **No order decomposition at this window for the nether.** The overworld's was
  run on 2026-08-03 and gave a core of 376,575; **the End's was run 2026-08-06
  and gave a core of 0** — eight distinct traversal digests, per-order totals
  125/125/127/1230/1105/168/637/125/125, nothing surviving all of them.
* Everything in `PARITY.md` §6 that is not a block state: block entities,
  entities, scheduled ticks.

## Reproducing it

```bash
# references (never overwrite an existing one — these are new directories)
GEN_ORDER=rows ci/gen-vanilla-region.sh 20260722 16 \
    oracle-work/van-nether-20260722-r16-a 25971 64 64 nether
GEN_ORDER=rows ci/gen-vanilla-region.sh 20260722 16 \
    oracle-work/van-nether-20260722-r16-b 25972 64 64 nether

# ours
RUSTMC_WORKERS=1 worldgen-dump full-nether 20260722 16 /dev/shm/wg-neth-r16 64 64

# diff — VANILLA FIRST, and --ybox is mandatory in the nether
region-differ diff oracle-work/van-nether-20260722-r16-a /dev/shm/wg-neth-r16/region \
    --bbox 48 48 80 80 --ybox 0 127 --twin oracle-work/van-nether-20260722-r16-b

# and the floor, which is the number that decides whether the above may be quoted
region-differ diff oracle-work/van-nether-20260722-r16-a \
    oracle-work/van-nether-20260722-r16-b --bbox 48 48 80 80 --ybox 0 127
```

The build machine was at 97–99% disk throughout, so every dump above was
written to `/dev/shm` rather than to `$HOME`.
