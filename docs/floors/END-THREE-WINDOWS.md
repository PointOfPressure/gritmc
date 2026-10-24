# §2's End "three windows" row: one window is real, two were never written down

**Measured 2026-08-18, branch `floors-n-campaign`.**

`docs/PARITY.md` §2 carries this row:

```
| the End | 20260722 | three windows, r=7 and r=9 | — | 0 | not measured | 0 |
```

It is the oldest row in the table — it has been there unchanged since
`3dc88f67`, 2026-08-02 — and it is the only row whose **denominator** is also a
dash. Both dashes turn out to be the same fact.

## Two of the three windows do not exist

`r=7` is `ci/end-city-window.sh`: seed 20260722, centre chunk (83,62), r=7,
y 0..255, bbox 76 55 90 69, denominator 14,745,600.

The two `r=9` windows are **not defined anywhere in this repository, in any
commit**. Searching every reachable revision for an End dump at radius 9 finds
nothing; the only End invocation that has ever existed in `ci/` is the r=7
one above, which appears identically in every commit that touches the file.
There is no script, no log, no figure in `ci/figures.toml`, and no centre chunk
recorded in any document.

So the row cannot be re-adjudicated as written, and it never could have been.
"Exact in three windows" is a claim whose evidence is two-thirds unrecoverable,
and the missing part was never destroyed in the 2026-08-07 disk failure — it
was never written down.

## The window that does exist has a floor, and the floor is zero

Five unmodified-vanilla references of the r=7 window are on the box: the
original pair `van-end-20260722-c8362{,-b}` and `van-end-20260722-c8362-open-
{a,b,c}` harvested 2026-08-18T09:19-09:20 by the End lane, all
`gen_order=rows`, all `dump_order=zx`.

Ten pairs. **Every one of them is zero.**

| N | union_N | innocent_N |
|---:|---:|---:|
| 1 | 0 | 0 |
| 2 | 0 | 0 |
| 3 | 0 | 0 |
| 4 | 0 | 0 |

Vanilla reproduces this window exactly across five independent runs, so the
instrument has **full resolving power here**: there is no position where
vanilla is unsure, and any disagreement of ours would be a defect and nothing
else. Our residual on it is 0.

**That is a stronger statement than the row was making, not a weaker one.** A
residual of 0 against a floor of 0 at N=5 is the best kind of result this
project can produce; a residual of 0 against `not measured` is not a result at
all, because an unmeasured floor could have been 252 — which is exactly what
the End's r=16 floor turned out to be when six references were finally taken
(§5 of `PARITY.md`, and the reason that dimension's "floor is exactly zero"
claim had to be withdrawn).

The withdrawal in §5 was about **r=16**. It does not transfer to this window,
and this measurement is why: at r=7 centre (83,62) the floor really is zero,
measured over five references rather than inferred from a lucky pair.

## What should happen to the row

Split it. The r=7 window is quotable at **0 of 14,745,600 against a floor of 0
(N=5)**. The two r=9 windows should be struck, not carried with a dash: a row
that cannot be reproduced is not evidence, and leaving it in place lets a
reader count three windows of support where the tree can supply one.

This is the third of the four `not measured` floors this campaign has reached,
and the only one that turned out to be zero.

## Adjudicated, on this tree, 2026-08-18

The floor above is vanilla against vanilla. This is our dump against the same
five references, generated `RUSTMC_DECORATE_ORDER=zx` to match their
`dump_order=zx` stamp, vanilla first in every call:

| N | union_N | innocent_N | admissible residual | verdict |
|---:|---:|---:|---:|---|
| 1 | 0 | 0 | 0 | **EXACT, floor 0** |
| 2 | 0 | 0 | 0 | **EXACT, floor 0** |
| 3 | 0 | 0 | 0 | **EXACT, floor 0** |
| 4 | 0 | 0 | 0 | **EXACT, floor 0** |

**0 of 14,745,600 against a floor of 0, at N=5 — 100.000000%.**

This is the only row in §2 that is exact against an instrument with full
resolving power, and the table currently shows it as `not measured` — the same
cell an unmeasured window gets.

### The verdict rule had this inverted

The first run of this adjudication reported **under floor**, because the rule
was `max(residual) <= max(innocent)` and `0 <= 0` is true. A floor of zero is
not a floor a residual can hide under: it means vanilla has no position it is
unsure about, so there is nowhere for a defect to hide and a residual of zero
is an exact match rather than a number lost in noise.

The rule was wrong at its own most favourable point, and it is this
repository's central defect class arrived at backwards — a check that could not
tell the best case from the worst, by being too harsh rather than too lenient.
Fixed in `ci/floor-both-definitions.py`; recorded here because the first
reading was taken and would otherwise have been quoted.
