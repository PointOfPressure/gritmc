# Cold start — branch `useitemon-gates`

### COLD START — branch `useitemon-gates` off `srvlevel-gate` (56 commits vs `main 2208`), 2026-08-18

What to run, and in what order.

```text
export PATH=$HOME/.cargo/bin:$PATH   # or ci/gen-gameplay-tables.sh dies 12 generators in
ci/placed-golden.sh                  # set_placed_by.json    (~35 s, jar)
ci/use-golden.sh                     # use_without_item.json (~90 s, jar)
ci/use-item-on-golden.sh             # use_item_on.json      (~4 min, jar, run twice)
ci/server-level-gates.py             # the 15 gates and what each one hides
ci/server-level-gates.py --check     # refuse if a gate, a population or a stamp moved
ci/server-level-gates-falsify.py     # 35 arms on shadow trees (~5 min, jar)
```

**Run `ci/placed-golden.sh` first and it now works in a fresh tree.** It did
not before: it runs `ci/server-level-gates.py --emit` before it writes
anything, and `--emit` refused on a missing `set_placed_by.json` — the file
that dumper is the only producer of. The error named
`ci/gen-gameplay-tables.sh`, which dispatches straight back to the same
script. Nothing `--emit` writes needs a table, so it does not ask for one any
more.

`data/vanilla/` is gitignored and generated; the two provisioning traps in
`docs/cold-start/probe-answering-environment.md` are both still live.

**The `useItemOn` dumper is not expensive and the number that said it was is
gone.** `docs/cold-start/server-level-gate.md` priced it at ~25 minutes, which
is why it was left as the optional fifth population. Measured on this tree:
**3m48s** for the whole script — `--emit`, two runs of 105 million probes, the
determinism `cmp` and three oracles. That is the figure that was holding the
sweep back, and it was an estimate.

`ci/gen-gameplay-tables.sh` had the right order of magnitude in a comment the
whole time — *"92 million probes and ~95 seconds a run"* — so the 25 was not
even consistent with the tree it was written in.

## T0, and the two reds that predate this branch

`ci/t0.sh` is red on `origin/srvlevel-gate` for two reasons unrelated to either
lane's subject. One is fixed here and one is not.

* **`ci/probe-absences-falsify.py` crashed in an arm's own setup** —
  `AssertionError: CommandBlock not found once`. Four arms named a ledger entry
  that branch retired when it closed `CommandBlock` by putting the gate in its
  row. Fixed here; they point at `setPlacedBy.StructureBlock` now.
* **`ci/figure-provenance.py --check` fails on 56 figures.** Identical on
  `origin/srvlevel-gate` and here — `56 figure(s) re-derived, 56 whose derived
  commit is not in their provenance`, rc=1 on both. Not touched: the two
  figures this branch owns are not among the 56, and `ci/check-figures.py` is
  GREEN. Whoever picks that up wants a lane of its own.

Everything downstream of those passes.

---

## The one-paragraph version

`docs/cold-start/server-level-gate.md` closed the `useWithoutItem` side, put
the gate in the row, and then **measured** the next tranche instead of
guessing at it: 7 more rules over 56 registry blocks on `useItemOn`, all seven
silent. This branch swept them. `false_cells` — the number to quote, a row
that exists and is wrong in a column the table has — goes **404 to 3,197**
over 85 blocks, and 2,784 of the 2,793 new ones are signs. Two things that
were believed about this area turned out to be wrong and both are load-bearing:
`SignBlock.useItemOn` does not announce, and one gate is not in the rule at
all.

## The seven, what each hides, and whether this project can take the branch

```text
  reachable        PumpkinBlock(1)   -> false in `writes`, `sound`, `stackAfter`
                   SignBlock(48)     -> false in `stackAfter`; sign.rs and
                                        blockentity.rs hold sign text today
  needs-subsystem  TntBlock(1)       -> no PrimedTnt, no explosion
                   VaultBlock(1)     -> no vault block entity, no trial loot
                   BeehiveBlock(2)   -> no bees, no honeycomb drop
                   CampfireBlock(2)  -> no campfire cooking; every cell refused
                   DecoratedPotBlock(1) -> particles only; no column anywhere
```

The verdicts are no more uniform than on the other side, and two of them stamp
nothing for reasons worth keeping apart from each other.

## `SignBlock.useItemOn` does not announce, and it is 2,784 of the 2,793

`SignBlock.useWithoutItem` calls `Util.pauseInIde`, `UseGolden` arms the pauser
against itself, and that one call is the whole reason 1,344 sign cells in
`use_without_item.json` are refusals rather than rows. It is the rule that made
this entire class visible.

`SignBlock.useItemOn` has no such call. Same class, same 48 blocks, the other
half of the same click, and this half is silent. The branch not taken is a
plain answer, not an alarm:

```text
  95-117  if (!(applicator != null && mayBuild) || waxed) SUCCESS else CONSUME
```

so `minecraft:oak_sign` reads `CONSUME` for most items and `SUCCESS` for the
sixteen dyes, the ink sac and the glow ink sac — **right in `result`**, because
`applicator != null` and `mayBuild` are both computed *above* the gate and are
real. What is on the far side is `SignApplicator.tryApplyToSign` and the
`ItemStack.consume` after it, so `stackAfter` is false on all 2,784.

