# The declared/strict gap in the gameplay corpus

Taking apart the difference between the two numbers `ci/t2-gameplay.sh` prints.
Nothing here rests on prose: every verdict comes from a verdict artefact on
disk, every mechanism from the recorded packets, and every packet id from
vanilla's own generated report.

**No corpus run was taken for this document.** All of it is analysis of
measurements that already existed.

---

## 0. Three corrections before anything else

### 0.1 `container-click-quick-move-on-an-empty-slot-changes-nothing` is a Match it has not earned

`ci/checks/gameplay/602-...toml` is baselined `match`
(`ci/checks/gameplay-baseline.toml:886`) and counts toward the declared 56.
**It passes against a server whose `QUICK_MOVE` is a one-line early return,
which is exactly the server we have.**

The proof is three artefacts, none of them the check's own text:

1. `crates/rustmc-server/src/click.rs:506-509` — `do_click` opens with
   `if input != ContainerInput::Pickup { return false; }`. Every one of the
   other six `ContainerInput` values produces no state change and therefore no
   packet. The doc comment above it says so.
2. The two positive controls for the same verb both fail. In the run at
   `main`'s tip, `601-container-click-quick-move-stores-a-hotbar-item` is
   `diverge` with `we never sent: container_set_slot` and
   `at 2: vanilla {container_set_slot slot 0 …}, ours said nothing more`;
   `603-container-click-quick-move-retrieves-a-stored-item` is `diverge` with
   `at 4: vanilla {container_set_slot slot 0 …}, ours said nothing more`.
   Our server answers a `QUICK_MOVE` with silence in **both** directions.
3. 602's own measured projection. In the recording pair of 2026-08-07 02:19,
   602's declared projection is 2 observations on each side and **both are from
   step 0**, the `place` that opens the chest. Its step 1 — the
   `container_click … input = "quick_move"` the check is named for — contributes
   **zero observations on either side**. The check's entire evidentiary content
   is "a chest opens", which `090`, `470`, `560` and `561` already establish.

602 is a negative control (*"this click must change nothing"*), and a negative
control is only evidence when its positive control is live. 561's is
(`560-storing-an-item-in-a-chest` is `match`, and our server does emit
`container_set_slot` for `PICKUP`). 571's is (`570` is `match`). 581's is
(`580` is `match`). **602's is not.** It is the only one of the four whose
subject verb is unimplemented, and it is scoring `match` for that reason.

A mechanical sweep over all 56 declared Matches — "which checks have a final
measured step that produced no declared observation on either side" — returns
exactly those four and no others, so this is the complete list of the shape,
not one example of it.

**Effect on the published number: the declared corpus is 55/86 earned, not 56.**
`compare.rs`'s `informative()` cannot see this: it subtracts the ack and the two
`handleUseItemOn` resyncs from the *whole* projection, so a rig step that opens a
container keeps `informative()` at 2 forever no matter what the measured step
does.

### 0.1a What was actually done, 2026-08-07 — branch `parity-cheapest-five`

**602 now carries its own positive control and is baselined `diverge`.** The
scenario gained a third step that `QUICK_MOVE`s a slot that is *not* empty, so
a server whose `QUICK_MOVE` does nothing fails at step 3 instead of passing on
the silence it owes to step 2. Measured on `ci/t2-gameplay-subset.sh`:
`diverge`, `we never sent: container_set_slot`, first difference at step 2 —
the same failure 601 and 603 report, which is the point. `ci/checks/gameplay-
baseline.toml` moved `match` -> `diverge` and **the declared corpus is 55/86.**

The alternative this document proposed — a `positive_control` field, with a
`match` relabelled `inert` while the control is not itself `match` — was
**rejected on a property of the harness, not on taste.** `Verdict::baselineable`
refuses `inert` unconditionally and `main.rs` counts every inert scenario as a
fault whatever the baseline says, so that fix would have left the corpus
permanently red rather than honestly scored. A control inside the scenario keeps
the check in the corpus, keeps the denominator at 86, and makes the verdict
something a run measures rather than something a table asserts. Rule 28 as
drafted below still stands as a rule; what it cannot be is a *relabel*.

The dashboard was changed in the same pass and independently:
`~/grit/tracker` now publishes `declared − unearned` with 602 named, and the
deduction lifts automatically when either of its two conditions stops holding
(602 re-baselined off `match`, or `click.rs` losing its early return). Both
directions are driven in `collect/selftest.py` §2b.

