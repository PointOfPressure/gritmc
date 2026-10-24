# Cold start — branch `work-method`

### COLD START — branch `work-method` off `main` e2fc07600, 2026-08-19

**Area: the working method itself — last run's lessons made mechanical instead
of remembered.** Every item below ships with a falsifier shown failing on the
unrepaired state before it passes, and every falsifier carries at least one
negative control. Nothing here needs a build, a jar or the box; the whole
addition to T0 costs about 23 seconds, 22 of which is the comment-guard scan.

## What is on this branch

**1. `unit` and `question` on every registered figure, enforced.**
`ci/figures.toml` gains a closed `[units]` vocabulary of 26 units and a `unit` +
`question` on all 169 figures; `ci/check-figures.py` requires both.
`ci/figure-units.py` has four rules — UNIT-NOUN, QUESTION-SHAPE,
QUESTION-COLLISION, FLOOR-N — and `ci/figure-units-falsify.py` has 15 cases.

**2. `docs/FINDINGS.md`**, seeded from `origin/assess-2026-08-19`, with
`merge=appendonly` so concurrent lanes never conflict. Two cases added to
`ci/merge-appendonly-falsify.sh` using the repo's OWN `.gitattributes`.
`ci/findings-index.py` reds a branch that records a status change in `docs/`
prose and does not index it; 15 falsifier cases.

**3. `hooks/agent-brief-gate.sh`** — the commit count is a sizing check, not a
target, and the hook is now tracked. `ci/brief-gate-falsify.sh`, 13 cases.
`MAX_AGENTS` is **2**, and what matters is that the DERIVATION is in the file
with both its inputs and their dates, not the constant. This number has been
wrong twice for one reason: 8 and the 4 before it were budget decisions, and
the 4 that briefly replaced them **inherited** a 22-branches-a-day drain rate
from `docs/ASSESSMENT-2026-08-19.md` §4.1 instead of measuring one. Today's
measurement is 9.4-11.6 branches/day (GREEN 107.9 min n=4, RED 26.1 min n=7),
against 6.3 branches per lane-day, giving 1.5-1.8. **Two lanes measured that
drain rate on the same day and got figures 2x apart**; the hook names which one
it rests on rather than averaging them. **Batching was refused** — do not carry
"6 with batching" as an available alternative; the `p = 0.8217 at N = 157`
behind it is not reproducible (28 logs survive, 11 verdicts, p = 0.3636).

**4. Class sweeps.** `ci/check-comment-guards.py` brought onto `main` from
`origin/t1-positive-controls`, plus `ci/shell-footguns.py` for `pkill -f` and
`grep -c … || echo 0`.

**5.** `docs/AGENT-BRIEF.md` points at the findings index and states why an
agent that ends its turn to wait pays a full context re-read.

## THE THREE THINGS THAT WERE FOUND, NOT BUILT

Each was found by a check on the day it was written, and none was known.

* **`ci/handoff.sh` published the literal words `0 skips` in its GREEN and
  MERGED verdicts from a counter that is structurally always 0.** The tree's
  only two shell emitters of `skipping: no vanilla` are called by nothing, and
  the comment naming `ci/dump-flat-preset.sh` as a live emitter was wrong twice
  over — that script prints a different string and then `exit 1`s. The
  guarantee is gone; the counter stays as an arm that would fire if an emitter
  ever appears.
* **Three `det-gradient` figures had byte-identical windows AND identical
  provenance with numbers two points apart** (97.474607%, 99.621506%,
  99.492021%). `taskset -c <one core>` and SerialGC were in
  `docs/DETERMINISM.md`'s table and in no field of the ledger. Now
  `window.jvm`.
* **`ow-r16-12345-census-n6` quotes an N=5 floor beside an N=6 residual**,
  deliberately and conservatively, recorded only in a prose `reason`. Now
  `floor_n = "5"` with a `floor_basis`.

## WHAT THE UNIT CHECK CANNOT CATCH — read this before trusting it

A defect in the DEFINITION rather than the label is invisible to it and to any
string comparison. `frozen` counted checks with an empty history beside checks
that agreed every build; its label was accurate, its arithmetic was correct,
and it was one of the six corrections this check was built for. It reds on
nothing.

The rule for that class is **`docs/ORACLE.md` rule 31 — a null result and an
empty result must be different words** — and it is stated rather than gated
because no checker reaches it. FLOOR-N reaches 13 of 102 floors, the rest
having no registered source, and the green line prints that number so it can be
watched. The no-prose count is printed for the same reason.

## COORDINATION DEBTS THIS BRANCH LEAVES

* **`ci/checks/reading-tax.toml` still owes `docs/FINDINGS.md` a
  `tier = "action"` entry, and `## Where everything else is` owes it a row.**
  Neither file exists on `main`; both land with
  `origin/docs-waiter-t2-blindspot`. `docs/FINDINGS.md` is wired from the brief
  as a paragraph and NOT as a numbered `## Read first` item precisely so
  `ci/reading-tax.py`'s cross-check does not demand a `core` entry — at 100+
  lines and growing, counting it would eat the ceiling's headroom alone.
* **Measured post-merge reading tax: 695 → 710 against a ceiling of 720.** Both
  additions to `docs/AGENT-BRIEF.md` applied to the sibling branch's rewritten
  copy, measured with that branch's own `ci/reading-tax.py`. Ten lines of
  headroom remain; the next hard rule added to the brief has to demote
  something.
* `ci/install-hooks.sh --check` reds when the installed hook has drifted from
  `hooks/agent-brief-gate.sh`. It is a copy and not a symlink so the gate cannot
  become a function of whichever branch is checked out.

## THE EIGHT `pkill -f` ARE DECLARED, NOT REPAIRED

Every one is in a falsifier's cleanup path, killing processes it started itself
in a temp directory it owns. They work today. The safe form is not a bracketing
trick — `ci/batch.sh` says plainly the bracket form is not enough when the
command line also carries the plain path — it is pgrep, skip `$$`, compare
`/proc/<pid>/environ`, which is a real rewrite of each cleanup. Rewriting eight
of them blind, in the scripts whose whole job is to prove other checks can fail,
is a worse risk than declaring them. **The ratchet buys that a ninth cannot be
written**, and that is all it claims.
