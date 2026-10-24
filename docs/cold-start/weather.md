# Cold start — branch `weather`

Append-only. A new `### COLD START` entry per turn on this branch; nothing
above is edited.

---

### COLD START — branch `weather`, off `main` (`1d109abcf`), 2026-09-06

**Item: 26.2's weather cycle on the server.** `ServerLevel.advanceWeatherCycle`
and everything it touches. Worktree `~/grit/work-weather`,
`RUSTMC_REMOTE_DIR=rustmc-weather`.

The mechanism, the constants and the refusals are in
`docs/PARITY-ROADMAP.md` §107 and in `crates/rustmc-server/src/weather.rs`'s
module header; neither is repeated here. This file holds the two things a
next agent needs that are not in either: **the corpus scenario, which did not
land and why**, and **the null results, so nobody re-runs them**.

#### What landed

Six commits. `crate::weather` (the state machine, 30 unit tests), the
`weather` console verb, the tick and join wiring, the two caveat lists this
branch made untrue, and `tests/weather_console.rs` — one test in its own
binary that drives a real fifo and asserts the next tick's broadcast, shown
red twice before it was shown green.

`cargo test -p rustmc-server --lib` 746 passed / 0 failed / 1 ignored, on
threadripper in `~/rustmc-weather` with `data/vanilla` populated. Clippy
`--all-targets -D warnings` clean.

#### THE SCENARIO DID NOT LAND, AND THE REASON IS ONE UNMERGED TYPE

The corpus can see this subsystem the moment `Step::Console` exists. It does
not exist on `main`:

```
$ git show origin/main:tools/oracle-runner/src/scenario.rs | grep -c 'Console {'
0
$ git show origin/world-systems-2:tools/oracle-runner/src/scenario.rs | grep -c 'Console {'
1
```

`origin/world-systems-2` (941dfd27 at the time of writing) carries both
`Step::Console { text }` and `Scenario::console_restore`, and this lane was
told not to touch that branch's files. Writing the scenario anyway would
**red T0**: `ci/corpus-falsifiable.py --check` refuses a scenario in
`ci/checks/gameplay/` with no ledger row, and the row cannot be produced —
`--sweep` re-compares recordings, and no recording of a scenario the runner
cannot parse can exist. So the file is here rather than in the corpus. It is
complete and it is written against world-systems-2's grammar.

**Why it has to be a console step and cannot be `do = "command"`.**
`WeatherCommand.register` gates `/weather` on `Commands.LEVEL_GAMEMASTERS`
— permission level 2 — and `ci/t2-gameplay.sh`'s own comment at the
`--console` block records that *"the harness's players are not operators"*.
Opping them is not a smaller change than waiting: `ClientboundCommandsPacket`
is permission-filtered and sent at join, the recorder decodes it as
`commands` with `root_count` and `roots`
(`tools/oracle-runner/src/trace.rs:1701`), and it would move for all 110
scenarios.

To land it, after world-systems-2 merges:

```bash
# in a worktree on a branch containing both this one and world-systems-2
mkdir -p ci/checks/gameplay
$EDITOR ci/checks/gameplay/860-weather-rain-reaches-a-client.toml   # body below
cargo build --release -p oracle-runner        # on the box, not the laptop
RUSTMC_CHECKS=ci/checks/gameplay/860-weather-rain-reaches-a-client.toml \
    ./ci/t2-gameplay.sh                       # takes the corpus lock; ~35 min queued
python3 ci/corpus-falsifiable.py --sweep --bin target/release/oracle-runner \
    oracle-work/gameplay/vanilla.json oracle-work/gameplay/rustmc.json
# then the ci/checks/gameplay-baseline.toml row, in the SAME commit as the
# ledger row, at whatever verdict the run printed.
```

The body, ready to drop in:

```toml
name = "weather-rain-reaches-a-client"
about = "`/weather rain` on the console, then a settle: the rain level climbs 0.01 a tick and crosses 0.2, and the client is told — the corpus's only look at vanilla's `weather` tick section, which `ci/t2-gameplay.sh` shuts for every check with `advance_weather false`"
observe = ["game_event"]
settle_ms = 3000
console_restore = ["weather clear"]

# THIS CHECK DRIVES A CONSOLE COMMAND, AND WHAT IT LEAVES BEHIND IS GLOBAL.
#
# Weather in 26.2 is ONE `WeatherData` for the whole server
# (`ServerLevel.getWeatherData()` is `this.server.getWeatherData()`), so a
# scenario that starts rain and does not stop it hands every later scenario a
# raining world — and a raining world sends `game_event` 7 and 8 on a tick
# boundary forever after, into whatever window the next check is measuring.
# `console_restore` is that undo, and it is required rather than advisory.
#
# `weather clear` also resets the two timers to 0/false through
# `setWeatherParameters(t, 0, false, false)`, so the restore is complete and
# not merely a level ramp back down.
#
# FROM THE JAR — `ServerLevel.advanceWeatherCycle`, javap -p -c -constants:
#
#   262: oThunderLevel = thunderLevel
#   270: thunderLevel += isThundering() ? +0.01f : -0.01f     (#1260)
#   317: oRainLevel    = rainLevel
#   325: rainLevel    += isRaining()    ? +0.01f : -0.01f
#   372: if (oRainLevel    != rainLevel)    -> RAIN_LEVEL_CHANGE    (id 7)
#   412: if (oThunderLevel != thunderLevel) -> THUNDER_LEVEL_CHANGE (id 8)
#   452: if (wasRaining != isRaining()) -> START_RAINING (1) or STOP_RAINING (2),
#                                          then 7 and 8 again, unconditionally
#
# The ramp is OUTSIDE the `advance_weather` game rule (which gates offsets
# 36..261 only), which is why this check needs no `[[gamerule]]`: the harness
# sets `advance_weather false` for the whole corpus and the levels still move.
# That is not incidental — it is what makes weather observable in a corpus
# whose substrate freezes the timers.
#
# WHAT THE WINDOW SEES. `Level.isRaining()` is `getRainLevel(1.0f) > 0.2`,
# the level accumulates 0.01f a tick, and twenty f32 steps sum to
# 0.20000002980232239 — so the transition lands on tick 20, 1.0 s in. At
# `settle_ms = 3000` (about 60 ticks) the window carries ~60 RAIN_LEVEL_CHANGE
# packets and exactly one START_RAINING, which is a projection with a stable
# count rather than a race.
#
# THE THUNDER SEND IS ABSENT AND THAT IS THE POINT OF `rain` OVER `thunder`.
# Nothing is thundering, so `thunderLevel` is clamped at 0 and the GUARDED
# send at 412 never fires; the UNCONDITIONAL one at 533 fires once, inside the
# transition arm. A check that saw 8 on every tick would not be able to tell
# the two send sites apart.

[[step]]
do = "console"
text = "weather rain 300"
```

`observe = ["game_event"]` is decoded by
`tools/oracle-runner/src/trace.rs:1605` into `event` (the byte) and `value`
(the float), so the projection already exists — nothing in the recorder needs
adding.

**Expected verdict when it first runs: `match`.** This server now sends the
same four ids with the same payloads on the same ticks, and
`tests/weather_console.rs` asserts exactly that sequence end to end. If it
comes back `diverge`, the first thing to look at is the tick position: this
server runs weather at the top of its tick loop and vanilla runs it second in
`ServerLevel.tick`, so a one-tick offset against the block broadcast is the
plausible failure and `crate::status`'s comment says where it sits.

#### NULLS — do not re-run these

* **Ice and snow cannot be observed in this corpus at any settle length.**
  Both reduce to `Biome.warmEnoughToRain` = `getTemperature(pos, seaLevel) >=
  0.15f`; the superflat biome is `minecraft:plains` at temperature 0.8. Not a
  rate problem, a predicate that is false everywhere in that world. Changing
  it is a generator-settings change to `ci/t2-gameplay.sh` that re-baselines
  all 110 scenarios.
* **Cauldron fill survives the biome and dies on the rate.** 56.5 s of settle
  for 99% confidence at `random_tick_speed = 1000`; the arithmetic is in
  §107. And there are no cauldrons in this server (`grep -ri cauldron
  crates/rustmc-server/src` -> 0).
* **`do = "command"` cannot reach `/weather`.** Permission 2; see above.
* **The seeded golden in `weather.rs` is not a vanilla comparison and cannot
  become one.** `Level.random` is seeded from
  `RandomSupport.generateUniqueSeed()` — a uniquifier times a constant, XORed
  with `System.nanoTime()` — at level construction. There is no vanilla run
  of any seed for that stream. Do not go looking for one.

#### The single next item

Land the scenario above once `Step::Console` is on `main`, and record its
verdict. Everything it needs on this side is in place and tested; nothing
else about this subsystem is blocked on this server.