This is the mirror of `docs/ORACLE.md` rule 27 and it is a different failure.
Rule 27 is a check whose **reference** is silenced by the instrument, so both
sides go quiet and the check measures the harness. This is a check whose
**subject** is silent for the one reason the check reads as correct, and the
reference is silent too because vanilla genuinely does nothing. Neither the
reference nor the harness is at fault; the check is, and only for as long as
its verb is unbuilt. It arrives looking the same way rule 27's does — the verb
lands, one check goes green immediately, and the green is the bug — and
`ci/checks/gameplay/README.md` already draws the general form of the argument
("if the server's reply depends on what you claimed, a green on the first run
is evidence about the harness"). Proposed as rule 28: **a check that asserts an
absence must name a positive control, and its `match` is worth nothing while
that control is not itself `match`.**

### 0.2 The strict number is not a property of the tree, and 31/86 was never `main`'s

`declared` is stable and `strict` is not. Across the fourteen full-corpus runs
that scored 56/86 declared, **every one had the identical 56/24/6 declared split
and identical per-scenario declared verdicts**, while strict ranged over five
values:

| finished (2026) | run | declared | strict | gap |
|---|---|---|---|---|
| 08-06 12:44 | `redstone-subject-gate` | 56/86 | 34/86 | 22 |
| 08-06 14:27 | `gate-proof-redstone-power` | 56/86 | 33/86 | 23 |
| 08-06 16:20 | `gate-redstone-power` | 56/86 | 34/86 | 22 |
| 08-06 17:16 | `gate-tick-randomtick-selection` | 56/86 | **31/86** | 25 |
| 08-06 18:14 | `gate-entity-move-collision` | 56/86 | 35/86 | 21 |
| 08-06 19:10 | `gate-village-savanna` | 56/86 | 33/86 | 23 |
| 08-06 20:13 | `gate-determinism` | 56/86 | 33/86 | 23 |
| 08-06 22:39 | `gate-movement-tick` | 56/86 | 32/86 | 24 |
| 08-06 23:43 | `gate-structures-trial` | 56/86 | 35/86 | 21 |
| 08-07 00:32 | `gate-structures-ruined-portal` | 56/86 | 34/86 | 22 |
| 08-07 01:38 | `gate-gate-worktree-release` | 56/86 | **31/86** | 25 |
| 08-07 02:35 | `gate-plugin-hook` | 56/86 | 33/86 | 23 |
| 08-07 03:24 | `gate-storage-dir` | 56/86 | **31/86** | 25 |
| 08-07 04:13 | `gate-ci-count-drift` (**= `main` tip 8be9657**) | 56/86 | **35/86** | **21** |

Logs under `~/grit/private/gates/`; the 12:44 row is `~/grit/logs/`.

**At `main`'s live tip the pair is 56/86 declared and 35/86 strict, and the gap
is 21.** The 31 in circulation is one of the three lowest samples in the series
and belongs to `tick-randomtick-selection`, `gate-worktree-release` or
`storage-dir` — none of which is `main`. A 25-scenario gap is the worst run of
fourteen quoted as if it were a measurement of the tree.

The structure under the spread is: **20 scenarios are in the gap in all twelve
runs whose per-scenario verdicts were recorded; 25 declared Matches are never in
it; 11 are in it between 1 and 10 times out of 12.** The gap is
`20 + (a coin-flip tail)`, and the tail is what moves 31 ↔ 35.

The tail's mechanism is the residual of the one already named in
`compare.rs::project`. The heartbeat filter removed `move_entity`; what is left
is `entity_position` (and `minecraft:waypoint`, below) landing inside a
scenario's window on one side and not the other. In the 02:19 pair,
`lever-attaches-to-a-floor-or-a-wall` differs by exactly one `entity_position`
present on vanilla only and `ladder-hangs-off-the-wall-behind-it` by exactly one
present on ours only — the same packet, in both directions, in the same run.
Neither scenario observes it.

**`strict` therefore has no single value, and a strict figure quoted without its
run identity is not a figure.** `ci/t2-gameplay-selfcheck.sh` already said this
about vanilla against itself; it is equally true of the subject column.

### 0.3 One gap member can never leave the gap, by construction of the instrument

`520-a-standing-player-is-rebroadcast-on-a-timer` is declared `match` and strict
**`vacuous`** in all twelve recorded runs. Its `observe` list is exactly
`["move_entity_heartbeat"]`, and `compare.rs::project`'s heartbeat filter keeps a
heartbeat only when `kinds.is_some_and(|k| k.contains(&o.kind))`. `strict` passes
`kinds = None`, so `is_some_and` is `false` and **every** heartbeat is dropped.
Both strict projections are empty, and an empty reference is `vacuous`.

That is not a statement about the server. It is the one scenario in the corpus
whose entire declared subject is a kind `strict` deletes unconditionally, so it
occupies a permanent slot in the gap that no server change can vacate. It should
either be excluded from the strict denominator or the filter's `kinds.is_some_and`
should be widened to consult the scenario's `observe` list on both denominators.

---

## 1. What the two denominators are, from the code

Both come from one function, `compare::compare` in `tools/oracle-runner/src/compare.rs`,
which calls `project()` twice per scenario and `verdict_for()` on each result.
`Counts::of(&outcomes, strict)` then reads either `o.verdict` or `o.strict`
(lines 938-954). The denominator is `outcomes.len()` in both cases, so the two
numbers are always over the same 86 scenarios; only the projection differs.

| | declared | strict |
|---|---|---|
| `observe_from` filter (which client) | applied | applied |
| `within` bound (Chebyshev, x/z) | applied | applied |
| entity placement table | applied | applied |
| `observe` kind filter | **applied** | **not applied** — every recorded packet, including the ones with no decoder, which travel as `packet:0x..` plus a digest of the undecoded bytes |
| `move_entity_heartbeat` | kept **only** if the scenario names it | kept **only** if the scenario names it — this row read "always dropped (`kinds` is `None`, so the exception cannot fire)" and `569323e` fixed exactly that by reading the `observe` list on both denominators |
| `entity_position` | kept **only** if the scenario names it | kept **only** if the scenario names it. Added 2026-08-07 as `PARITY-ROADMAP.md` §2 R3: the packet fires on `wasOnGround != onGround`, a bit the *client* sends, and the reference and the subject are two separate client sessions. `540` and `080` name it and are the controls |
| `timing` | the scenario's `Timing::Bucket` if it declares one | `Timing::Ignore`, always |
| `collapse_repeats` | applied | applied. This row read "not applied", and `PARITY-ROADMAP.md` §2 R1 changed it 2026-08-07: the collapse was keyed on adjacency **in the whole projection**, so the same declaration collapsed everything in the denominator that had already deleted the interleaving kinds and nothing at all in the one that had not |
| entity renumbering | over the declared projection | over the strict projection, independently |
| `Inert` override | applied (`Match` + `informative()==0` → `Inert`) | **never applied** |

So a scenario counts toward **declared** and not **strict** when, inside its own
box and on its own client, the two servers said the same thing about the kinds
the check named *and a different thing about something else*. Three sources of
"something else", in order of size measured below: a kind no check names
(`sound`, `level_event`, entity metadata); a packet no decoder covers
(`waypoint`, `rotate_head`, `recipe_book_add`, `update_attributes`,
`bundle_delimiter`); and the repeat-cadence of a kind a check chose to collapse.
Timing cannot contribute, because strict nulls it on both sides.

The reverse direction — strict Match, declared not — is possible in principle
(`Inert` is declared-only; `collapse_repeats` was too until 2026-08-07) and
**does not occur**. In all
twelve runs with per-scenario detail, and exactly in the cross-table below,
every strict Match is also a declared Match. So the gap is precisely
`declared_match ∧ ¬strict_match`, and `declared − strict` counts it without
cancellation.

---

## 2. The classification, at `main`'s tip

**Run read:** `~/rustmc-gate/oracle-work/gameplay/report.json`, written
2026-08-07 04:13 by `ci/t2-gameplay.sh` under
`ci/handoff.sh branch=ci-count-drift tip=8be9657709b53e7efd4c8d6ad4dd69cb55573db6`,
which is `origin/main`'s tip. Reference: vanilla 26.2 on `127.0.0.1:26502`;
subject: `rustmc-server` on `:26503`; superflat, creative, peaceful,
`generate-structures=false`, `spawn-radius=0`, `view-distance=4`, seed 20260803.
Console log: `~/grit/private/gates/gate-ci-count-drift-20260807-032550.log`.
**Not stale**: it is the most recent full-corpus run in existence and was 34
minutes old when this document was written.

```
CORPUS  declared  56/86 match   (24 diverge, 6 absent, 0 vacuous, 0 inert, 0 error)
CORPUS  strict    35/86 match   (47 diverge, 3 absent, 1 vacuous, 0 inert, 0 error)
```

Cross-table (from `report.json`, all 86):

| declared \ strict | match | diverge | absent | vacuous |
|---|---|---|---|---|
| **match** (56) | 35 | 20 | 0 | 1 |
| **diverge** (24) | 0 | 24 | 0 | 0 |
| **absent** (6) | 0 | 3 | 3 | 0 |

**The gap is the 20 + 1 in the top row: 21 scenarios.**

### 2.1 The instrument used for the "why" column

`report.json` records each scenario's strict *verdict* but not what the strict
comparison saw. Recovering that needs both traces, and `main`'s own vanilla
recording had already been overwritten by the next gate. The trace pair used
instead is `~/rustmc-chatcmd/oracle-work/gameplay/{vanilla,rustmc}.json`,
recorded 2026-08-07 02:02 and 02:19 on branch `chatcmd` (`1da9bc7`), whose corpus
is `main`'s 86 plus 2. It is a legitimate proxy and that is checked rather than
assumed: **all 86 shared scenarios have identical declared verdicts to `main`'s
tip**, and the strict verdicts differ on three of the eleven flaky members only.

The per-observation detail was produced by a Python replica of
`compare.rs::{project, collapse_runs, renumber_entities, informative,
verdict_for}`. A second instrument is only admissible if it is pinned to the
first, so it was: **the replica reproduces that run's own `report.json` exactly —
all 88 declared verdicts, all 88 strict verdicts, and both observation counts for
all 88 — with zero disagreements.** Any figure below that the replica alone
produces is marked as such.

### 2.2 All 86 scenarios

`in gap` shows how often the scenario was declared-`match` and strict-non-`match`
across the twelve gate runs from 2026-08-06 16:20 to 2026-08-07 04:13 that carry
per-scenario detail. `strict delta` is the per-kind count difference between the
two strict projections in the 02:19 pair, vanilla → ours; `-` means the strict
projections were identical **in that pair**. A row marked `yes` with a `-` delta
is the flake of §0.2 — the divergence is real at `main`'s tip and did not
reproduce in the diagnostic pair, which is the whole point of the flake — except
for `520`, whose `-` is the structural emptiness of §0.3. A row with a non-empty
delta and no `yes` is a declared-`diverge` or declared-`absent` scenario, where
the same missing kinds are visible under both projections.

| # | scenario | declared | strict | in gap | strict delta (vanilla -> ours), 02:19 pair |
|---|---|---|---|---|---|
| 010 | `break-block` | match | match |  | - |
| 020 | `place-block` | match | match |  | - |
| 030 | `place-log-axis` | match | match |  | - |
| 040 | `stairs-and-furnace-face-apart` | match | diverge | **yes** (12/12) | `recipe_book_add` 1->0 |
| 050 | `torch-falls-with-its-support` | match | diverge | **yes** (12/12) | `add_entity` 1->0; `container_set_slot` 1->0; `level_event` 1->0; `bundle_delimiter` 2->0; `remove_entities` 1->0; `set_entity_data` 2->0; `take_item_entity` 1->0 |
| 060 | `coral-dries-out-unprompted` | match | match |  | - |
| 070 | `sand-falls-when-unsupported` | diverge | diverge |  | `add_entity` 2->0; `block_update` 8->2; `level_event` 1->0; `bundle_delimiter` 4->0; `remove_entities` 2->0; `set_entity_data` 4->0 |
| 080 | `other-player-is-an-entity` | match | ~~diverge~~ **match** (2026-08-09) | **yes** (12/12) | ~~`waypoint` 2->0; `sound` 2->0~~ — closed |
| 080 | `spawn-egg-makes-a-mob` | absent | diverge |  | `add_entity` 2->0; `entity_position` 2->0; `bundle_delimiter` 4->0; `move_entity_rot` 2->0; `rotate_head` 16->0; `update_attributes` 2->0; `set_entity_data` 4->0; `set_entity_motion` 2->0; `sound` 4->0 |
| 090 | `chest-opens-a-screen` | match | diverge | **yes** (12/12) | `rotate_head` 7->0; `sound` 1->0 |
| 100 | `redstone-torch-powers-wire` | match | match |  | - |
| 110 | `server-answers-a-command` | match | match |  | - |
| 120 | `slab-half-follows-the-cursor` | match | match |  | - |
| 130 | `door-places-both-halves` | match | match | (1/12) | - |
| 131 | `a-copper-door-places-both-halves` | match | match |  | - |
| 132 | `digging-the-top-of-a-door-takes-the-bottom` | match | diverge | **yes** (12/12) | `level_event` 1->0 |
| 140 | `fences-connect-to-each-other` | match | match | (1/12) | - |
| 150 | `torch-on-a-wall-faces-out` | match | diverge | **yes** (10/12) | - |
| 160 | `water-spreads` — **PARKED 2026-08-10, not in the corpus** | absent | absent |  | `block_changed_ack` 1->0; `block_update` 4->0; `recipe_book_add` 10->0; `waypoint` 162->0; `section_blocks_update` 24->0; `sound` 7->0 |
| 170 | `slab-stacks-into-a-double-slab` | diverge | diverge |  | - |
| 180 | `stairs-make-a-corner-when-two-meet` | match | match | (1/12) | - |
| 190 | `lever-attaches-to-a-floor-or-a-wall` | match | match |  | `entity_position` 1->0 |
| 200 | `ladder-hangs-off-the-wall-behind-it` | match | match |  | `entity_position` 0->1 |
| 210 | `trapdoor-half-follows-the-cursor` | match | match | (1/12) | - |
| 220 | `repeater-faces-away-from-the-player` | match | match | (4/12) | - |
| 230 | `bed-places-two-blocks` | match | match |  | - |
| 235 | `a-bed-facing-north-puts-its-head-the-other-way` | match | match |  | - |
| 236 | `digging-a-bed-takes-both-halves` | match | diverge | **yes** (12/12) | `level_event` 1->0 |
| 237 | `digging-a-tall-flower-takes-both-halves` | match | diverge | **yes** (12/12) | `level_event` 1->0 |
| 238 | `digging-the-top-of-a-tall-flower-takes-the-bottom` | match | diverge | **yes** (12/12) | `level_event` 1->0 |
| 239 | `tall-grass-places-both-halves` | match | match |  | - |
| 240 | `chest-pairs-with-its-neighbour` | match | match | (3/12) | - |
| 250 | `snow-layers-stack` | diverge | diverge |  | `block_update` 5->4 |
| 260 | `digging-a-door-takes-both-halves` | match | diverge | **yes** (12/12) | `level_event` 1->0 |
| 270 | `sign-takes-a-sixteenth-of-a-turn` | match | match |  | - |
| 280 | `door-opens-when-right-clicked` | match | match |  | - |
| 281 | `clicking-the-top-of-a-door-opens-the-bottom` | match | match |  | - |
| 290 | `lever-flips-when-right-clicked` | match | diverge | **yes** (12/12) | `sound` 1->0 |
| 300 | `rail-curves-towards-its-neighbour` | match | match | (1/12) | - |
| 310 | `hoe-turns-grass-into-farmland` | diverge | diverge |  | `block_update` 3->2 |
| 320 | `flint-and-steel-lights-a-fire` | diverge | diverge |  | `block_update` 3->2 |
| 330 | `button-unpresses-itself` | match | diverge | **yes** (12/12) | `sound` 1->0 |
| 335 | `a-wooden-button-stays-pressed-longer` | match | diverge | **yes** (12/12) | `sound` 1->0 |
| 350 | `a-dropped-item-goes-into-your-inventory` | absent | diverge |  | `add_entity` 1->0; `container_set_slot` 1->0; `level_event` 1->0; `bundle_delimiter` 2->0; `remove_entities` 1->0; `set_entity_data` 2->0; `take_item_entity` 1->0 |
| 360 | `a-lever-lights-a-lamp` | match | diverge | **yes** (12/12) | `sound` 1->0 |
| 370 | `wire-power-falls-off-with-distance` | match | match | (2/12) | - |
| 380 | `a-piston-extends-when-powered` | diverge | diverge |  | `block_event` 1->0; `block_update` 5->3; `sound` 1->0 |
| 390 | `a-piston-pushes-the-block-in-front` | diverge | diverge |  | `block_event` 1->0; `block_update` 4->3; `section_blocks_update` 1->0; `sound` 1->0 |
| 400 | `a-redstone-torch-turns-off-when-its-block-is-powered` | diverge | diverge |  | `block_update` 4->3; `sound` 1->0 |
| 410 | `a-repeater-delays-the-signal-it-passes-on` | diverge | diverge |  | `entity_position` 1->0; `section_blocks_update` 1->0 |
| 420 | `a-comparator-switches-to-subtract-mode` | diverge | diverge |  | `sound` 1->0 |
| 430 | `a-note-block-retunes-when-right-clicked` | diverge | diverge |  | `block_event` 1->0; `sound` 1->0 |
| 440 | `an-observer-pulses-at-the-block-it-faces` | diverge | diverge |  | `block_update` 5->3 |
| 450 | `redstone-wire-climbs-the-block-beside-it` | match | match |  | - |
| 460 | `placing-a-sign-opens-its-editor` | match | match |  | - |
| 470 | `a-furnace-opens-a-screen-a-chest-does-not` | match | match | (6/12) | - |
| 480 | `a-crafting-table-opens-a-menu-with-no-block-entity` | match | match |  | - |
| 490 | `a-bystander-sees-a-chest-open` | match | diverge | **yes** (12/12) | `sound` 1->0 |
| 500 | `dropping-an-item-throws-it-on-the-floor` | diverge | diverge |  | `entity_position` 1->0; `move_entity` 2->0; `remove_entities` 1->0; `set_entity_data` 2->0; `set_entity_motion` 4->0; `set_equipment` 1->0; `take_item_entity` 1->0 |
| 510 | `swapping-to-the-offhand-shows-on-both-channels` | absent | diverge |  | `block_changed_ack` 0->1; `container_set_slot` 2->0; `entity_event` 1->0 |
| 520 | `a-standing-player-is-rebroadcast-on-a-timer` | match | vacuous | **yes** (12/12) | - |
| 530 | `walking-away-drops-a-standing-player-from-view` | match | diverge | **yes** (12/12) | `update_attributes` 1->0; `set_entity_data` 1->0; `set_equipment` 1->0 |
| 540 | `a-player-that-leaves-the-ground-is-resynced` | diverge | diverge |  | `move_entity` 1->0; `waypoint` 2->0; `sound` 2->0 |
| 550 | `attacking-another-player` | absent | absent |  | `sound` 1->0 |
| 551 | `attacking-from-out-of-range` | absent | absent |  | `sound` 1->0 |
| 552 | `sneaking-with-a-block-in-hand-places-it-on-a-lever` | diverge | diverge |  | - |
| 553 | `a-block-in-hand-does-not-stop-a-lever-flipping` | match | diverge | **yes** (12/12) | `sound` 1->0 |
| 560 | `storing-an-item-in-a-chest` | match | diverge | **yes** (12/12) | `sound` 1->0 |
| 561 | `clicking-an-empty-chest-slot-changes-nothing` | match | diverge | **yes** (12/12) | `sound` 1->0 |
| 562 | `a-crafting-table-turns-a-log-into-planks` | match | match |  | - |
| 570 | `typing-on-a-sign-you-placed-sets-its-text` | match | match |  | - |
| 571 | `a-second-edit-of-the-same-sign-is-ignored` | match | match | (5/12) | - |
| 572 | `a-sign-update-for-the-back-face-is-carried` | match | match |  | - |
| 580 | `swinging-an-arm-shows-to-a-bystander` | match | match |  | - |
| 581 | `a-second-swing-inside-the-animation-is-swallowed` | match | match |  | - |
| 582 | `the-off-hand-swings-with-a-different-action` | match | match |  | - |
| 601 | `container-click-quick-move-stores-a-hotbar-item` | diverge | diverge |  | `container_set_slot` 2->0; `entity_position` 1->0; `sound` 1->0 |
| 602 | `container-click-quick-move-on-an-empty-slot-changes-nothing` | match | diverge | **yes** (12/12) | `sound` 1->0 |
| 603 | `container-click-quick-move-retrieves-a-stored-item` | diverge | diverge |  | `container_set_slot` 4->2; `sound` 1->0 |
| 604 | `container-click-swap-exchanges-a-hotbar-slot` | diverge | diverge |  | `container_set_slot` 4->2; `sound` 1->0 |
| 605 | `container-click-throw-drops-one-item` | diverge | diverge |  | `add_entity` 1->0; `container_set_slot` 5->4; `move_entity` 2->0; `bundle_delimiter` 2->0; `set_entity_data` 2->0; `set_entity_motion` 4->0; `sound` 1->0 |
| 606 | `container-click-throw-drops-the-whole-stack` | diverge | diverge |  | `add_entity` 1->0; `container_set_slot` 5->4; `entity_position` 1->0; `move_entity` 2->0; `bundle_delimiter` 2->0; `set_entity_data` 2->0; `set_entity_motion` 4->0; `sound` 1->0 |
| 607 | `container-click-pickup-all-gathers-matching-stacks` | diverge | diverge |  | `sound` 1->0 |
| 608 | `container-click-with-an-out-of-range-slot-changes-nothing` | diverge | diverge |  | `sound` 1->0 |
| 609 | `sneaking-with-a-block-in-hand-does-not-open-a-chest` | diverge | diverge |  | `block_event` 0->8; `block_update` 3->2; `container_set_content` 0->1; `open_screen` 0->1 |
| 611 | `closing-a-crafting-table-returns-the-grid-contents` | diverge | diverge |  | `container_set_slot` 4->3; `set_player_inventory` 1->0 |

---

## 3. The gap ranked by cost to close

"Close" here means converting the scenario to a strict `Match`; all 21 are
already declared `Match`. Blocker classes are the three the brief asks for:
**(a)** unimplemented server behaviour, **(b)** the scenario cannot presently
observe what it claims to test, **(c)** a harness limitation.

| group | scenarios | what is missing | blocker |
|---|---|---|---|
| **G1 `level_event`** | 132, 236, 237, 238, 260 | exactly one `ClientboundLevelEventPacket`, event 2001, per scenario, and nothing else | (a) |
| **G2 click sounds** | 290, 330, 335, 360, 553 | exactly one `ClientboundSoundPacket` per scenario, constant pitch (lever 0.6, button 1.0) | (a) |
| **G3 chest sounds** | 490, 560, 561, 602 | exactly one `ClientboundSoundPacket` per scenario, **random pitch** (measured 0.9268, 0.9339, 0.9552, 0.9952 for the same event) | (a) + (c) |
| **G4 heartbeat-only check** | 520 | nothing — strict deletes the only kind it observes | (c), permanent |
| **G5 entity tracking** | 530 | `set_entity_data`, `set_equipment`, `update_attributes` (packet 131) when a player re-enters tracking range | (a) |
| **G6 flake** | 150 | nothing — an unobserved `entity_position` on one side only | (c) |
| **G7 head rotation** | 090 | `rotate_head` (packet 83) ×7, plus a G3 chest sound | (a) + (c) |
| **G8 recipe book** | 040 | `recipe_book_add` (packet 74) ×1 | (a) |
| **G9 waypoint** | 080 | ~~`waypoint` (packet 138) ×2, plus `sound` ×2~~ — **CLOSED 2026-08-09** by `gameplay-place-sound`: 080 is a strict `match`, corpus strict 57/86 -> 58/86. Both 0x8A bodies are byte-exact and both are `TRACK`, not `UPDATE` (`BlockConnection.isBroken()` is `distanceManhattan() > 1`) | (a); also (b) |
| **G10 drop cascade** | 050 | `level_event`, `add_entity`, `set_entity_data` ×2, `take_item_entity`, `container_set_slot`, `remove_entities`, `bundle_delimiter` ×2 — the whole break-drop-pickup chain | (a); also (b) |

Split by blocker: **(a) 18**, **(b) 3** (050, 080, 602 — overlapping with (a)),
**(c) 7** (090, 150, 490, 520, 560, 561, 602).

The three (b) cases, each with the assertion that cannot observe its subject:

* **602** — §0.1. Its measured step contributes zero observations on both sides;
  the verdict rests entirely on the rig step. It is the only one of the three
  where (b) makes the declared verdict wrong.
* **050 `torch-falls-with-its-support`** — `observe` is
  `["block_update", "section_blocks_update", "block_changed_ack"]`. Vanilla's
  actor stream for this scenario carries 11 observations; nine of them are the
  torch dropping as an item and being picked up (`add_entity`,
  `set_entity_data` ×2, `take_item_entity`, `container_set_slot`,
  `remove_entities`, `level_event`, two bundle delimiters). The check is named
  for a torch falling and cannot see that vanilla also handed the player a torch.
  Not a wrong verdict — the block half is genuinely right — but the scenario's
  coverage is a third of what its own rig provokes.
  **2026-08-08: the nine it cannot see are not merely unobserved, they are
  unreproducible.** Vanilla fails 050 against itself under `strict` and the
  drop chain is why: whether the torch is picked up inside the window is
  decided by three `Level.random` draws. No port converts it.
  `docs/PARITY-ROADMAP.md` §11.
* **080 `other-player-is-an-entity`** — observes `["move_entity", "entity_position"]`.
  In its window vanilla sends no `move_entity` at all; the whole declared
  projection is `entity_position`, which our server produces by copying the
  client's own packet (`world_send.rs`). The check matches without any server-side
  entity movement existing, which is the state `540` reports as `diverge`.

### 3.1 The trap in G1, named before somebody walks into it

`010-break-block` is strict-`match` today and must stay that way. The reason it
is not in G1 is visible in the raw trace: for a block the player dug themself,
the actor's stream carries **no** `level_event` and the observer's carries one at
the same position and the same millisecond. The five G1 scenarios each carry
**one** `level_event` on the *actor's* stream, for the position destroyed as a
side effect rather than by the dig. `DoublePlantBlock`'s cascade site
(javap `-p -c -constants`, offsets 102–114) passes an `Entity` to
`Level.levelEvent(Entity, int, BlockPos, int)`, and `ServerLevel`'s implementation
of that overload broadcasts through `PlayerList.broadcast(Player, …)`, which
excludes its first argument; the 3-argument `Level.levelEvent(int, BlockPos, int)`
does not. **Which of the two sites emits the packet the actor actually receives
must be read out of the bytecode per block family before anything is written** —
a `level_event` emitted at the player's own destroy site would turn
`010-break-block` from strict-`match` into strict-`diverge` and would be a
regression bought with the fix.

---

## 4. The cheapest five, in order

1. **`level_event` 2001 on the cascade destroy — closes 5** (132, 236, 237, 238,
   260). One clientbound packet; each of the five differs from vanilla by exactly
   one of them and by nothing else. Read the emission site first (§3.1). Best
   ratio in the gap by a distance.
2. **`sound` on lever flip and button press/release — closes 5** (290, 330, 335,
   360, 553). One clientbound packet plus the ids the traces already record:
   lever 935 at pitch 0.6 volume 0.3, stone button 1597, wooden button 1849, all
   constant pitch, so nothing in the harness has to change. Each of the five
   differs by exactly one `sound` and nothing else. Note the same actor
   exclusion as §3.1: the actor's stream carries only the *un-press* (1597 /
   1849) while the observer's carries the press as well (1598 / 1850), so an
   implementation that broadcasts both to everybody trades this group for a new
   divergence on the observer channel.
