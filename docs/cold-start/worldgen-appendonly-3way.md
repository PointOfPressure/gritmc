# COLD START — branch `worldgen-appendonly-3way`

### COLD START — branch `worldgen-appendonly-3way`, 2026-08-17

**The item: six of the seven `REBASE-CONFLICT` rows in the morning's
`preflight.tsv` were worldgen branches, and five of the six were not
conflicting with anything.** `ci/merge-appendonly.py` refused
`docs/DETERMINISM.md` whole the moment either side had touched existing text,
and its refusal wrote *every line of both sides* between conflict markers.

Measured on `main` at `b7ff97fc`, where `docs/DETERMINISM.md` is 7,191 lines:

| branch | driver's conflict | `git merge-file --diff3` on the same inputs |
|---|---|---|
| `clamped-floor-probe`, first commit | one hunk, **7,318 lines** | one hunk, **167 lines** |
| `worldgen-sulfur-r18` | one hunk, **51,298 lines** | unchanged — see below |

The driver's docstring ended *"it never makes the non-append case worse"*.
That is the claim this branch refutes: before the driver existed git produced
a legible hunk, and a hunk the size of the file is not one. It is also how the
file gets corrupted — `regiondiff-live-fixture` reached **origin** with a
`docs/DETERMINISM.md` of 34,711 lines, the body repeated five times, with two
`||||||| base` markers committed into it, and the same thing had already
happened once on 2026-08-12. `ci/t0.sh`'s note on that incident said the
driver "behaved correctly"; corrected in place — true of its contract, false
about the outcome.

**What changed.** The whole-file fast path is untouched. The refusal path is
now `git merge-file --diff3` followed by the append-only rule applied per
hunk: a hunk with an empty base side is both branches adding text at one
anchor and is resolved ours-then-theirs; a hunk where one side left the base
region byte-identical and added text beside it is adjacency rather than
disagreement, so the addition is kept and the other side's edit taken; every
other hunk with a non-empty base is left conflicted with a non-zero exit.

**What it is measured to buy**, replaying this morning's tips onto `main` plus
the fix with no human in the loop:

```text
  clamped-floor-probe   4601fd8c   7,318-line hunk -> 49 -> CLEAN
  village-pool-elements 6435e4d7                        CLEAN
  worldgen-sulfur-r18   614ff114   51,298 lines, REFUSED and correctly so
```

**The refusal is the honest half and it is worth more than the two greens.**
`worldgen-sulfur-r18` carries a commit that rewrites the file whole — it
duplicated the body five times and its own follow-up deduped it — so both
sides really did change the same text. That branch was landed by squashing
the pair, whose net change is four lines. **This fix removes the conflicts
the driver was inventing, not the ones a branch brings with it.**

**Nulls, so nobody re-runs them:**

* **`merge=union` on `docs/DETERMINISM.md` — do not.** Killed before it was
  tried, for the reason already written into `.gitattributes`: it interleaves
  lines inside an evolving paragraph and has produced a real defect on
  `docs/WORKLIST.md`. Per-hunk union with an empty-base precondition is not
  the same rule and is the one that shipped.
* **Auto-resolving a deletion — refused deliberately, do not "improve" it.**
  If the side replacing a base region is empty or blank, the merge is refused
  however unambiguous it looks. Deletion is the only edit that loses text and
  this file's contract is that text is appended, not removed.
  `ci/merge-appendonly-falsify.sh` case 2f is that refusal.
* **Resolving a hunk both sides rewrote — never.** That is how a stale figure
  survives beside its own correction, which is the `merge=union` damage still
  legible in WORKLIST's index. Case 2b.

**What stops the union rule being unfalsifiable.** It cannot drop text; what
it could do is keep two things that should be one, and two existing gates
already catch that: `ci/check-doc-claims.py` refuses a duplicated `## N.` and
`ci/check-figures.py` refuses a figure whose print-count drifts from its
ledger. Both fired for real the same day — the ledger caught
`ow-r16-12345-floor-n15` going from 7 prints to 8 when `main` and
`worldgen-aggregate` each added a section quoting the same floor.

`ci/merge-appendonly-falsify.sh` is 84 assertions, ~7 s, and runs in
`ci/t0.sh`. Case 2 is the one assertion in that file that got **weaker** —
it asserted the refusal this branch removes — and it says so at the case
rather than restating quietly. Case 2d is new and is the discriminator: with
`merge.appendonly.driver` unset, case 1's shape must conflict. Without it
every PART 1 case would also pass under git's built-in driver and none of them
would be measuring this file.

**The single next item, and what closes it:** `ci/merge-docregions.py` has the
same shape of precondition on `docs/WORKLIST.md`, `docs/PARITY-ROADMAP.md`
and `ci/figures.toml` — it resolves only inside a declared `MERGE-REGION` and
refuses the file otherwise. Nothing measured says its refusals are oversized
the way this one's were: `village-pool-elements` was the only branch of the
seven that conflicted in WORKLIST, and it merged. Closing condition: replay
the same seven tips with `merge.docregions.driver` instrumented to record the
size of every hunk it refuses, and either produce a case where a legible
hunk was turned into a file-sized one — in which case the fallback here ports
across unchanged — or record it as a null so nobody looks again.
