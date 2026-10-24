# COLD START — branch `ci-required-checks-ratchet`

### COLD START — branch `ci-required-checks-ratchet`, 2026-08-17

**The item: a check that is not in the tree does not fail, it does not run —
and nothing said so.** Two instances turned up within an hour, both by
accident. `regiondiff-live-fixture` reached origin carrying two committed
`||||||| base` markers and its body repeated five times; a gameplay branch was
found rebased onto a main that predated `ci/check-conflict-markers.py`, so its
marker check never ran and `preflight.tsv` recorded that as GREEN.

**The sweep, all 33 queued tips, adjudicated by MAIN's copy of the scanner via
`--root` so that no branch's own copy scores itself:**

```text
  committed conflict markers, any of the four forms          0 branches
  branch TIPS carrying no ci/check-conflict-markers.py       4 branches
```

**CORRECTION, same day, by running the thing instead of reading it.** The
first write-up of this branch said three of those four "read GREEN in
preflight.tsv while never having run the check". That is wrong.
`preflight.sh` rebases each branch onto main *before* it adjudicates, so the
tree it checks carries main's `ci/` whatever the tip carried:
`gameplay-block-placement` has no `ci/check-conflict-markers.py` at its tip
and has it after the rebase, and a full preflight run after the change
produced **35 rows, 33 GREEN and not one `missing:` row** — which is the
measurement that refuted it. The vacuous pass is real wherever a tip is
evaluated **unrebased**, which is an agent running `./ci/t0.sh` in its own
worktree before rebasing, and that is how the instance the corpus agent found
arose. It is not a hole in preflight's rebased path.

The four: `gameplay-block-placement`, `gameplay-block-schemas`,
`gameplay-serverbound-reach`, `gameplay-sound-coverage`. **Every one is
marker-clean** — the hole was open and nothing had fallen through it. The fix
for each is a rebase onto current main, which is how they get the script;
none was force-pushed here, because they belong to the corpus area and a
restack moves a branch's own baseline underneath whoever is holding it.
`gameplay-entity-coverage` already carries the script at main's exact digest.

**What was already closed, measured rather than assumed** — this is the half
that bounds the incident, and it is worth more than the fix:

* `ci/handoff.sh:376` refuses any branch **BEHIND** main at exit 3. A tree
  with a `ci/` older than main's therefore **cannot reach the gate at all**,
  so no vacuous pass can put anything on main. The blast radius was queue
  triage only.
* `preflight.sh` read `[ -f "$W/ci/$script" ] || continue` and now reports
  `missing:<check>`, plus it runs main's copy of the marker scanner against
  every branch tree regardless of what that tree carries. **Its value is
  narrower than it first looked** — see the correction above: because
  preflight rebases first, the only missing check it can now see is one the
  branch DELETED, which does survive a rebase. That is worth having and it is
  falsify cases 3 and 4, but it was not what made those four rows green.

**What was left, and what this branch closes:** a branch that deletes a check
**and** its invocation in the same commit. It is not behind main, nothing is
inconsistent, no script and no call and no error — and T0 is green because it
has stopped looking. `ci/checks/required-checks.toml` is the third thing such
a branch would have to delete, and `REQUIRED_FLOOR` in
`ci/check-checks-present.py` is why deleting it is a code diff rather than a
data edit.

**Nulls and refusals, so nobody re-derives them:**

* **Do not discover `wired_in` from the call graph. Two versions were built
  and both were the defect they refuse.** Crediting any mention made a planted
  orphan look wired, because the falsify script *names the orphan it plants* —
  the guard read its own fixture as coverage. Excluding falsify hops then made
  `check-conflict-markers` look wired with its real invocation commented out,
  because the guard's own docstring names it in a sentence and a docstring is
  not a `#` comment. Declared `wired_in` is decidable and cannot be satisfied
  by prose.
* **`ci/check-use-probe.py` is not wired into T0 and that is correct.** Inside
  T0 only `ci/check-use-probe-falsify.sh` runs it, and only against synthetic
  fixtures; the real-data adjudication is `ci/use-golden.sh:168` and
  `ci/use-item-on-golden.sh:205`. A discovering guard would have called this
  either dead or wired depending on its heuristic. The manifest records it.
* **Do not make the guard consult `origin/main`.** T0 runs on a tree rsynced
  without `.git`, so any rule needing a ref is undecidable exactly where it
  has to run. Staleness-against-main is an out-of-tree question and belongs to
  `preflight.sh`, which has a repo.
* **The ratchet must stay lowerable.** Falsify case 5 requires that removing a
  check *honestly*, by lowering the constant, goes green. A ratchet nobody can
  legitimately lower is one the next agent routes around by deleting the guard.

**How regiondiff-live-fixture got corrupt — answered, inside the ten minutes,
and it is not a script.** The two committed markers were `||||||| base` and
**only** that: no `<<<<<<<`, no `>>>>>>>`. That is what is left when a human
strips the outer markers by hand and misses the middle one. The file also grew
by whole multiples of its own body (6,762 → 20,841 → 34,711), which is
keeping every side of a whole-file conflict rather than merging it. Both point
at one cause: `ci/merge-appendonly.py` refused `docs/DETERMINISM.md` **whole**
whenever either side touched existing text, and nobody resolves a 51,298-line
hunk by reading it. That driver was fixed earlier today on
`worldgen-appendonly-3way`. **No sweep script or merge driver writes markers
into a commit; the driver wrote them into a working tree, correctly, and the
hand resolution was wrong.**

**The single next item, and what closes it:** the four tips above still want
rebasing onto current main — not for preflight's sake, which rebases them
itself, but because until they are, **an agent or a local `./ci/t0.sh` in
those worktrees runs a gate with no marker check in it**, which is exactly the
instance that started this. Closing condition: those four rebased, and a
`preflight.tsv` in which no row reads GREEN while missing a check — the run on
2026-08-17 after this change was 35 rows, 33 GREEN, 0 `missing:`. `preflight-falsify.sh` (8 assertions, 5
cases, outside the repo beside `preflight.sh`) is the proof that the triage
half can go red; `ci/check-checks-present-falsify.sh` is the proof for the
in-tree half.