"SignBlock is the one vanilla tells you about" is true of one method. It is not
true of the class, and it was being carried as though it were.

## `TntBlock`'s gate is not in `TntBlock.useItemOn`

The `instanceof` the walk finds hides a game-rule read and an overlay message.
Neither is a column, so on that reading `minecraft:tnt` is an incomplete row
and nothing more. It is not.

```text
    0-35   if (!stack.is(FLINT_AND_STEEL) && !stack.is(FIRE_CHARGE)) super
   36-44   if (prime(level, pos, player)) {
   47-61       level.setBlock(pos, AIR, 11)          <-- unreachable here
   62-113      consume or hurtAndBreak, awardStat
          } else if (level instanceof ServerLevel) { <-- the gate it finds
  129-162      if (!gameRules.get(TNT_EXPLODES)) { overlay; return PASS; }
          }
  163      return SUCCESS
```

`TntBlock.prime` is **itself** `instanceof ServerLevel` at its own offset 0 and
returns `false` when it is not one, so the `setBlock` at 47 never runs — and 47
is on the caller's **fall-through**, 69 bytes before the gate. The subtraction
cannot find it: `prime` is called at 41, before the branch, so it is reachable
from both sides and cancels, correctly, by the same rule that makes the join
work. **The effect is one level down a call, and a walk of one body is the
wrong shape for it however carefully it is done.**

`gated_callees` detects it and `--check` refuses an unledgered one. It is a
**detection and not a derivation**: `hides` for the callee is what `prime`
performs itself, and what the *caller* loses on the strength of a constant
`false` is a dataflow question nothing here answers. The ledger owes prose.

**Measured over all four probe methods in 26.2: one.** The first cut reported
**four** — `BeehiveBlock`, `PumpkinBlock` and `TntBlock` on `useItemOn`, and
`SweetBerryBushBlock` on `useWithoutItem` — which was `super.useItemOn`
resolving to `Block.useItemOn` and the simple-name lookup landing back on the
rule itself. The walk seeing its own reflection, and only on the rules that
call `super`.

(The commit that added the detection says "three of the seven and all eight of
the swept rules". That was wrong when it was written: `useItemOn` was not in
`PROBES` yet, so the swept side reported `SweetBerryBushBlock` alone. Four in
all, re-derived here by removing the self-exclusion and counting.)

## The live bug, and it is on the server rather than in the table

`crates/rustmc-server/src/world_send.rs::handle_use_item_on` asks
`useWithoutItem` three ways and **never asks `useItemOn` at all**. Nothing
under `crates/` opens `use_item_on.json`. For most blocks that costs nothing,
because the rule declines and vanilla falls through to the same place. For one
it is a wrong write:

```text
  right-click minecraft:tnt holding minecraft:flint_and_steel
    vanilla   the block becomes air, a PrimedTnt spawns, the item takes
              damage, SUCCESS, and the item rule at offset 223 never runs
    here      no useItemOn arm; minecraft:tnt has no row in
              use_without_item.json either, so the block passes and
              crate::itemuse::flint_and_steel places minecraft:fire on the
              clicked face
```

The TNT survives with a fire sitting on it. **The item rule is ported
correctly and wins by default**, which is what makes this worse than the
un-ported cases — shears on a pumpkin does nothing here, and nothing is at
least not a lie about the world.

`PumpkinBlock` is the closer analogue of `SweetBerryBushBlock` in the *table*:
`setBlock` inside the gate, `return SUCCESS` outside it, and the cell reads
`SUCCESS, consumes: true, sound: false, writes: []` where vanilla carves the
pumpkin, drops the seeds, plays `PUMPKIN_CARVE` and damages the shears. It is
not a live bug **only** because no module reads this table. That is the
difference between the two halves, and it is worth being exact about: on
`useWithoutItem` the false column was consumed by `crate::useblock`; here the
false column is consumed by nobody, and the defect is the arm that is missing.

## What the row representation needed, beyond copying it

The `gated` column carries over unchanged — vocabulary is the table's own
column names, a refused cell is never stamped, rows carry `serverLevelGate`,
the dumper counts independently of the walk and `--check` refuses if the two
disagree. Three things did not carry over.

**A column belongs to a table, not to a token.** `opens` is a field of
`use_without_item.json` and of neither other; `stackAfter` is a field of
`use_item_on.json` and of neither other; **`blockEntityWritten` is a field of
neither of the last** — `UseItemOnGolden` does not track it, and four of these
seven rules hide a block-entity write. `COLUMNS` is keyed by probe. A row
telling a consumer to disbelieve a field it has never carried reads as a defect
in the table rather than a limit of the harness.

