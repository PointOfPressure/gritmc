# Cold start — branch `ci-figures-ledger`

### COLD START — branch `ci-figures-ledger`, 2026-08-13

**Ledger and checker only. No generator source was touched, no vanilla
reference was generated, no dump was run, no number moved, and the corpus lock
was never taken.** Every figure in this file is read out of `ci/figures.toml`
or out of a Python run over it.

#### The item's second half is refuted, and the refutation is cheap to re-check

Briefed to land eight `claim_count` bumps ahead of the unmerged
`worldgen-aggregate`, on the grounds that its edits to eight existing lines in
`ci/figures.toml` are the one shape a union merge cannot resolve. **Both halves
of that are wrong on this tree.**

* **`ci/figures.toml` has no merge driver at all.** `.gitattributes` carries
  exactly one line — `docs/DETERMINISM.md merge=appendonly` — and its own
  comment says `docs/WORKLIST.md` deliberately has none. The ledger gets git's
  default 3-way merge, which resolves an edit to an existing line fine unless
  the other side touched that same line. There is no union resolver in the
  path to be defeated.
* **The eight bumps cannot land ahead of the branch that earns them.**
  `claim_count` is not a bookkeeping number, it is *the count of occurrences in
  the document*, and rule 1 recomputes it on every T0. All eight are **correct
  on `main` today**. They become correct only once `worldgen-aggregate`'s
  `docs/DETERMINISM.md` §33 adds the extra occurrences (§35 at the time this
  was written; `worldgen-aggregate` renumbered off it 2026-08-16 — 695a66d5 —
  because `worldgen-deliver-once` had landed §35 first with unrelated content.
  Renumbered here to match, so this reference resolves to the section that
  will actually merge rather than to the one that no longer will). Applied
  alone to this branch's tree, `ci/check-figures.py` returns exactly **8
  DOC-DRIFT failures and nothing else** — verified, not argued:

  ```
  git archive HEAD | tar -x -C $W    # bump the eight, then:
  cd $W && python3 ci/check-figures.py     # FIGURES RED — 8 failing rule(s)
  ```

  So the bumps are inseparable from §33 and were **not** landed. Any branch
  that lands them without the doc text reds T0 until §33 merges.

The eight, for whoever does merge that branch:
`nether-r16-n6-matched` 2→3, `end-r16-n6-exact` 6→7,
`ow-r16-12345-floor-n15` 6→7, `ow-r16-12345-admissible-topmaterial` 1→2,
`ow-r16-12345-level-n15-lakeice-before` 1→2,
`ow-r16-12345-level-n15-lakeice` 1→2,
`ow-r16-12345-level-n15-iceberg-before` 1→2,
`ow-r16-12345-level-n39-iceberg` 1→5.

#### What the r=16 README figure became: WITHDRAWN

`readme-restates-ow-r16-admissible` republishes `ow-r16-admissible-n2`'s number
to the block — 29,605 of 107,053,056, same pair, same dump — and that entry was
withdrawn 2026-08-10 for an uncontrolled chunk order. It inherits the
withdrawal. **Nothing replaces it.** The seed-20260722 r=16 overworld window
carries no exactness figure at all: the order-matched successor
(`ow-r16-admissible-matched`, 2,450) sits inside the 2,202..5,263 the same
instrument charges an innocent vanilla run, so it is corroboration and not a
level either.

**`README.md` was not edited, and that is the finding rather than a shortcut.**
Its prose has been correct since 2026-08-10 — it prints the figure as *not
quotable*, names the *uncontrolled chunk order*, prices it at 117,628 blocks on
a candidate that is vanilla, and leads with the indistinguishability claim
instead. Only the machine-readable half disagreed, for three days. That is the
half a consumer enumerating this ledger reads, and the half nobody re-reads.
It also means rule 4's `UNQUALIFIED` check passed unchanged the moment the flag
flipped: the disqualifier was already printed beside the number.

