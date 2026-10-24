# Cold start — branch `shape-three-position-extract`

### COLD START — branch `shape-three-position-extract` off `f2641a6` (`tables-refusal-ledger`, main 2182), 2026-08-18

Two things happened on this branch and only one of them was the brief.

```text
ci/shape-three.sh                 build data/vanilla/update_shape3.json (~20 min, jar)
ci/shape-three.sh --check         re-derive and refuse if the table or census drifted
ci/shape-three.sh --only <Class>  one block class, cost-first; checked against the census
ci/shape-golden.sh                update_shape.json — discovery is exhaustive now
ci/shape-reads.sh                 the census, which now measures BOTH discovery breadths
ci/refusal-ledger.py --check      six populations, and the partition is exhaustive
ci/refusal-ledger-falsify.py      34 arms, hermetic, in T0
ci/probe-absences.py              the structural pins, now over four harnesses
```

---

## 1. The extract, and the one sentence that makes it not a census

`ci/checks/shape-refusal-census.json` re-asks `update_shape.json`'s refusals
against a level that answers **every** position with a filler block. That level
**invents the world**, which is why nothing under `crates/` may read what it
produces and why `ci/shape-reads.sh`'s header says so twice.

`data/vanilla/update_shape3.json` is not that. Its level answers **three**
positions — the subject, the neighbour that changed, and the one further
position the rule was *measured* to read — and **throws for a fourth**, exactly
as `ShapeGolden` throws for a third. Nothing is invented, so it is a table a
port may read the day a port exists. Nothing reads it yet.

**The third position is not chosen.** `ShapeGolden`'s narrow probe records the
offset it was asked for and threw on, and that offset is what the extract
widens to. There is no list of "positions walls care about" anywhere in this
branch, and there must not be.

The distinction is pinned mechanically, not by prose. `ci/probe-absences.py`
counts the ways out of each harness's `stateAt`:

| harness | ways out | throws | what that means |
|---|---:|---|---|
| `SignalGolden` | 1 | yes | subject only |
| `ShapeGolden` | 2 | yes | subject + neighbour |
| `ShapeThreeGolden` | 3 | yes | subject + neighbour + the measured third |
| `ShapeReadsGolden` | 3 | yes | **both**: throws with no filler, invents with one |
| `UseGolden` | 1 | no | answers air for anything it was not given |

`ShapeReadsGolden` needed a third category (`DUAL`). It has a throw *and* an
extra way out, so `IMMUNE` would call it broken and `EXPOSED` would call it
broken the other way. Losing its throw silently turns the narrow probe into an
inventing one, and every count it asserts against `update_shape.json` goes on
balancing while describing a different extract.

## 2. Two breadths, both stated, because one cross product is not runnable

The answer is now a function of two block states. Sweeping both exhaustively is
32,366 x 32,366 per pair and nobody can run it. So:

  * the **neighbour** axis is exhaustive — all 32,366 states — because that is
    the axis `update_shape.json` is keyed on and a bitmap over it is what a
    consumer reads;
  * the **third position** axis is the 1,196 block defaults, `ShapeGolden`'s
    own discovery breadth. `extraBreadth` says so in the output.

What makes it affordable is that the third axis does not need an exhaustive
neighbour sweep per value. Two third-position states with the same answer
vector over the 1,196 default neighbours are one **extra class**, and only one
member of each class pays the exhaustive sweep. Classes are compared by
content — `Arrays.equals` over the kept vectors — and not by a hash, because
"collides about once in never" is the kind of claim this repo writes a check
for rather than relies on.

**Cost was measured before it was believed.** `--only FenceGateBlock`: 768
pairs, 1,150,111,488 probes, all 768 closed at two third-position classes each.
That is 1.5M probes per pair, so the whole position-refused population is about
1.1e11 — minutes on the box, against the 6.3e11 an exhaustive third axis would
have cost.

## 3. The other thing: 164 pairs were in no count at all, and now they are

