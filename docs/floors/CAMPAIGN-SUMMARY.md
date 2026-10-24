# The floor-N campaign: all twelve §2 rows, before and after

**2026-08-18, branch `floors-n-campaign` off `origin/main` cf443588,
Threadripper. 36 unmodified-vanilla references harvested in 35 minutes.**

`docs/PARITY.md` §2 gained a floor column on `cf443588` and it immediately
showed that seven of its twelve floors were N=2 or absent. A floor from two
runs is a range rather than a measurement, and it was adjudicating the
project's headline claims. This is what taking them to a real N did.

## The finding that reframes the rest

**The floor column is two quantities that move in opposite directions as N
grows** — `docs/floors/FLOOR-IS-TWO-QUANTITIES.md`.

```
union_N     positions where N vanilla runs are not unanimous.
            Null for a TWIN-MASKED residual.     NON-DECREASING in N.
innocent_N  what the instrument charges a known-innocent vanilla run.
            Null for an ADMISSIBLE residual.     NON-INCREASING in N.
```

They are the same number at N=1, which is why nine rows at N=2 hide it. On the
same six references of the same window they read 4,835 and 37,248 — a factor of
7.7 that grows with N.

So "N=2 is not a measurement, buy more references" is **true for the admissible
rows and false in the other direction for the twin-masked ones.** Adding
references to a `union_N` row makes its floor bigger and its verdict worse. The
three 🚫 rows could never have been rescued by raising N on the instrument they
were quoted on.

## The twelve rows

| # | row | floor before | floor after | verdict change |
|---|---|---|---|---|
| 1 | ow 20260722 chunks 57..71 | *not measured* | 11,344 union N=2 / 126..3,862 innocent N=7 | **clean-looking → UNDER FLOOR** |
| 2 | ow 31415926 chunks 59..69 | *not measured* | 1,222 union N=2 / 31..527 innocent N=7 | **clean-looking → UNDER FLOOR** |
| 3 | ow 20260722 r=16 | 35,531 union N=2, destroyed pair | 86,240 union N=8 / 2,202..5,263 innocent N=8 | unchanged (stale) |
| 4 | ow 12345 r=16 | 12,774 union N=2 | 37,248 union N=5 | unchanged (not re-measured) |
| 5 | ow 12345 r=16 order matched | 4,835 innocent N=5 | 2,241 at N=15, 362 at N=39 | **✅ → 🚫 → ✅ across N** |
| 6 | ow 31415926 r=16 | 49,570 union N=2, **no references existed** | 113,938 union N=7 / 4,758..8,859 innocent N=7 | **🚫 UNDER FLOOR → CLEARS** |
| 7 | the End three windows | *not measured* | **0** (both definitions, N=5) | **not measured → EXACT** |
| 8 | the End r=16 | 252 union N=6 ✅ | unchanged; owned by the End lane | unchanged |
| 9 | nether 20260722 chunks 57..71 | 48 union N=2 | superseded by `nether-biome-census` (8,472 → 104) | not re-measured here |
| 10 | nether 20260722 chunks 93..107 | *not measured* | 217 union N=7 / 0..38 innocent N=7 | **not measured → CLEARS 12.6x** |
| 11 | nether 31415926 chunks 57..71 | 95,240 union N=2 | 303,381 union N=7 / 11,709..22,377 innocent N=7 | 🚫 unchanged, **strengthened** |
| 12 | nether 20260722 r=16 | 317,725 union N=2 | 766,490 union N=5 | 🚫 unchanged; superseded |

**Rows that changed adjudication in each direction, which is what was asked:**

* **Up:** row 6 (🚫 → clears at N=7), row 7 (not measured → exact against a
  floor of 0), row 10 (not measured → clears by 12.6x).
* **Down:** rows 1 and 2 — the two rows §2 presents FIRST, both with no warning
  marker and an order-invariant core of 0, are the two rows in the table with no
  resolving power at all. 1,079 against a floor of 3,862 is 0.28x; 6 against a
  floor of 527 is 0.01x. Neither ever had it, and `not measured` is what hid it.
* **Both, depending where you stop:** row 5, which clears at N=5, fails at
  N=15 and clears again at N=39.

## Three registered figures reproduced to the block

Also the reference-rot check the campaign needed before trusting anything:

| figure | registered | re-measured |
|---|---|---|
| `ow-r16-12345-floor-n5` | 2,577..4,835, median 3,387 | identical |
| `ow-r16-20260722-floor-n8` | 2,202..5,263, median 3,281 | identical |
| `nether-r16-n6-matched` floor | 766,490 | identical |
| `ow-r16-12345-floor-n15` via `ci/floor-vs-n.py --selfcheck-floor` | 141 / 590 / 2,241 | identical |

Those four ensembles have not rotted. The seed-31415926 r=16 window had nothing
to rot: it had **no references at all**.

## What this campaign says to do next

1. **Print the floor's definition next to every floor.** 4,835 and 37,248 are
   both true of the same six runs; a floor without its definition is not a
   number, in the same way a floor without its N is not one.
2. **Stop buying references for twin-masked rows.** It makes them worse. Re-take
   them on the admissible instrument instead, which needs a dump as well.
3. **Use the rate, not the level, to decide whether to buy at all.** A residual
   made of the floor's own positions falls at the floor's rate. Row 6's fell
   four times slower and crossed within two references; row 11's has tracked its
   floor at a ratio of ~0.48 for three rungs and will not cross. Three points on
   the curve cost three references and settle it.
4. **`not measured` is not a neutral cell.** It hid both of the campaign's
   downgrades, on the two rows a reader reaches first. Four rows carried it and
   they landed at both extremes: one exact against a floor of zero, one clearing
   by 12.6x, and two with no resolving power whatsoever.
