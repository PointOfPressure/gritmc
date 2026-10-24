# COLD START — `ci-docregions-check-vacuous`

### COLD START — branch `ci-docregions-check-vacuous`, 2026-08-18, area "gate throughput"

Two holes in the guard for the merge-driver machinery — the queue's
best-paying throughput lever, the thing that took branches which would not
rebase onto main from 26 of 48 to 9. Both are this project's central defect
class.

**1. `ci/merge-docregions.py --check` WITH NO FILES EXITED 0.** `do_check()`
looped `for p in paths` over an empty list and returned 0, reporting "no
MERGE-REGION marker was lost" having read nothing. `ci/t0.sh` (via
`ci/setup-merge-drivers.sh:143`) treats that zero as the marker guard passing.
The file list arrives as an **unquoted** `$REGION_FILES` — unquoted because it
must word-split — and an unquoted variable that goes empty contributes no
arguments at all rather than an empty one. It now refuses with **exit 2**,
deliberately distinct from a content RED's exit 1.

Found by running `--check` by hand with no arguments against a file whose END
marker had just been deleted, and getting a green.

**2. `.gitattributes` AND `$REGION_FILES` ARE ONE FACT KEPT TWICE.**
`.gitattributes` decides which files the driver *runs* on; `$REGION_FILES`
decides which get `--check`ed. Nothing compared them. Add a seventh
`merge=docregions` line and forget the variable and the new file is driven but
never checked — a lost marker in it is silent, the exact state `--check` exists
to make loud. The reverse drift is quieter: a name in `$REGION_FILES` that
`.gitattributes` no longer carries makes the script assert a registration that
is not in force. `setup-merge-drivers.sh` now derives the list from
`.gitattributes` and refuses on any disagreement, naming both sides. The
literal is kept rather than replaced, because the literal is what a reader
needs to see and the point is that the two must match.

**Evidence.** `ci/merge-appendonly-falsify.sh` cases 29 and 30, both with
controls — a real file still passes so 29 is not "always refuse", and the guard
goes green again when the injected line is removed so 30 is not stuck red. Both
drift directions are asserted separately because they fail differently.

**Also made the summary line count itself.** It read "29 cases, 89 assertions"
against an actual 28 and 112 — stale in both directions, in the one line a
reader uses to judge coverage. Now **30 cases, 119 assertions**, computed. A
count that has to be remembered is a caveat list that lags the code.

**WHAT I KILLED AS A NULL.** `$REGION_FILES` cannot go empty *today* — it is a
literal. The refusal is for the day it is derived from `.gitattributes`, which
is the obvious next refactor and is exactly what would remove hole 2 entirely.
Do not remove the refusal on the grounds that the variable is a literal.

**The single next item.** Derive `$REGION_FILES` from `.gitattributes` outright
and delete the comparison, which turns two facts into one. Cases 29 and 30 both
survive that change unmodified, which is the point of writing them as
behaviour rather than as text assertions.
