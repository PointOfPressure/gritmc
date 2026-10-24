# Cold start — branch `gameplay-place-sound`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `gameplay-place-sound`, rebased onto `main` (`05c9a2d3`), 2026-08-09

**Item: `PARITY-ROADMAP.md` §3 S3 — place sound + step sound + waypoint.**
Worktree `~/grit/work-placesound`; box work in `~/rustmc-placesound`.
Started off `container-quick-move` (`add7297a`) and rebased onto `05c9a2d3`
after S1, S2, `census-not-applicable` and `perf-surface-biome-id` landed.

**THE BASELINE MOVED UNDER THIS BRANCH AND THE FIGURE TO BEAT IS 57, NOT 56.**
S2 (`recipe_book_add`) converted `040` and took strict from 56/86 to 57/86
before this branch was measured. The before-figure for S3 is therefore

```
BEFORE   declared  60/86 match   (20 diverge, 6 absent, 0 vacuous, 0 inert, 0 error)
         strict    57/86 match   (26 diverge, 3 absent, 0 vacuous, 0 inert, 0 error)
```

read from `~/rustmc-recipebook/oracle-work/gameplay/report.json`, written
2026-08-08 23:06 by `ci/t2-gameplay.sh` on the S2 branch that became part of
`05c9a2d3`. **That same report records `080-other-player-is-an-entity` as
declared `match`, strict `diverge`** — which is this item's red-before, taken
by a different tree, with the full instrument, and with no dependence on this
branch's build cache.

**WITHDRAWN, and why.** An earlier red-before for `080` was taken on
2026-08-08 22:31 with `ci/t2-gameplay-subset.sh` over nine checks, against a
`git archive add7297a` tree rsynced into `~/rustmc-placesound`. **It is not
quotable and it is not merely stale: it cannot be shown to have measured the
base at all.** The target directory then held release artifacts built from
this branch's tip minutes earlier, and `rsync -a` gave the base's sources
mtimes *older* than those artifacts — the exact hazard `docs/AGENT-BRIEF.md`
now carries. Whether cargo rebuilt (because three source files had vanished,
which invalidates a dep-info) or reused the tip's binary is not recoverable:
`t2-gameplay-subset.sh` deletes its traces on exit, so the one artefact that
would have settled it — whether that run's `rustmc.json` contains any
`packet:0x8A` — is gone. It is replaced above by a figure that never touched
this tree.

**S3's heading is right and the row is three mechanisms, not two.** They share
one table (`SoundType`) and nothing else, and only two of the three are on
`080`'s critical path:

| mechanism | who it is for | converts |
|---|---|---|
| place sound, `BlockItem.place` 208-240 | the **observer** half of the 32 `*.place` scenarios | nothing on its own |
| step sound, `Entity.playStepSound` | `080`, and the observer half of `540`/`551` | half of `080` |
| waypoint, `ServerWaypointManager` | `080` | the other half |

**Everything below was read out of the pinned jar with `javap -p -c
-constants` before a line was written, and the waypoint half was then
confirmed byte-exactly against a recorded vanilla trace.**

**1. `Player.playSound(SoundEvent, float, float)` is overridden and passes
`this`.** `Entity.playSound` passes `aconst_null` — everybody hears it —
but `Player`'s override passes `aload_0` into `Level.playSound(Entity, DDD,
…)`, so **a player's step sound excludes the player who took the step**.
That is the whole reason 080's two step sounds are on the observer's stream
and not the actor's, and a step sound broadcast to everybody would be as
wrong as a place sound broadcast to everybody. `Player.getSoundSource()` is
`SoundSource.PLAYERS`, ordinal **7**, not `BLOCKS`.

**2. The step cadence is `moveDist` / `nextStep`, and it predicts 080's count
exactly.** `Entity.applyMovementEmissionAndPlaySound`: `moveDist +=
horizontalDistance * 0.6` (the constant is `0.6000000238418579d`, `0.6f`
widened), and a step fires when `moveDist > nextStep`, after which
`nextStep = floor(moveDist) + 1`. 080 is two 4-block hops (`hop_to`'s `HOP`
is 4.0 and 080 walks 4), so `moveDist` is 2.4 then 4.8 against `nextStep`
1 → 3 → 5: **two sounds**, which is exactly the `sound 2->0` the gap table
records.

**3. The waypoint packets are `TRACK`, not `UPDATE`, and the reason is a
threshold.** `WaypointTransmitter$BlockConnection.isBroken()` is
`distanceManhattan() > 1`; a 4-block hop breaks the connection, so
`updateConnection` remakes it and the remake calls `connect()`, which sends
`TRACK`. `update()` — which sends `UPDATE` — is only reachable for a move of
one block or less.

**4. Both of 080's waypoint packets are reproduced byte-exactly, from the
bytecode alone.** The trace records `packet:0x8A` with `rest` digests
`48d2aef46e816d05` and `a94195be44c89f19` (vanilla run of 2026-08-08 21:26,
`~/rustmc/oracle-work/gameplay/vanilla.json`, 080's anchor `[-256,-60,0]`).
The body derived from `ClientboundTrackedWaypointPacket.STREAM_CODEC` /
`TrackedWaypoint.write` —

```
  varint operation (TRACK=0) | 0x01 (Either.left) | 16-byte offline UUID of
  "oracle" | string "minecraft:default" | 0x00 (no colour) |
  varint type (VEC3I=1) | VarInt x, y, z          (VarInt, NOT zigzag)
