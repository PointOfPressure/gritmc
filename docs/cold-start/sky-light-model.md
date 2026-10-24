# COLD START — branch `sky-light-model`

### COLD START — branch `sky-light-model`, the sky and light model, 2026-08-18

**The item.** Two lanes reached one gap from opposite sides on 2026-08-18 and
neither owned it: `crates/rustmc-server/src/useblock.rs` transcribed
`DaylightDetectorBlock.updateSignalStrength` into a doc comment and did not run
it, and `data/vanilla/use_without_item.json` refused a bucket of states because
`UseGolden`'s level was asked for `environmentAttributes`. This branch is that
gap.

---

## What was measured first, because two briefed figures were wrong

**`use_without_item.json`'s refusal split is 128 + 32 = 160**, over 17 blocks:

| cause | states | blocks |
|---|---|---|
| `UseGolden's level was asked for environmentAttributes` | **128** | 16 beds, 8 states each — the head half |
| `UseGolden's level was asked for getChunkSource` | 32 | `minecraft:daylight_detector` |

`useblock.rs`'s header said `199` for the total and enumerated `39 + 160 + 32`,
which is 231. Three separate mistakes were compounding:

* **199 was stale.** `golden-useitemon-axis` bound item components on
  2026-08-12 and the 39 `Components not bound yet` refusals — 38 potted plants
  and the composter — went away with them. `docs/WORKLIST.md` records that as
  "199 -> 160" and the crate comment was never updated;
* **the 39 row described a cause that no longer exists**;
* **the 160 was the new total sitting in the row for one of its two parts**,
  which is why the three rows over-counted by exactly the total.

`docs/SERVER.md` has said 128 since 2026-08-04 and was right the whole time.

> **AND THE FIRST CORRECTION OF THIS, EARLIER THE SAME DAY, WAS ALSO WRONG.**
> It was measured against `~/rustmc/data/vanilla/` — a **shared** copy of the
> extract — which still carried 199 with the 39 component refusals present.
> That is precisely the hazard `docs/AGENT-BRIEF.md` describes when it says a
> shared `data/vanilla` gates every branch against whichever branch last
> regenerated it, and it was caught only because this branch's own
> `ci/gen-gameplay-tables.sh` produced a different answer. **Re-derive it from
> the table your own tree generated.**

**`710`'s trace is not merely green, it is byte-identical to vanilla's.**
Measured on `~/rustmc-corpusgap/oracle-work/gameplay/rustmc.json` (2026-08-18,
`main`): every observation, every field, including `state_id`. It was identical
while `updateSignalStrength` did not exist in this crate at all. The audit's
"green for a reason that has nothing to do with the mechanic it names" is
right and is stronger than it says.

`ci/checks/gameplay-baseline.toml` still carries
`a-daylight-detector-inverts-when-right-clicked = "diverge"`, which is stale in
the safe direction. **Deliberately not tightened**: one recording pair from one
tree is N = 1, and tightening a ratchet on N = 1 is how a flapping check
becomes somebody else's gate failure.

---

## In 26.2 the sky is DATA, and that is the whole trap

Every earlier version of this game computed the sun's angle from the time of
day with a closed-form expression. 26.2 has none. `Level.updateSkyBrightness`
is, in its entirety,

```text
  skyDarken = (int)(15.0f - environmentAttributes().getDimensionValue(SKY_LIGHT_LEVEL));
```

and `updateSignalStrength` reads `getValue(SUN_ANGLE, pos)`. Neither names the
time. Both resolve through `EnvironmentAttributeSystem` onto **keyframe
tracks** carried by `minecraft:timeline` registry entries — a datapack
registry. `visual/sun_angle` is two keyframes at tick 6000 (360.0 and 0.0) with
a cubic-Bezier ease of `(0.362, 0.241, 0.638, 0.759)` over a 24,000-tick
period; `gameplay/sky_light_level` is four linear keyframes with a `multiply`
modifier over a base of 15.

So the curve is **extracted** — `ci/dump-timelines.sh` into
`data/vanilla/timelines.json`, the same category as `flat_preset.json` — and
only the sampling machinery is transcribed, in `crates/rustmc-server/src/sky.rs`.

