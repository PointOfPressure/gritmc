# COLD START — branch `clamped-floor-probe`

Append-only. One entry per turn on this branch, newest last.

### COLD START — branch `clamped-floor-probe`, 2026-08-17

**The question this branch existed to answer, answered: no.**

`docs/DETERMINISM.md` §37 clamped `-Dmax.bg.threads=1` over 25 chunks and got an
exact match on all six pairs. If that held at r=16, the overworld window would
carry a quotable exactness for the first time in this project. It does not hold,
and the reason turned out to be more interesting than the result.

## What was measured

All on sayer-desktop, 2026-08-17, `GEN_ORDER=rows`, vanilla first everywhere.

**`ci/clamped-floor-r16.sh`** — seed 12345, r=16, centre (64,64), 1,089 chunks,
bbox `48 48 80 80`, 107,053,056 blocks. This is the window `docs/PARITY.md`
publishes 69,288 / 99.935277% on, so the result lands on the headline directly.

```text
  CONTROL  4 unclamped refs,  6 pairs   13,317..17,779   median 16,302.5
  CLAMPED  6 clamped refs,   15 pairs    6,829..9,738    median 8,951

(Both readings above are registered unquotable in `ci/figures.toml`: 99.935277% is **superseded** by the matched-order retake, and the clamped 6,829..9,738 is a floor and therefore **not a level** — it is a spread, and nothing may be scored against it.)
                                         15 of 15 non-zero
```

**Median floor down 45.1%, and not to zero.** Compare medians, not ranges: the
clamped arm has 15 draws against the control's 6, so its range is wider for a
reason that has nothing to do with the clamp. The control arm re-measures a
floor an earlier section already prints at n=30 (median 16,270) and lands on
16,302.5 — that agreement is what says the instrument is live, and the script
refuses outright if any control pair reads zero.

**`ci/clamped-floor-ladder.sh`** (new) — the same six clamped references read
through narrower concentric bboxes. Nothing regenerated, so seed, clamp, harvest
order and generated extent are fixed by construction and only the *observed*
area varies.

```text
  r=1    9 chunks   unclamped 0..55      clamped 0..32      (12/15 nz)
  r=2   25          unclamped 25..278    clamped 6..308     (15/15 nz)
  r=4   81          unclamped 835..1343  clamped 214..889   (15/15 nz)
  r=8  289          unclamped 3760..5019 clamped 1579..3456 (15/15 nz)
  r=12 625          unclamped 8020..10880 clamped 4563..6602 (15/15 nz)
  r=16 1089         unclamped 13317..17779 clamped 6829..9738 (15/15 nz)
```

Read through 9 chunks the clamped references still disagree. So the failure is
not a threshold between 25 chunks and 1,089 — it is present at low density
everywhere.

**`ci/clamp-extent-control.sh`** (new) — seed 12345 at §37's own 25-chunk
extent, which is what separates "the seed" from "how much was generated".

```text
  CONTROL  3 unclamped, 3 pairs   148 / 5 / 143
  CLAMPED  4 clamped,   6 pairs   e1 = e3 = e4 exactly; e2 differs from all by 190
```

**Read through the 25-chunk box that is two discrete worlds** — a mode drawn
three times and one alternative drawn once. **Read through the inner 9 chunks,
which is the window §37 and `ci/t2-regiondiff.sh` actually use, all six pairs
are exactly 0, at BOTH seeds**, against unclamped controls of 10/17/27 and
0/89/89. Same references, same run, different `--bbox`.

## The finding, and I got it wrong once in public first

**An earlier §39, committed and pushed, said the clamp only narrows the harvest
to a few discrete worlds and that §37's exact match was "the mode drawn four
times". That is wrong and my own next measurement refuted it.** The first draft
compared §37's six zeros against a 25-chunk reading of a 25-chunk harvest; §37's
window is the inner 9 chunks. The constant 187/190 lives entirely outside the
smaller box. Nothing about the clamp changed between the two numbers, only the
`--bbox` did.

The corrected finding is three regimes, and the variable is **how many chunks
were generated in one harvest** — not the seed, and not how much you read:

1. **Interior of a small harvest — exact.** 6/6 pairs at 0, at both seeds. §37
   is reproduced at a second seed and stands as written.
2. **Periphery of that same harvest — not exact.** A second world appears in
   the boundary ring, same shape at both seeds: three identical, one off all
   three by a single constant.
3. **Large generated extent — exact nowhere**, interior included: 12 of 15
   pairs disagree over the very same 9 chunks that are exact in regime 1.