```

— hashes to those two digests at `(-252,-60,0)` and `(-256,-60,0)`. **Use the
tree's own multiplier `0x1000_0000_01b3` and not FNV's**; the real prime is
one hex digit shorter and gives an answer that agrees in the bottom five bytes
and differs in the top three, exactly as `trace.rs` warns.

**5. Rule 27 is settled by implementing the transmitter, not by touching the
gamerule.** `GameRules.LOCATOR_BAR` is `registerBoolean("locator_bar", PLAYER,
true)` — `iconst_1`, default **true** — and `ci/t2-gameplay.sh` never sets it,
so both servers run at the default and nothing is being silenced.

**6. The exclusion, for the place sound.** `BlockItem.place` offsets 208-212
pass `aload 6` — the placing player — into `Level.playSound`, and
`PlayerList.broadcast` skips `except`. Volume is
`(SoundType.getVolume() + 1.0F) / 2.0F` (223-231) and pitch
`SoundType.getPitch() * 0.8F` (232-239); the `SoundType` comes from `aload 8`,
the state **read back from the level** after the write, not the state that was
asked for.

**7. A prediction of mine that measurement killed, before it could mislead
anyone.** A unit test asserted that the step count cannot depend on how a walk
is chopped into packets, because `moveDist` is a path length. T0 refuted it:
**eight one-block packets give four sounds and two four-block packets give
two.** `nextStep()` is `floor(moveDist) + 1`, so it sets the next threshold
from where the walk *is* and throws away the overshoot, and a large hop skips
every threshold it jumps over. Consequence: 080's *two* is a fact about
`hop_to`'s `HOP = 4`, not about eight blocks. Both servers see the same packet
stream, so both count the same — but a future `HOP` change moves 080's sound
count on both sides at once, and anyone reading "two" as a property of the
walk will be wrong.

**8. Four independent corroborations taken before the run, all free.** The
recorded vanilla stream for the four scenarios that place — `020`, `030`,
`130`, `300` — carries exactly one sound each, on the **observer** only,
`source: 4`, volume `1.0`, at the block centre, with event ids 1601, 1856,
1856 and 978. `data/vanilla/protocol_ids.json` resolves
`block.stone.place`/`block.wood.place`/`block.metal.place`/`block.grass.step`
to **1601 / 1856 / 978 / 759** — the recording's four numbers, from a table
generated on a different day by a different script. And `sound_types.json`
gives `GRASS` volume 1.0 pitch 1.0 (so a step is 0.15/1.0) and `METAL` 1.0/1.5
(so a rail places at 1.0/1.2, §3 S3's own figure).

**THE PREDICTION, WRITTEN BEFORE THE RUN THAT MEASURES IT** (committed
2026-08-08 while the corpus was still queued behind two other trees):

* `080-other-player-is-an-entity` goes **strict `diverge` -> strict `match`**.
  Its declared verdict was already `match` and stays there.
* **Declared totals do not move: 60/86 stays 60/86.** Every one of the 32
  place scenarios is `observe_from = "actor"`, so the sound this branch adds
  lands on a stream their projection deletes; and no walking scenario names
  `sound` in its `observe` list. The place sound's whole value here is the
  *observer half* it unblocks, and an unblock is not a conversion.
* **Strict goes 57/86 -> 58/86**, the one being 080. (Written as
  `56 -> 57` before the rebase; S2 landed `040` in between and moved the
  baseline, so the *delta* predicted is unchanged at +1 and the endpoints
  are restated rather than the claim.)
* The scenario most likely to disprove this is **`490-a-bystander-sees-a-chest-open`**,
  the only place-ish scenario that is both `observe_from = "observer"` and a
  strict `match`. Its measured step right-clicks a chest with a **stick**, so
  no `BlockItem.place` runs and no place sound should appear; if 490 goes red,
  the cause is that our placement path fires where vanilla's does not.
* `530` and `540` cannot be lost — both are already strict `diverge` — and
  `551`/`550` are `absent` for an attack sound this branch does not touch.

**THE RESULT — measured 2026-08-09, `ci/t2-gameplay.sh`, full corpus,
tip `65b1e04b` in `~/rustmc-placesound`.**

```
BEFORE   declared  60/86 match   (20 diverge, 6 absent, 0 vacuous, 0 inert, 0 error)
         strict    57/86 match   (26 diverge, 3 absent, 0 vacuous, 0 inert, 0 error)