**A guard needs an item axis.** Five of the seven branch on the held stack
before they reach the gate. A state-only vocabulary answers every one of them
`all-states`, which for `minecraft:pumpkin` stamps a grid 1,536 of whose cells
never reach the branch. The grammar is a `&&` conjunction now — `item=<ids>`,
`item-any`, `item-unknown`, `<prop>=<enum>` beside the integer form — and it
is evaluated **twice**: by the dumper against the real `BlockState` and
`ItemStack`, and by the check against the strings the table printed. That is
what keeps the two routes independent now that the swept-probe coincidence is
gone.

**A cell is not a state.** `use_item_on.json` prints a `default` plus `except`
groups, so counting states called `minecraft:pumpkin` one cell and
`minecraft:oak_sign` 1,344 where the figures are 2 and 2,784. One recorded
answer is one cell — the default is one however many items fall into it, and a
group is one however many items share it. Deliberately **not** this table's own
`cells` summary, which counts (state, item) grid points differing from their
default: 44,078 of them, and the two are not interchangeable.

## The two rules that stamp nothing, and why they are not the same

`BeehiveBlock` is the one whose cell **did not exist until it was stamped**.
The gate was not a false cell there; it was a missing one. `honey_level >= 5` reaches the gate, the
shears branch is *inside* it and the glass-bottle branch *outside*, so the
bottle path runs correctly and the shears path does nothing at all — and a cell
that does nothing is exactly the row's default. `UseItemOnGolden` names an item
only where its answer differs from the default, so shears never got a group.

**The stamp is what brought it into being.** `gated` is part of `cellJson`, so
a stamped cell no longer equals the default it would have merged into: the
table went 44,078 (state, item) cells to **44,086**, and the eight are exactly
`minecraft:shears` on the four honey-full states of each of the two hives.
That is the column doing the one thing a `reads`-style annotation cannot —
recovering a cell rather than qualifying one.

The contrast now sits inside one row:

```text
  minecraft:beehive[facing=north,honey_level=5]
    minecraft:glass_bottle -> SUCCESS, sound, honey_level=0, a honey bottle
    minecraft:shears       -> TRY_WITH_EMPTY_HAND, nothing,
                              gated: ["sound", "stackAfter"]
```

`CampfireBlock` stamps nothing for a different reason: all 64 of its states are
**refusals** on `recipeAccess`, and a refused cell is never stamped. Its guard
says `item-unknown` — the branch is
`recipeAccess().propertySet(CAMPFIRE_INPUT).test(stack)` and no route here can
answer it — so the zero is arrived at from both sides rather than one covering
for the other. `DecoratedPotBlock` has two independent reasons as well: its
`item-any` guard rejects the only cells that survive, which are the 16
empty-hand ones, and its gate hides `sendParticles` alone, which is a column no
table has.

**Both are ledgered rather than left out**, and `cells_absent` is a declared
field with an arm behind it: a rule with columns whose guard admits no cell has
to say so, because a rule that stamps nothing and a stamping mechanism that
resolved no block are indistinguishable from the outside. Nothing carries it
today — the beehive was the candidate and the stamp gave it eight cells — and
it stays because the next population will have one.

## What holds it honest

`ci/server-level-gates-falsify.py` has **35 arms**, twelve of them new. The one
worth naming is `stamp-stripped`: every arm before it compared the jar to a
ledger, and two files can agree with each other while the table says something
else entirely. That arm rewrites what the **dumper wrote** — strips every
`gated` array out of a table on a shadow tree — and requires the walk and the
stamp to stop agreeing. Nothing had ever exercised the cross-check from that
side.

`guard-item-axis` is the other one: it sets `PumpkinBlock`'s guard to
`all-states`, which makes the check derive two stamped cells where the dumper
stamped one. It is the arm that makes the item axis load-bearing rather than
decorative.

**And two for the thing that went wrong here.** Sweeping the fifth population
reddened every tree that does not have its table — the seven ledger entries
read as `VANISHED`, which is a claim about the *jar*, and the totals were
declared for fifteen rules. That was live for four commits and is exactly what
`server-level-gate.md` predicted. Optional-probe entries are skipped rather
than vanished now, and there are two totals blocks: `[meta.totals]` is the two
required probes and is checked in **every** tree, `[meta.totals_full]` adds
`useItemOn` and is checked only where it was measured.
`optional-absent-green` and `optional-absent-subtotal` are the arms.

```text
  with the table     15 gated rule(s), 14 silent over 90 blocks, 3197 stamped
  without it          8 gated rule(s),  7 silent over 34 blocks,  404 stamped
```

## What is still silent after this

```text
  1  StructureBlock     out-of-scope   sets the block entity's author, which no
                                       column in any of the three tables records
  1  JukeboxBlock       out-of-scope   needs a placement input no table sweeps
  1  PitcherCropBlock   no-op          `0: return`
```

Unchanged, and still on `useWithoutItem`. On `useItemOn` there is nothing left
silent: `--census` and `--check` now derive the same seven and the vocabulary
is closed at 0 unmapped calls.

**What is not swept at all is `setPlacedBy`'s neighbours** — `ci/probe-absences.py`
counts the overrides with no row and says 3, none of them the probe's
environment. That number is unmoved by this branch.

And the 164 discovery-missed pairs in `docs/SERVER.md` still need a
discovery-breadth change and a new cross product. Not cheap, and unrelated.
