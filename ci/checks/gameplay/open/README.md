# Checks that do not yet measure anything

These are written down and **not run**. Each was recorded against real
vanilla and proved to demonstrate nothing, which the harness reports as
`vacuous` or `inert` — a fault in the check, not in the server. Neither may
ever be counted, in either direction: such a check cannot pass and cannot
fail, because vanilla itself gave no answer to compare against.

They are kept rather than deleted because the mechanic is real and the
question they raise is open.

## Four checks left this directory on 2026-08-03

`break-block`, `sand-falls-when-unsupported`, `spawn-egg-makes-a-mob` and
`other-player-is-an-entity` were all parked here for the same reason, and it
turned out to be one reason with one cause.

Between them they said: a pig five blocks from a stationary client is never
announced to it; two players five blocks apart are never told the other
moved; `add_entity` for a player arrives only after a 24-block round trip;
and whether a dig's `block_update` reaches the digging client varies run to
run with no change to either server.

**All four are `ChunkMap.isChunkTracked`.** The harness never sent
`chunk_batch_received`, so vanilla's `PlayerChunkSender` — constructed with
`maxUnacknowledgedBatches = 1` — delivered nine chunks and stopped, and

```java
ChunkMap.isChunkTracked(player, x, z)
    = player.getChunkTrackingView().contains(x, z)
      && !player.connection.chunkSender.isPending(ChunkPos.pack(x, z))
```

is the only filter on both `ChunkHolder.broadcastChanges`'s recipient list
and `ChunkMap.TrackedEntity.updatePlayer`'s pairing test. A chunk still
queued for delivery is a chunk whose block changes *and entities* are
withheld from that client. `ci/chunk-batch-probe.sh` measures it: 9 chunks
without the acknowledgement, 117 with it.

The round trip in the third result is the tell, and it was there all along —
walking away and back moves the player between chunks, and one of the nine
delivered ones is where the pairing finally happened.

