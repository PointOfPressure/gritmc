# Is the order-matched row a real effect or an order artefact?

**Measured 2026-08-18, branch `floors-n-campaign`.**

`docs/PARITY.md` §2's one ✅ row:

```
| overworld | 12345 | r=16, order matched, N=5 | 107,053,056 | 69,288 ✅ | 4,835 (N=5) | 65,119 |
```

It is the only quotable exactness figure any r=16 overworld window carries, it
reads as the project's best result, and the question is whether it is one.

Three answers, and they are not the same answer.

## 1. Harvest order really does dominate — confirmed on a second seed

§15 established on seed 20260722 that two vanilla runs of the same order differ
by ~32,000 blocks and two of different orders by ~150,000. That has never been
checked on another seed. Measured here on 12345 r=16, bbox 48 48 80 80:

| pair | orders | width |
|---|---|---:|
| `c` vs `rr-a` | rows vs rows_rev | **93,821** |
| `d` vs `rr-b` | rows vs rows_rev | **96,296** |
| `rr-a` vs `rr-b` | rows_rev vs rows_rev | 19,008 |
| `c` vs `d` | rows vs rows | 17,779 |

*(Counts are `region-differ`'s own `mismatches=` field. An earlier revision of
this table took them by counting lines of `--positions` output, which is
inflated by the two summary lines and the y-distribution histogram the same
flag prints — 54 too many on each row. The ratio was unaffected and the floor
tables never used that route: `ci/floor-both-definitions.py` parses positions
and agrees with `mismatches=` to the block.)*

**5.3x between cross-order and within-order**, against 20260722's 4.6x. The
finding replicates. The *magnitude* does not — 94,000 here against 150,000
there — which is the project's own rule that order is not a constant fraction
and must never be estimated from another window's.

So controlling the order is necessary, and the row is right to do it.

## 2. This row is not itself an order artefact — but its sibling was

The two seeds mean opposite things by "order matched".

| | unmatched | matched | change | verdict after matching |
|---|---:|---:|---:|---|
| 12345 | 74,224 | **69,288** | **-6.7%** | clears its floor 14x |
| 20260722 | 29,605 | **2,450** | **-92%** | **below** its floor (3,281) |

On 12345 the order was set from the first measurement — the dump was generated
`zx` to match the references' own `HARVEST-ORDER` stamp before any level was
taken — so there was no artefact to remove and matching moved the number 6.7%.
On 20260722 the published residual had been taken with our dump x-major against
z-major references, and removing that artefact left **2,450 against a floor of
3,281: nothing**. That window's entire published residual was very nearly the
order artefact itself.

**So the 12345 row is not an order artefact.** Its condition was controlled
from the start, which is exactly what its sibling's was not.

## 3. Its advertised margin IS an artefact — of the floor definition

The row's claim to be the best result is "14x separation". That 14x is
`69,288 / 4,835`, and **4,835 is the only `innocent_N` floor in the table**.
Its eleven neighbours are scored against `union_N` — or against a pairwise
width at N=2, which is `union_2`.

Measured on this row's own six references, at this row's own N=5:

| definition | floor at N=5 | separation from 69,288 |
|---|---:|---:|
| `innocent_5` (what the row quotes) | 4,835 | **14.3x** |
| `union_5` (what its neighbours quote) | 37,248 | **1.86x** |

Neither re-scoring is *correct* — each residual needs its own null, and 69,288
is an admissible residual whose null is the innocent floor. The conclusion is
narrower and worse than "the row is wrong": **the row is right and the column
is not comparable**, and a reader told to read the floor column before the
residual column will conclude this row beats its neighbours by an order of
magnitude on evidence that says no such thing.

## 4. The row is also stale in both columns, and the ledger already knows

`ci/figures.toml` has carried better numbers for this row since 2026-08-10.
§2 shows none of them.

| N | residual | innocent floor | ratio | verdict |
|---:|---:|---:|---:|---|
| 5 (what §2 shows) | 69,288 | 4,835 | 14.3x | clears |
| 5, after `iceberg` | **7,940** | 4,835 | 1.64x | clears |
| 15 | **1,329** | **2,241** | **0.59x** | **UNDER FLOOR** |
| 39 | **1,279** | **362** | 3.53x | clears |

*(`ow-r16-12345-admissible-iceberg`, `-level-n15-iceberg`, `-level-n39-iceberg`,
`ow-r16-12345-floor-n15`.)*

**Provenance of that table, stated because half of it is relayed rather than
re-measured.** The FLOOR column is reproduced here: `ci/floor-vs-n.py
--selfcheck-floor` over the sixteen-reference matrix returns (141, 590, 2,241)
against the registered (141, 590, 2,241), so the N=15 floor is confirmed on
this tree and the seed-12345 references have not rotted. The RESIDUAL column is
**read from `ci/figures.toml` and not re-taken**, and deliberately so: those
charges were measured on the post-`iceberg` tip and a charge is a property of
the code tip that produced it, so re-running them on `cf443588` would produce
different numbers and would not be a check of theirs. The non-monotonicity is
therefore a ledger fact about three registered figures rather than a
measurement of this branch.

**The verdict on this window is not monotonic in N. It clears at N=5, fails at
N=15, and clears again at N=39.** Both quantities shrink as references are
added and they shrink at different rates, so the ratio between them is free to
cross 1 more than once.

That is the most important sentence in this document. A row that changes
verdict as N grows was never adjudicated by its N=5 reading — it was
adjudicated by an arbitrary stopping point, and the ledger records the row
going quotable, unquotable and quotable again while the dashboard shows a
single ✅.

## What should happen to the row

* Quote it at the **highest N its references support**, which is 39, not 5.
  The references are on the box and cost nothing further.
* Print the **floor's definition** beside the floor, because 4,835 and 37,248
  are both true of the same six runs.
* Do not present it as beating its neighbours. It is measured on a different
  instrument from all of them.
* Carry the **non-monotonicity** with it. "Clears at N=39" is only meaningful
  next to "failed at N=15", and a reader who is not told that will assume more
  references can only help.