`quotable_override = "no-mask"` was dropped rather than moved. It was sound on
its own terms and is the wrong instrument: an override answers *is there a
blind spot bigger than the residual*, the disqualifier is *was the chunk order
set on both sides*, and no answer to the first is an answer to the second.

#### The five predicates, red-then-green

`ci/figures-predicate-red-20260813.txt` holds every run and the commands to
reproduce them. Commit `65895aca` adds rules 9-12 and nothing else, so it reads
`main`'s ledger and prints **4 failing rules**; the next commit greens it. Rule
13 came later, out of the audit, and its red is recorded against `main`'s
`docs/PARITY.md`.

| # | rule | what it asks |
|---|---|---|
| 9 | `RESTATED-QUOTABILITY` | an entry republishing its `compare_to` target's number may not be `quotable` when that target is not |
| 10 | `RESTATED-WINDOW` | for that same relation, `compare_keys` must cover **every** window key the target records |
| 11 | `FLOOR-UNDECLARED` | a quotable entry of a rule-6-applicable kind must carry a `floor` or a `floor_note` |
| 12 | `DUMP-ORDER-UNRECORDED` | ratchet: the count of quotable chunk-order-sensitive entries with no `dump_order` may not rise |
| 13 | `VERDICT-MARKER` | a withdrawn figure's claim may not be printed on a line carrying the ✅ these docs use for "settled" |

**Restatement is detected numerically, and that choice is load-bearing.** Same
`percent`, or same `residual` over the same `denominator`. The tempting test —
an identical `claim` string — is refuted by an entry already in the ledger:
`use-table-probe-moves-20260812` repeats its predecessor's sentence verbatim
and is an independent re-measurement from a different generator, which
deliberately drops `generator` from its `compare_keys` because the generator is
what changed. Matching on the sentence would demand it compare on the one key
its whole finding is about. Both shapes are green controls in the falsifier.

Rule 10 is what caught the *reason* rather than the symptom: the bad entry's
`compare_keys` omitted `dump_order`, the single key its source was disqualified
on, so rule 5 compared the two rows on every axis except the fatal one and
reported agreement. Its own window omitted `dump_order` entirely.

#### Rule 13 came out of the audit, and it is the same fault pointing the other way

**`docs/PARITY.md` carried a ✅ on a figure it had withdrawn twenty lines
earlier.** One paragraph reads *"That figure was taken with an uncontrolled
chunk order and is no longer quotable; §15, §16"*; the next `✅` paragraph opens
*"If only one number fits, it is 99.972345% ... and a floor of 29,560 that the
residual clears by 45 blocks."* A reader landing on the second has found an
endorsement, and the first does not reach them.

**Rule 4 was green on it the whole time, and that is the lesson: proximity is
not attachment.** Rule 4 is satisfied by the disqualifier appearing anywhere
within 4,000 characters, and it was. So rule 13 binds the marker vocabulary
these documents already use — the same file writes `99.972345% 🚫 uncontrolled
chunk order` in its own table two hundred lines earlier. Six ✅ against nine 🚫
in `docs/PARITY.md`, three against four in
`docs/WIDE-WINDOW-R16-ALL-DIMENSIONS.md`: an existing convention, now binding.

Tested across the whole ledger before being written: **it fires on exactly one
line.** The ✅ was moved, not the number — the paragraph is kept under a 🚫
withdrawal header, per this file's own rule that a number is retired by being
labelled and not by being overwritten. `99.972345%` still appears 5 times in
`docs/PARITY.md`, so `ow-r16-admissible-n2`'s `claim_count` is untouched.

Note what this says about the fault I was sent for: **the flag was wrong in the
ledger and the prose was right in `README.md`; here the flag is right and the
prose is wrong in `docs/PARITY.md`.** Same defect, both directions, three days
apart. Neither side is the trustworthy one by default.

