# Cold starts — branch `ci-doc-heading-guard`

### COLD START — branch `ci-doc-heading-guard`, 2026-08-12

**The item.** `docs/PARITY-ROADMAP.md` carried a duplicate `## 22.` heading.
The `corpus-dark-systems` renumber (`docs: renumber the dark-systems section
to PARITY-ROADMAP 26`, `84f789db`) added a new `## 26.` line without deleting
the old `## 22.` line immediately above it, so the section legitimately owned
by `PARITY-ROADMAP-22-use-table-probe-dependence.toml` (line 4893) and the one
now owned by `PARITY-ROADMAP-26-per-scenario-gamerule.toml` (was line 5212)
both read `## 22.` for one line. `ci/check-doc-claims.py` did not catch it:
`DUPLICATE-CLAIM` only inspects the claims directory, and `HEADING-NO-CLAIM`
/ `CLAIM-NO-HEADING` compare a **set** of heading numbers against a set of
claimed numbers — two physical `## 22.` lines and one `## 26.` line collapse
to the same `{22, 26}` as one of each, because presence was checked and count
never was.

**Trust the tree over the brief, and here is where it fired.** The brief
states the checker "never looks at the headings in the documents themselves."
It does — `HEADING-NO-CLAIM` and `CLAIM-NO-HEADING` were already live on
`main`, added in the file's one prior commit (`8027c7e8`), and AGENT-BRIEF.md
itself already documents both. Only the third rule, duplicate physical
occurrences of one number, was actually missing.

**What was built.** `find_headings` now returns `(number, line_no)` per
occurrence instead of a deduplicated `set`. `check()` groups occurrences by
number and adds one new arm, `DUPLICATE-HEADING`, firing when any number's
occurrence count is >1 and naming every line it occurs on.
`HEADING-NO-CLAIM`/`CLAIM-NO-HEADING` are unchanged in behaviour (still built
from the deduplicated set) and `DUPLICATE-CLAIM` (claims-directory duplicates)
is untouched, as required.

**Proof each arm can fail** — `ci/check-doc-claims.py --selftest` is 18/18
green (one new case added: two `## 1.` headings in one fixture doc), and every
arm was additionally proven against a **scratch copy of the real
`docs/` tree**, not just fixtures:

* `DUPLICATE-HEADING` — re-injecting the exact stray `## 22.` line into a
  scratch copy of `PARITY-ROADMAP.md` reproduces
  `DUPLICATE-HEADING PARITY-ROADMAP §22 appears 2 times in
  PARITY-ROADMAP.md (line 4893, line 5211)`, exit 1.
* `HEADING-NO-CLAIM` — a scratch `## 999.` heading with no claim file gives
  `HEADING-NO-CLAIM PARITY-ROADMAP §999 has a heading ... and no claim file
  registers it`, exit 1.
* `CLAIM-NO-HEADING` — a scratch `PARITY-ROADMAP-999-scratch-ghost.toml` with
  no matching heading gives `CLAIM-NO-HEADING PARITY-ROADMAP §999 has a claim
  file and no `## 999` heading ...`, exit 1.
* `DUPLICATE-CLAIM` (sanity only, unchanged) — a second file claiming
  `(PARITY-ROADMAP, 26)` gives `DUPLICATE-CLAIM PARITY-ROADMAP §26 claimed by
  2 files ...`, exit 1.

Every injection was reverted; the scratch copy checks clean (`0 reds, 61
claims`) both before the first injection and after the last.

**The fix.** Deleted the stray `## 22.` heading line in
`docs/PARITY-ROADMAP.md` (was line 5211, immediately above `## 26.`). Nothing
else in that section changed.

**Tree-wide result.** Before the fix, `ci/check-doc-claims.py` on the real
tree printed exactly one red — `DUPLICATE-HEADING PARITY-ROADMAP §22 appears
2 times ...` — matching the one violation the brief named. After the fix:
`check-doc-claims: 61 claims registered across 2 docs, 0 collisions, 0
drift`, exit 0. No other violation exists anywhere in `docs/DETERMINISM.md`
or `docs/PARITY-ROADMAP.md`; the tree has no unfixed defects of this class.

**Null.** Building `HEADING-NO-CLAIM`/`CLAIM-NO-HEADING` was not needed — they
were already live and already gate `ci/t0.sh` via `check-doc-claims.py`. Do
not re-build them.

**Next.** None owed by this branch — scope was the checker and the one stray
line. `ci/check-doc-claims.py` (standalone, or via `ci/t0.sh`) now refuses a
repeated `## N.` heading by itself; no further doc-hygiene work is implied.

### COLD START — branch `ci-doc-heading-guard`, 2026-08-12 (later same day) — a reported second and third duplicate did not exist

The orchestrator reported the newly-armed `DUPLICATE-HEADING` would also red
`docs/DETERMINISM.md` on `main` — `## 4.` and `## 17.` each claimed twice —
and asked for them to be resolved the same way as `## 22.`, with a note that
two genuinely distinct sections sharing a hand-assigned number would need a
renumber and a cross-reference chase.

**Checked against the tree rather than acted on.** Rebased onto
`origin/main` (`0679f236`, unrelated worldgen-doc commits; one WORKLIST index
conflict, both rows kept), then ran the extended checker three ways: the
real rebased tree, and a faithful `git show origin/main:...` extraction
(bypassing my own worktree entirely). All three agree: **`DETERMINISM.md`
has 38 headings, all 38 numbers distinct — no duplicate at any level** —
and the tree-wide run before my fix landed on `main` shows exactly the one
red this branch already fixes:
`DUPLICATE-HEADING PARITY-ROADMAP §22 appears 2 times ... (line 4893, line
5211)`. `## 4.` (line 212) has five sub-sections `4.1`..`4.5`; `## 17.`
(line 1993) has one, `17.1`. `canon_number`/`HEADING_RE` already treat `4`
and `4.1` as different keys — this is the exact case `--selftest` names
"decimal sub-sections (4.1) are not confused with 4" — so a top-level
section with sub-sections was misread as a duplicate, almost certainly by a
`grep` that strips the decimal continuation (`grep -oE '^## [0-9]+\.'`
matches `## 4.1` as `## 4.` and cannot tell it from a second `## 4.`).
Confirmed the count directly: that pattern against `origin/main`'s
`DETERMINISM.md` reports `6 ## 4.` (the header plus its five sub-sections)
and `2 ## 17.` (the header plus its one sub-section) — the shape a naive
grep produces, not a shape `check-doc-claims.py` produces.

**Nothing renumbered, no cross-references chased — there was nothing to
chase.** Re-ran after the rebase: `--selftest` 18/18 green, real tree
`64 claims registered across 2 docs, 0 collisions, 0 drift`, exit 0.
Force-pushed the rebased commit (`09580fd0`) and verified it on origin.