`carried + connective + refused == sensitive` has been checked for a while and
it balanced. It was never **exhaustive**. Discovery swept the 1,196 block
defaults, so a pair that reacts to a non-default neighbour and to no block's
default was dropped before the exhaustive pass: no row, no refusal, no place in
any of the three terms. `ShapeReadsGolden` measured it — 90,878 pairs dropped,
164 of them move, 160 being the double-chest pairing rule, which reacts to a
chest already `type=left` or `type=right`.

A partition that adds up while real rows sit outside every term in it is this
repo's own name for the defect, and it was in the oldest and largest table
here. The fix is discovery's loop bound: it reads all 32,366 states now.

It is nearly free, because discovery early-exits. A pair anything moves stops
at the candidate it always stopped at, so the sensitive population pays
nothing; only the genuinely inert pairs read 32,366 instead of 1,196.

### The prediction about the refusal set was wrong by two, and that is the finding

The census reported `discoveryLateRefusal = 0` — no non-default neighbour
reaches past the harness where no default did — and that was read as "widening
adds 164 to `sensitive` and **0** to `refusedPairs`". It was written into a
commit message before the regeneration ran. The regeneration says the 164 split
**32 carried, 130 connective, 2 refused**.

`discoveryLateRefusal` is a statement about **discovery** and not about
**resolution**, and the two passes refuse at different moments. Discovery stops
at the first candidate that moves the answer; resolution then sweeps every
state from the beginning and can meet a throw discovery stopped short of. A
pair discovery never found cannot reach resolution at all, so no measurement of
discovery could have said what resolution would do with it.

Stated generally, because it is not about this table: **a count of what one
pass refuses is not a bound on what a later pass will refuse.** Only a
partition re-derived after the breadth change says what happened, which is why
the tables were regenerated rather than reasoned about.

### The measurement stays after the fix, and this is the important part

`missedAtDefaults` is what the **1,196-default** breadth would still be
dropping. It is a property of the jar, not of a loop bound, so it must not go
to zero when the loop bound changes. `discoveryMissed` is what the **current**
breadth drops — structurally 0, and checked to be 0 by
`ci/refusal-ledger.py`. Two different questions, both answered, and a number
that can only ever be zero is not left standing in for the one that cannot.

`ci/refusal-ledger-falsify.py`'s `census-not-exhaustive` arm fires on
`discoveryMissed = 164`. That is not a round number chosen to look plausible;
it is what the jar said before the widening.

## 4. What to run, and what not to bother running

**`--only` asserts now, and did not.** A whole-table run re-derives
`update_shape.json`'s refusal set and refuses if its own discovery disagrees
with the table's `refusedPairs`. `--only` exited before that block, so the four
measured rows of §83.9 — 25,740 pairs — were counts of whatever this harness
found, with nothing comparing them to the set the table refuses. They were
right; nothing established it. The per-class denominator is not in
`update_shape.json`, which carries one total, so the check reads `byClass` out
of `ci/checks/shape-refusal-census.json`, and a missing census is a skip with a
printed reason rather than a silent pass.

`ci/shape-three.sh` is about twenty minutes and **is not in T0**, for the same
reason `ci/shape-reads.sh` is not: a five-minute JVM run per gate buys nothing
that the cheap half does not already catch. The cheap half is
`ci/refusal-ledger.py --check`, which reads the extract as an optional sixth
population and ties it to `update_shape.json` — if the extract's re-derived
refusal set is not that table's, it widened a different set and every closure
figure in it is about something else.

`ci/gen-gameplay-tables.sh` has an arm for `update_shape3.json` and **it does
not fire**, because the required set is derived from what the sources mention
and no source mentions it. Same shape as `use_item_on.json`. The arm is there
so the first consumer is a one-line change.

## 5. What the extract closed, and the one class answered without running it

| class | pairs | verdict |
|---|---:|---|
| `WallBlock` | 41,472 | **answered** (bytecode); carried-vs-connective unmeasured |
| `StairBlock` | 19,200 | **refused** — a fourth position, 4,800 each on the four horizontals |
| `FireBlock` | 3,072 | **refused** — a fourth position, at 2 probes/pair |
| `NoteBlock` | 2,700 | **connective** — 8 and 20 third-position classes |
| `FenceGateBlock` | 768 | **closed**, 1,152 rows |