### Four things that would each have been a plausible wrong answer

1. **`LerpFunction.ofDegrees` is right there and is not the one.**
   `SUN_ANGLE`'s attribute type is `ANGLE_DEGREES`, whose second
   `LerpFunction` is `ofDegrees(threshold)` — shortest-arc, which on a
   0 → 360 track would `wrapDegrees` the difference to **0** and freeze the
   sun for ever. Traced through: `AttributeType.ofInterpolated(codec, map,
   lerp1, lerp2, toFloat)` assigns `lerp1` to `keyframeLerp`, `stateChangeLerp`
   *and* `spatialLerp`, and only `partialTickLerp` gets `lerp2`;
   `AttributeTrack.bakeSampler` takes `modifier.argumentKeyframeLerp(attr)`,
   and both `OverrideModifier` and `FloatModifier$Simple` resolve that to
   `LerpFunction.ofFloat()`. The keyframe lerp is plain `Mth.lerp`.
2. **`Mth.cos(double)` is a 65,536-entry lookup table, not `Math.cos`.**
   Re-exported from `rustmc_worldgen::math` rather than transcribed a second
   time — that table is already proved entry-for-entry against the JVM by
   `golden_mth_sin`.
3. **The base value is the dimension's own attribute map, not the attribute's
   registered default.** `minecraft:the_nether` carries a static
   `gameplay/sky_light_level` of **4.0**, so its `skyDarken` is
   `(int)(15 - 4) = 11` at every tick — no timeline, no variation. A test in
   this branch asserted 0 on the reasoning that the nether has no track, and
   the port refuted it. The end has no such static and really is 0.
4. **`(int)` is a truncation, not a round.** `15.0 - 14.18857` is 0, not 1.

---

## The golden, and the two ticks that make it a golden

`tools/goldengen/vanilla/SkyGolden.java` drives vanilla's **own**
`AttributeTrackSampler` over the real `minecraft:timeline` registry.
`ClockManager` is a functional interface, so a sampler can be walked to any
tick without a world; `applyTimeBased` caches its argument by tick id, so every
sample passes a distinct one — reusing an id would give one repeated number
that still looked plausible.

513 ticks, compared as **raw float bits with no tolerance**, dense at both zero
crossings of the detector's cosine and one tick either side of all four
`sky_light_level` keyframes.

**Two ticks of the 24,000 tell `Mth.cos` from a platform cosine**: tick 2447
(the table says `power` 12, `libm` says 13) and tick 10625 (11 against 10).
That is 0.008% of the day. Both are named in the sample grid and asserted in
`crate::sky`'s own tests, because a grid that sampled noon and dusk would let
the whole lookup table be replaced by `cos()` and stay green.

**The first run of that test was vacuous and reported three passes.** All three
cases skipped for a missing `data/vanilla/timelines.json`. The table is now in
`ci/gen-gameplay-tables.sh`'s `generator_for()` and named as a literal by
`crates/rustmc-server/src/main.rs`, so it is in the derived required set;
`ci/sky-falsify.sh` case 0 hides the extract and asserts the skip is announced.

---

## What was ported

| piece | source | note |
|---|---|---|
| `EasingType.CubicBezier` | `javap`, offsets given in `sky.rs` | 4 Newton-Raphson steps, ±0.25 clamp, bisection fallback |
| `KeyframeTrackSampler` | ditto | both wrap segments, `loopTicks` as `Math.floorMod` |
| `FloatModifier` | ditto | six operations |
| `ValueSampler.computeValueNotPositional` | ditto | base, layers in order, `sanitizeValue` |
| `Level.updateSkyBrightness` / `getSkyDarken` | ditto | the whole method |
| `LevelReader.getEffectiveSkyBrightness` | ditto | `getBrightness(SKY,pos) - getSkyDarken()`, **unclamped** |
| `DaylightDetectorBlock.updateSignalStrength` | offsets 0-148 | flag 3 at offset 143 |
| `DaylightDetectorBlock.getTicker` / `tickEntity` | ditto | sky-lit dimensions only; `getGameTime() % 20 == 0` |
| the sky-light **seed** | `crate::light` | 15 where the column above propagates; refuses elsewhere |

