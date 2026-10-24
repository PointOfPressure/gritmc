# Cold start — branch `tables-refusal-ledger`

### COLD START — branch `tables-refusal-ledger` off `f82b2eb` (main 2182), 2026-08-18

The four tools this branch adds, and what to run when.

```text
ci/refusal-ledger.py            every refusal in data/vanilla, counted, with its unit
ci/refusal-ledger.py --check    refuse if a population moved (T0, needs the tables)
ci/refusal-ledger.py --cite     refuse if a sentence quoting one has gone stale
ci/refusal-ledger-falsify.py    24 arms on tables built to be wrong (T0, hermetic)

ci/probe-absences.py            every rule the jar declares that a table has no row for
ci/probe-absences.py --check    refuse if an absence is unexplained or stale (T0, needs the jar)
ci/probe-absences.py --cite     the same tie, on the absence figures
ci/probe-absences-falsify.py    11 arms on shadow trees (T0, needs the jar)

ci/shape-reads.sh               regenerate the update_shape refusal census (~5 min, jar)
ci/shape-reads.sh --check       re-derive and diff against the committed census
```

Both `--check`s and both falsifiers are wired into `ci/t0.sh` behind guards on
what they need, and both guards **print when they skip**. A tree that says
nothing about the ledger and a tree whose ledger is fine must not look the same
in a log.

---

## Read this before you quote a number from any of them

**There are two refusal populations and they read identically when quoted.**

* `ci/unported-inventory.py` counts the **crate's** debt — 85 markers, 32 gap
  clusters, 48 gaps expanded.
* `ci/refusal-ledger.py` counts the **tables'** — what four golden generators
  said they could not answer.

Neither is a proxy for the other and they diverge in both directions.
`signal.json` refuses **1,215 `redstone_wire` states that `crate::power`
hand-ports outright** (`power.rs:561` — the table answers the other 81 and the
port carries the `shouldSignal` field itself), so a table's refusal count read
as the server's debt is wrong by three orders of magnitude. `update_shape.json`
refuses **79,113 pairs that no marker in the crate mentions**, so the crate's
markers read as the tables' debt hide every one of them.

**And the unit is part of the number.** `update_shape.json` keys on
(state, direction) pairs — 194,196 exist, 103,482 are sensitive. The other
three key on states. "79,113 refused" and "1,250 refused" are not on the same
axis. The ledger prints the unit on every line for this reason; quote it too.

## What the four tables refuse, on the pinned 26.2 jar

```text
  update_shape.json      79,113 of 103,482 pairs    76.45%
  signal.json             1,250 of  32,366 states    3.86%   1,215 wire + 35 block entity
  use_without_item.json     160 of  32,366 states    0.49%   128 beds + 32 daylight_detector
  set_placed_by.json         12 of  32,366 states    0.04%   both pistons, getChunkSource
```

Regenerate all four with `ci/gen-gameplay-tables.sh --force` before quoting;
every figure above came out of a fresh run, each identical across two runs.

## Three things that will trip you

**1. A refusal is not a debt.** `ShapeGolden`'s discovery pass breaks out of
the candidate loop on the *first* throw, so a pair is refused before anything
establishes that its answer ever moves. `ci/checks/shape-refusal-census.json`
takes the 79,113 apart: 48,081 need a world, 22,010 are a single-alternative
destruction rule invariant under air, stone and water, 5,609 are the connection
family counted twice, 3,089 are inert over **all** 32,366 neighbour states, and
324 survive widening. **The debt is 48,405.**

Do not read a `recoverable` verdict as a row you may use. The census's level
*invents* the world past the neighbour, which is precisely the plausible answer
this project refuses; it is a claim about the jar, not an extract. Nothing under
`crates/` may open it, and nothing does.

Two more things the census says that the refusal count cannot. **164 pairs
were in no count at all** — and are not any more, as of
`shape-three-position-extract`. Discovery swept only the 1,196 block defaults,
so a pair that reacts to a non-default neighbour and to no default was dropped
before the exhaustive pass: no row, no refusal, no place in `carried`,
`connective` or `refusedPairs`, while
`carried + connective + refused == sensitive` balanced anyway. Discovery reads
all 32,366 states now and the 164 landed 32 carried, 130 connective, **2
refused**. `missedAtDefaults` keeps measuring what the old breadth would drop,
because a number that can only be zero once its cause is fixed cannot fail any
more, and `ci/refusal-ledger.py` checks `discoveryMissed == 0` — which is the
difference between balanced and exhaustive.

**`discoveryLateRefusal = 0` did not mean what this file said it meant.** It
read "the refusal set is complete at discovery breadth", and widening the
breadth added 2 refusals. Discovery stops at the first candidate that moves the
answer; resolution sweeps from the beginning and can meet a throw discovery
stopped short of, and a pair discovery never found cannot reach resolution at
all. A count of what one pass refuses is not a bound on what a later pass
will refuse.

**2. A refusal is loud; an absence is silent, and the absences are bigger.**
`UseGolden.TableLevel.getBlockState` answers `Blocks.AIR` for a position it was
not given and `getBlockEntity` answers `null`. A rule that reads either takes
the empty branch and the probe emits **no row at all**.

`DiodeBlock.setPlacedBy` is the whole lesson in seventeen bytes:

```java
if (shouldTurnOn(level, pos, state)) level.scheduleTick(pos, this, 1);
```

`shouldTurnOn` reads the input side of the world; the harness world is air; so
there is no row for `minecraft:repeater` or `minecraft:comparator`, and a
consumer reading "no row, no tick" ships a repeater that never wakes when it is
placed already powered. **The table and the server hold the same absent input,
so they agree** — and agreeing is all any check that compares them can measure.

