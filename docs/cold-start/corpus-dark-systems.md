# Cold starts — branch `corpus-dark-systems`

### COLD START — branch `corpus-dark-systems`, 2026-08-12

**The item.** `docs/PARITY-ROADMAP.md` §21.5 ranks rows 6, 7 and 8 of its work
order — natural spawning (6 tick sections), random ticks (1) and weather (1) —
as the cheapest coverage on the board *because they are dark by our own hand*:
`ci/t2-gameplay.sh` sets `spawn_mobs false`, `random_tick_speed 0` and
`advance_weather false` before it records anything. The item was to reach them
without destroying the 4/92 floor at `proj:7cefde0fb56d9f3e`, which is measured
under exactly those settings.

**What was built.** A scenario can declare a game rule for the length of its
own measured window:

```toml
[[gamerule]]
rule    = "random_tick_speed"
value   = "1000"
restore = "0"
```

`record` applies it after the rig is built and quiet — so the effect lands in
the measured window and not in the discarded setup — and restores it after the
settle window, restoring even when the window errors. It goes down
`<VAN_DIR>/console.fifo` (`record --console`, passed by `ci/t2-gameplay.sh`'s
vanilla pass and by nothing else). Ninety-two checks declare nothing and are on
exactly the substrate they were measured under. `docs/PARITY-ROADMAP.md` §26
has the whole argument; `scenario::GameRule` and `record::Console` have the
per-decision ones.

**Which of the three became observable: one.**

```text
  random_tick_speed   OBSERVABLE   780-random-ticks-turn-covered-grass-to-dirt
                                   0/15 pairs at N = 6, resolving power 39.3%
  advance_weather     NULL         reference projection empty in all 6 runs
  spawn_mobs          NULL         15/15 pairs, 6 distinct projections
```

**The census moved by one section and no more.** Same script both ways, same
ensemble `coverage92-selfcheck-n6-20260811`, the only difference being one file
in `ci/checks/gameplay/`:

```text
                     PROVABLY OFF   unclassified   provably dead
  without 780             8             79          41/120 = 34.2%
  with    780             7             80          40/120 = 33.3%
```

`unclassified` is **unknown**, not covered. What moved is a negative.

**Killed as nulls — do not re-run these.**

* **`advance_weather` cannot be observed by this harness, and the reason is
  arithmetic rather than a flap.** `advanceWeatherCycle`'s `ifeq` at offset 33
  gates the *timers*, not the rain/thunder level ramp below it, and
  `WeatherData`'s no-arg constructor starts every counter at 0 — so the first
  weather tick samples `RAIN_DELAY`, which is `UniformInt.of(12000, 180000)`.
  **The floor of that is ten minutes against an eight-second window, and it is
  a fresh draw per boot.** Measured as well as read: six runs, projection empty
  in all six. Do not write a weather scenario. If somebody ever wants this, the
  route is `/weather`, which is a different command with a different permission
  problem, and it does not exercise the rule.
* **`spawn_mobs` is unmeasurable, not hard.** Peaceful does *not* leave the
  world empty — 8 to 21 `add_entity` per eight-second window, different types
  and positions every run, 113..531 observations. Floor: **15/15 pairs, six
  distinct projections over six runs.** `EntityType.spawn` takes the spawn yaw
  from `serverLevel.getRandom().nextFloat()`, which is the same unseeded field
  `spawn-egg-makes-a-mob` already fails 25/25 on at N = 26. There is no subset
  a check could project and reach `match` on. This is the case
  `gameplay-unstable.toml`'s header is about: the cause is a field in the
  reference, so there is nothing to remove and nothing to declare — the honest
  move is not to write the check.
* **`Step::Command` cannot set a game rule and never could.** Six runs of
  `probe-gamerule-as-a-chat-command`: vanilla answers `command.unknown.command`,
  not a permission refusal, because `gamerule` is level 2 and is therefore not
  in the command tree `sendCommands` builds for the harness's level-0 players at
  all. Nothing in `ci/vanilla-server.sh` writes an `ops.json`. Op-ing them was
  rejected on cost, not on principle: `sendPlayerPermissionLevel` changes the
  join traffic of all 92 checks.
* **A wider `within` on 780 would buy a real flap.** Every run also produces one
  grass-*spread* `block_update` in the hole `record::warm_up` digs at the spawn
  workspace, at `[30,-2,-5]`, `[26,-2,-4]`, `[23,-2,-7]`, `[40,-2,0]`,
  `[40,-2,-7]` — the spread arm of `randomTick` is the one that draws from the
  generator. The default `within` of 16 excludes all of them with seven blocks
  to spare. Do not widen it.

**The instrument.** `ci/dark-systems-probe.sh` — its own port, its own world,
its own throwaway corpus, no corpus lock, N cold vanilla runs, and it **copies**
the shipped `780` rather than paraphrasing it so the floor is a floor for the
file that ships. `DARK_RUNS=2` for a cheap weak reading. It does not touch
`gameplay-baseline.toml`, the standing floor, or `gameplay-unstable.toml`.

**The gate.** `T0 GREEN`, 0 `skipping: no vanilla`. Full corpus 2026-08-12,
Threadripper, seed 20260803: **73/93 declared, 69/93 strict, 0 vacuous, 0 mute,
0 inert, 0 error**, against 72/92 and 68/92 before. No existing check regressed;
780 is baselined at `absent` with `reference_obs` 2 and `subject_obs` 0. The
three `BETTER` advisories the run prints are the standing ones from branches
already in `main` and are not this branch's to lock in.

**The next item, and the result that closes it.** `randomTick` is *reachable*
and this server still does nothing when it fires:
`crates/rustmc-server/src/randomtick.rs` ports the selection loop, no
`randomTick` override, and `DEFAULT_SPEED = 0` with a module comment saying the
harness quiets one server and not the other. That comment is now half stale —
the harness can quiet or unquiet the reference per scenario, and the missing
half is a way to tell *this* server. **The item that closes 780 is
`SpreadingSnowyBlock.randomTick` plus a game-rule store this server can be told
about**, and the result that closes it is 780 going `absent` -> `match` on a
corpus run. Nothing else in §21.5's rows 6-8 is worth another turn.
