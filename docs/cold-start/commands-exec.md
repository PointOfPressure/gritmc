# Cold start — branch `commands-exec`

Every `### COLD START` entry ever written for this branch, newest last.

---

### COLD START — branch `commands-exec` off `main` (`d48904cb5`, 3973), 2026-10-01

**THE ONE LINE: the command tree had roots with no executors; there is now one
executor module, `crate::cmdexec`, that gives every executor a SOURCE (console
or player, plus the entity/position/rotation `execute` moves), and 15 roots
run through it.** `ci/command-surface.py --check` matches the jar; ported roots
12 -> 22 at `546c28953` (now more: `docs/generated/command-surface.tsv` is the
count, not this file).

Worktree `~/grit/work-cmdexec` (detached), remote dirs `~/rustmc-cmdexec`
(recordings) and `~/rustmc-cmdexec-t0` (T0), lane scripts in
`/tmp/claude-1000/-/befd9b5b-…/scratchpad/cmdexec/` (`cx-*.sh` — the bare
scratchpad is shared with other lanes; see FINDINGS 2026-10-01).

**Roots added, each mirrored in `CommandsGolden.registerOurs`:**

| root | shape | wire effect a scenario grades |
|---|---|---|
| `difficulty` | whole | `change_difficulty` to every player (116) |
| `gamemode` | whole; adventure/spectator not answered | abilities, game_event 3, abilities, `gameMode.changed` (117, 126) |
| `defaultgamemode` | whole | none (future joins only) |
| `teleport`/`tp` | no `rotation`/`facing` | `player_position` with the next teleport id (119, 127) |
| `weather` | whole, existing executor | — |
| `experience`/`xp` | whole | `set_experience`, `player.levelup` at every fifth level (123, 124) |
| `title` | `clear`/`reset`/`times` | `set_titles_animation`, `clear_titles` (125) |
| `time` | `set`/`add`/`query time|gametime` | `set_time` at once (ambient: no scenario) |
| `execute` | `run`/`as`/`at`/`positioned [as]` | (126, 127, 128, 129, 143) |
| `setblock` | whole; `destroy` over non-air not answered | `block_update` from the tick (128) |
| `fill` | no `replace <filter>` | `section_blocks_update` (129) |
| `playsound` | whole | inline-holder `sound` (143) |
| `stopsound` | whole | `stop_sound` (144) |

Plus console `say`/`me`/`msg` (121, 122) and the level-0 refusal of a
level-2 root through the player path (118).

**Gates:** `tests/cmdexec_vanilla.rs` compares every reply byte for byte with
the component vanilla's OWN code builds — the command's private message lambda
called by reflection, its static exception type raised, or
`CommandSourceStack.sendFailure` run on a real stack (`CommandsGolden.
appendExecutorReplies`). `tests/command_failure_vanilla.rs` grew from 80 to
110 rows (permission pruning for the new roots at all five levels).
`tests/command_parse_vanilla.rs` now READS vanilla's own `minecraft:block_state`
rows (the parser left the declared-unread set).

**Not answered, by design, and said on the console:** `@e`/`@n`/`@r`
selectors; local (`^`) coordinates; `gamemode adventure|spectator` (no ability
model, as `status::set_game_type` already says); `setblock`/`fill destroy`
over a non-air block (the drop is a loot roll); an `execute`-chained root that
only `crate::commands` answers.

**Nulls killed:** op-ing the harness players to make player-path feedback
visible — `ci/command-tree-parity.py` refuses a run whose declared tree varies
between scenarios, and the console already reaches every root. `/summon` —
`mob::summon` exists with no caller, but a summoned pig outlives its scenario
and there is no `/kill` on this branch to restore with (the `damage` lane owns
`kill`/`effect`; `mining` owns `give`/`clear`).

**Next item:** record 116-131 (`ci/t2-gameplay-subset.sh`), sweep the subset
recordings into a temporary ledger and graft the rows
(`ci/ledger-graft.py`), then T0. Closing result: every new scenario tier 2,
T0 green.

**State at HALT, 2026-10-01 ~12:30:** 30 roots ported (surface tsv), 21 scenarios 116-135 committed; 116-126 match vanilla 10/10 in subset run `~/grit/private/cmdexec-rec/sub3`, rest await `cmdexec-chainB` (subset sub5 + splice of the gate pair + `--sweep` + grades, outputs `scratchpad/cmdexec/{ledger-new,grades-new}.toml`, NOT yet committed).
T0 (`~/rustmc-cmdexec-t0`, at `c0d264f27`): targeted suites green (lib 1265, cmdexec_vanilla 17, parse 4, failure 3, tree 8); remaining reds = falsifiable ledger + grades rows for the new scenarios (need the sweep), machinery-falsify knock-on, t1-par/tick load flakes.
Next: commit ledger-new/grades-new after inspecting, rebase onto main (trial rebase clean), full T0.

---

### COLD START — branch `commands-exec`, rebased onto `main` (4024), 2026-10-01 afternoon