That is why `ci/probe-absences.py` derives its trigger from the **jar** and
never from the table. 22 overrides with no row, 102 registry blocks, **20 of
the 22 the probe's environment rather than the jar**. Exactly one is a real
no-op (`PitcherCropBlock` is `0: return`).

**Those were the counts on the morning of 2026-08-18 and they are history.**
Branch `probe-answering-environment` built the environment the same afternoon:
the derivation now says 3 overrides with no row, **3 registry blocks,** 0 of
them the environment. Read `docs/cold-start/probe-answering-environment.md`,
`docs/cold-start/server-level-gate.md` and `docs/PARITY-ROADMAP.md` §80 and §87
before quoting anything in this paragraph.

`ShapeGolden` and `SignalGolden` *throw* for an unheld position, so their
absences are measurements and they cannot have this defect. That split is
asserted in `ci/probe-absences.py` by brace-matching the body of the method
that answers a position and counting the ways out — not by grepping for the
throw, because the falsifier's `structure-immune` arm defeats a grep by
*inserting* `if (true) return AIR;` above it and leaving the substring intact.

**3. Do not transcribe a count from a doc.** Five figures across three
documents and one generator header were stale when this branch started, and
the largest of them had three different values in three places for the same
table. `useblock.rs` said 199, its own cause rows summed to 231, and the table
carried 160 — because when the `Components not bound yet` cause closed, the new
*total* was written into the `environmentAttributes` *cause* row. Re-derive
with the ledger; it counts every population off the rows **and** off the
generator's own summary and refuses when the two disagree.

## What is not cheap, so nobody re-scopes it as a probe fix

* **The 48,405 `update_shape` pairs need a world — and the three-position
  extract now exists, so read `docs/cold-start/shape-three-position-extract.md`
  before re-scoping this row.** 54,724 of the 79,113 read exactly one position
  past their own neighbour, and 43,021 read `0,1,0` and nothing else.
  `ci/shape-three.sh` builds `data/vanilla/update_shape3.json` for exactly that
  set: a level that answers three positions and **throws for a fourth**, so
  unlike this census it invents nothing and a port may read it. It is about
  twenty minutes and it is not in T0; the cheap half is
  `ci/refusal-ledger.py --check`, which reads it as an optional sixth
  population and ties its re-derived refusal set to this table's.
  **`ci/checks/table-refusal-ledger.json` has to actually record it, and until
  2026-08-19 it never had** — five tables in every commit of its history, while
  `ci/refusal-ledger.py` carried the derivation. A tree that generates the
  extract against a ledger that does not record it reads `DRIFTED`, and
  `ci/t0.sh` turns that into `T0 RED`, so running the generator reddened the
  gate. See trap 6 in `docs/cold-start/shape-three-position-extract.md` for the
  reproduction and for the only ordering that repairs it without dropping
  `use_item_on.json` on the way past. By class the
  population is concentrated to the point of being almost a single job:
  `WallBlock` is 41,472 pairs on its own, `StairBlock` 19,200, and with
  `FireBlock` and `NoteBlock` that is 84% of the whole refusal population.
* **The 100 hidden blocks need a block entity in the harness.** Making
  `TableLevel.getBlockEntity` throw is one line and it changes what ten
  existing rows mean — the level's own comment says so and says `UseItemOnGolden`
  already took the other choice. It belongs to whoever owns that table.

  **What that choice is worth is measured, not argued.** Generate
  `data/vanilla/use_item_on.json` (`ci/use-item-on-golden.sh`, ~95 s, nothing
  requires it — and as of 2026-08-18 it stamps its own output, because until
  then running it by hand left an unstamped table, reddened T0 through
  `check-use-probe-falsify`'s `real-table-use-item-on` arm, and pointed the
  reader at `ci/gen-gameplay-tables.sh`, which does not generate this table) and the same registry through the other half of the same
  right-click gives **2,593 block-entity refusals over 2,576 of 4,537 states,
  56.78%, one cause and no others** — against `use_without_item.json`'s zero
  refusals and 90 silently absent blocks.

  **Both halves of that sentence were closed on 2026-08-18 by branch
  `probe-answering-environment`, and it is the same repair that closed them.**
  The level builds the real block entity now, so the ask is *answered* rather
  than refused on one side and taken as an empty branch on the other:
  `use_item_on.json` refuses **64** states, all on `recipeAccess`, and records
  4,319; `use_without_item.json` has a row or a refusal for all 90. Read
  `docs/cold-start/probe-answering-environment.md`. `ci/refusal-ledger.py` reads it as an
  optional fifth population and skips it out loud where it is not present.
* **One sky model is owed and it is counted three times in three units.**
  `update_shape.json` refuses 96 pairs on `environmentAttributes`,
  `use_without_item.json` refuses 128 states on it, and `crate::useblock`
  carries `getEffectiveSkyBrightness` and the `SUN_ANGLE` arithmetic written
  out and deliberately not run against the same missing model. Neither sighting
  alone says it is one gap. It is the highest-leverage item in this ledger and
  it is owned by the lane that owns the sky model, not by this one.

## Where the numbers came from

Everything above was re-derived on this tree rather than quoted: all four
tables regenerated from `oracle-work/vanilla-26.2` with `ci/gen-gameplay-tables.sh`,
each identical across two runs; the census over 1,129,212,893 probes, also
identical across two runs; the override sets from `javap -p -cp <jar>` over
`net/minecraft/world/level/block/**` read out of the jar's zip index, with no
extraction step. Clean room throughout — no tracked file here names another
server.