So none of the four needed a probe of its own, and the answer was never
about entities, about who was watching, or about either server. What was
already ruled out stays ruled out and is recorded here so nobody re-runs it:
the packet ids are right (all 45 cross-checked against Mojang's report), the
decoders work, a failed decode is recorded as `packet:0x..` rather than
dropped, and the pig really did spawn — vanilla's own console found it.

## `a-pig-hit-once-reports-the-damage`, parked 2026-09-06

The third file here, and the first parked for a **reference-side crash**
rather than for a silent reference. Vanilla disconnected the actor on the
attack step: `obs = 0, error = "Broken pipe (os error 32)"` against `obs = 186`
and a correct-looking `damage_event` on this server. The scenarios either side
of it recorded normally, and `090-chest-opens-a-screen` is
`fresh_session = true` in that recording and `false` in every other tree's, so
the socket died inside this check and the harness reconnected after it.

`handleAttack` has one `disconnect` and four cases for it; **which one fired is
not known**, because `t2-gameplay.sh` `rm -rf`'s `$VAN_DIR` on exit and the
console log went with it. The probe that would have named it failed on a full
root filesystem, which is a separate fault and **not** the cause of the
original pipe: every `No space left on device` on this box is timestamped
2026-09-06 07:10 or later and the recording is from ~21:00 the day before. The file's own header lists what is already ruled out
so nobody re-runs those.

It is here rather than in the corpus with a baseline row because the sweep
graded it **tier 0, `killed_by = []`** — nothing could move an outcome whose
reference side is an error — and a baseline row taken from that run would pin
the crash.

## It was empty from 2026-08-03 to 2026-08-10

Everything parked here had been re-armed, and each one left for a reason worth
keeping. **It holds one check again: `160-water-spreads.toml`, re-parked
2026-08-10** — see the section below, which says in its own words that it is a
quarantine and not a closure. The corpus is 87 scenarios while it sits here.

## `water-spreads`, and a check that passes because nothing happened

Parked 2026-08-03 and still parked, for a reason that is genuinely about the
check.

The scenario placed a water bucket with `use_item_on`. `BucketItem` is not a
`BlockItem`: it is used through `use_item`, which ray-traces from the
player's own eyes, so a right-click *on a block face* does nothing at all.
Vanilla's own resync said so — `[2, 0, 0] air` — and this server, which then
had no fluid ticks whatsoever, produced exactly the same nothing. (It has
both since 2026-10-01 — `crate::fluid`, `crate::bucket` — and the flow is
measured by `161`-`168`, whose rigs stay inside one chunk section; this file
is still parked on the cross-chunk order, not on the mechanic.) Two servers
agreeing to do nothing is a match, and it is worthless.

**This is the failure mode that `vacuous` could not catch**, and the reason
the `inert` verdict exists. Vacuous means the reference produced *no
observations under the projection*. Here it produced three — the two
`block_update` resyncs `handleUseItemOn` sends unconditionally, plus the
ack — and every one of them would have been sent whether or not the mechanic
fired. The general shape: **after the resync rule landed, any scenario built
on `place` produces a non-empty vanilla trace by construction.** `inert`
subtracts exactly those and refuses the check if nothing is left; see
`tools/oracle-runner/src/compare.rs::informative`.

**Re-armed 2026-08-03**, and the second half of the reason was a harness bug,
not the check. `use_item` **carries the rotation in its packet** and the
harness hardcoded `(0.0, 0.0)`, so a `look` step was silently ignored by every
`use_item` and the ray always went out horizontally at eye level — which on
flat ground hits nothing within reach. The session tracks yaw and pitch now
and sends them, and the check looks straight down, where the ray
unambiguously hits the block under the player.

Its landing position is still not confirmed against vanilla; the `inert`
verdict is what makes that safe to find out by running it, because a bucket
that empties into nothing produces only the acknowledgement and is refused
rather than counted.

### Re-parked 2026-08-10 — and NOT because it is broken

**This is a quarantine, not a closure.** The mechanic is real, the scenario is
correct, and nothing here is fixed. Read the numbers before re-arming it.

The `worldgen-iceberg-912` merge gate went red with
`!! water-spreads: the server disconnected us in play state` on the *vanilla*
side, and the brief written from that log said the check "cannot be run at
all — on either side". **Both halves of that are refuted by measurement.**

* **Ours ran it.** The same gate recorded `88 scenario(s), 0 could not be run`
  against rustmc and `88 scenario(s), 1 could not be run` against vanilla.
  Only the reference failed.
* **Vanilla runs it too, nearly always.** Over the 33 corpus `vanilla.json`
  recordings on the box, `water-spreads` recorded cleanly **32 times and
  errored once** — the iceberg gate. Re-run alone against a fresh vanilla on
  2026-08-10 it recorded `1/1, 0 could not be run`. The disconnect is a ~3%
  event that has never been reproduced on demand, and vanilla's own log says
  only `oracle lost connection: Disconnected` with no reason logged.

So why park a check that works 32 times in 33?

* **It has never adjudicated anything and cannot.** Its baseline verdict is
  `absent`, and `ci/checks/gameplay-unstable.toml` measures it at **24 flaps
  in 25 comparisons at N = 26** — vanilla disagrees with *itself* about it
  almost every time, because `ChunkHolder.broadcastChanges` coalesces a
  tick's changes and which spreads share a tick is wall-clock jitter. The fix
  is named there (compare the set of `(position, final state)` rather than the
  packet sequence, behind a per-scenario opt-out) and is not written yet.
* **An error can never be blessed.** `Verdict::Error.baselineable()` is
  `false` by design — an errored scenario is a fault baseline or not — so
  there is no way to record the 3% as expected. Every time it fires it reds a
  merge gate, and it buys no coverage in exchange.

**What is fixed, and is not this file's business, is the blast radius.** The
one error used to take 38 healthy checks down with it, because
`record::record` drops the party on failure and the fresh login resets
`Session::seq` and the container counter on that side only, so every later
scenario compared against a counter 31 ahead. `compare::rebase_session_counters`
now rebases both per scenario — three, since 2026-08-11, when
`container_set_slot`'s menu `state` joined `seq` and `window`. Re-adjudicating the iceberg gate's own two
traces with it takes `declared` from 26/88 to 63/88 with the error still in
place.

**To re-arm it:** write the set-comparison opt-out named in
`gameplay-unstable.toml` so it can actually adjudicate, and move it back. Do
not re-arm it merely because it records — recording is not the bar it fails.
