# COLD START, branch `xp-orbs`, off `main` (4024+)

Worktree `~/grit/work-xp-orbs`. Remote dirs `rustmc-xp-orbs` (subset
recordings) and `rustmc-xp-orbs-t0` (T0 only, so a gate never shares a tree
with a recording).

## What this branch is

The experience system, which did not exist on `main`: no orb entity, no player
experience state, and no `set_experience` ever sent (`respawn.rs:228`'s
encoder had no caller).

* `crates/rustmc-server/src/experience.rs` — a player's level maths
  (`giveExperiencePoints`/`giveExperienceLevels` in single precision and in
  bytecode order, the level-up sound gate, the setters), the per-player
  registry, the `doTick` half (`takeXpDelay`, the orb touch, the
  `set_experience` comparison) in `connection_tick`, and console `xp`.
* `crates/rustmc-server/src/xporb.rs` — `ExperienceOrb`: both constructors,
  `tick` (gravity, water, lava pop, merge scan, follow, move, friction,
  bounce, age), `award`/`awardWithDirection`/`tryMergeToExisting`, the touch
  (`Player.aiStep` + `playerTouch` + `take`), `repairPlayerItems` (pure),
  `/summon` with its tag, the furnace ledger, and `sources` — the amount
  computation of every other experience source, transcribed.
* Synched data: the player's score (accessor 18) and the orb's value (8).
  A dirty score goes to trackers and to the player (`broadcastAndSend`).
* Persistence: `XpLevel`, `XpP`, `XpTotal`, `Score` saved and loaded.
* Console: `xp|experience add|set|query`, `summon [minecraft:]experience_orb
  x y z [tag]`, `execute at <player> run ...`, queued to the clock and run
  after the connection tick (`DedicatedServer.tickConnection`).
* Furnace: `setRecipeUsed` feeds a ledger; a click that lowers slot 2 of a
  furnace-family menu pays out `createExperience` at the taker's feet.

## Hook points for sources on branches that have not landed

Each is a call into code that exists on this branch; none of those branches
was edited.

* **mining** (`origin/mining`, `crates/rustmc-server/src/dig.rs`,
  `destroy_block` -> `drop_resources`): after `drop_resources`, for a
  survival break with the `block_drops` rule on,
  `xporb::sources::block_range(block)` then
  `crate::mob::level_random(|r| xporb::sources::block(range, silk, r))`, then
  `xporb::award(level, atCenterOf(pos), n)` when `n > 0`. Spawner:
  `sources::spawner`, no silk test.
* **animals** (`origin/animals`, `crates/rustmc-server/src/mob/breed.rs`,
  `finalize_spawn_child_from_breeding`): when `mob_drops` is on,
  `xporb::sources::breeding(&mut parent.random)` and
  `xporb::spawn_with_direction(level, parent_pos, Vec3::ZERO, n)` — `Animal`
  constructs the orb directly, so no merge. Kill experience:
  `sources::animal_kill`, awarded by `LivingEntity.dropExperience`.
* **menus-workstations** (`origin/menus-workstations`,
  `crates/rustmc-server/src/workstation.rs`, `grindstone_on_take`):
  `sources::grindstone(min_costs_of_non_curse_enchantments, level_random)`
  then `award(level, atCenterOf(pos), n)`.
* **be-furnace-brewing** (`origin/be-furnace-brewing`, `click.rs:891` says the
  orb does not appear): nothing to do — the take is detected in
  `world_send::handle_container_click` by the fall in slot 2, independent of
  how `click.rs` moves the stack.

## Not ported, named

`unstuckIfPossible`, `moveTowardsClosestSpace`, lava fire damage to an orb,
`applyEffectsFromBlocks`, orb persistence, and **Mending wiring** — the pure
function is tested but a player's equipment lives on the connection thread,
so the touch passes no candidates (vanilla-identical for a player holding no
damaged Mending item). The furnace ledger is not written to `RecipesUsed`,
and its multi-recipe award order is first-use, not vanilla's identity-hash
order.

