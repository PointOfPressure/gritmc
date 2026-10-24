# Cold start — branch `worldgen-resolving-power`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `worldgen-resolving-power` off `published-figures-audit` (`492473d`), 2026-08-07

**Item: the r=16 overworld had no quotable exactness figure. It has one again,
and the window did not change — the instrument did.** Worktree
`~/grit/work-resolving`; box work in `~/rustmc-determinism` (already had a
built `region-differ`); artefacts in `~/rpower/` and
`~/rp-logs/ow-e2e-pos/`. Not merged — the orchestrator gates.

**The figure: 30,279 blocks of 107,053,056 match NEITHER of two unmodified
vanilla references. Seed 20260722, centre (64,64), r=16, 1,089 chunks, full
height, vanilla first, NO MASK.** It is a lower bound, not a point estimate,
and it is 352 blocks *worse* than the unquotable twin-masked number it
replaces. `ci/admissible-residual.py`, `docs/DETERMINISM.md` §12,
`ci/figures.toml` id `ow-r16-admissible-n2`.

**Why the floor could not be reduced, measured rather than argued:**

* `ChunkPyramid`'s `GENERATION_PYRAMID` gives `FEATURES` the only non-zero
  `blockStateWriteRadius` in the whole pyramid (1), and `LIGHT` requires
  `INITIALIZE_LIGHT` at radius 1, whose parent is `FEATURES` — so a chunk
  cannot reach FULL owed a neighbour's writes. **Partially populated edges are
  impossible**, not merely unobserved.
* The **outer ring is the quietest part of the window** (11.8 mismatches per
  chunk against 112.9 at ring 3, 0 at the centre), and widening the compared
  box to 46..82 adds 280 chunks at 17.4 per chunk against 32.6 inside. No
  boundary artefact.
* **No structure palette appears in the floor at all** — 136 distinct pair
  families and not one of them is a structure block.
* 93.6% of it is 25 families, all read-then-write decorations: `sulfur_pool`
  (a `minecraft:lake` with a `can_place_feature` predicate that reads blocks),
  `sulfur_spike_cluster` (a `speleothem_cluster` with a 12-block
  floor-to-ceiling search), the three stone variants overwriting each other's
  ore blobs, and kelp/seagrass in water.
* **`GEN_ORDER=perchunk` makes the overworld floor WORSE**: 2,535 under `rows`
  against 2,728, on one 121-chunk square at centre (59,66), with a positive
  control of 3,345 between the two orders. Four fresh unmodified references.
  §2's "do not try this again" now covers the overworld too.

**Two things a future agent should not re-derive:**

1. **A floor belongs to the run pair, not to the seed and window.** The same
   121 chunks show 12,185 at the r=16 extent and 2,535 at r=5. Extent and box
   load both differ and the experiment does not separate them; the conclusion
   that survives either way is that a floor is not transferable.
2. **`99.974913%`'s missing denominator is recovered.** It is
   `225 * 98,304 - 15,060 = 22,103,340` — the twin-masked one, never written
   down. The arithmetic was always right. It stays retired because that
   sub-window's own floor of 15,060 exceeds its residual of 5,545.

**What is NOT done and is the obvious next link:** the same instrument has
never been run on seeds 12345 or 31415926 at r=16, and both of their windows
were disqualified by rule 6 on the masked route (31415926's floor 49,570
exceeds its residual 39,143; 12345's 12,774 does not, and it is the one window
that always had resolving power and has not been re-measured since). Each needs
two unmasked diffs against references that already exist. The nether at r=16
is the bigger prize — floor 317,725 against residual 254,799 — and it is
exactly the shape this instrument was built for.