`propagatesSkylightDown`, `lightDampening` and `lightEmission` were added to
`BlockPropsGolden.java` and `data/vanilla/block_props.json`. Measured on the
regenerated table: air `true/0/0`, stone `false/15/0`, glass `true/0/0`, water
`false/1/0`, glowstone `false/15/15`. **Water is why the property is extracted
rather than derived** — it occludes nothing and renders non-solid, so both
`!canOcclude` and `!isSolidRender` would put full sky light at the bottom of a
one-block puddle.

---

## The substrate could not grade anything time-dependent, and nothing said so

`advance_time=false` stops the sun. It does not agree with two servers about
*where* it stopped: each freezes wherever its day time had got to when the
console command landed, a few hundred ticks after boot and a different few
hundred every run. `round(15 * cos(sunAngle))` moves by one for roughly every
**200 ticks** of drift, so a check on the detector would have been a coin toss
dressed as a measurement.

`ci/t2-gameplay.sh` now pins `SUBSTRATE_DAY_TIME=6000` on both sides from one
constant — vanilla through a new `time_set_or_die`, this server through
`--day-time`/`--advance-time`, because it has no console.
`ci/t2-gameplay-subset.sh` takes the same constant. `time_set_or_die` asserts
the **positive** half as well: vanilla answers `Set the time to 6000`, and the
absence of that line is the failure.

**Blast radius, checked rather than hoped.** `710` observes
`inverted=true,power=0`; the inverted arm is `15 - getEffectiveSkyBrightness`,
which is 0 under open sky at *every* daytime tick, so moving the substrate to
noon does not change `710`'s recording. Nothing else in the corpus reads the
time — `spawn_mobs`, `random_tick_speed` and `advance_weather` are all off,
which is what the other timeline-driven attributes need in order to move.

---

## What is gradeable now, and what is not

**Gradeable — `711-a-daylight-detector-recomputes-its-power-from-the-sky`.**
`710`'s second click. The inverted arm answers `15 - 15 = 0`, which is also the
`power` a freshly placed detector carries and the only `power` a server with no
sky can produce; the non-inverted arm answers `round(15 * cos(0)) = 15` and an
unported server writes 0. Its positive control is the red-then-green pair
recorded below.

**Ungradeable, and the reason — `DaylightDetectorBlock.tickEntity`.** It is
ported and no corpus check grades it. Both of `711`'s clicks compute `power`
themselves, so `711` would pass with the ticker deleted. It is not graded
because on this harness it *cannot* be: the only way to observe a ticker is to
place a detector and wait, and whether the placement's write and the ticker's
write land on the same server tick — coalescing into one packet instead of two
— is a one-in-twenty coin flip **independently on each server**, so the check
would disagree about two runs in twenty.

The independence is not an assumption. `tickEntity` fires on
`getGameTime() % 20 == 0`, and the two game clocks start when each process
starts, so their phase modulo 20 is whatever the boot skew happened to be that
run. **Pinning the day time, which this branch does, does nothing about it** —
`dayTime` and `gameTime` are different counters and `advance_time` stops only
one of them. That is also why the fix is a game-time verb and not a second
`SUBSTRATE_DAY_TIME`.

**What would make it gradeable is a scenario verb that sets the game time**, the same shape as `[[gamerule]]`,
delivered to both servers; this server would need a console for that, which is
`scenario::GameRule`'s own stated gap. In-crate it is covered by
`sky::tests` and by the ticker's own `% 20` arithmetic.

**Ungradeable — sky light anywhere but under open sky.** `crate::light` ports
the light engine's *seed* and refuses the propagation half. A detector under a
one-block cover reads 14 in vanilla (sky light arriving from the sides) and
this server declines to write at all. That is a known divergence, it is a
missing packet rather than a wrong one, and closing it is a light-engine item,
not a sky item.

