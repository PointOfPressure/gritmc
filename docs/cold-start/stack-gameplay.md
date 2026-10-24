# COLD START — branch `stack-gameplay`

Append-only. Newest entry last.

### COLD START — branch `stack-gameplay`, 2026-09-04, the gameplay corpus landing stack

Worktree `~/grit/work-stackgp` (local branch `stackgp-work`, pushed as
`origin/stack-gameplay`); gate worktree `~/grit/work-stackgp-gate`; remote dir
`~/rustmc-stackgp`; gate logs `~/rustmc-private/gates/stackgp/t0-<n>-*.log`.
Built from `origin/main` `f4047ae1f` by rebasing each queued gameplay branch
onto the growing stack, largest first, repairing each branch's T0 red the way
its own check says, and running `ci/t0-remote.sh` between folds. No corpus
lock was taken; no T2 was run.

## What the queue actually was

`git cherry` + `git patch-id --stable` over the 25 branches in the brief:
**654 distinct unlanded patches**, not the 1,240 the per-branch counts sum to.
The containment that matters:

* `land-train` (263) = `land-gameplay` (139 shared) + **124 worldgen/bench
  patches** (`worldgen-block-entities`, `worldgen-jigsaw-random`,
  `worldgen-isolated-drift`, `worldgen-sulfur-r18`, the +14% bench arms,
  `village-pool-elements`, `block_pile`), three of them (the spruce census /
  measure) already on main under other patch-ids. Not a gameplay branch.
* `land-gameplay` (157) = `boxcopy` (24 of 26) + 22 patches whose subjects main
  already carries (piston, 790, block classes, check-doc-claims) + 2026-08-19
  copies of the gameplay branches that were re-rebased on 2026-08-26 + stack
  fixups. The 08-26 branches are the newer text, so the stack is built from
  them and neither land branch is folded.
