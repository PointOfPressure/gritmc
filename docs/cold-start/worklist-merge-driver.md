# COLD START — branch `worklist-merge-driver`

### COLD START — branch `worklist-merge-driver`, 2026-08-16 — the queue's largest source of friction was three files with no merge driver

**Baseline, measured rather than briefed.** On `main` `8228c037`, sayer-desktop,
by rebasing every unmerged `origin/*` branch onto main in a throwaway detached
worktree (`git rebase`, drivers registered exactly as `ci/setup-merge-drivers.sh`
installs them):

* **26 of 48** unmerged branches would not rebase onto main.
* Conflicting paths: `docs/WORKLIST.md` **13**, `docs/PARITY-ROADMAP.md` **11**,
  `ci/vanilla-reach-census.py` 6, `ci/figures.toml` **4**,
  `docs/DETERMINISM.md` 1, `docs/AGENT-BRIEF.md` 1, `ci/t2-gameplay.sh` 1.
* **18 of the 26 conflicted on nothing but the four ledger docs.**

**The file's real shape, which is the whole design problem.** Over the last 40
commits touching `docs/WORKLIST.md`, **36 add exactly one row to the COLD START
INDEX table** at the same anchor (hunk headers all in lines 897..919) and 4 also
rewrite prose inside `## NOW — worldgen features`. So the brief's suspicion is
confirmed and sharpened: WORKLIST is **one small append-only region embedded in
a document that is rewritten in place everywhere else**. Neither side of any
merge is ever a byte-prefix extension of the base, so `merge=appendonly` would
refuse *every* merge; `merge=union` is the defect already paid for.

**The union's damage was still legible in the tree, and is repaired on this
branch.** The COLD START INDEX carried `ci-unfalsifiable-root` **twice**, one row
saying *"10 of 84 (kind, field) pairs are read by no check"* and the other
*"4 of 84"*; `ci-figures-ledger` twice; `useblock-port-the-17` three times. Each
is a branch that replaced its own row per `docs/AGENT-BRIEF.md` and had both
kept by `ci/lanes.sh`'s union. **Main was publishing two contradictory figures
for one measurement**, which is the exact failure `merge=union` was removed for.

**The discriminator, because "the later row wins" is not evidence.** A duplicate
is union damage when all its rows carry **the same date**, the branch's
`docs/cold-start/<branch>.md` holds **fewer `### COLD START` entries than there
are rows**, and each earlier row's content is **carried forward by the later
one**. It is a legitimate repeat when the rows carry different dates and the
file holds one entry per date — a branch that genuinely cold-started twice.

| key | rows | dates | entries in its file | verdict |
|---|---|---|---|---|
| `ci-unfalsifiable-root` | 2 | both 2026-08-13 | 1 | union damage, repaired |
| `ci-figures-ledger` | 2 | both 2026-08-13 | 1 | union damage, repaired |
| `useblock-port-the-17` | 3 | all 2026-08-12 | 2 | union damage, repaired |
| `chat-commands` | 2 | **08-09 and 08-06** | **2** | **legitimate — LEFT ALONE** |

**`10 of 84` is dead and here is what killed it, so nobody re-derives it.**
Four independent lines of evidence, all agreeing:

1. `3ab32a02` wrote the 10 at 17:10:58; `292393c4` — *"ci: dead fields are 4,
   not 10 — and the falsify suite's own control was unsound"* — wrote the 4 ten
   minutes later at 17:20:38. Both are on main; the union kept both rows.
2. The same work exists un-unioned at `ce0fdf2b`, which **rewrites that one row
   in place** from 10 to 4. The branch's own intent was a replacement.
3. `docs/cold-start/ci-unfalsifiable-root.md` records *"4 of 84 (kind, field)
   pairs are read by no check"* and *"All four are rotation: `move_entity_rot`
   and `rotate_head`"*.
4. **The live instrument in this tree.** `ci/checks/gameplay-falsifiable.toml`
   carries `dead_fields = 4`, `field_universe = 84`, and its four
   `[[dead_field]]` entries are `move_entity_rot/{entity,rest}` and
   `rotate_head/{entity,rest}` — all rotation, matching the surviving row's
   parenthetical entry for entry.

**Why the 10 was wrong, from `292393c4`'s own body:** the detector's axis B was
under-detecting, and re-sweeping with `first_difference` in the signature made
the localisation test finer. *"Every one of the six that moved was this
instrument's false positive rather than a field that became live"* —
`entity_position.at`, for instance, is read by
`a-player-that-leaves-the-ground-is-resynced`, demonstrated by perturbing all 32
of its observations directly. So this is not two measurements of different
things: **it is one measurement, and the 10 was an artefact of an unsound
control.** Both rows say "of 84" and the ledger holds one `field_universe = 84`,
so the `corpus=`-stamp reading — two true figures over different denominators —
does not apply here and was checked before the row was touched.