**A null, recorded so nobody re-runs it.** `Math.round(float)` is
`floor(x + 0.5)` and Rust's `f32::round` is half-away-from-zero. Swept over all
24,000 ticks at every sky brightness `1..=15`: no `15 * cos(a)` lands on an
exact half, and the `clamp(0, 15)` erases every negative where the two could
differ. The distinction is **unreachable** through `daylight_detector_power`,
so `ci/sky-falsify.sh` case 5 expects green and says why, rather than leaving a
red somebody would widen a tolerance to satisfy. The `floor(x + 0.5)` form is
kept because it is what the bytecode does and the next caller may not clamp.

---

## The rest of the chain was already ported — the sky was the only missing link

Measured on the regenerated `data/vanilla/signal.json`: **all 32
`minecraft:daylight_detector` states are present and correct**, `source: true`,
with `[inverted=false,power=15]` giving `signal [15,15,15,15,15,15]` and
`direct [0,0,0,0,0,0]` — a weak source, as `DaylightDetectorBlock.isSignalSource`
plus `ownSignal` returning `state.getValue(POWER)` requires. The detector is
**not** in either of that table's two refusal buckets (1,215 `redstone_wire`
states on `getSignal read 0,1,0`, and 35 block-entity states on `comparator`,
`jukebox`, `test_block`, `trapped_chest`).

So `crate::power` has always known what a daylight detector at `power = 15`
emits. What it never saw was a detector whose `power` was anything but 0. The
sky model is the last piece of an otherwise complete chain, which is worth
stating because it changes what the next check should be.

### The next scenario, with its exact shape

**A daylight detector lights a lamp placed beside it.** Setup places the
detector under open sky and lets the block-entity ticker take it to
`power = 15`; the measured step places a `minecraft:redstone_lamp` adjacent, so
`RedstoneLampBlock.getStateForPlacement` reads `hasNeighborSignal` and the lamp
arrives **already lit**. Against an unported sky the detector is at 0 and the
lamp arrives dark.

It is written this way round on purpose: **the measured step must be the
placement and not a click that turns the lamp off**, because the lamp's
four-tick unlight is a scheduled tick that main does not have — it is closed on
`origin/server-unported-port` and not merged, and a check that needed it would
be grading somebody else's branch.

---

## The single next item

**A `[[time]]` scenario declaration, and the console this server does not
have.** The result that closes it: `711`'s sibling placing a detector and
observing the block-entity ticker's write, red against a build with
`tick_detectors` stubbed and green with it.

### And a correction to the obvious next item, made after the above was written

The 128 bed refusals look like the cheapest thing this branch unblocks —
`BedRule.can_sleep` is `when_dark`, `crate::sky` now holds every piece, and
`UseGolden.TableLevel.environmentAttributes()` merely throws. **Do not do it.**
Read out of `BedBlock.useWithoutItem` rather than assumed:

* the foot half returns `CONSUME` at offset 53 without reading anything, which
  is why exactly **8 of each bed's 16 states** refuse — the head half,
  `facing` times `occupied`;
* the head half reads `BED_RULE` at offset 58. The overworld's is
  `{can_set_spawn: always, can_sleep: when_dark}` with no `explodes`, so it
  falls through to `Player.startSleepInBed`, whose answer is a function of the
  **time of day**, the player's position, and what is nearby. (The nether's and
  the end's are `{can_sleep: never, explodes: true}` and would be constant; the
  overworld is the one the probe uses.)

A state-keyed table has nowhere to put a clock. Those 128 rows would record
what one probe saw at one instant of one day — the same shape as the twelve
`FenceGateBlock` rows recording what one player facing south saw, at **2.7
times the scale**. The fence gate's dependence was found after the fact and
cost a stance sweep to bound; this one is visible before the change is made.
They are a hand port of `BedBlock.useWithoutItem` against a real clock, or they
stay refused, and refused is honest until a bed does anything at all.


---

## Which findings depend on the projection's filters, and which do not

The brief this branch worked from asks for this explicitly, and the answer
splits cleanly in two.

**Projection-dependent, and they are the two headline corpus findings:**

