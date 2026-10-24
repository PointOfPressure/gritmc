# `worldgen-land` — refused, and the difference that matters

**Read this before trying to land `worldgen-land`.** 116 patches is a large
enough prize that the obvious move is to make the gate accept them. Four rules
stand in the way and **not one of them can be satisfied without either raising
a ceiling or inventing a fact.** That is the difference between a branch that
is not ready and a branch that would have to be lied about, and it is why this
is a refusal rather than a queue item.

## What is NOT wrong with it

Stated first, because every previous diagnosis of this queue was that the
branches conflict, and they do not.

* **It replays clean.** `worldgen-land` rebases onto `origin/main` with **zero
  conflicts**, and stacks cleanly on top of `boxcopy` as well. The merge
  drivers resolve every `ci/figures.toml` and `docs/WORKLIST.md` append with
  nothing left conflicted.
* **It compiles.** `cargo build --workspace --all-targets` is clean with it
  applied, before and after stacking.
* **It was NOT-FF, and that is all the gate ever said.** `main` was 54 commits
  ahead, `ci/handoff.sh` exits 3 in that state before measuring anything, and a
  queue of exit-3s reads exactly like a queue of conflicts.

So nothing about the code is in question. Every remaining failure is in the
figure and claim ledgers.

## The reds are the branch's own, not a stricter checker's

Worth settling before anyone re-runs it, because `boxcopy` tightens
`ci/check-figures.py` and the obvious suspicion is that the tightening caused
this. It did not. Running **main's own** `ci/check-figures.py` against the tree
with `worldgen-land` applied returns the same four classes and the same
counts — 3 DOC-DRIFT, 8 PROVENANCE-NO-COMMIT, 1 ECHO-UNQUALIFIED, 1
SELF-ARTEFACT. `main` itself is GREEN, so they arrive with the branch.

## Already repaired — start from four rules, not fourteen

Two of the six classes were facts rather than judgements and are done, on
`land-train`:

* **three `claim_count` values.** `bench: narrow the unattributed ~14%` adds a
  section to `docs/BENCH.md` citing three published figures in new prose;
  `+7.03%` and `+4.08%` go 2 -> 3 mentions and `+22.44%` 2 -> 4. The count
  records how many times a claim appears in its `doc`, so the ledger was stale
  and the document was not. Same repair the branch already made twice for the
  same reason, `55e6ffab6` and `6988ddc24`.
* **eight provenance shas**, recovered with `ci/figure-provenance.py`, each
  stamped with the witness count behind it.

That took the failing rules from 14 to 4.

## The four that remain, and why each is a refusal

### 1. `PROVENANCE-NO-COMMIT` — `nether-r16-20260722-anchored-floor-n5`

`ci/figure-provenance.py` recovers a provenance by running `check-figures`
rule 1 backwards through history. For this entry its four witnesses return
**four different commits**:

| witness | commit | date |
|---|---|---|
| `residual` | `cdf4702e6` | 2026-08-10 |
| `denominator` | `53f62e362` | 2026-08-06 |
| `claim` | `1b8e9088e` | 2026-08-12 |
| `artefact_claim` | `b2abb8177` | 2026-08-12 |

The tool reports a **single** witness behind its answer, and that answer
disagrees with the entry's own ledger date. Rule 18 exists because "a sha that
was typed rather than copied reads exactly like one that was" — so picking one
of four and writing it down produces a record indistinguishable from a real
one, which is worse than the red. **Only the lane that took the measurement
can say which tree it was taken on.**

### 2. `ECHO-UNQUALIFIED` — 18 against a ratchet of 14

`docs-figures-ledger` drove this 24 -> 14 and recorded in its own cold start
that the remaining 14 are all in `docs/DETERMINISM.md`, vocabulary only, and
"deliberately left to whoever is already in that file". `worldgen-land` is in
that file and adds four more unqualified mentions of a withdrawn figure's
claim.

**Repair:** print the disqualifier beside each of the four mentions, or cite
the figure by id instead of restating its value. **Not** the ceiling — a
ratchet that rises to admit the work that broke it is not a ratchet.

### 3. `SELF-ARTEFACT` — 9 against a ceiling of 7

`worldgen-aggregate-n5` and `worldgen-aggregate-asledger-n-mixed` both set
`artefact` equal to their own `doc`, so rule 2 re-reads the file rule 1 already
read and cannot fail independently of it — the measurement half of the guard
goes vacuous.

The complication is that there is **nothing honest to point `artefact` at**.
Both entries are arithmetic over other ledger entries and say so:
`"arithmetic over ... No new measurement."` There is no script, table or census
that produced them. And the commit that registered them, `1b8e9088e`, says in
its own subject that there is **"no defensible aggregate"**.

**Repair:** either give the aggregate a real artefact — a script or table that
carries the claim and can disagree with it — or withdraw the two entries.
Both are the worldgen lane's call about its own figures.

### 4. `DANGLING-SECTION-REF` — 9 against a ceiling of 8

`ci/check-doc-claims.py`, not `check-figures`. All nine point into worldgen's
own renumbering: `DETERMINISM §46.2` from `ci/figures.toml:3275` and
`docs/cold-start/worldgen-aggregate.md:189`, `§46.5` from `docs/PARITY.md:464`,
`§36`/`§37` from `ci/be-census-12345.sh`, `§38` from
`docs/cold-start/worldgen-sulfur-r18.md`. The branch renumbered its DETERMINISM
sections twice while rebasing — `224c03830`, `c79dc6e79`, `fbb9b0e85` — and the
citations did not all move with the headings.

**Repair:** move the citations with the headings. Mechanical, and the only one
of the four that needs no judgement about a measurement.

## The trap waiting for whoever does the provenance work

A provenance sha recovered on a **replayed** stack names a commit of that
stack. Rebase again and every stamp names an object that is no longer the one
meant — and nothing local says so, because `sha_resolves` asks `git cat-file
-e`, which is object existence and not reachability. The orphan still resolves
in the worktree and the rule stays green there; it is the gate copy that fails,
whose object store ships only what the branch reaches.

This happened for real during this lane's run and cost a gate. **Recover
provenance on the branch before it is replayed, or on `main` after it lands,
never on a stack that is still going to move.**
