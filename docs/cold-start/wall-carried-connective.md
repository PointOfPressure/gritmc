# Cold start — branch `wall-carried-connective`

### COLD START — branch `wall-carried-connective` off `b1dc71ad8` (`shape-three-position-extract`, main 2208), 2026-08-19

`shape-three-position-extract` built the three-position extract, made
`carried + connective + refused == sensitive` exhaustive for the first time,
and then named exactly two runs it could not take because the box was handed
back. This branch takes them.

```text
ci/shape-three.sh --only WallBlock    the carried/connective split, ~5.9e10 probes
ci/shape-three.sh                     the whole table, twice, cmp'd
ci/shape-three.sh --check             a third derivation against the committed artefacts
ci/shape-three-byclass.py             the per-class table, out of the artefact
ci/shape-three-byclass.py --cross     census verdict vs extract verdict
ci/wall-reads-check.sh                how many positions WallBlock.updateShape reads
ci/wall-reads-falsify.sh              11 arms, every one red on a mutation
ci/shape-reads.sh                     the census, now with a third count
ci/refusal-ledger.py --check          six populations, for the first time
```

---

## 1. What this branch measured, and what it re-derived first

Nothing here is carried from §83. The gate directory is `~/rustmc-wall`, which
this branch owns; `data/vanilla` was generated in it by this tree's own dumpers
and never taken from the shared `~/rustmc/data/vanilla` or from `grit/repo`,
whose copy predates `randomlyTicking`. The freshly generated
`block_props.json` carries `randomlyTickingStates`, which is the cheap way to
tell a live generation from an inherited one.

The census was regenerated **before** anything on this branch was touched and
came back byte-identical to the committed one:

```text
  103482 sensitive pairs: 12748 carried, 11621 connective, 79113 refused
  debt 48405 = world 48081 + 324 that need a real level
```

That byte-identity is what makes everything below a before/after rather than
two unrelated measurements, and it is the same defence §83.1 set up for its
own branch against the same named failure.

## 2. §83.10's bytecode reading, re-derived and then given a diff

§83.10 settled `WallBlock` — 41,472 pairs, more than the other four large
classes together — from `javap` rather than from a sweep, because the machine
was not available. Re-derived here independently before the sweep was run:

  * `updateShape` dispatches three ways. `DOWN` to `Block.updateShape`, `UP` to
    `topUpdate`, everything else to `sideUpdate`.
  * `topUpdate` reads **no** position. Zero `getBlockState`, zero `BlockPos`
    navigation. The block above *is* the changed neighbour on that direction.
  * `sideUpdate` has exactly **one** `BlockPos.above` and exactly **one**
    `getBlockState`. The class has six `getBlockState` call sites in total and
    the other five are all in `getStateForPlacement`, which is not on this path.
  * No `Direction.values()`, no iterator, and **no backward branch** in
    `sideUpdate`. It has four `goto`s and all four jump forward — they are the
    four ternaries. The class's only iterator is in `lambda$makeShapes$0`.
  * From this tree's own `block_props.json`: 32 blocks ending `_wall`, 324
    states each, 10,368 wall states, times 4 horizontal directions = **41,472**,
    the refusal census's `WallBlock` count to the pair.

`ci/wall-reads-check.sh` is that reading with a `diff` behind it and
`ci/wall-reads-falsify.sh` is eleven mutations proving it can refuse. Both are
in T0 behind the same jar guard as `ci/probe-absences.py`, and for the same
reason: the jar is pinned, so the reading will not drift on its own — it is
**repinned**, and on that day a sentence in a roadmap section is not a thing
anybody re-derives.

The falsifier earned its place on its first run: the `connectsTo` arm
substituted a `getValue` that `connectsTo` does not contain, wrote a file
identical to its input, ran the checker on the real disassembly and got rc 0.
Any falsifier that only compares return codes would have called that a passing
arm. Each arm now `cmp`s its mutation against the original first and calls a
no-op a failure.

## 3. `--only` published five figures and asserted none of them