* **"`710`'s trace is byte-identical to vanilla's"** is a statement about what
  `710` observes, which is `block_update`, `section_blocks_update` and
  `block_changed_ack` from the actor. It is *not* a claim that the two servers
  did the same thing — they did not; vanilla ran `updateSignalStrength` twice
  during that scenario and this server ran it never. Widen the observe list and
  the identity may not survive. Read it as "the projection cannot see the gap",
  which is the whole point of the audit row it corroborates.
* **`711` discriminates only because `block_update` carries the full `state`
  string.** Its red and its green differ in one property of one block —
  `power=0` against `power=15` — so a projection that dropped or truncated
  `state` would take this check straight back to being invisible, exactly as
  `710` is. `state_id` would do as well, but nothing else in the observation
  would.

**Projection-independent, because no recorder is involved at all:**

* the 513-vector golden and every one of `ci/sky-falsify.sh`'s six cases;
* the two ticks where `Mth.cos` and a platform cosine disagree, and the
  24,000-tick sweep that found them;
* the `Math.round` null;
* the `128 + 32 = 160` refusal split, which is a property of a generated table;
* the nether's `skyDarken` of 11.

**And one that is neither**: the block-entity ticker's ungradeability is a
property of the *harness*, not of the projection. No observe list can fix a
one-in-twenty coalescing coin flip; only a way to pin the game time can.

---

## HANDOFF — what is measured, what is queued, and how to finish it

**T0 is green except for one rule, and that rule needs a corpus recording.**
`ci/corpus-falsifiable.py --check` refuses
`711-a-daylight-detector-recomputes-its-power-from-the-sky` as `UNREGISTERED`,
which is a T0 failure by design: a new scenario carries no red-when-broken
evidence until it has been swept, and it cannot be swept without recordings.
Everything else in T0 was green on 2026-08-18 in `~/rustmc-sky`
(`~/sky-t0d.log`), including the two the branch itself provoked —
`check-test-targets`'s `UNACCOUNTED TARGET` and its follow-up refusal of a
`cargo test` line carrying no sentinel.

**The corpus lock was six deep when this branch stopped**, with the oldest
waiter at 233 minutes, so the run is queued rather than taken. `~/sky-final.sh`
on the box drives the rest; its stages each write their own log and their own
exit sentinel:

| stage | log | what it settles |
|---|---|---|
| build | `~/sky-final-build.log` | `BUILD-EXIT=` |
| flags | `~/sky-flags.log` | **already `FLAGS-OK`** — the server booted with `--day-time 6000 --advance-time false` |
| evidence | `~/sky-evidence.log` | 711 red then green — **failed fast, see below** |
| corpus | `~/sky-t2.log` | `T2-EXIT=`, and 711's real verdict |
| sweep | `~/sky-sweep.log` | writes `ci/checks/gameplay-falsifiable.toml` |
| T0 | `~/sky-final-t0.log` | `T0FINAL-EXIT=` |

**The evidence stage refused and it refused correctly.** `cargo fmt` had
rewritten the body line `ci/sky-corpus-evidence.sh` anchors its patch on, and
the script printed `anchor has moved; re-point the patch` and exited rather
than patching nothing and reporting a red it had not caused. The anchor is
re-pointed in this branch and `~/rustmc-sky` predates that fix, so **re-run
`./ci/sky-corpus-evidence.sh` after rsyncing this branch's tip.**

### To finish

1. wait for `~/sky-final.out` to carry `SKY-FINAL-DONE`;
2. read 711's verdict out of `~/sky-t2.log` and **tighten
   `ci/checks/gameplay-baseline.toml` from that verdict**, not from this file —
   the row is currently the conservative `diverge` and says so;
3. copy `ci/checks/gameplay-falsifiable.toml` back out of `~/rustmc-sky` and
   commit it; that is what turns T0 green;
4. rsync this branch's tip into `~/rustmc-sky`, re-run
   `./ci/sky-corpus-evidence.sh`, and paste the red/green pair into
   `ci/checks/gameplay/711-...toml`'s header where it promises them;
5. re-run `./ci/t0.sh`.

**Do not rsync into `~/rustmc-sky` while the queued run is in flight** — the
corpus reads `ci/checks/` and runs the built binary, and moving either
underneath it is the 2026-08-06 mtime hazard by another door.