* `signs-applicators` (107) ⊃ `useitemon-dispatch` (93) ⊃ `useitemon-gates`
  (77 ⊃ 43 of `probe-answering-environment`'s 44).
* `corpus-step-respawn` (93) ⊃ `corpus-step-vocabulary` (48), `combat-parity`
  (26); 48 of its patches are on main by subject (the falsifiability audit).
* `proj2-drops-ledger` (97) ⊃ 38 of `corpus-projection-audit` = `corpus-sound-
  pitch` (47, identical), 25 of `entity-parity` (33 ⊃ `gameplay-entity-
  coverage` 8), 29 on main by subject (gate batching).
* `gameplay-menus-round2` (21) ⊃ 13 of `gameplay-container-menus` (17); the
  other 4 are older forms of menus-round2's own commits.

## Folded, in order, with the repair each needed

| fold | patches | its T0 red, and the repair |
|---|---|---|
| `entity-parity` | 32 of 33 | `ci/run-detached-falsify.sh`: its `84a981d78` deletes `ci/run-detached.sh`, which main has since `7bd943185`. That commit left out; the same patch came back through `proj2-drops-ledger` and is reverted in `57838cc11` |
| `gameplay-menus-round2` | 21 | grades ledger UNREGISTERED 740-757: rows re-taken (below). `[meta] floor` of the falsifiable ledger = tier-2 count |
| `gameplay-container-menus` | 0 | entirely superseded by menus-round2 (13 identical, 4 older) |
| `useitemon-gates` | 77 | `ci/placed-golden.sh` → `server-level-gates.py --emit` measured a STALE `use_item_on.json` (`LIVE SignBlock.useItemOn — ledger 2784 != measured 0`); `da3df0500` treats a stale-stamped table as absent under `--emit` |
| `proj2-drops-ledger` | 65 | §45 collision with menus (branch's own central §60 adopted); `||||||| base` + 18 duplicated open-row-table rows at `PARITY-ROADMAP.md:789` (on the branch itself) deleted; `REQUIRED_FLOOR` = manifest count (16), which `check-checks-present-falsify` case 4 demands |
| `gameplay-sound-eventid` | 16 | `UseGolden`'s `recordSound` ported by hand onto the 12-stance probe; `use_sound` gets the table at the call site main's 790 refactor moved; figure `use-table-sound-coverage-20260817` given its commit (`fbf9d7e58`) and its one doc mention; re-taken on the 247-block table (next entry) |
| `gameplay-corpus-gaps` | 16 | its §53 collided with main's entity-parity §53 → the branch's central §95 adopted; note-block `trigger_event` merged with main's piston dispatch (`Triggered { delivered, sound }`); `fallable.rs`'s 8 raw `skipping:` sites → `fixture::available` |
| `gameplay-block-placement` | 14 | none beyond the ledger merge |
| `gameplay-block-schemas` | 11 | none |
| `gameplay-sound-coverage` | 10 | none |
| `gameplay-useitemon-unit` | 9 | none (its `items` axis is what `pump_expendable` later lost, repaired in the stack) |
| `gameplay-serverbound-reach` | 8 | `observe_from = "probe"` needs the rebuilt comparator; nothing in the tree |
| `gameplay-motion-combat` | 1 | the other 4 are inside corpus-gaps |

**Left out, and why each cannot be made GREEN by anything written here:**

* `useitemon-dispatch` + `signs-applicators` (30 patches beyond useitemon-gates):
  a second reader of `use_item_on.json`, 1,987 lines, API `Answer`/`Outcome`/
  `Click`, no `FlowerPotBlock`, 15 of 28 patches on `world_send.rs` where main's
  callers use `use_item_on`/`UsedItem`/`given_back`. A re-port onto
  `crate::useitemon`, not a rebase.
* `corpus-step-respawn`, `combat-parity`, `corpus-step-vocabulary`: 11 of their
  12 new scenarios are in no recording on the box (`MAX_UNSWEPT` is 4), and
  combat's tip committed a sweep that took 53 of 121 existing ledger rows to
  `tier = 0` because its recordings lacked those scenarios.
* `gameplay-click-classes`: 9 of 9 new scenarios unrecorded.
* `gameplay-block-events`: 2 of 2 unrecorded, and the tip is a wip marked
  untested.
* `land-train`, `land-gameplay`: see above.

## Two instrument facts every later lane will hit

* **38 of the 65 corpus recordings on the box are unreadable to the current
  `oracle-runner`** — every 2026-08-18/19 gate directory, `~/rustmc-land` and
  `~/rustmc-gpland` included: `unknown variant 'probe', expected 'actor' or
  'observer'`. Those two are the only recordings holding the 25 new menu and
  entity scenarios. `ci/corpus-history.py` reports the failure as `0
  recordings, 0 checks`. The grades ledger on the stack is re-taken over the
  27 readable ones and the 25 are `unseen`; a corpus run under this comparator
  is what registers them.
* A stale `target/release/oracle-runner` reads as a comparator regression:
  the gate-4 history reading printed `0 moved / 108 frozen` because the
  release binary predated `serverbound-reach`'s `Probe` variant and refused
  scenario 920's `observe_from = "probe"` on every recording. Rebuild before
  reading.

## Nulls

* `git rebase` does not drop a patch main absorbed in *corrected* form; 61 of
  proj2's and 48 of corpus-step-respawn's commits had to be dropped by subject.
* `git show :2:/:3:` are gone the moment `git add` runs on the path.
* Resolving a roadmap conflict as `--theirs` on a large hunk dropped eleven
  sections (2,318 lines) once; measured by heading count and reverted.

## Next

The chain's T2 on `origin/stack-gameplay`. After it lands: a corpus run under
this comparator to register the 25 `unseen` checks, then the useItemOn
re-port and the three unrecorded branches, in that order.

### COLD START — branch `stack-gameplay`, 2026-09-05, rebased onto main f4fa28ff8

Same worktree and remote dir as the entry above. The chain reported
`SKIP — REBASE CONFLICT` on 3c3ed4779 against the new main; this is what that
was and what was left underneath it.

## The rebase: one conflict in 290 patches

285 of 290 replay untouched. The stack and main overlap on **three files
only** — `ci/t0.sh`, `docs/FINDINGS.md`, `docs/WORKLIST.md`, by `comm -12` over
the two `--name-only` diffs from the merge base `f4047ae1f` — and git's 3-way
resolves two of them. The single conflict is an add/add hunk at EOF of
`docs/FINDINGS.md`, both sides appending their own findings.

**`docs/FINDINGS.md` is the one shared append-only document in this tree with
no `MERGE-REGION`**, which is why a 290-patch stack stops on it. WORKLIST,
PARITY-ROADMAP and `ci/figures.toml` all declare one; FINDINGS does not. If
anything is worth doing to make the next stack cheaper it is that.

## The two reds

**The grades ledger had drifted: 148 scenario files against 133 rows.** The 15
UNREGISTERED are the six `730-735` block-sound checks, `890`, `920` and the
four `960-963`, added after `324d9e766` took the last reading. Re-taken the way
the check's own message says, no lock and no server:
`ci/corpus-history.py ci/checks/gameplay <a readable vanilla.json> hist.json`
then `ci/corpus-grades.py --write --history hist.json`. **148 checks; 42 moved
(floor 41), 6 strict-only, 60 frozen (ceiling 61), over the 27 recordings this
comparator can read.** Neither ratchet moved, because the 15 appear in no
recording on the box and are therefore `unseen`, which both denominators
exclude by design.

**`useblock::the_shipped_table_says_which_sound_and_admits_when_it_cannot` was
pinned at the wrong instrument's number.** The table describes 1,525 sounding
states over **35** event names; the use path can emit 1,509 over **34**; the
whole difference is the sixteen `decorated_pot` states whose rule consumes the
click and writes nothing, and they are the only states carrying
`block.decorated_pot.insert_fail`, so `1525 - 1509 = 16` states and `35 - 34 =
1` name are the same fact. The neighbouring assertion carried the same
confusion as `(1509, 2944)` and was corrected to `(1525, 2944)` one line up
(`gates/stackgp/t1-5.log`), leaving this one red at 35 vs 34 from that commit
onward. Both readings are asserted now, named apart.

## The fourth red: a refusal that was an allowlist

`ci/entity-id-allocator.py` refused with *a mob EntityKind now exists
(FALLING_BLOCK), so `Mob.serverAiStep`'s goal phase is reachable and this
check's standing refusal to grade it is stale*. `FALLING_BLOCK` is
`MobCategory.MISC` and `ci/mob-tick-census.py` on the same tree reports
`our_mob_kinds = 0`, so the refusal was false.

The implementation was `[k for k in entity_kinds if k not in ("PLAYER",
"ITEM")]` — an allowlist frozen when the file was written, when the only
plausible third constant was a mob, and the comment beside it says exactly
that: "counted rather than asserted so the row moves on its own the day a
third one lands". **A third one landed and it was not a mob.**

Two things in that file already had the criterion right. The refusal's own
`why` string says "whose intersection with the jar's `Mob` subtypes is empty",
and `ci/entity-id-allocator-falsify.sh` arm 5 injects
`name: "minecraft:pig"` — a registry id, which only matters if the id is what
gets read. **The prose and the falsifier were both written against the
measurement; only the code was a list.** It now intersects our constants'
registry ids with the jar's own non-MISC `EntityType` set, via
`ci/mob-tick-census.py`, which runs earlier in `ci/t0.sh` and caches.

**Why eight falsifier arms could not catch it: not one asserted the refusal
stays SILENT.** Arm 5b does, injecting `minecraft:falling_block`. It also
checks its own injection landed — and that is not belt and braces: the first
draft of 5b had a broken heredoc, python exited non-zero, `entities.rs` was
never touched, and the arm printed OK. **An arm that asserts an ABSENCE reads a
failed injection as a pass**, which is a false green inside the arm written to
catch a false green. Any arm of that shape needs the same guard.

## Nulls

`ci/projection-drops.py` — **do not repair it.** The chain's RED
(`gate-stack-gameplay-20260904-231542.log:360`, `sound`'s observer list naming
10 of the 17 check files that read it) was taken at tip `a2a237cd0` and the
stack's own later patches had already fixed it. It is green on the rebased tip.
Re-gating an older tip of a moving stack measures a tree nobody proposes to
merge.

The six branches left out of the stack stay out; their reasons are in
`docs/FINDINGS.md` on this branch and none of them changed.

## The third red, and whose it was

`ci/mob-tick-census.py --check`: `our_entity_kinds pinned 2, measured 3`.
**Not main's.** `git diff f4047ae1f f4fa28ff8 -- crates/rustmc-server/src/entities.rs`
is empty and main still declares two; the third is this stack's own
`2313ff73c`, which added `FALLING_BLOCK` with the block half of a gravity
block and did not re-pin. `ci/checks/mob-surface.json`'s `_expected_to_move`
says re-pinning `our_entity_kinds` belongs in the same commit as the port, so
this is the gate working one fold late — check that before assuming a rebase
broke it.

Re-derived rather than edited to match: **17 figures, exactly one moved.**
Every jar-derived figure is identical (158 entity types, 86 spawning, 114 `Mob`
subtypes, 115 hierarchy classes, 3,870 declarations, 1,754 ai methods, 164
pathfinder methods), so the jar pin is intact. `our_mob_kinds` is still 0 —
`FALLING_BLOCK` is not a mob — so §94's conclusion is unchanged.

**Its reason is not.** "Unreachable by construction" was written when the
workspace held zero occurrences of `GoalSelector`, `NaturalSpawner`,
`PathNavigation`, `registerGoals`, `serverAiStep`, `SpawnEggItem` or
`spawn_egg`. Since main `f4fa28ff8` five of those seven are present —
`mobs-goals` landed the goal system and the pathfinder in
`crates/rustmc-entity/src/ai`, and `serverAiStep` reaches
`crates/rustmc-server/src/entities.rs`. The three still at zero are
`NaturalSpawner`, `SpawnEggItem` and `spawn_egg`, and they are now the whole of
the gap: the mob code exists and nothing can be spawned into it. Corrected in
§94 and appended to `docs/cold-start/mob-tick-surface.md`.

## The next item

**A corpus run under this comparator.** 38 of the 65 recordings on the box are
unreadable to it (`probe` role), including the only two that hold the 25 new
menu/entity scenarios, and the 15 checks folded since are `unseen` on top of
those. Until that run exists, `history` cannot say anything about 40 of this
stack's 148 checks, and `positive_control` is `false` for all of them.
