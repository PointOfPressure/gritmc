# §2's floor column is two quantities, and they move in opposite directions

**Measured 2026-08-18, branch `floors-n-campaign`, on the Threadripper.**

`docs/PARITY.md` §2 gained a floor column on `cf443588` and told the reader:

> **READ THE FLOOR COLUMN BEFORE THE RESIDUAL COLUMN.**
> A floor belongs to the ensemble that produced it, so the N is part of the
> number and not a footnote.

Both sentences are right. What the column does not say is that **N moves
different rows in opposite directions**, because the column is not one
quantity. It is two, and eleven of the twelve rows use one of them while the
single row marked ✅ uses the other.

## The two definitions

```
union_N     | union over j of { p : r1(p) != rj(p) } |

            positions where N vanilla runs are NOT unanimous. This is
            ci/floor-ensemble.sh's floor_N. It is the null for a TWIN-MASKED
            residual, whose mask is built from the same disagreements, so the
            mask and the floor widen together.
                                                    NON-DECREASING in N.

innocent_N  max over candidates X of
            | { p : X(p) differs from EVERY reference in S } |

            what the instrument charges a run known to be innocent — an
            unmodified vanilla run substituted for ours. It is the null for an
            ADMISSIBLE residual |ours not in S|, which is an intersection and
            shrinks as references are added, exactly as this does.
                                                    NON-INCREASING in N.
```

**At N=1 they are the same number**, the pairwise width |A != B|. That is why
the divergence is invisible in a table where nine rows sit at N=2, one at N=5
and one at N=6.

Each row is internally coherent: in every case the null is correctly matched to
its own instrument. What is not sound is the operation the column's header asks
for — read the floor, compare the rows.

## Measured on identical references

Both floors, same references, same window, same run of the same script
(`ci/floor-both-definitions.py`):

### overworld, seed 12345, r=16 — six `rows` references c..h

| N | union_N | innocent_N (min..med..MAX) |
|---:|---:|---|
| 1 | 13,317 .. 17,779 | 13,317 .. 16,270 .. 17,779 |
| 2 | 21,016 .. 25,855 | 5,725 .. 7,970 .. 9,896 |
| 3 | 27,194 .. 31,616 | 3,576 .. 5,472 .. 7,074 |
| 4 | 32,413 .. 34,680 | 2,950 .. 4,247 .. 5,891 |
| 5 | **37,248** | 2,577 .. 3,387 .. **4,835** |

The innocent arm reproduces `ci/figures.toml:ow-r16-12345-floor-n5`
(2,577..4,835, median 3,387, n=6) **to the block**, from an independent
implementation.

### overworld, seed 20260722, r=16 — nine `rows` references a..i

| N | union_N | innocent_N (min..med..MAX) |
|---:|---:|---|
| 1 | 24,526 .. 39,085 | 24,526 .. 32,500 .. 39,085 |
| 2 | 41,578 .. 54,986 | 8,821 .. 16,256 .. 24,932 |
| 4 | 62,040 .. 72,358 | 4,017 .. 8,279 .. 12,234 |
| 6 | 76,194 .. 81,636 | 2,608 .. 5,190 .. 9,720 |
| 8 | **86,240** | 2,202 .. 3,281 .. **5,263** |

The innocent arm reproduces `ci/figures.toml:ow-r16-20260722-floor-n8`
(2,202..5,263, median 3,281, n=9) **to the block**.

### nether, seed 20260722, r=16 — six `rows` references a..f

| N | union_N | innocent_N (min..med..MAX) |
|---:|---:|---|
| 1 | 247,383 .. 409,796 | 247,383 .. 366,675 .. 409,796 |
| 3 | 557,233 .. 671,274 | 70,822 .. 123,926 .. 197,334 |
| 5 | **766,490** | 37,951 .. 80,241 .. 111,015 |

`union_5 = 766,490` reproduces `ci/figures.toml:nether-r16-n6-matched`'s floor
of 766,490 **exactly**, which is what identifies that row's floor as the union
definition rather than the innocent one.

## The number that matters

**On the identical six references of the identical seed-12345 window, the floor
is 4,835 or 37,248 depending only on which definition is used — a factor of
7.7.** On 20260722 at N=8 it is 5,263 or 86,240, a factor of 16.4. On the
nether at N=5 it is 111,015 or 766,490, a factor of 6.9.

The gap grows with N, because the two definitions are walking away from each
other.

## What this does to §2

The ✅ row — `overworld 12345 r=16 order matched, N=5`, residual 69,288 against
floor 4,835, "14x separation" — is the **only** row scored against
`innocent_N`. Its eleven neighbours are scored against `union_N` or against a
pairwise width at N=2, which is `union_2`.

Score that row the way its neighbours are scored and its floor is 37,248, so
the separation is **1.86x rather than 14x**. Score its neighbours the way it is
scored and their floors collapse by an order of magnitude.

Neither re-scoring is *correct* — each residual needs its own null. The
conclusion is narrower and worse: **the rows cannot be compared at all**, and
the column invites precisely that comparison.

## The consequence for buying references

This is the part that changes what the project should do next.

"N=2 is not a measurement, buy more references" is true for the admissible
rows and **false in the other direction** for the twin-masked ones. Adding
references to a `union_N` row makes its floor **bigger** and its verdict
**worse**. A row at N=2 with a floor just under its residual is not a row
waiting to be rescued by more references; it is a row that more references will
disqualify.

So the three 🚫 UNDER FLOOR rows cannot be cleared by raising N on the
instrument they are quoted on. They can only be cleared by **re-taking them on
the admissible instrument**, which needs our dump as well as more references —
and that is a different measurement, not a better-resourced version of the same
one.