**No floor was subtracted.** Neither figure is registered in `ci/figures.toml`,
so neither carries a rule-6 floor; the surviving `useblock-port-the-17` row
carries its own *"floor `4/92` NOT quotable beside it"* clause and keeps it
verbatim, because the survivor is the longest row rather than a rewrite. The
three earlier rows were verified segment by segment to have nothing that the
survivor does not carry — `ci-unfalsifiable-root`'s single uncarried segment was
the refuted `10 of 84` sentence itself, and `ci-figures-ledger`'s two were the
superseded *"Four predicates … falsified 12/12"* and *"Audit found one more"*,
both restated in the survivor as five/13 and *"two more"*.

## What shipped

`ci/merge-docregions.py`, a **section-aware** driver. A file declares its own
append-only regions with marker lines carried inside the host file's comment
syntax (`<!-- MERGE-REGION BEGIN name key=… check=… -->`, `# MERGE-REGION BEGIN
figures to-eof …`). The driver runs `git merge-file --diff3` — git's own engine,
so behaviour outside a region is git's default behaviour and not a
reimplementation — then walks the diff3 output and resolves a conflict hunk only
if it lies **inside a declared region** and one of three finer-grained 3-way
answers applies:

1. **base slice empty** — both sides merely inserted at one anchor. Ours' then
   theirs'.
2. **keyed rows** (`key=first-cell`) — a hunk of table rows merged per key.
3. **contiguity** — one side contains the whole base slice contiguously and
   exactly once, so that side edited nothing and only inserted around it.

(2) and (3) exist because git's hunks are line ranges: an edit and an insertion
that do not overlap get conflated whenever they are *adjacent*, and adjacency is
the normal case here. Both were added against a measured branch — (2) for
`t0-assertion-floor`, where main inserted two rows above that branch's own row
while the branch replaced it exactly as AGENT-BRIEF instructs; (3) for
`gameplay-block-schemas`, where main appended 381 lines directly above that
branch's own heading, which the branch had renumbered `## 44.` -> `## 48.`
because main claimed 44 first.

**Not one of the three can keep both sides of a rewritten line.** Every refusal
is preserved: both sides rewriting one row, either side deleting a row, both
sides adding one key differently, and anything at all outside a region.

Regions placed: `docs/WORKLIST.md` — one **bounded** region around the COLD
START INDEX table body only. `docs/PARITY-ROADMAP.md` and `ci/figures.toml` —
one **`to-eof`** region each, because every branch appends its `## N.` section
or `[[figure]]` at EOF and a to-eof region needs no cooperation from the
appending branch. `docs/DETERMINISM.md` **stays on `merge=appendonly`**: it is
the one file where every edit really is a whole new tail, that driver's
byte-prefix refusal is the strictest thing available, and there was one measured
collision on it — on a branch that conflicts on two other files anyway.

Post-conditions, run **only when a hunk was auto-resolved**, so the driver can
never refuse a merge git's default would have completed: `unique-first-cell`,
`unique-heading-number`, `toml-figures`. Each takes the form *the merged file
may not assert a key more times than the side asserting it most*, which is
exactly what a union violates and a legitimate concurrent append does not.

## Result

| | before | after |
|---|---|---|
| unmerged branches that will not rebase onto main | **26 of 48** | **9 of 48** |
| conflicting on `docs/WORKLIST.md` | 13 | 1 |
| conflicting on `docs/PARITY-ROADMAP.md` | 11 | 0 |
| conflicting on `ci/figures.toml` | 4 | 0 |

All three branches the item named — `t0-assertion-floor`,
`gameplay-block-classes`, `worldgen-sulfur-r18` — rebase clean onto this branch
and none of them onto main. **Every one of the 9 that still conflicts does so on
real code** (`ci/vanilla-reach-census.py` 6, `ci/t2-gameplay.sh` 1) except
`worldgen-12345-next-items`, which conflicts on `docs/AGENT-BRIEF.md` and
`docs/DETERMINISM.md` as well.

**Correctness control, not just conflict-freedom.** For all 17 branches that
flipped CONFLICT -> CLEAN, `ci/check-doc-claims.py` and `ci/check-figures.py`
were run on the rebased tree **and on the branch tip as the control**: 17 of 17
identical, all green. Spot-checked byte-for-byte on `t0-assertion-floor` that
main's two newest index rows and the branch's own replaced row all survive
verbatim, `grep -c '^## NOW'` is still 3, `ci/figures.toml` parses with 93
unique ids, and no conflict marker survives anywhere.

