# The two nether rows this campaign owns: one clears by 12.6x, one never will

**Measured 2026-08-18, branch `floors-n-campaign`. Eight unmodified-vanilla
references each, all `gen_order=rows`, all UNCLAMPED.**

`origin/nether-biome-census` re-took the `chunks 57..71` seed-20260722 row and
the `r=16` row this morning; those two are not re-measured here. These are the
two it did not replace.

Every reference used is **unclamped**. The `-clamp-` variants on the box were
harvested with `-Dmax.bg.threads=1`, and that flag does not clamp the harvest:
`Util.makeExecutor` passes `maxAllowedExecutorThreads()` as a `ForkJoinPool`
**parallelism** rather than a thread cap, so the pool spawns compensation
workers anyway. Mixing them would be mixing a condition, not tightening one.

## Row 10 — nether 20260722, chunks 93..107. Floor NOT MEASURED before today.

Two `open` references existed; six more were harvested for this.

| N | union_N | innocent_N (MAX) | admissible residual | verdict |
|---:|---:|---:|---:|---|
| 1 | 22..164 | 164 | 491..602 | CLEARS |
| 4 | 86..216 | 99 | 477..492 | CLEARS |
| 7 | **217** | **38** | **477** | **CLEARS** |

**477 of 7,372,800 — 99.993530% — against an innocent floor of 0..38 at N=7.**
A separation of **12.6x**, and it clears at every N from 1 upward.

The striking thing is how quiet this window is. Its pairwise width is **22..164,
median 74**. The other nether window in this document, the same size in the same
dimension and the same seed family, reads **104,754..151,425**. Three orders of
magnitude between two 15x15 nether windows — which is why "the nether is noisy"
is not a property of the dimension and a floor may never be carried from one
nether window to another.

This row also clears on the twin-masked instrument: §2's published residual of
10,038 against a `union_7` of 217 is 46x. It is quotable either way, and §2
records its floor as `not measured`.

## Row 11 — nether 31415926, chunks 57..71. §2: 39,941 against 95,240 (N=2) 🚫

Three `open` references existed; five more were harvested.

| N | union_N | innocent_N (MAX) | admissible residual | verdict |
|---:|---:|---:|---:|---|
| 1 | 104,754..151,425 | 151,425 | 85,218..135,437 | under floor |
| 5 | 263,220..280,972 | 42,084 | 11,738..17,752 | under floor |
| 6 | 286,479..294,696 | 29,393 | 10,392..14,309 | under floor |
| 7 | **303,381** | **22,377** | **9,560..10,728** | **under floor** |

**Still under floor at N=7**, at 99.854%..99.870% of 7,372,800 — quoted here
only as BELOW FLOOR corroboration and never as an exactness figure. The verdict
does not change and this campaign strengthens rather than overturns it.

Note that the published floor of **95,240 sits below the entire N=8 pairwise
distribution** (min 104,754 over 28 pairs). It was not a typical reading of
this window — it was the most favourable pair available.

## The diagnostic these two rows establish

The two rows fail and pass for a reason visible **before** either reaches its
crossing point, and it is the rate rather than the level:

```
                      residual falls   floor falls   ratio res/floor
row 6  (ow 31415926)      -6% / ref     -24% / ref   0.93 -> 1.10 -> 1.42
row 11 (nether 31415926) -22% / ref     -27% / ref   0.42 -> 0.49 -> 0.48
row 10 (nether 20260722)  -1% / ref     -50% / ref   3.0  -> 4.8  -> 12.6
```

**A residual made of the same positions as the floor falls at the floor's
rate.** Row 11's does, so its 9,560 is noise that more references will keep
shrinking in step with the thing it is measured against; the ratio has been
flat at ~0.48 for three rungs and no crossing is in sight. Row 6's does not —
it falls four times slower than its floor — so it is a defect count standing on
ground vanilla reproduces, and it crossed within two references. Row 10's
barely moves at all.

This is worth more than either verdict. **The ratio's trajectory over three
rungs says whether buying references can ever rescue a row**, and it costs
three points on a curve rather than an open-ended purchase. A row whose
residual tracks its floor should not be bought references; it should be
re-examined for what the shared positions are.
