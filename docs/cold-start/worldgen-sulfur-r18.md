# Cold start — branch `worldgen-sulfur-r18`

Append-only. A new `### COLD START` entry per turn on this branch; nothing
above is edited.

---

### COLD START — branch `worldgen-sulfur-r18`, off `main` (`d049ef5a`), 2026-08-12

**BASE SHA `d049ef5a9c504ce61af44b939c7b19b0d9fe4b65`.** Recorded first because
every figure on this branch is only valid against the generator that produced
it. `worldgen-decoration-margin` is live on whether `DECORATION_MARGIN = 2`
converges; **at this sha that branch points at exactly `d049ef5a` and
`DECORATION_MARGIN` is still 2**, so it has landed no behavioural change and
this adjudication is measured against a generator that still exists. If it
merges a behavioural change *after* this, every number here is void and must be
re-run.

**Item: are the 35 `sulfur -> cinnabar` on chunk (77,47) real, or were the r=16
references measuring their own generation extent?** Answered: **real.**
`docs/DETERMINISM.md` §34; `ci/sulfur-r18-adjudicate.sh` reproduces all of it
and exits 1 rather than 0 if the row is not 35.

**Worktree `~/grit/work-sulfur-r18`, not `~/grit/work-sulfur`.** The brief named
the latter and it already existed (detached at `a17daf5d`); `AGENT-BRIEF`'s
"pick a `work-<yours>` that does not already exist" wins. `RUSTMC_REMOTE_DIR`
was `rustmc-sulfur` as briefed. T0 GREEN on the base before any measurement,
`grep -c "skipping: no vanilla"` = 0.

**The answer, in one line.** Chunk (77,47) against the two r=18 twins: **51
charged, of which 35 are `sulfur -> cinnabar`, against a floor of 0 derived
from the twins themselves.** The r=16 pool-40 reading was 49 with 35. **The row
does not move by one block.**

**The floor is the twins' own disagreement, and it is never subtracted.** Not
inherited from the r=16 pool — a floor belongs to the pair of runs that
produced it. Over the window `44 44 84 84` the twins disagree by **28,668** of
165,249,024; on chunk (77,47) they disagree by **0 of 98,304**, reproducing it
block for block. Our charge on that window is 47,237 (x1) / 47,851 (x2), which
is 1.65x the floor, so **the window carries no exactness level and none is
claimed** — registered `quotable = false` as `ow-r18-12345-level-n2`.

**The window was derived, not chosen.** `ci/ref-chunk-extent.py --centre 64 64`
reports r=20 on both twins (they complete 44..84), and
`worldgen-dump full 12345 20` writes exactly 44..84. 1,681 chunks, every side
`minecraft:full` across all of them.

**The coverage refusal fires — watched, not asserted.** Three paths, all rc=3:
`ref-chunk-extent.py --window 43 43 85 85` on the twins (168 of 1,849 chunks at
`initialize_light`); `--window 44 44 84 84` on our r=16 dump (592 of 1,681
absent); and `refs-12345-widen2.sh charge` end to end. **And the silent failure
it prevents was measured:** `region-differ diff x1 <our r=16 dump>` reports
`chunks=1089 blocks=107053056 mismatches=10650` on the honest `48 48 80 80`
window **and the identical numbers on `44 44 84 84`** — a window 54% larger
scored not one extra chunk and said nothing about it.

**Nulls and controls — do not re-run these.**

* **"Extending the dumper" is not a code change.** `worldgen-dump full 12345 R`
  already writes `[64-R, 64+R]`; r=20 covers the twins' whole completed extent.
  It cost 95 s at `RUSTMC_WORKERS=1`, nothing else.
* **The tip did not move the generator.** Our r=20 dump at `d049ef5a` is
  **byte-identical in every region file** to the one taken at `31e4615f`, and
  `git diff 31e4615f d049ef5a -- crates/rustmc-worldgen/ tools/` is empty. Do
  not re-dump for tip reasons.
* **Our own extent does not reach (77,47).** ours-r18 vs ours-r20: 0 on that
  chunk, 0 over the whole ring z=47, 2,020 on ring x=46, 690 on x=47, **0 on
  x=48 and x=49**. Our truncation reaches exactly two rings inward and stops.
* **r=16 and r=18 are one distribution on this chunk.** r16-vs-r18 reads
  99.766..100.000 and r16-vs-r16 reads 99.766..100.000, and every r=16
  reference gives the *same* value against x1 as against x2. There is no
  cross-generation signal to find.