## 685 is parked, not dropped

`docs/cold-start/xp-orbs-685-for-be-furnace-brewing.toml` is a complete check
(raw gold smelted, slot 2 taken, the orb picked up). Vanilla recorded it on
2026-10-01: `take_item_entity`, `remove_entities`, `set_experience` (progress
0.14285715, level 0, total 1). Ours was `absent` — not because of the payout,
but because on `main` a click into the ingredient slot never runs
`AbstractFurnaceBlockEntity.setItem`'s cook-time reset
(`crate::cooking::set_item` has no caller), so `cookingTotalTime` stays 0 and
the `cooking_timer == cooking_total_time` test at `cooking.rs:192` never fires:
the smelt never completes. `origin/be-furnace-brewing` adds
`set_item_from_menu`. Move the file back into `ci/checks/gameplay/` when that
branch lands; the payout in `world_send::handle_container_click` needs no
change.

## State at the pause (2026-10-01 ~17:15), and the exact next steps

Verdicts so far (subset run `r2`, 6 checks, vanilla vs this branch before the
tracker fix): 680 match, 681 match, 682 match (strict diverge), 683 match
(strict diverge), 684 match (strict diverge), 685 absent (parked, above). The
strict diverges were the orb's second `set_entity_data`, fixed by the
`sendChanges` order commit. 683's flight matched vanilla to the bit
(`set_entity_motion` -0.05389733260086671, `entity_position` 2.66451143196403).

Detached jobs that may have finished while paused — results land in
`/tmp/claude-1000/-/befd9b5b-5163-468a-9d93-210a7ac9ac95/scratchpad/xp/runs/`:

* `xp-r3` (`~/.rustmc-detached/xp-r3.log`): subset of 680-684 (then numbered
  640-644; scenario names unchanged, recordings keyed by name) plus 080-089,
  on the tracker-order fix. Copies `vanilla.json`, `rustmc.json` and
  `subset.log` into `runs/r3/`. Compare 080-089 against main's
  verdicts (stack report: 080 diverge/diverge, 081 match/diverge, 082
  diverge/diverge, 083-085 match/match, 086-088 match/diverge, 089
  match/match); any move there is the tracker fix's and must be explained.
* `xp-t0b` finished rc=1 on an older tree (`runs/t0-b.log`). Its reds and what
  was done: projection-drops, entity-reach, mob-observability, order-blind,
  entity-id-allocator, mob-tick-census, clippy — all fixed and committed;
  `cargo fmt --check` was a mid-edit sync. **Still open:** corpus-falsifiable
  (UNREGISTERED 680-684) and corpus-grades (no rows) — both need the splice
  below — and the `*-falsify.sh` reds, which cascaded from the guards being red.

Next, in order:

1. Splice: `python3 scratchpad/xp/splice.py runs/r3 runs/splice` (reads the
   stack pair read-only; replaces/adds by name).
2. In `rustmc-xp-orbs-t0` after `t0-remote.sh --sync`: `python3
   ci/corpus-falsifiable.py --sweep --bin target/release/oracle-runner --checks
   ci/checks/gameplay --ledger ci/checks/gameplay-falsifiable.toml
   <splice>/vanilla.json <splice>/rustmc.json`; copy the ledger back and commit.
3. `ci/corpus-history.py ci/checks/gameplay <splice>/vanilla.json hist.json`
   then `ci/corpus-grades.py --write --history hist.json`; commit
   `ci/checks/gameplay-grades.toml`.
4. Full T0 via `scratchpad/xp/t0b.sh <tag>` under `ci/run-detached.sh`
   (`T0_KEEP_GOING=1`, own dir). Push after every commit:
   `git push origin HEAD:refs/heads/xp-orbs` + `ls-remote`.
5. WORKLIST `COLD START INDEX` row for `xp-orbs`.