## The one deliberate refusal, named so nobody re-opens it

`worldgen-12345-next-items`'s WORKLIST conflict is a **pure add/add inside
`## NOW — worldgen features`**: main appended 2,360 lines of dated log entries
at the same anchor where that branch appended 100. A region declared inside
`## NOW` would resolve it. **It is refused on purpose.** `## NOW` also carries
status claims that are *rewritten*, and the boundary between "the dated log
below" and "the status header above" moves every time an agent edits the block.
A marker on the wrong side of that moving boundary would silently union a status
claim, which is the precise failure this whole item exists to prevent. One
branch of 48, and that branch conflicts on two other files anyway. Cost of the
refusal: measured and small. Cost of getting the boundary wrong: a stale figure
nobody can see.

## Nulls — do not re-run these

* **`merge=appendonly` on `docs/WORKLIST.md` is not merely imperfect, it refuses
  100% of merges.** No side is ever a byte-prefix extension of the base, because
  rows are inserted under the table separator and not at EOF. Do not try it.
* **A `to-eof` region whose BEGIN marker sits at the *current* EOF does not
  work** for branches written before the marker existed: their append lands
  *before* the marker, outside the region, and still conflicts. The BEGIN marker
  has to precede the divergence point, which is why PARITY-ROADMAP's sits above
  `## 0.` and figures.toml's above the first `[[figure]]`.
* **`ci/figures.toml` was evaluated for exclusion and included on evidence.** Its
  edits *are* scattered in place, which argued against a region; but two of the
  three branches the item named conflict on it and its appends are all at EOF,
  so it is included with a `check=toml-figures` post-condition rather than left
  out. `docs/PARITY.md` was left out: no driver, no measured collision, so
  removing it from `UNIONABLE` would have been an unmeasured behaviour change.

## A live finding this item did not go looking for

**`ci/lanes.sh` clones each lane, and a clone does not inherit local config —
so `merge.appendonly.driver` has never been registered in any lane.**
`.gitattributes` naming a driver git has not been told about falls back to the
default 3-way merge with no warning. Every lane has therefore been rebasing with
`docs/DETERMINISM.md merge=appendonly` **inert**, and `rebase_with_doc_union`
was silently doing that driver's job as well as its own. `lane_setup` now calls
`ci/setup-merge-drivers.sh`, and that is what made it safe to shrink `UNIONABLE`
from five files to one. A union running *after* a driver would override exactly
the refusals that are the driver's whole value.

## Falsifier

`ci/merge-appendonly-falsify.sh`, already in T0, extended from **2 cases / 8
assertions to 19 cases / 61 assertions**, every one a real three-way `git merge`
in a throwaway repo against the drivers in the checkout by absolute path.
**Eight cases are refusals** and they are the point: two rewrites of one table
row, two rewrites of one `## NOW` line, an add/add in `## NOW` where no region
is declared, delete-vs-edit of a row, a duplicated table key, a duplicated
`## N.`, a duplicated `[[figure]]` id, and a malformed region marker. The last
proves the failure mode of a marker that stops parsing is a **conflict**, never
a union. `ci/setup-merge-drivers.sh` additionally runs
`ci/merge-docregions.py --check`, which needs no git and therefore still has
teeth in an rsynced build copy — verified red by deleting an END marker.

**Case 19 was unfalsifiable on its first draft and that is worth recording.** It
proves the multiplicity post-condition is scoped to the REGION rather than the
file — necessary because `docs/WORKLIST.md` holds other tables whose first cell
legitimately repeats, so a file-wide count refuses an innocent merge. The first
draft put the two sides' edits far enough apart that git merged them with no
hunk at all: the driver returned early, the post-condition never ran, and the
case was green **with the fix injected out**. Caught by injecting the fix out
and re-running, which is the only thing that establishes a case has teeth. It
now requires both sides to append an index row, and is red without the scoping
and green with it.

## The single next item

**Nothing here is blocking.** The next thing worth measuring on this line is
whether `## NOW — worldgen features` can carry a region safely, which needs a
stable, mechanically-checkable boundary between its rewritten status header and
its dated append log — probably a marker the *writing agent* places, checked by
a T0 rule that the status header holds no dated `### YYYY-MM-DD (branch …)`
entry. The result that closes it: `worldgen-12345-next-items` rebasing clean on
its WORKLIST hunk while a two-sided rewrite of a status figure still conflicts,
proved as a 19th falsifier case.