**A third position is necessary and not sufficient.** It answers `WallBlock` and
`NoteBlock`, closes only `FenceGateBlock`, and the two classes that refuse do so
on a *fourth* position whose offset is now named rather than guessed.

`WallBlock` was settled from the pinned jar with `javap` rather than a sweep,
because "how many positions does the rule read" is a property of the bytecode:
`updateShape` sends `DOWN` to the base, `UP` to `topUpdate` (which reads
nothing — the block above *is* the changed neighbour there), and the four
horizontals to `sideUpdate`, which has exactly one `BlockPos.above()` and one
`getBlockState`, with the four cases unrolled and no loop in the chain. The
arithmetic agrees to the pair: 32 wall blocks x 324 states x 4 horizontal
directions = 41,472, the census's whole `WallBlock` count, so the refusals are
precisely the horizontals. What that does **not** settle is `closed` vs
`connective`, and `ci/shape-three.sh --only WallBlock` (~5.9e10 probes, ~45 min
idle) is the run that would. It was not taken.

## 6. Four traps this branch walked into, in the order it hit them

Written down because every one of them cost a re-run, and none of them is
mentioned anywhere else.

**1. `ci/t0-remote.sh --sync` overwrites the gate directory's committed
artefacts.** `ci/checks/` is inside the rsync. Regenerate
`ci/checks/shape-refusal-census.json` on the box, sync, and you have silently
replaced it with the version in your worktree — then `ci/refusal-ledger.py
--check` compares a fresh table against a stale census and prints five
INCONSISTENT lines that look like a real drift. **Copy an artefact out of the
gate directory before the next sync, not after.** Cost here: one twelve-minute
census run.

**2. `ci/refusal-ledger.py --write` in a tree that lacks an optional table
drops that population from the ledger.** The committed ledger recorded five
tables; this gate directory had four, so `--write` recorded four, and any tree
that *does* generate `use_item_on.json` would then read DRIFTED. The
`optional-unrecorded` arm exists for exactly this and it is about the check, not
about the person running `--write`. **Generate every optional table the
committed ledger records before re-writing it.**

**3. Calling a golden generator directly does not stamp its output.** Only
`ci/gen-gameplay-tables.sh` stamps (and `ci/use-item-on-golden.sh`, which was
taught to). `ci/check-use-probe-falsify.sh`'s `real-table` arm reddens T0 for an
unstamped table and the red points at `ci/gen-gameplay-tables.sh` — which, for
a table it does not generate, cannot repair it. `ci/shape-three.sh` stamps its
own output for that reason, even though nothing checks the stamp yet.

**4. A gate directory needs the *whole* required set, not the tables you care
about.** `ci/readiness-falsify.sh` boots the server, the server wants
`data/vanilla/flat_preset.json`, and T0 fails with "a readiness proof's
fixtures are not what the server prints" — which says nothing about a missing
table. Run `ci/gen-gameplay-tables.sh` and let it derive the 19.

And one that is not a trap but a mechanism worth knowing: **raising a floor in
`ci/checks/*.toml` is a published-figure change.** `min_cites` is a
`gate-floor` entry in `ci/figures.toml`, so moving it 15 -> 34 reddened T0 with
`FIGURES RED [DOC-DRIFT] refusal-citations-floor` until the figure was updated
too. The citations ledger's floor had rotted because nothing watched it grow;
the figures ledger's entry *for that floor* could not, because rule 8 reads the
declaration out of the file on every gate. One ledger guards a number and the
other guards the guard.

## 7. Where the numbers came from

Everything below was re-derived on this tree and nothing was transcribed. The
tables were regenerated from `oracle-work/vanilla-26.2` into a gate directory
this branch owns (`~/rustmc-shape`), never against the shared
`~/rustmc/data/vanilla`. The committed census was verified byte-identical
against a fresh run **before** any generator on this branch was touched, which
is what makes the before/after a comparison rather than two unrelated numbers.
Clean room throughout: no tracked file here names another server.

---

## 8. A fifth trap, found on `wall-carried-connective` — never write a script the gate directory is running

Added 2026-08-19. It cost nothing here only because it was noticed within a
minute, and it would have been indistinguishable from a harness bug.

