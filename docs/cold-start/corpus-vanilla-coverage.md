# COLD START — branch `corpus-vanilla-coverage`

### COLD START — branch `corpus-vanilla-coverage`, 2026-08-11

**The item: produce a denominator for the corpus, or prove one cannot be
produced honestly. Produced. `docs/PARITY-ROADMAP.md` §21.**

`72/92 declared` is a score against a set we wrote. This branch measures the
corpus against **vanilla**, with every denominator read out of the pinned
`server-26.2.jar` and every numerator measured off the six vanilla recordings
in `~/rustmc-cov2/oracle-work/gameplay-selfcheck` (ensemble
`coverage92-selfcheck-n6-20260811`).

**Nothing was booted, nothing was built, and the box-wide corpus lock was never
taken.** The whole item is static: the jar, `generated/reports/`, the 92
scenario files and recordings already on disk. Three other agents were live on
the box throughout and none of them was disturbed.

### What it measured

```text
  UNIT 1  clientbound play packet types    31/141 = 21.99% named by a check
                                           29/36  = 80.56% of what vanilla emitted
                                           29/48  = 60.44% if the 12 housekeeping
                                                    types are counted as emitted
  UNIT 2  vanilla's per-tick sections      41/120 = 34.17% provably never run
                                                    (8 gated off, 33 unpopulated)
                                           79 unclassified = UNKNOWN, not covered
  UNIT 3  registries      sound_event  29/1968   1.47%
                          block        43/1196   3.60%
                          block_state  87/32366  0.27%
                          entity_type   4/158    2.53%
                          block_entity  1/49     2.04%
                          menu          3/25    12.00%
```

**The answer moves 55x between the most and least flattering unit.** That
spread is the finding, and it is the same defect as §15's "a floor is a number
under a comparator" moved from the comparator to the denominator.

### Instruments left behind

* **`ci/vanilla-reach-census.py`** — the census. `--explain` prints the unit,
  the provenance and the exclusions; `--weak` prints the instrument that
  collapsed; `--housekeeping-emitted` prints the other bound on UNIT 1;
  `--json` writes it machine-readable; `--selftest` runs in `ci/t0.sh`.
* **`ci/jarcallgraph.py`** — a pure-Python method-level call graph over the
  jar. 7,434 classes, 53,685 methods with code, ~3 s, no javap, no decompiler.
  Constant pool + Code attributes + `BootstrapMethods`, so a method reference
  (`this::tickNonPassenger`) is a real edge. Reusable for any "what does
  vanilla call from here" question.
* **`ci/reach-census-n6-20260811.txt`** — the census output, tracked, and the
  artefact all five `corpus-reach-*` figures pin themselves to.

### Killed as nulls — do not re-run these

* **The packet-witness call-graph instrument.** "For each tick section, which
  clientbound packets can it construct, and were any emitted?" It is built, it
  runs, and it has **no resolving power**: devirtualised `invokevirtual` on
  `Entity`/`Level`/`BlockState` makes vanilla's tick graph one giant SCC, so
  `wardenBrain` reaches 51 of 139 packet types and only **6** sections come out
  dark against the two sharp rules' **41**. Kept under `--weak` with the
  collapse stated. **Do not rebuild it, and do not try to rescue it by tuning
  the devirtualisation** — the over-approximation is the sound direction and
  the collapse is what soundness costs here.
* **Exclusive reach as a size measure for a tick section.** Attributing to each
  section the methods only it can reach gives 0 for 108 of 120 sections, for
  the same SCC reason. Measured, useless.
* **`EntityType.<clinit>` as a source of entity-type-to-class mappings.** In
  26.2 it holds three instructions; the registrations are elsewhere. The census
  does not need it — the per-mob rule resolves `wardenBrain` -> `minecraft:warden`
  through Mojang's own naming convention and verifies the result against the
  `minecraft:entity_type` registry.

### What this cannot say — read §21.4 before quoting any of it

No floor exists and cannot: a census is not a comparison, so ORACLE rule 6 has
no premise. The reproducibility spread stands in its place and only
`sound_event` moves (27..28 per run, 29 pooled). Items are not censused at all
— `ItemStack` is inside the digested `rest` tail. Every positive tier is
*presence*, never coverage, and the 79 `unclassified` tick sections are 79
things this census knows nothing about.

### The single next item, and the result that closes it

**Turn the three cheapest rows of §21.5's work order on.** Rows 6-8 —
spawning, random ticks, weather — are dead because `ci/t2-gameplay.sh` sets
`spawn_mobs false`, `random_tick_speed 0` and `advance_weather false`, and
those gamerules are *ours*. They are one harness change from being reachable,
which is a precondition nothing else on the list has.

It closes when `ci/vanilla-reach-census.py` re-run against a new ensemble
reports fewer than 8 sections in `PROVABLY OFF` **and** the corpus floor is
re-stamped on that ensemble — because turning random ticks back on is exactly
the change that will make scenarios non-reproducible, and the floor is the
instrument that says by how much. Do not turn them on without re-taking the
floor in the same branch; §13 and §15 are what that costs otherwise.