`ci/t2-regiondiff.sh` runs in regime 1, so its exact-match assertion is sound
and the doubt the first draft cast on it is retracted in the same commit.
Written up as `docs/DETERMINISM.md` §38 and §39.

## What that cost elsewhere, and it was fixed in the same branch

Three tracked headers still said the clamp makes the harvest reproducible, and a
correction that lives only in the file it corrects does not travel:

* `ci/gen-vanilla-region.sh` — "the harvest is bit-reproducible", and "the clamp
  decides whether this artefact is *the* world of its seed and window". Both now
  say what the clamp actually buys. It still recommends clamping new references,
  because 45% of a floor is worth having.
* `ci/t2-regiondiff.sh` — **this one matters, because it is a live gate.** Its
  LIVE part harvests two fresh clamped references and asserts they match at
  100.000000%. Under §39 that is asserting two independent draws land on the
  same discrete world, which has a failure rate that is a property of that
  window and had never been measured on it. Header and failure message corrected
  to lead with "re-run, this may not be a defect"; **gate behaviour deliberately
  unchanged** — the assertion still stands and still goes red.
* `ci/clamp-extent-control.sh` parametrised by `EXTENT_SEED`/`EXTENT_PORTBASE`
  so that rate can be measured on t2-regiondiff's own window (20260722) rather
  than inferred from seed 12345's.

## Killed as nulls — do not re-run these

* **"The clamp holds at r=16."** It does not: 15 of 15 pairs non-zero.
* **"The clamp holds locally and only fails at scale."** It does not: the
  9-chunk rung of the ladder already disagrees. Killed by re-reading existing
  references; it costs no generation, so do not spend a harvest on it.
* **"§37 vs §38 is a seed effect."** It is not. The two seeds behave
  identically in all three regimes; the generated extent is the variable.
* **"`ci/t2-regiondiff.sh`'s exact-match assertion is a coincidence."** It is
  not — measured on the gate's own window, 6 of 6 clamped pairs are 0 against a
  live unclamped control. I claimed otherwise for one commit; do not re-open it.
* **"`GEN_ORDER=perchunk` is a substitute for the clamp."** Already killed by
  §37 (0..79 at N=4) and re-killed here — that 0..79 is itself bimodal.

## Both numbers, taken on one tree on one afternoon

`ci/order-level-retake.sh` run twice over the same window, instrument and tree —
seed 12345, r=16, centre (64,64), 1,089 chunks, 107,053,056 blocks, harvest
`rows`, dump order `zx` read off the references — once against the six unclamped
references and once against the six clamped ones:

```text
  N=5          our charge                    innocent vanilla floor         sep
  UNCLAMPED    1,330..1,884 (median 1,342)   2,577..4,835 (median 3,387)   -2,045
  CLAMPED      1,917..2,333 (median 1,988)   2,006..3,265 (median 2,643)     -655
```

**We are below floor in both arms**, so rule 6 fires on both and neither is a
publishable exactness — in the good direction, because what the residual fell
below is the noise.

**The counter-intuitive row is the clamped one and it is not the generator
doing worse.** The admissible instrument charges us only where our block matches
neither reference, so an ensemble that disagrees with itself more forgives more.
Clamping narrows the admissible sets, which is why the charge goes *up* and the
floor comes *down*, and why the clamped column is the harder test. Registered as
`ow-r16-12345-admissible-{unclamped,clamped}-n5-20260817`.

Neither is comparable to `docs/PARITY.md`'s standing 69,288, which predates the
work landed on this window since and was not re-taken here.

## The single next item

**The owner's choice, and it is deliberately left open** — see "The clamped
harvest, and the choice this file has not made yet" in `docs/PARITY.md`, which
sets out three options and picks none. The r=16 headline does not become
quotable through the clamp; what is on the table is which floor travels beside
an unquotable residual, and whether the standing headline is re-based at the
same time.

The result that closes it: the owner picks one, and `ci/figures.toml` ends with
one registered r=16 admissible entry for this window rather than two.

**One thing there is NOT a judgement call and can be acted on immediately:** for
attribution, use the clamped ensemble whatever the published figure ends up
being. Per-pair floors fall with the aggregate, and that is what separates an
item from noise. At N=6 clamped, `sulfur -> cinnabar` is charged **350 against a
per-pair floor of 9** — an unambiguous item, and the largest single one on this
window — while `diorite -> andesite` at 87 sits under a floor of 428 and is
nothing. That first row is the next real worldgen defect and it is already
named by a branch (`worldgen-sulfur-cinnabar-order`).