The workflow in §6 is "edit in the worktree, copy into the gate directory, run
there". `ci/shape-three.sh --only WallBlock` is a forty-five minute run, which
makes it the obvious thing to do *other work* during — and the other work was
editing `ci/shape-three.sh` and copying it over.

**Bash does not read a script into memory. It reads it in chunks and keeps a
byte offset**, so a script blocked in a long-running command has the rest of
itself still on disk, unread. Replace that file and the offset it resumes at
now points somewhere else in the new text. The edit here added about fourteen
lines to the header comment — *above* the read position, which is the case that
feels safest and is the one that does damage, because every byte after it
shifts. The run would have resumed in the middle of a statement.

It does not produce a clean failure. Depending on where the offset lands you
get a syntax error attributed to a line you did not touch, or a fragment that
parses and runs, and either way it arrives forty-five minutes into a run whose
output you were about to quote as a measurement.

**So: while a gate directory is executing something, nothing in it gets
written.** Edit in the worktree, and copy in between steps rather than during
them. If a patched script must be tested while a run is in flight, give it
another name — the offset problem is about the path, not the contents.

The same argument covers `ci/t0-remote.sh --sync` mid-run, which is trap 1 with
a wider blast radius, and it is why both of these are about the same rule: a
gate directory is a running process's working set, not a place to stage edits.

## 9. A sixth trap, and it is why the extract is not in the ledger: **running the generator reddens T0**

Found 2026-08-19 on `wall-carried-connective`, and it explains an artefact this
branch left behind that §83.7 describes as if it were not there.

§83.7 says `update_shape3.json` "is the ledger's sixth population". That is true
of `ci/refusal-ledger.py`, where `update_shape3` sits in `OPTIONAL` with a
derivation, a `byClass` breakdown and six crosschecks. It is **not** true of
`ci/checks/table-refusal-ledger.json`, the artefact `--check` compares against.
That file records **five** tables, and `git log` on it says it has recorded
five or four in every commit of its life. The extract has never been in it.

That is not an oversight anybody could have avoided by being careful, because
the state it leaves behind punishes the careful path:

  * `ci/refusal-ledger.py --check` treats an optional table that the ledger
    records and the tree lacks as a **skip**, printed out loud. Correct, and it
    is the `optional-skipped` arm.
  * The reverse — a table the tree **has** and the ledger does not record — is
    `DRIFTED`, rc 1. Also correct, and it is the `optional-unrecorded` arm.
  * `ci/t0.sh` runs `ci/refusal-ledger.py --check` and turns a non-zero into
    `T0 RED`.

So **generating the extract makes T0 red in that tree**, and the only way back
to green is to delete the table or to re-write the ledger. Reproduced with the
falsifier's own fixture machinery rather than argued:

```text
  ledger recorded: set_placed_by, signal, update_shape, use_without_item
  tree also has:   update_shape3.json
  --check rc = 1
      DRIFTED  update_shape3.json
          population: ledger None, tables 5
          refused:    ledger None, tables 1
```

And re-writing the ledger is trap 2 with a loaded chamber: `--write` in a tree
that lacks an optional table the committed ledger *does* record drops that
population, so the repair for one optional table silently un-records the other.
The branch that documented that trap is the branch whose ledger came back with
`use_item_on.json` restored and `update_shape3.json` still absent — which is
exactly the shape the trap produces.

**The order that works, and it is the only one:**

1. generate every optional table the committed ledger records — today that is
   `ci/use-item-on-golden.sh`;
2. generate the extract, `ci/shape-three.sh`;
3. `ci/refusal-ledger.py --write`, and then check that the five populations
   already recorded did not move, because `--write` rewrites all of them;
4. `ci/refusal-ledger.py --check`, expecting six.

Stated generally, because it is not about this table: **an optional artefact
whose presence reddens the gate is an artefact nobody keeps.** The skip path
was designed with care so a tree without the table is not punished; the
having-it path was not, and the population that fell out of the ledger is the
one whose generator is the most expensive to run — which is to say, the one
where a red gate is most likely to be resolved by deleting the output.
