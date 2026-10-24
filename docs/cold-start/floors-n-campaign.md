# Cold start — branch `floors-n-campaign`

### COLD START — branch `floors-n-campaign` off `origin/main` cf443588, 2026-08-18

**Every floor in `docs/PARITY.md` §2 taken to a real N, and the column turned
out to be two different quantities.** Remote dir `~/rustmc-floors`, references
under `~/rustmc-floors/oracle-work/floors-*`, our dumps under `~/rp-floors`,
logs under `~/grit/logs/floors-*` and `~/grit/logs/fbd-*`. Not merged — the
orchestrator gates.

#### The one thing to read before anything else

§2's floor column mixes two nulls that move in **opposite** directions as
references are added:

```
union_N     positions where N vanilla runs are not unanimous.
            Null for a TWIN-MASKED residual.        NON-DECREASING in N.
innocent_N  what the instrument charges a known-innocent vanilla run.
            Null for an ADMISSIBLE residual.        NON-INCREASING in N.
```

Equal at N=1, which is why nine rows at N=2 hid it. On the identical six
references of the identical seed-12345 window: **4,835 and 37,248**.

**Consequence, and it inverts the obvious plan:** buying references improves an
admissible row and *worsens* a twin-masked one. The three 🚫 rows could never
have been cleared by raising N on the instrument they were quoted on.

#### What moved

Five of twelve rows changed verdict, in both directions.

| row | before | after |
|---|---|---|
| ow 31415926 r=16 | 🚫 UNDER FLOOR, floor 49,570 (N=2) | **CLEARS at N=7**, 12,542 vs 8,859 |
| the End r=7 c(83,62) | *not measured* | **EXACT**, 0 vs floor 0 (N=5) |
| nether 93..107 | *not measured* | **CLEARS 12.6x**, 477 vs 38 (N=7) |
| ow 20260722 57..71 | reads clean, 1,079, core 0 | **BELOW FLOOR** — 1,079 vs 11,344 |
| ow 31415926 59..69 | reads clean, 6, core 0 | **BELOW FLOOR** — 6 vs 31 minimum |
| nether 31415926 57..71 | 🚫, floor 95,240 (N=2) | 🚫 **strengthened**, 10,728 vs 22,377 |

#### Things a successor must not re-derive

* **The corpus lock was never a constraint here.** `ci/gen-vanilla-region.sh`
  boots its own vanilla on its own port in its own scratch dir and takes no
  lock. 37 references took 35 minutes with three other lanes recording corpora.
* **Box load does not move a floor.** Harvest wall time varied 2.9x (70 s quiet
  to 200 s at load 100) and the pairwise floor moved -5%, the wrong sign, well
  inside the spread quiet references already show. `docs/floors/LOAD-
  CONDITIONALITY.md`. n=3, stated as a negative at n=3.
* **The seed-31415926 r=16 window had NO references at all**, not merely two.
  `DETERMINISM.md` §4.5 priced the ladder on 2026-08-10 and nobody took it, so
  the 49,570 belonged to a pair not on the box — the destroyed-pair condition
  reached by never saving.
* **Two of the End row's three windows are defined nowhere in this repo, in any
  commit.** Not destroyed; never written down.
* **The rate is the cheap diagnostic.** A residual made of the floor's own
  positions falls at the floor's rate. Row 6's fell 4x slower and crossed within
  two references; the nether 31415926 row's has tracked its floor at ~0.48 for
  three rungs and will not cross. Three points settle it before any purchase.
* **The order-matched ✅ row is real but its margin is not.** Its order was
  controlled from the first measurement (6.7% effect), unlike 20260722's, whose
  published residual was very nearly the order artefact itself (92%, and it fell
  below its floor once removed). But 14x is `69,288/4,835` and 4,835 is the only
  `innocent_N` floor in the table; scored as its neighbours are it is 1.86x. And
  its verdict is **not monotonic in N** — clears at N=5, fails at N=15, clears at
  N=39, all three already in `ci/figures.toml`.

#### Reference-rot check, which passed

Four registered figures reproduced to the block from an independent
implementation: `ow-r16-12345-floor-n5` (2,577..4,835 med 3,387),
`ow-r16-20260722-floor-n8` (2,202..5,263 med 3,281), `nether-r16-n6-matched`'s
766,490, and `ci/floor-vs-n.py --selfcheck-floor` (141 / 590 / 2,241).

#### Reproduce

```
ci/floors-n-campaign.sh          # the 44-reference batch, idempotent by refusal
ci/floors-load-probe.sh          # the box-load control
ci/floors-dumps.sh               # our side, RUSTMC_DECORATE_ORDER=zx
ci/floors-readjudicate.sh        # every window, both floors, the verdicts
ci/floor-both-definitions.py     # the instrument
```

#### Left undone, deliberately

* The 31415926 r=16 floor is **still falling 24% per reference at N=7**, so
  8,859 is an upper bound and 1.34x is a weak clearance. Re-take at N=15.
* The End r=16 row is owned by the End lane, which was harvesting that window
  while this ran; not touched here beyond confirming its references exist under
  `gen-wide-20260722-r16-end-*` rather than the `det-ens-end-*` the documents
  name.
* The nether 31415926 row should be **censused for what its shared positions
  are**, not re-measured at higher N — the rate says N will not help.