3. **Drop `sound.pitch` from the comparison — closes 0 alone, unblocks 4.**
   `trace.rs`'s sound arm already reads and discards the `long seed` for exactly
   this reason ("a dimension no check claims to test, removed from the comparison
   rather than declared unstable"). The chest-open pitch is drawn the same way and
   was measured at four different values for the same event in one run. Until it
   goes, G3 is unreachable however right the server is. One line, beside the
   existing `let _seed`.
4. **Chest-open `sound` — closes 4** (490, 560, 561, 602), given (3). Same packet
   as (2); only the pitch stands in the way.
5. **`520`'s strict `vacuous` — closes 1, permanently.** Either exclude the
   scenario from the strict denominator or make the heartbeat filter consult the
   scenario's `observe` list on both denominators rather than only on `declared`.
   One line, and it removes a member of the gap that no amount of server work
   could ever remove.

Together: **16 of 21**, of which 10 are two clientbound packets.

Left over, and correctly so: `530` (entity metadata at tracking start), `090`
(`rotate_head`), `040` (`recipe_book_add`), `080` (`waypoint`), `050` (the item
drop and pickup cascade, which is the same work as `350` and `500` and is blocked
behind entity motion). `150` is not work at all — it is the flake tail of §0.2.

---

## 5. What vanilla says and we do not, corpus-wide

Per-kind totals over all 88 strict projections of the 02:19 pair (replica; the
projections are the ones the harness compares). This is the raw material of the
gap and of much of the diverge column too.

| kind | vanilla | ours |
|---|---|---|
| `waypoint` (packet 138) | 166 | 0 |
| `section_blocks_update` | 49 | 23 |
| `sound` | 39 | 0 |
| `container_set_slot` | 38 | 25 |
| `rotate_head` (packet 83) | 23 | 0 |
| `bundle_delimiter` (packet 0) | 20 | 4 |
| `set_entity_data` | 19 | 0 |
| `set_entity_motion` | 14 | 0 |
| `set_equipment` | 2 | 0 |
| `recipe_book_add` (packet 74) | 11 | 0 |
| `entity_position` | 10 | 4 |
| `add_entity` | 10 | 2 |
| `move_entity` | 9 | 2 |
| `level_event` | 8 | 0 |
| `remove_entities` | 6 | 1 |
| `take_item_entity` | 3 | 0 |
| `update_attributes` (packet 131) | 3 | 0 |
| `move_entity_rot` (packet 56) | 2 | 0 |
| `set_player_inventory` (packet 108) | 1 | 0 |
| `entity_event` | 1 | 0 |
| `block_update` | 203 | 183 |
| `block_event` | 114 | 119 |
| `container_set_content` | 15 | 16 |
| `open_screen` | 15 | 16 |
| `set_cursor_item` (packet 96) | 13 | 13 |
| `container_set_data` (packet 19) | 4 | 4 |

Numeric packet ids resolved from `oracle-work/vanilla-26.2/generated/reports/packets.json`
— vanilla's own generated report, not from memory.

`waypoint` is the largest single undecoded contributor at 166 occurrences to our
zero, and it has no decoder arm, so it travels as `packet:0x8A` plus a digest.
It is not in the gap analysis above because it happens to fall outside most
scenarios' windows, but any future check with a wider window will meet it.

---

## 6. What this changes

* **`docs/SERVER.md` and anything else that quotes a strict figure needs the run
  identity beside it.** Strict at a fixed tree is a five-valued quantity today.
* **The declared corpus is 55/86 earned.** 602 is a `match` a server without
  `QUICK_MOVE` earns, and it should be `inert` until the verb exists.
* **`informative()` cannot catch 602's shape.** The generalisation it is missing
  is not per-scenario but per-corpus: a check that asserts an absence needs a
  named positive control, and a `match` on it is worthless while that control is
  not itself `match`.
* **Two of the gap's 21 members are instrument artefacts, not deferred
  verification** (520 permanently, 150 as flake), so the honest reading of the
  gap at `main`'s tip is **19 scenarios of real deferred verification, not 25**.

---

## 7. The cheapest five, implemented and measured — 2026-08-07, `parity-cheapest-five`

Everything below is a **measured** verdict from `ci/t2-gameplay-subset.sh` over
the 23 scenarios this branch touches or risks, run on the Threadripper in
`~/rustmc-cheapfive` against real vanilla 26.2. No verdict here rests on the
code that produced it. **A subset is not a corpus figure** — the denominator is
whatever was passed on the command line, and §4's own instrument note says so —
so nothing here is quoted as a corpus score. What a subset *can* say is which
verdict a named scenario reached, and that is all it is used for.

### 7.1 The exclusion rule, derived before anything was written

§3.1 left one question open and it decides the whole of item 1: which of
vanilla's two `levelEvent` overloads produces the packet the **actor** receives.
Answered from the bytecode, and then checked against the recorded traces:

```text
  Level.levelEvent(Entity, int, BlockPos, int)     -- ServerLevel, offsets 0-63
      -> PlayerList.broadcast(entity instanceof Player ? p : null, ..., 64.0, ...)
      -> offsets 33-39:  if (players.get(i) == except) continue;   <- EXCLUDES
  Level.levelEvent(int, BlockPos, int)             -- no entity, excludes nobody
```

Three sites pass a real player and are therefore invisible to the digger:
`Block.spawnDestroyParticles` (reached from `Block.playerWillDestroy`, and the
reason `010-break-block`'s actor sees nothing),
`DoublePlantBlock.preventDropFromBottomPart` (offsets 102-114) and
`BedBlock.playerWillDestroy` (offsets 102-115).

**One site passes `null`, and it is the one the corpus needs.**
`Block.updateOrDestroy`'s air branch, offsets 21-39, calls
`level.destroyBlock(pos, (flags & 32) == 0, null, recursionLeft)` — `aconst_null`
in the entity slot — and `Level.destroyBlock` offsets 24-45 emits
`levelEvent(2001, pos, Block.getId(state))` through the three-argument overload.
So **a block turned to air by a shape cascade is announced to everybody,
including the player whose dig caused the cascade.**

That reading reproduces the trace pair of 02:19 packet for packet, and the two
upper-half digs are the interesting case: `preventDropFromBottomPart` writes air
*below*, the cascade comes back *up* and destroys the upper half, and the actor's
one event is therefore at the **upper** position for a dig at either half.

So the emission went in exactly one place — `crate::shapes::destroy_block` — and
`010-break-block` is untouched by construction rather than by luck.

### 7.2 The control run, which is what makes the numbers below evidence

Every verdict in this section comes from **two runs of the same instrument over
the same 23 scenarios**, differing only in the tree under test:

* **before** — `e894224`, this branch's first commit, which changes `602` and
  nothing else. Log: `threadripper:~/grit-logs-cheapfive-before.log`.
* **after** — the branch tip. Log:
  `threadripper:~/grit-logs-cheapfive-subset2.log`.

```
before   CORPUS  declared  20/23 match    strict   3/23 match  (19 diverge, 1 vacuous)
after    CORPUS  declared  20/23 match    strict  16/23 match  ( 7 diverge, 0 vacuous)
```

A "before" taken from the twelve gate runs would have been a different
instrument measuring a different corpus; this one is the same script, the same
scenarios, the same subset ordering and the same box, so a verdict that moved
moved because the tree did.

| # | scenario | strict before | strict after |
|---|---|---|---|
| 010 | `break-block` | **match** | **match** |
| 050 | `torch-falls-with-its-support` | diverge | diverge |
| 060 | `coral-dries-out-unprompted` | diverge | diverge |
| 090 | `chest-opens-a-screen` | diverge | **match** |
| 132 | `digging-the-top-of-a-door-takes-the-bottom` | diverge | **match** |
| 236 | `digging-a-bed-takes-both-halves` | diverge | **match** |
| 237 | `digging-a-tall-flower-takes-both-halves` | diverge | diverge |
| 238 | `digging-the-top-of-a-tall-flower-takes-the-bottom` | diverge | **match** |
| 260 | `digging-a-door-takes-both-halves` | diverge | **match** |
| 290 | `lever-flips-when-right-clicked` | diverge | **match** |
| 330 | `button-unpresses-itself` | diverge | **match** |
| 335 | `a-wooden-button-stays-pressed-longer` | diverge | **match** |
| 360 | `a-lever-lights-a-lamp` | diverge | **match** |
| 470 | `a-furnace-opens-a-screen-a-chest-does-not` | match | match |
| 480 | `a-crafting-table-opens-a-menu-with-no-block-entity` | match | match |
| 490 | `a-bystander-sees-a-chest-open` | diverge | **match** |
| 520 | `a-standing-player-is-rebroadcast-on-a-timer` | **vacuous** | **match** |
| 553 | `a-block-in-hand-does-not-stop-a-lever-flipping` | diverge | **match** |
| 560 | `storing-an-item-in-a-chest` | diverge | diverge |
| 561 | `clicking-an-empty-chest-slot-changes-nothing` | diverge | **match** |
| 601 | `container-click-quick-move-stores-a-hotbar-item` | declared diverge | declared diverge |
| 602 | `container-click-quick-move-on-an-empty-slot-changes-nothing` | declared diverge | declared diverge |
| 603 | `container-click-quick-move-retrieves-a-stored-item` | declared diverge | declared diverge |

**Thirteen scenarios converted. None regressed.** `470` and `480` were carried
specifically because they are the two container checks that must *not* gain a
sound, and both held.

**`010-break-block` is a strict `Match` in both runs.** The trap §3.1 named was
real — the exclusion decides which site may emit — and it was avoided by
deriving the rule before writing the emission, not by trying it.

**`060` was strict `diverge` in the control as well**, so it is not a regression
and never was one. It is the subset's own leftover ordering, not this branch:
060 reads the actor's stream, its difference is one `entity_position`, and this
branch emits no `entity_position` and puts neither a `sound` nor a
`level_event` anywhere in 060's projection.

### 7.3 The five items, with the verdicts that actually moved

| item | §4 predicted | measured |
|---|---|---|
| 1. `level_event` 2001 on the cascade destroy | 132, 236, 237, 238, 260 — five | **four**: 132, 236, 238, 260. 236 needed a second fix (§7.4); 237 is blocked on an `entity_position`, not on the packet (§7.5) |
| 2. lever / button `sound` | 290, 330, 335, 360, 553 — five | **five**, all of them |
| 3. drop `sound.pitch` | closes 0, unblocks 4 | one line in `trace.rs` beside the existing `let _seed`; without it nothing in item 4 could match |
| 4. chest-open `sound` | 490, 560, 561, 602 — four | **two** — 490, 561 — **plus 090**, which §3 had assigned to G7. 560 is blocked on the lid cadence (§7.5); 602 is declared-`diverge` by §0.1a |
| 5. `520`'s strict filter | 520 — one, permanently | **one**, `vacuous` -> `match` |

**Thirteen against a prediction of sixteen**, and the three-scenario shortfall is
not where §4 expected: 237 and 560 turn out to be blocked on the two instrument
artefacts §0.2 and §1 already describe, while 090 — predicted to need
`rotate_head` — converted without it.

**090's conversion is the one number here that is a property of the run.** At
`main`'s tip vanilla put seven `rotate_head` packets in 090's window; in both
subset runs it put none, on either side. So 090 converted here because the
window was quiet, and G7's `rotate_head` gap is untouched and real. It is
counted in the thirteen because that is what was measured, and flagged because
a full-corpus run may well not reproduce it.

### 7.4 What the measurement found that the analysis could not

**236 was wrong in a way only the new packet could show.** With item 1 in and
nothing else, `236-digging-a-bed-takes-both-halves` still read `diverge`, and the
trace says why: vanilla's actor event is at `[1, 0, 0]` carrying state 2162 (the
**foot**), ours was at `[1, 0, 1]` carrying 2161 (the **head**).

The block outcome had always been right — both halves went, and `236` was
`match` — because `remove_block` at the foot cascaded through
`BedBlock.updateShape` at the head. Vanilla does the opposite:
`BedBlock.playerWillDestroy` clears the **head** by hand with flags 35, and the
cascade that write sets off destroys the **foot**. Same two blocks, same
`section_blocks_update`, different block destroyed by the cascade — and
invisible for as long as no `level_event` existed to carry which.

`BedBlock.playerWillDestroy` is now ported (`crate::shapes::bed_will_destroy`)
and 236 is a strict `match`. This is the shape `docs/AGENT-BRIEF.md` warns
about — *reproducing the observable and not the mechanism* — caught by a packet
rather than by a reading.

### 7.5 What did not convert, and why — measured, not assumed

* **237 `digging-a-tall-flower-takes-both-halves`** — the `level_event` is
  **correct**: `[1, 1, 0]`, data 12915, on the actor's stream, identical to
  vanilla's. What differs is one `entity_position` present on ours and not on
  vanilla's. That is §0.2's flake, in the same direction §0.2 recorded for
  `ladder-hangs-off-the-wall-behind-it`, and no server change closes it.
* **560 `storing-an-item-in-a-chest`** — the chest sound is present and
  identical. What differs is the **step attribution of the chest lid's
  `block_event`**: vanilla's second one lands in step 1, ours in step 0, and
  strict does not apply the scenario's own `collapse_repeats`. That is §1's
  third source — "the repeat-cadence of a kind a check chose to collapse" —
  and G3 did not predict it because a per-kind *count* delta cannot see an
  ordering difference. Whether it converts in a full-corpus run, where the
  cadence phase differs, is **not established by this run**.
* **602** — declared `diverge` by design; see §0.1a.
* **050 `torch-falls-with-its-support`** — the `level_event` it was missing is
  now emitted, and it is still `diverge` on the whole item drop-and-pickup
  chain (`add_entity`, `set_entity_data` ×2, `take_item_entity`,
  `container_set_slot`, `remove_entities`, two bundle delimiters). §3 said so
  and this run agrees; it is the same work as `350` and `500`.
  **And it is the same *outcome* as 350 and 500, which was the part nobody
  drew out until 2026-08-08: both of those are declared in
  `ci/checks/gameplay-unstable.toml` as checks nothing can pass, and 050
  inherits their instability whole because its strict projection contains
  350's declared one. `docs/PARITY-ROADMAP.md` §11.**
* **060 `coral-dries-out-unprompted`** is *not* on this list and is worth a
  line anyway, because it looked like a regression until the control run said
  otherwise. It reads strict `diverge` in **both** subset runs where it is
  strict `match` at `main`'s tip, so the difference belongs to the subset's
  leftover ordering and not to this branch — which the mechanism confirms
  independently: 060 reads the actor's stream, its actor-side difference is one
  `entity_position` present on vanilla only, and this branch emits no
  `entity_position` and puts neither a `sound` nor a `level_event` in 060's
  projection at all. A subset that reorders the corpus reorders the leftovers
  with it, and that is the cost of the one-minute instrument.

### 7.6 What is still not emitted, named rather than left to be found

* **The player-excluded `2001`s.** An observer is told about one destroyed
  block where vanilla tells it about two or three, because the three
  four-argument sites are not emitted at all. Nothing in the corpus reads an
  observer's stream through a dig, so nothing measures it; it needs a
  per-recipient exclusion, which `crate::players::Players::broadcast_except`
  now exists to give it.
* **The 64-block `levelEvent` radius and the 16-block `playSound` radius.**
  Neither is applied; every broadcast in this server goes to every connected
  player, exactly as the block-update path already does.
* **Sounds for the other 56 blocks that play one.** `use_without_item.json`
  records `sound` as a boolean — `UseGolden` counts the call rather than
  reading its argument — so the table says *that* a block sounds and never
  *which*. Only the lever and the button are ported, and generalising it means
  teaching the generator to record the event.

  **This said 182 until 2026-08-18 and the number was never measured.** 182 is
  `184 blocks with behaviour − 2 ported`, which assumes every block that does
  something on right-click also plays a sound. Counted off the `sound` column
  it names: **71 blocks have at least one sounding state**, of which 15 are the
  lever and the 14 buttons, leaving **56**. By class the remainder is doors
  (16), trapdoors (16), fence gates (12), the weathering-copper doors and
  trapdoors (4 each), the note block, the comparator, the composter and the
  decorated pot — which is a far more tractable list than 182, and is most of a
  family rather than a long tail. Derived by `ci/refusal-ledger.py` and tied to
  this sentence by `ci/checks/refusal-citations.toml`, so the next reader gets a
  number that reddens when it stops being true.

  **It read 70 and 55 for the six hours between being measured and being
  re-measured**, and the block that moved it is the point rather than an
  inconvenience: `minecraft:decorated_pot` had no row at all until the probe's
  level started supplying a real `DecoratedPotBlockEntity`, so a block that
  wobbles and plays a sound on every right-click was in neither the sounding
  count nor any other. See §80. A figure derived off a generated table moves
  when the generator gets better eyesight, which is the whole reason it is
  cited rather than typed.
* **The chest *close* sound**, whose pitch is drawn the same way as the
  open's. Nothing in the corpus observes a container closing.