#### The audit — 53 quotable entries, how each one earns it

The checker now prints this census on every run, green included:

```
FIGURES quotable basis: floor-note 9, kind-exempt 31, override 2, rule-6 10
```

* **rule-6 10** — carry a `floor` and a `residual`, so rule 4 adjudicates them.
* **override 2** — `end-r16-n6-exact`, `nether-r7-basalt-core`. Rule 4 fires and
  is overridden explicitly, with a reason, and they are enumerable.
* **floor-note 9** — five `corpus-reach`, three `table-audit`, plus
  `ow-r16-blindspot-depth`. Each says in the file why no floor exists.
* **kind-exempt 31** — `worldgen-floor`, `vanilla-self`, `corpus-floor`,
  `bench`, `bench-target`, `gate-floor`. By construction, not convenience: a
  floor has no floor, and CPU-seconds have no reference ensemble.

**One further entry failed the predicate**, out of the 53:
`ow-r16-blindspot-depth` was quotable with no `floor` for rule 6 to fire on and
nothing saying why — its quotability rested on nothing the checker could reach.
It gets a `floor_note`: none exists in rule 6's sense because **its denominator
is the floor**. The 29,560 positions are exactly those where the two references
disagree, so 16,650 / 12,795 / 115 is an exact partition of the floor set and
not a residual measured against one. Caveated as a property of the N=2 pair.

#### What cannot be checked mechanically, and why — now explicit in the file

* **11 quotable chunk-order-sensitive entries record no `dump_order`.** Frozen
  by `[meta] max_quotable_without_dump_order = 11`, listed by name in the
  checker's own red output. Four are `det-gradient-*`, where the harvest order
  IS the variable under test; four are nether/End rows whose references predate
  `HARVEST-ORDER` stamping and honestly say `harvest = "unrecorded"`; three are
  overworld. **The number is not lowered by inference** — ORACLE rule 30, an
  assertion is not a stamp, and for several the honest value is recoverable
  only by re-diffing against a stamped reference, which is a measurement.
* **"Does the named source still exist at the named commit" is NOT viable on
  this ledger, and the reason is a convention rather than rot.** 50 of 82
  entries name no commit at all. Of the 32 whose `provenance` holds an 8-hex
  token, 15 are seeds and dates (`20260722`, `20260811`) — requiring one `a-f`
  character leaves 23 real ones, all of which resolve. But the convention is
  `branch X off main <sha>`, naming the **base the measuring branch forked
  from**, so the artefact the figure cites did not yet exist there in **17 of
  those 23**. The sha records where the measurement stood, not where its
  artefact lives. Do not wire this in as written.
* **`provenance` is free text and the checker still never reads it.**
  `RESTACKED ... NOT RE-MEASURED` appears 7 times and means nothing mechanical
  — unchanged from `worldgen-aggregate`'s reading, and not fixed here.

#### The falsifier

`ci/check-figures-falsify.py`, wired into `ci/t0.sh` beside the checker. 13
cases, 13 behaved, ~0.5 s, no build and no box, hermetic fixture in `mktemp -d`.
Necessary because **the fix removed the only entry that violated each of rules 9-13** —
they now pass on an empty set, which is this project's twenty-one-times defect
class. `[meta] min_restatements = 1` keeps the relation alive; the falsifier
proves the rules still bite. Two of the thirteen are green controls and they are
the argument: a rule that reds on a mere comparison against a withdrawn figure
would be indistinguishable from a correct one on this ledger and wrong on the
next.

#### What I killed as a null — do not re-run these

* **Do not try to land the eight `claim_count` bumps ahead of
  `worldgen-aggregate`.** Measured above: 8 DOC-DRIFT, red until §33 merges
  (renumbered from §35 — see the note above).
  There is no ledger design that avoids this — `claim_count` is a function of
  the document, so a doc-editing branch must own its bump.
