# Cold start — branch `gamerules-console`

Append-only. A new `### COLD START` entry per turn on this branch; nothing
above is edited.

---

### COLD START — branch `gamerules-console`, off `main` (`bba29ee2`), 2026-08-17

**Item: game rules and the console — the subsystem `random-ticks-turn-covered-
grass-to-dirt` is `absent` for.** `tools/oracle-runner/src/main.rs:239` and
`tools/oracle-runner/src/record.rs:75`: "this server has no console and no
game rules". Scoped but not started in `docs/cold-start/gameplay-parity-
scope.md` (branch `gameplay-parity-scope`, unmerged as of this cold start).
Worktree `~/grit/work-gamerules`, `RUSTMC_REMOTE_DIR=rustmc-gamerules`.

**FROM THE JAR.** `javap -p -c -constants` against
`net/minecraft/world/level/gamerules/{GameRuleType,GameRule,GameRules}.class`
and `net/minecraft/server/commands/GameRuleCommand.class` in the pinned 26.2
jar:

* The type set is exactly two — `GameRuleType.INT` / `.BOOL` — no third kind
  in this version.
* `GameRules.<clinit>` registers 59 rules by name, category and default;
  transcribed in registration order into `crates/rustmc-server/src/
  gamerules.rs::DEFAULTS`. `random_tick_speed` is `registerInteger("random_
  tick_speed", UPDATES, 3, 0)` — default 3, min 0 — matching
  `crate::randomtick`'s pre-existing doc, derived independently.
* Storage is per-`ServerLevel` (`CommandSourceStack.getLevel().getGameRules()`
  in `GameRuleCommand.setRule`), and `/gamerule` requires
  `LEVEL_GAMEMASTERS` (permission level 2) — confirmed independently by
  `tools/oracle-runner/src/record.rs`'s existing `Console` doc comment.
* The console: `net/minecraft/server/ConsoleInput` is a `(msg, source)`
  record; `MinecraftServer.createCommandSourceStack()` (the console's
  `CommandSourceStack`) carries no permission check at all — a process's own
  stdin has nothing to check one against.

**THE SCOPE DECISION.** A full gamerule registry (60 behaviours) is bigger
than what unblocks the corpus, and most of the 59 rules have no consumer in
this server at all (no mobs, no weather, no day/night progression). Split:

* **Complete as data**: all 59 rules, typed, defaulted from the jar, settable
  and queryable — `crates/rustmc-server/src/gamerules.rs`. Costs nothing
  beyond transcribing a table vanilla already publishes.
* **Complete as a real capability**: the console itself —
  `crates/rustmc-server/src/console.rs`, a `--console <fifo>` CLI flag
  (`crates/rustmc-server/src/main.rs`) opening a real fifo and dispatching
  `gamerule <name> [<value>]` against the real per-process store. Not a chat
  command (`crate::commands`'s own header explains why an argument-taking
  command answered over the wire needs byte-exact `ClientboundCommandsPacket`
  work this item does not need) and not a test-only backdoor (a dedicated
  server's console is a first-party vanilla feature).
* **Live for exactly one rule**: `random_tick_speed`, read every tick by
  `crate::randomtick::tick` (new), which is the only rule any corpus scenario
  needs. The other 58 are settable and inert — named as such in
  `gamerules.rs`'s own doc, the same "derived but not wired" shape
  `crate::tick`'s module doc already uses for several other systems.

**THE RANDOM-TICK WIRING ITSELF**, `crates/rustmc-server/src/randomtick.rs`
(the selection-loop half already existed and is untouched):

* `random_tick_grass` ports `SpreadingSnowyBlock.randomTick`'s covered-arm
  only (offsets 29-62 of the scenario's own disassembly, reproduced in both
  places). `canStayAlive`'s real light query
  (`getMaxLocalRawBrightness(pos.above()) >= 9`) has no consumer here — this
  server tracks no light at all (`crate::world`: "light is empty masks; the
  client lights the chunk itself") — so it is substituted with "is the
  position above anything other than air", exact for an opaque occluder
  (this scenario's own rig) and a named, undischarged gap for a translucent
  one. The spread arm (offset 63 on, four `nextInt` neighbour picks) is not
  ported, per this module's pre-existing "why this is not wired" doc.
* **Which sections tick** is not vanilla's `tickingBlockCount` counter (this
  server keeps none) — it is narrowed to the one section containing the
  active flat preset's grass layer (`crate::flat::FlatPreset::surface_y() -
  1`), which is exact for the only world type the corpus runs in and honestly
  absent (`crate::flat::selected()` is `None`) for `--level-type normal`.
* **Which chunks tick** is a fixed 2-chunk radius around each connected
  player (`CHUNK_RADIUS`), not vanilla's simulation-distance chunk map this
  server does not track — a documented, bounded deviation, chosen to keep a
  tick's cost (`state_at` calls: `HashMap` probe + `String` alloc per draw)
  well inside the 50ms tick budget at the corpus's `speed = 1000`.
* `ci/t2-gameplay.sh`: `--console` and a `console.fifo`/`server.pid` pair for
  the **subject** now, mirroring `$VAN_DIR`'s layout so
  `ci/vanilla-server.sh console` is reused rather than reimplemented; the
  same `SUBSTRATE_GAMERULES` quiet-down vanilla always got is now delivered
  to rustmc too, via a `rustmc_gamerule_or_die` paralleling the existing
  `gamerule_or_die`. **This is not cosmetic**: with `random_tick_speed`
  defaulting to vanilla's 3 (was hardcoded 0 in this server before this
  branch), an un-quieted subject would random-tick every rig's grass for all
  ninety-one *other* scenarios — reproducing, on this server, the exact flap
  the harness was built to stop on vanilla's side in 2026-08-03.

**Falsifiable.** `crates/rustmc-server/src/randomtick.rs`'s
`covered_grass_becomes_dirt` / `uncovered_grass_survives_because_the_spread_
arm_is_not_ported` / `a_covered_non_grass_block_is_untouched` mutate the
subject directly (drop the covering block, change the block under test,
change the block over test) and are pinned to the opposite verdict each time.
`780-random-ticks-turn-covered-grass-to-dirt` is already tier-2 in
`ci/checks/gameplay-falsifiable.toml` from before this branch
(`killed_by = ["retype", "speak"]`, `why = "baseline: silent at its measured
step where vanilla is not"`) — this branch does not edit that check's
`observe`/steps, so no re-sweep is owed per `ci/corpus-falsifiable.py`'s own
staleness rule; `--check` still passes (see corpus run below).

**Corpus result**: recorded separately once the run completes — see the next
entry or the commit that lands it, not asserted here ahead of the evidence.

---

### COLD START — branch `gamerules-console`, continued off `main` (`208e34ef`), 2026-08-17

Picked up the branch as-is (previous agent starved 3h10m on the corpus lock,
per `ci/corpus-lock.sh status`: `run pid 3308547 ... waiting 189m`); the
worktree's code was already byte-identical to this branch's tip, so nothing
was re-derived. Rebased onto `208e34ef` (3 commits, clean, no conflicts).
`RUSTMC_REMOTE_DIR=rustmc-gamerules` (pre-existing, `~/rustmc-gamerules`),
`./ci/t0-remote.sh --sync` then `--run`: **T0 GREEN**, fmt/clippy/check/test
all pass (795+114+8+... unit tests, 0 failed).

**Mutation-kill proof taken live**, not merely asserted: `random_tick_grass`'s
`minecraft:dirt` output swapped to `minecraft:stone`, `cargo test -p
rustmc-server --lib randomtick::tests::` — `covered_grass_becomes_dirt`
FAILED naming the exact values (`left: "minecraft:stone" right:
"minecraft:dirt"`), the other 12 randomtick tests stayed green. Reverted,
diffed clean against the branch tip, re-ran green (13/13).

**The already-queued recording finished** (it had been waiting since before
this turn started, `RUSTMC_CHECKS=/tmp/checks-780`, scoped to the one
scenario): acquired the corpus lock after 204m total wait, both substrates on
protocol 776, `/tmp/t2-780.log`:

```
CORPUS  declared  1/1 match   (0 diverge, 0 absent, 0 vacuous, 0 mute, 0 inert, 0 error)
BETTER  random-ticks-turn-covered-grass-to-dirt: was absent, is now match
```

`ci/checks/gameplay-baseline.toml`: `random-ticks-turn-covered-grass-to-dirt`
locked `absent -> match`, cited in place. `docs/PARITY-ROADMAP.md` §61
records the mechanism and the evidence. No tracked file names another server
(checked the diff, not assumed). Pushed `gamerules-console` to origin.
