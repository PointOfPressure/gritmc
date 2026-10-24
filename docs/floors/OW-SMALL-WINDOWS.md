# The two overworld windows §2 recorded as `not measured` — and the one that moved the wrong way

**Measured 2026-08-18, branch `floors-n-campaign`. Eight unmodified-vanilla
references each, `gen_order=rows`, harvested for this.**

These are the top two rows of `docs/PARITY.md` §2, and they are the two that
read cleanest:

```
| overworld | 20260722 | chunks 57..71 | 22,114,295 | 1,079 | not measured | 0 |
| overworld | 31415926 | chunks 59..69 | 11,894,185 |     6 | not measured | 0 |
```

No `⚠`, no `🚫`, an order-invariant core of **0** on both. They are also the
two whose residuals are in no ledger entry at all, which §2 says of itself.

## Row 1 — overworld 20260722, chunks 57..71

Eight references, 28 pairs.

| N | union_N | innocent_N (min..med..MAX) | admissible residual | verdict |
|---:|---:|---|---:|---|
| 1 | 6,032..17,129 | 6,032 .. 11,344 .. 17,129 | 1,320..11,864 | under floor |
| 2 | 11,481..23,397 | 1,128 .. 5,913 .. 11,335 | 509..6,662 | under floor |
| 4 | 20,033..28,363 | 155 .. 3,060 .. 7,961 | 459..2,115 | under floor |
| 6 | 26,360..29,952 | 128 .. 2,057 .. 4,533 | 459..511 | under floor |
| 7 | **30,078** | 126 .. 1,746 .. **3,862** | **459..495** | **under floor** |

Our charge is **459..495 of 22,118,400 — 99.997925% at its worst reading — and
it is BELOW FLOOR, so it is corroboration and not an exactness figure.**

**The published 1,079 of 22,114,295 sits about ten times under the floor its
own instrument carries.** The pairwise width of this window is 6,032..17,129 with a median of
11,344, so a twin-masked residual of 1,079 was inside vanilla's own
disagreement from the beginning. On the admissible instrument the same verdict
holds at every N: our 459 against an innocent floor whose worst reading is
3,862.

So this row moved **the wrong way**. It went from a cell that reads as one of
the project's cleanest results to *no measurement at all*, and nothing about it
changed except that somebody finally measured the floor. `not measured` is what
hid it: an empty cell reads like a small number and it is not one.

**The core survives.** The order-invariant 0 in the last column is the
eight-order intersection and this says nothing against it. What is withdrawn is
the raw residual, which was never evidence.

### Why our residual is *smaller* than an innocent vanilla run's

Our dump against each of the eight references:

```
  a 11,152   b 10,927   c 6,370   d 1,320
  e  6,062   f 10,602   g 11,864  h 7,659
```

against vanilla-versus-vanilla pairs of 6,032..17,129. **We disagree with
reference `d` in 1,320 positions, which is less than any two vanilla runs
disagree with each other.** That is not a fidelity result. `ci/figures.toml`
already records the mechanism for the r=16 window of this same seed: a
deterministic generator locked to the references' own harvest order beats a
vanilla run carrying scheduler jitter, and two properties neither of which is
fidelity are enough to produce that ordering.

Being below an innocent floor is the instrument saying it cannot tell this port
from vanilla on this window. It is corroboration, and this campaign now has it
measured rather than absent.

## Row 2 — overworld 31415926, chunks 59..69

Eight references, 28 pairs. This is the smallest window in §2 — 11x11 chunks —
and it is also the quietest overworld window measured anywhere in this project.

| N | union_N | innocent_N (min..med..MAX) | admissible residual | verdict |
|---:|---:|---|---:|---|
| 1 | 284..2,190 | 284 .. 1,222 .. 2,190 | 221..1,464 | under floor |
| 2 | 847..2,800 | 51 .. 608 .. 1,785 | 6..585 | under floor |
| 4 | 1,862..3,444 | 31 .. 375 .. 1,401 | 6..13 | under floor |
| 7 | **3,759** | 31 .. 293 .. **527** | **6** | **under floor** |

Our charge is **6 of 11,894,784 — 99.999950% — and it is BELOW FLOOR**, which is
what 6 against a floor of 31..527 means: corroboration, not an exactness figure.

**Our admissible residual converges to exactly 6**, which is the number §2 has
published for this row since 2026-08-02. That is a pleasing corroboration from
a completely independent route — a different instrument, a different reference
set, eight years' worth of references that did not exist when the 6 was taken —
and it is worth stating plainly: **the 6 is real.**

It is also not a measurement. The floor is 31 at its *minimum* reading over
eight innocent candidates and 527 at its worst, and two vanilla runs of this
window disagree with each other by 284..2,190 blocks. A 6-block disagreement
sits below even the most generous reading of the noise.

So this row joins row 1: **a residual that is exactly right and carries no
information.** The window is too quiet to resolve anything at this scale — not
because our port is good on it, but because eleven-by-eleven chunks of this
seed contain almost nothing vanilla is uncertain about *and* almost nothing to
get wrong.

## Both rows together

The two rows §2 presents first, with no warning marker and an order-invariant
core of 0 each, are the two rows in the table with **no resolving power at
all** — and this is the first time either has had a floor.

| | published residual | floor at N=7 (innocent, MAX) | ratio |
|---|---:|---:|---:|
| 20260722 chunks 57..71 | 1,079 | 3,862 | 0.28x |
| 31415926 chunks 59..69 | 6 | 527 | 0.01x |

Neither should be quoted as an exactness figure. Both are corroboration, and
both of their order-invariant cores of 0 stand untouched — the core is an
eight-order intersection and nothing measured here bears on it.

The lesson is about the empty cell rather than about either window. `not
measured` reads like a small number. On these two rows it was hiding a floor
several times larger than the residual beside it, on the two rows a reader
reaches first.
