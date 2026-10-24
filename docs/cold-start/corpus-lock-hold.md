# COLD START — branch `corpus-lock-hold`

### COLD START — branch `corpus-lock-hold`, 2026-08-16

**The item: a merge gate holds the box-wide corpus lock for the whole of
`ci/t2-gameplay.sh`, `gate-lanes` measured the Amdahl ceiling of lanes at 1.40x
at any N, so the hold itself is the only lever. What in it depends on the
branch?**

Answer, measured three ways: **half of it does not.** Full method, tables and
caveats in `docs/CORPUS-LOCK-HOLD.md` — read that, not this.

| phase | rate | at N=106 | share |
|---|---:|---:|---:|
| vanilla reference recording | 12.09 s/scenario | 21.3 min | **49.8%, branch-invariant** |
| rustmc subject recording | 12.21 s/scenario | 21.6 min | 50.2%, branch-dependent |
| compare + floor | — | <0.1 min | <0.3% |

Instruments: per-phase mtimes in five trees that ran a corpus, and — independently,
sharing no input — twenty-six cold vanilla boots read out of the selfcheck
servers' **own `logs/latest.log`**, which survived the 2026-08-07 disk failure
even though the recordings did not. Linear in N, zero intercept, ±0.1 min over
six runs of one size. The two agree to 0.2%.

## What I shipped

* **`ci/reference-cache.sh`** — records the vanilla reference once per (jar,
  corpus, recorder, projection, substrate, tables, seed) and serves it to any
  gate whose inputs match. Content hashes only, never paths, never mtimes.
  **Opt-in, `RUSTMC_REFERENCE_CACHE=1`, default off.**
* **`ci/reference-cache-falsify.sh`** — 22 cases, ~2 s, no jar/server/lock.
* **`ci/reference-cache-integration.sh`** — 6 cases, drives the real
  `ci/t2-gameplay.sh` against stubbed servers in a private lock namespace and
  asserts `vanilla-server start` is reached on a miss and **never** on a hit.
* **`ci/corpus-floor.sh` refuses byte-identical recordings** passed as separate
  runs — see the null below, it is the important part.
* **`ci/corpus-hold-sample.sh`** — passive phase sampler for a live gate.
* `ci/t2-gameplay.sh` now stamps `recorder=` into the recording's `.prov`,
  closing the gap `ci/corpus-floor.sh`'s header names.

All three falsifiers are in `ci/t0.sh`.

## The finding that mattered most, and it is against my own change

**A reference cache silently destroys `ci/corpus-floor.sh`.** That script's own
usage line globs `~/rustmc*/oracle-work/gameplay/vanilla.json`; every tree that
hits the cache leaves a byte-identical file at exactly that path. Identical
recordings disagree by zero at every scenario, so enough of them report every
scenario as *reproduced by the reference* — **a floor of zero manufactured by
the instrument**, in the file whose header says a floor of 0 from one recording
would be this project's favourite defect. Never checked because never possible;
measured 2026-08-16, all 55 recordings on the box are byte-distinct. The guard
ships with the cache that created the hazard.

## Nulls — do not re-run these

* **The reference recording is NOT purely a function of the jar and the
  scenario file.** `client::Tables::load` reads
  `data/vanilla/{items,block_props,entity_types}.json`, which are generated from
  the tree's own source and decide which item id and block state the harness
  sends *to vanilla*. Measured across six trees: identical in five, different in
  `rustmc-items`. Any cache omitting them is wrong. Do not "simplify" the key.
* **The projection is not known to be a recording input** — `compare::project`
  runs at compare time, not record time. It is in the key anyway because
  `scenario.rs` is in `PROJECTION_SOURCES` *and* drives recording, and because
  an over-specified key costs hit rate while an under-specified one costs
  correctness. Not comparable prices. Do not remove it to raise the hit rate.
* **The brief's `38.4 min` hold is a lower bound by construction.** It came from
  waiter heartbeats, and a waiter measures from *its own arrival* to the
  release, never the holder's full hold — it can only undercount. Directly
  measured, record work alone is 42.8 min at N=106. The brief's `53.9 min` gate
  is below the fastest full gate in `gate-chain.log` (22 gates got past T1;
  55.2–177.7 min, median 81.6).
* **The live phase sampler produced nothing on 2026-08-16, and not because it
  is broken.** The chain draining `queue7.txt` was failing every branch at T0 or
  T1 in under a minute, so no gate reached stage 6 while it was armed. The
  instrument is committed and correct; it had nothing to watch.

## The single next item

**Turn `RUSTMC_REFERENCE_CACHE=1` on for one merge gate and measure the hold
both ways with `ci/corpus-hold-sample.sh`.** That is the one number this branch
could not take, because it was not permitted to run a live gate. Everything else
is measured; the hold reduction is *projected* from the measured 21.3 min
(→ ~21.6 min, 1.99x on the serialised section) and **must not be quoted as
observed until that run exists**.

Then: re-run `ci/lanes`' Amdahl figure against the new serial fraction — halving
the hold raises the lanes ceiling rather than competing with it, and the two
multiply.