A whole-table run re-derives `update_shape.json`'s refusal set with its own
copy of `ShapeGolden`'s narrow probe and refuses if the count is not the
table's. `--only` exited before that block. So §83.9's four measured rows —
25,740 pairs — were counts of whatever this harness's discovery found, with
nothing anywhere comparing them to the set the table refuses. The numbers were
right. Nothing established that they were, and `--only` exists precisely to
publish a figure *instead of* paying for the whole run, which makes it the path
that most needs the check rather than the one that can do without it.

The per-class denominator is not in `update_shape.json` — that table carries
one total — so the check reads `byClass` out of
`ci/checks/shape-refusal-census.json`, and a missing census is a skip with a
printed reason rather than a silent pass.

**And `--only` runs once.** A whole-table run generates twice and `cmp`s;
doubling a forty-five minute per-class measurement to check itself is not that
trade. It does not have to be: the whole-table run measures every class already
under a *different work partition*, so its `byClass` entry is an independent
second measurement of the same number. `--only` compares against it wherever a
whole-table census exists, and says out loud that it is a single run where one
does not.

## 4. Two traps this branch hit that are not in §83's list

Both are appended to `docs/cold-start/shape-three-position-extract.md` as traps
5 and 6, because that is where somebody running these generators will look.

**5. Never write a script the gate directory is executing.** Bash reads a
script in chunks and keeps a byte offset, so a script blocked in a long command
has the rest of itself unread on disk. The 45-minute `--only` run is exactly
the thing you do other work during, and the other work was editing that script
and copying it over. The edit added fourteen lines to the *header* — above the
read position, which is the case that feels safest and shifts every byte after
it. Caught within a minute by restoring the original bytes; it would have
surfaced as a syntax error blamed on a line nobody touched, 45 minutes in.

**6. Generating the extract reddens T0.** `ci/refusal-ledger.py --check` calls
an optional table that the tree *has* and the ledger does not record
`DRIFTED`, and `ci/t0.sh` exits 1 on that. `ci/checks/table-refusal-ledger.json`
has recorded five tables or four in every commit of its life and has never
recorded `update_shape3.json`, while §83.7 describes it as "the ledger's sixth
population" — true of the code, false of the artefact. An optional artefact
whose presence reddens the gate is an artefact nobody keeps, and the population
that fell out is the one whose generator costs twenty minutes, which is where a
red gate is most likely to be resolved by deleting the output.

## 5. The third count the census was missing

§83.3's finding is that `discoveryLateRefusal = 0` was about **discovery**,
three documents read it as "the refusal set is complete", and the regeneration
added 2. The prose was corrected everywhere. The output was not: a reader still
finds `missedAtDefaults = 164` with `lateRefusalAtDefaults = 0` beside it and
nothing measuring the pass that actually added the 2.

`ShapeReadsGolden` reports `resolutionRefusalAtDefaults` now — pairs the
1,196-default breadth drops that reach past the harness in *resolution*, where
no default reached it in *discovery*. §83.3 got its 2 by subtracting two
re-derived tables, which is weaker than it reads: it needs both runs to exist,
a tree holding one cannot check it, and whatever else moved between the two
runs is charged to the widening without being asked.

It is deliberately **not** on `ci/refusal-ledger.py`'s must-be-zero list. It is
2, and the count beside it that genuinely is 0 is about the other pass; filing
them together is the confusion §83.3 exists to correct.

## 6. A comment can require a twenty-minute table

`ci/gen-gameplay-tables.sh` derives its required set by grepping `crates`,
`tools` and `plugins` for a **quoted** `"data/vanilla/<name>.json"`. It does
not distinguish a comment from a call — verified by construction, not argued: a
Rust doc comment reading `/// see "data/vanilla/update_shape3.json"` puts that
table in the required set, and every tree on the box then pays about twenty
minutes at setup for a table nothing opens.

`tools/goldengen/vanilla/ShapeGolden.java` already names it in a comment inside
that scan root, in **backticks** — one character from the quoted form. So does
`ci/gen-gameplay-tables.sh`'s own arm. Both arms are commented "the first
consumer is a one-line change", which is true and is also the description of
the accident. The script now names the line that required an expensive optional
table, so a real consumer reads as source that opens it and an accident reads
as a comment.

<!-- RESULTS: the per-class table and the WallBlock split are appended below
     when the whole-table run lands. -->