* **Do not add a commit-existence predicate over `provenance`.** 17 of 23 fail
  by convention, as above.
* **Do not make `dump_order` a hard `REQUIRED_WINDOW` key.** 26 of the 52
  chunk-order-sensitive entries lack it and filling them in is a measurement.
  The ratchet is the affordable half.
* **Do not match restatements on the `claim` string.** The ledger already holds
  the counterexample and the falsifier locks it in as a green control.
* **Do not "fix" `ow-r16-blindspot-depth` by inventing a floor for it.** Its
  denominator is the floor; a floor for it would be vanilla's irreproducibility
  inside a set defined by vanilla's irreproducibility.

#### The single next item, and the result that closes it

**Retire the eleven `dump_order` holes, cheapest first, lowering
`max_quotable_without_dump_order` by one per retirement.** The four
`det-gradient-*` rows are the cheap end and may be pure bookkeeping — their
`harvest` values (`rows`, `perchunk`) are the variable under test and the
`dump_order` may be recoverable from `ci/decorate-order-sweep.sh`'s own logs
without generating anything. The four `harvest = "unrecorded"` nether/End rows
are the expensive end and probably close as *withdrawn* rather than as
recorded, since ORACLE rule 30 says an unstamped reference's order is
recoverable but not known.

It closes when the ceiling reaches a number the next agent cannot lower without
booting vanilla — at which point that number is the honest size of this
ledger's unconditioned set, and belongs in `docs/PARITY.md` beside the figures
rather than only in `[meta]`.

#### A STANDING T0 RED ON `main` THAT IS NOT MINE — check before you blame a branch

**`ci/t0-remote.sh` exits 1 on this branch, on `ci/check-use-probe-falsify.sh`,
and it does so identically on a pristine `main`.** Verified rather than
assumed: `git archive main` into a scratch remote dir, same `data/vanilla`,
same result — `9 passed, 1 failed`, the failing case being `real-table`.

Root cause, and it is a provisioning defect rather than a code one:
`ci/gen-gameplay-tables.sh` writes a `data/vanilla/use_without_item.json` with
**0 `probeStances`**, while `ci/checks/use-probe-dependent.toml` pins
`stances = 12` — the value `golden-useitemon-axis` set from the full
`ci/use-golden.sh` probe sweep. The checker says so in its own words: *"the
dumper swept 1 stances, ci/checks/use-probe-dependent.toml pins 12. A sweep
that stops sweeping reports every row as constant."* The canonical
`~/rustmc/data/vanilla/use_without_item.json` has 0 stances too.

So **every tree provisioned the way `docs/AGENT-BRIEF.md`'s HARD PRECONDITION
mandates reds T0 here**, because that block requires `gen-gameplay-tables.sh`.
It goes green only in a tree that has run the full probe sweep. That is the
AGENT-BRIEF's own warned-about class — a gate whose verdict depends on how the
tree was provisioned — and it belongs to whoever owns
`ci/checks/use-probe-dependent.toml`, not to this branch. This branch touches
no `.rs`, no `.java`, no generator and no `ci/checks/`.

Everything this branch is responsible for is green in that same run:
`FIGURES GREEN`, `check-figures-falsify: 13 passed, 0 failed`,
`check-doc-claims: 65 claims, 0 collisions, 0 drift`,
`MERGE-APPENDONLY-FALSIFY GREEN`, `FLOOR-CENSUS-FALSIFY GREEN`.
Log: `/home/sayer/grit/t0-ci-figures-ledger-20260813.log`.

#### Files

`ci/check-figures.py` (rules 9-13), `ci/check-figures-falsify.py` (new, in
`ci/t0.sh`), `ci/figures.toml` (two entries fixed, two `[meta]` ratchets added),
`docs/PARITY.md` (the ✅ moved off a withdrawn figure; no digits changed),
`ci/figures-predicate-red-20260813.txt` (the red-then-green record), `ci/t0.sh`.