**THE ONE LINE: seven more roots through `crate::cmdexec` — `/rotate`
(subset), `/swing`, `/particle`, `/teammsg` + `tm`, `/dialog clear`
(subset), `/bossbar` (whole) — ported 30 -> 37 in
`docs/generated/command-surface.tsv`, with scenarios 136-139, 141, 142.**
Worktree `~/grit/work-cmdexec2`, remote dir `~/rustmc-cmdexec2`, lane
scripts `scratchpad/cmdexec2/{sync,unit,subset,ledger}.sh`.

| root | wire effect | scenario |
|---|---|---|
| `rotate <target> <rotation>` | `player_rotation` (73) to the target, tracker turns it for others | 136 |
| `swing [targets] [mainhand\|offhand]` | `animate` to trackers AND self (`updateSelf`) | 137 |
| `particle` (simple types) | `level_particles` (47) per viewer in 32/512 | 138 |
| `teammsg`/`tm` (level 0) | red `commands.teammsg.failed.noteam` — no `team` root, so no team | 139 |
| `dialog clear <targets>` | `clear_dialog` (139), empty body | 141 |
| `bossbar` | `boss_event` (9): ADD/REMOVE/PROGRESS/NAME/STYLE to holders only | 142 |

`crate::bossbar` keeps `CustomBossEvent`'s two player sets (for-UUIDs vs
holders), `HashMap` iteration order for `/bossbar list`, the level-random
UUID draw, and the connect/disconnect hooks in `status.rs`. Every bossbar
reply is byte-compared against vanilla's own lambda over a real
`CustomBossEvent` (`cmdexec_vanilla`, 19 tests).

**Not ported, said why:** `rotate ... facing` (sends `player_look_at`),
`dialog show` (needs the dialog registry/SNBT), `particle` with options or a
`{}` tag (Unsupported in the parser), `/setworldspawn` and `/spawnpoint` —
there is no world-respawn state and the join never sends
`set_default_spawn_position` (FINDINGS 2026-10-01); a scenario could not
restore vanilla's worldgen spawn either.

**Nulls / hazards:** the previous chain's `cx-ledger.sh` passed other trees'
`vanilla.json` into `ci/corpus-history.py`'s `<reference> <out>` slots and
would have overwritten one; killed before it ran, script disabled
(FINDINGS). Queued `cmdexec-sub4` killed for the lock-pool relaunch.

**Next item:** one subset recording of 116-139, 141, 142 on this tree
(`subset.sh`), then `ledger.sh` (splice into the gate pair, sweep, history,
grades) and graft the rows with `ci/ledger-graft.py`; then T0. Closing
result: every scenario 116-142 tier 2, T0 green.

**State at PAUSE, 2026-10-01 ~17:00:** tip has 7 new roots + nbt component
key order + 129 moved inside one chunk. Subset `cmdexec2-rec1`
(`~/grit/private/cmdexec-rec/sub6`, 26 lane scenarios on this tree): 24/26
match; the 2 divergences (129 chunk order, 134 key order) fixed in the last
code commit. Sweep over the splice: 204/204 tier 2. Detached job
`cmdexec2-rec2` may still be running: re-records 129+134 into
`~/grit/private/cmdexec-rec/sub7`, runs rustmc-world nbt tests, then
`scratchpad/cmdexec2/ledger.sh sub6 sub7` writing
`scratchpad/cmdexec2/{ledger-new,grades-new}.toml` (log
`~/.rustmc-detached/cmdexec2-rec2.log`).

**Exact next steps:** (1) read that log: 129 and 134 must `match`, nbt tests
ok. (2) `python3 ci/ledger-graft.py --from scratchpad/cmdexec2/ledger-new.toml
--to ci/checks/gameplay-falsifiable.toml $(cat scratchpad/cmdexec2/lane-files.txt)`
(add `--replace` if rows exist) and
`python3 scratchpad/cmdexec2/graft-grades.py scratchpad/cmdexec2/grades-new.toml
ci/checks/gameplay-grades.toml <same files>`; commit, push. (3) T0 via
`ci/run-detached.sh` in `~/rustmc-cmdexec2` (sync.sh, gen twice,
`T0_KEEP_GOING=1 ci/t0.sh`). Old `cx-ledger.sh` is disabled; do not use it.

---

### COLD START — branch `commands-exec`, landing repair, 2026-10-01 evening

State (replayed onto main 994e795b9 after be-furnace-brewing landed; ledgers re-grafted, projection-drops counts recomputed): gate RED at T0 (`gate-commands-exec-20261001-175203.log:133`): `particle_types.json` was still in `ci/check-generator-arms.sh`'s DORMANT list though `crate::particles` now reads it — entry deleted. Checks 130-132 collided with main's door checks and are 143-145 now. Scenario 129's last divergence was a real defect on main: `shortset::ShortOpenHashSet` grew one key early against fastutil's `size++ >= maxFill` (FINDINGS). Subset `land-cx-rec` over all 25 lane checks on `c516033d2`: declared 25/25 match, strict 22/25; ledgers swept over a read-only copy of the `~/rustmc-stack` pair with that recording spliced in (204/204 tier 2), 25 falsifiable + grades rows grafted, 25 baseline rows `match`.
Next: the chain's gate.