AFTER    declared  60/86 match   (20 diverge, 6 absent, 0 vacuous, 0 inert, 0 error)
         strict    58/86 match   (25 diverge, 3 absent, 0 vacuous, 0 inert, 0 error)
```

**`080-other-player-is-an-entity` converts, and it is the only verdict change
in the corpus** — `(match, diverge) -> (match, match)`. Declared is
unchanged, strict is +1, nothing regressed, and the run is `GAMEPLAY GREEN`
against the ratchet. **The prediction committed before the run said exactly
this** (declared unchanged, strict +1, the one being 080), which is the first
prediction in this branch that survived measurement — the earlier one, that
the step count is invariant to hop size, did not.

The five `BETTER container-click-*` lines in the run's output are inherited
from `main`'s S5 and are **not** this branch's.

**WHAT HAPPENED TO THE 32, WHICH IS THE PART THAT IS NOT A VERDICT.** §3 S3
names 32 scenarios that carry a `*.place` sound on vanilla, all
`observe_from = "actor"`, so none of them can convert on a sound their own
projection deletes. What is measurable is whether the *observer* stream is
now right, and it is:

**26 of the 32 observer streams are identical to vanilla on every compared
sound field** — event id, position, source and volume — where before this
branch all 32 were empty. The six that differ are each a defect **somewhere
else** that emitting the sound has made visible for the first time, and in
every one of the six the place sound this branch adds is itself correct:

| # | scenario | vanilla / ours | what it exposes |
|---|---|---|---|
| 170 | `slab-stacks-into-a-double-slab` | 2 / 2 | both `wood.place`; vanilla's second is at `[1.5, 0.5, 0.5]` and ours at `[1.5, 1.5, 0.5]` — **we put the second slab in the block above instead of making a double slab.** A placement defect the sound reports faithfully |
| 250 | `snow-layers-stack` | 2 / 1 | we place once where vanilla places twice — layer stacking |
| 380 | `a-piston-extends-when-powered` | 2 / 1 | our `metal.place` is exact; the missing one is `block.piston.extend`, which needs the piston |
| 390 | `a-piston-pushes-the-block-in-front` | 2 / 1 | same |
| 552 | `sneaking-…-places-it-on-a-lever` | 1 / 1 | vanilla plays `stone.place` at `[1.5, 1.5, 0.5]`; we play `lever.click`. **No sneak flag**, so `useWithoutItem` is not suppressed and we flip the lever instead of placing on it — `docs/SERVER.md`'s `isSecondaryUseActive` gap, now audible |
| 609 | `sneaking-…-does-not-open-a-chest` | 1 / 1 | same gap; we play `chest.open` where vanilla plays `stone.place` |

So the unblock is real and it is measured: **32 observer streams went from
silent to 26 exactly right and 6 wrong for reasons that are now named.**

**One over-emission, with its cause pinned.**
`530-walking-away-drops-a-standing-player-from-view` gets **22** step sounds
on vanilla's observer stream and **68** on ours; 51 of ours are further than
16 blocks from the workspace, where vanilla's stop at 20. That is
`ServerLevel.playSound`'s `volume > 1.0 ? volume * 16 : 16` broadcast
distance, which this server does not model — see `crate::sounds`. It cost no
verdict (530 is strict `diverge` on G5 regardless) and it is the first thing
a walking check further out than 16 blocks would need. **Before the step
sound existed this gap was unobservable**, because every sound this server
played was at a block the actor was standing next to.

`water-spreads` shows vanilla 364 waypoints and 11 sounds against our 0, and
that is not a waypoint defect: vanilla's own server ticks the player, water
pushes it, `Entity.setPosRaw` fires, and this server moves a player only when
the client says so. It is `absent` in both denominators before and after.

**What is implemented, and what is knowingly not.** `crate::sound_types`
(new generated table, 126 distinct SoundTypes over 32,366 states, keyed by
state because `getSoundType` is virtual — exactly **1** block has a state that
disagrees with its default), `crate::step`, `crate::waypoints`, and two new
constructors in `crate::sounds`. Not modelled, each written up in its module:
`mainSupportingBlockPos` (so `getOnPos` uses vanilla's fallback, which agrees
on flat ground and can differ on a slab); the sneak/swim/climb/rails/flight
clauses of `vibrationAndSoundEffectsFromBlock`; and the chunk and azimuth
waypoint connection kinds, so two players far enough apart get a position
waypoint here and a coarser one in vanilla.