* **A two-reference floor on this chunk is a coin flip.** Across r=16 pairs the
  same statistic draws from {0, 34, 196, 230}; under {c,d} and {e,f} the row is
  AT/UNDER its floor at 0.31x. **The twins' 0 is a lucky draw and the result
  does not rest on it** — the row reads 35 under eight pools (N=2 twins, four
  of five r=16 pairs, N=6, N=40), two radii, two independent harvests.
* **The window-wide row is not the item.** 1,062 against a binding floor of
  298, rim-dominated. Do not quote it beside the chunk's 35.
* **Three of the published 312 are handed back.** (79,49) 6 -> 4 and (80,50)
  1 -> 0 against the r=18 twins, which cannot be an N effect — the charge only
  grows as N shrinks — so those positions are spelled our way by an r=18
  reference. (77,47), (77,48) and (78,48) do not move at all.

**What is closed and what is not.** The *adjudication* is closed: the 35 are a
port defect, vanilla writes `sulfur` there in forty independent r=16 runs and
both independent r=18 runs. The *mechanism* is not, and this branch did not
touch it — §21's order-inversion finding still owns the cause, and
`feature/speleothem_cluster.rs` writing sulfur over the noise stage's cinnabar
is still where the census puts it.

**The single next item, and the result that closes it.** Attribute the 35 to a
draw rather than to a position: `RUSTMC_TRACE_WRITE` on the 35 coordinates in
chunk (77,47) (they are in `~/rp-logs-sulfur-r18/pos/x1-vs-ours.pos`, chunk
x 1232..1247 z 752..767) and find which feature writes `cinnabar` there and
which vanilla feature writes `sulfur`. **It closes when the writing feature is
named and a poison control returns exactly those 35 and no others** — the
pattern `ci/iceberg-poison-control.sh` already sets. Note first that
`DECORATION_MARGIN` non-convergence is now measured, not suspected: 2,020 + 690
blocks in the two outermost written rings and 0 beyond them, which is
`worldgen-decoration-margin`'s item and not this one.


### COLD START — branch `worldgen-sulfur-r18`, 2026-08-17 (corroboration only, no adjudication moved)

**A ninth pool, and it is a different kind of pool.** The entry above rests the
35 on eight pools that are all unclamped harvests. `clamped-floor-probe` built a
six-reference ensemble of the r=16 window harvested under `-Dmax.bg.threads=1`
(`docs/DETERMINISM.md` §38), which is vanilla run with its chunk-status executor
clamped to one worker — a system property the stock server reads for itself, and
the parallelism an unmodified server already runs at on a two-core box. Scoring
our `zx` dump against it at N=6 with the same admissible instrument:

```text
  vanilla -> ours          charged   worst innocent   ratio
  sulfur -> cinnabar           350                9     39x
```

**It is the largest single unambiguous item on that window**, ahead of
`cave_air -> sulfur` (139, floor 135 — marginal) and
`sulfur -> deepslate[axis=y]` (94, floor 2).

**What this does and does not change.**

* The adjudication of the 35 is unchanged and was already closed. Nothing here
  is needed to support it and nothing here re-opens it.
* **The window-wide row may be worth reconsidering, and that is the new part.**
  The entry above says *"The window-wide row is not the item. 1,062 against a
  binding floor of 298, rim-dominated. Do not quote it beside the chunk's 35."*
  That caveat is correct for the pool it was taken on. On the clamped pool the
  same row is **350 against 9**, a ratio of 39x where the unclamped one was
  3.6x. The two are different pools at different N and are **not** a before/after
  of one measurement — but a row that clears its floor by 39x is adjudicable
  where one clearing by 3.6x is not, so the reason for the caveat may simply not
  apply to a clamped pool. **Re-take the window-wide row on the clamped ensemble
  before quoting it, and do not convert either number into the other.**
* The mechanism is still untouched. §21's order inversion and
  `feature/speleothem_cluster.rs` writing sulfur over the noise stage's cinnabar
  remain where the census puts the cause, and the next item stated above —
  `RUSTMC_TRACE_WRITE` on the 35 coordinates, closing when the writing feature
  is named and a poison control returns exactly those 35 — is unchanged and is
  still the next item.

Nothing was generated for this: it is a read of an ensemble that already
existed, through `ci/order-level-retake.sh` pointed at `/home/sayer/grit/clamp-refs`.
