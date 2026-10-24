# Cold start — branch `gate-skips-and-scenario-verbs`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `gate-skips-and-scenario-verbs` off `9f3723d`, 2026-08-05

**The gate can no longer report green through a skipped test, and the
scenario language can hit things and sneak.** T0 green, not merged — the
orchestrator gates with `./ci/handoff.sh`. Remote dir
`~/rustmc-chain-skipverbs` on the **Threadripper**; logs on the laptop under
`~/rustmc-private/chain-skipverbs/`.

#### A missing fixture is now a failure, and the guard was made to fail first

`real_vanilla_chunks_survive_a_rewrite` — the only test this project has ever
had that checks the chunk writer against bytes real vanilla wrote — had skipped
in **every** gate run on the box for its whole life, and no grep could ever have
seen it: a test that returns early *passes*, and cargo captures a passing test's
stderr. Seventeenth check that could not fail, and it was inside the mechanism
certifying the other sixteen.

**The default is inverted rather than the grep widened.** `rustmc_data::fixture`
panics on a missing fixture; `RUSTMC_FIXTURES=optional` is an explicit opt-out,
under which `ci/t0.sh` and `ci/t1.sh` print a *different* sentinel so
`grep -c "^T0 GREEN"` returns 0 and `ci/handoff.sh` reddens without knowing the
variable exists. `ci/t1-gate.sh` refuses optional mode up front. Blindness now
takes an explicit act; forgetting is the safe direction.

Four arms on the box, chain dir's `oracle-work` symlink repointed at an empty
directory and back — the shared reference directory was never touched:

```text
  arm A  fixture absent, default        exit 101, grep "^T0 GREEN" = 0
  arm B  fixture absent, optional       exit 0,   grep = 0, "T0 PASS"
  arm B2 optional, ci/t1-gate.sh        exit 1, red before the suite runs
  arm C  RUSTMC_FIXTURES=optionl        exit 101, grep = 0, "is not a mode"
  arm D  fixture present, default       exit 0,   grep = 1, "T0 GREEN"
```

Only arm D lets a gate through. Logs:
`~/rustmc-private/chain-skipverbs/falsification-link1.log`.

**The fixture now lives where the gate reaches it.** It was in
`~/rustmc-chain-persist` only; it is now `~/rustmc/oracle-work/t2-regiondiff`
(14 MB, 16 region files) — the shared read-only reference directory
`ci/handoff.sh` stage 1 links every gate directory from — and linked into
`~/rustmc-gate` directly. What that buys, from the test's own `--nocapture`
output, which no gate had ever printed:

```text
  rewrote 240 real vanilla chunks; palette sizes seen:
  {1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,17,18,19}
```

Its own fixture-variety assertion wanted a palette larger than 2 and had never
once been evaluated.

**Two call sites converted, not the ~60 that share the pattern — because the
rest were measured rather than guessed at.**
`cargo test -q --workspace --no-fail-fast -- --nocapture 2>&1 | grep -E "^skipping:"`
in the chain dir, over all 95 binaries, returns **three** real skips and they
are all the same probe family:

```text
  skipping: no oracle-work/van-ruin--58_28
  skipping: no oracle-work/van-ruin--57_42
  skipping: no generated world at /tmp/ruin-w/region
```

So ~57 of the ~60 sites are already fixture-satisfied on this box and
converting them is a provable no-op that arms the guard. **That is the next
item on this link** — a mechanical sweep, cheap, and the census above is the
evidence it is safe. The three underwater-ruin fixtures are a named hole, not
a discovered one.

#### The scenario language got two verbs, and the jar refuted the brief twice

A read-only sweep (`~/rustmc-private/FINDING-coverage-omission-2026-08-05.md`)
found 33 of 92 mechanics with zero checks, and the top five gaps were
**unwritten verbs, not unwritten checks**.

Both derivations came out different from every account older than 26.2, and a
port written from memory would have compiled and been silently wrong:

* **Attack is not an `interact`.** `ServerboundAttackPacket` is a record of one
  field, `int entityId`, id **1**. `ServerboundInteractPacket` is a flat
  four-field record with **no `Action` at all** — entityId, hand, location,
  usingSecondaryAction. The ATTACK/INTERACT/INTERACT_AT discriminant is gone in
  both directions.
* **Sneak is not in `ServerboundPlayerCommandPacket`.** That enum has seven
  entries in 26.2 and neither shift-key entry is among them. Sneak is
  `FLAG_SHIFT = 32` in `ServerboundPlayerInputPacket` (id 43), one byte,
  `Input.STREAM_CODEC`. Jump is bit 16 of the same byte — **which means
  jumping, ranked item 2 of the coverage sweep, is now one bool away**, and it
  is not built here only because it needs the player physics that
  `## SERVER-SIDE PLAYER PHYSICS — SCOPED, NOT BUILT` describes.

Four checks, all measured with `ci/t2-gameplay-subset.sh` against real vanilla
26.2 protocol 776:

```text
  550 attacking-another-player       absent   vanilla plays sound 1304
                                              entity.player.attack.nodamage
  551 attacking-from-out-of-range    absent   two attacks, far then near;
                                              vanilla answers with ONE sound
  552 sneaking-with-a-block-in-hand  diverge  vanilla places the stone
  553 a-block-in-hand-does-not-...   match    vanilla flips the lever
```

552/553 are the pair that finally covers the divergence `docs/SERVER.md` has
described in prose for weeks. **Our traces for 552 and 553 are identical to
each other** — this server never consults the sneak flag. 553 matching is what
a degenerate half is for: it passes under the bug, and what it forbids is the
wrong fix.

552 also closes the corpus's other named ordering hole. All fourteen
interactive checks hold `minecraft:stick`, so the right-click-ordering rule had
fourteen checks and none could see it.

#### THE FIRST GATE WENT RED, AND THE CAUSE GENERALISES — read this before writing any check

`ci/handoff.sh` came back **RED on T2** with T0 and T1 green:
`attacking-from-out-of-range` was **`absent` in `~/rustmc-chain-skipverbs` and
`vacuous` in `~/rustmc-gate`, on the same commit.** A check whose verdict
depends on which directory it ran in is not yet a check.

**It was not a coin flip, and that was measured before anything was changed.**
The check as written, run alone in the chain dir ten times, came back
**10/10 `absent`** (`~/rustmc-logs/rate-before-551.log`). By
`ci/t2-gameplay-selfcheck.sh`'s own model — `P(detect) = 1 - p^N - (1-p)^N` —
an even flap would have been caught at N=10 with probability 99.8%, and a
one-in-six flap 84% of the time. So it is stable in isolation and still
produced the other verdict in the gate: **environment, not chance.**

**The gate's own log contains the controlled comparison**, which is better
evidence than any derivation and was there from the start: in the same failing
run, `550` was **`absent`** and `551` was **`vacuous`**. `absent` means
vanilla's projected trace was non-empty, so the actor-side attack sound *did*
arrive in the environment that starved the observer-side `animate`. Same
server, same run, same second, two checks differing in which stream they read.

**The cause, derived rather than assumed.** That check's entire vanilla trace
was *one observation* — an `animate` broadcast to the observer, from the swing
the attack verb sends — because the attack itself is refused at the reach test
and produces nothing, which is the whole point of the check. And an entity
broadcast is not unconditional. `ChunkMap$TrackedEntity.updatePlayer`,
`javap -p -c -constants`, offsets 75..130:

```text
  dx² + dz² <= min(getEffectiveRange(), getPlayerViewDistance()*16)²
    &&  entity.broadcastToPlayer(player)
    &&  chunkMap.isChunkTracked(player, entity.chunkPosition().x(),
                                        entity.chunkPosition().z())
```

`isChunkTracked` is `getChunkTrackingView().contains(x, z) &&
!chunkSender.isPending(x, z)`. The check walked the actor from the workspace
origin (chunk z 0) to z = 20, **chunk z 1**, so whether the observer heard the
swing depended on whether it had finished streaming a chunk it had no other
reason to want. In a two-scenario subset it had; sixty scenarios into the gate
it had not.

**`scenario.rs`'s `From` docs already said this about block broadcasts** — "a
chunk still queued for delivery is a chunk whose block changes are withheld
from that client" — and the same filter gates entity packets. The check was
built on the fragile side of a hazard this project had already paid to learn.

**Three rules that follow, and they are not specific to combat:**

1. **A check that asserts an absence needs positive content in the same
   scenario.** Otherwise its non-vacuity rests on whatever incidental packet
   happens to be nearby. 551 now attacks twice in one scenario — far, then near
   — so the near attack's sound keeps the trace non-empty *by construction* and
   the far attack's silence is measured against it. It can only be vacuous now
   if 550 is also broken.
2. **Prefer the actor's own stream for anything that must not disappear.** An
   observation at the actor's own position sits in the chunk the actor is
   standing in, which it necessarily tracks. `observe_from = "actor"` is not a
   smaller projection here, it is a *safer* one — which is the opposite of what
   `From`'s doc comment says in general, and the difference is that this is a
   broadcast rather than a direct send.
3. **Never let a fragile kind sit in an `observe` list "just in case".**
   `animate` never decided 550's verdict, because vanilla's attack sound was in
   the same trace and we send nothing either way — but it would have started
   deciding it the moment the sound was implemented. A check that becomes
   unstable when the server improves is worse than one that is unstable now.
   Dropped from both.

**A second environmental difference, found while reproducing this and worth
its own item.** `ci/t2-gameplay.sh` refuses to run in a tree whose
`oracle-work` is a whole-directory symlink — "the gate would record into, and
read from, a tree it is not gating" — and **`ci/t2-gameplay-subset.sh` does
not**. So every subset run in this chain recorded into
`~/rustmc/oracle-work/gameplay/subset` and booted its vanilla there, in a world
whose workspaces earlier runs had already generated; the gate booted a fresh
world and streamed chunks for 67 workspaces as it went. That is consistent with
the chunk-streaming mechanism above and was not isolated as a cause, but it is
exactly the "measuring a tree you are not gating" hazard the full runner's own
preflight exists to refuse. **The subset runner should carry the same refusal**
— it is the instrument agents reach for precisely when they are in a chain dir,
which is where the hazard lives. Not fixed here: the gate was red and this is
not what made it red.

**Verified in the environment that broke it.** `./ci/t2-gameplay.sh` — the
gate's own stage 5, whole corpus, fresh vanilla world — in
`~/rustmc-chain-skipverbs`:

```text
  T2-GAMEPLAY GREEN
  CORPUS declared 37/67 match (24 diverge, 6 absent, 0 vacuous, 0 inert, 0 error)
    the failing gate run:  37/67 (24 diverge, 5 absent, 1 vacuous, ...)
```

The only delta against the red run is the one intended: 551 vacuous -> absent,
its baseline entry. Same 37 match, same 24 diverge, no REGRESSED line. The four
new checks all sit at their declared verdicts — 550 absent, 551 absent, 552
diverge, 553 match — and 551 runs **65th of 67**, deepest into the run, which is
the worst case for the chunk-streaming pressure that starved it.

(The `strict` line reports one vacuous; it is
`a-standing-player-is-rebroadcast-on-a-timer`, pre-existing, and strict is
explicitly not ratcheted and not quotable as a parity figure.)

**Isolation runs cannot validate this fix, and that is the point of the number
above.** The *old* check was 10/10 stable alone while still failing the gate, so
ten runs of the fixed one alone would return 10/10 and prove nothing about the
failure mode. The meaningful evidence is a whole-corpus run and the gate itself.

Logs: `~/rustmc-private/chain-skipverbs/rate-before-551.log`,
`t2-full-corpus-551fix.log`, and `~/rustmc-logs/rate-before-551.log` on the box.

#### Two predictions this branch made and then refuted — do not re-derive these

1. **`handleAttack`'s literal `3.0d` is NOT the attack range.** 551 was first
   written at the observer's own 5-block offset on that reading, and vanilla
   played the sound anyway. It is a *padding* passed through
   `Player.isWithinAttackRange` -> `AttackRange.isInRange`, added to the
   `ENTITY_INTERACTION_RANGE` attribute; `AttackRange` also carries separate
   `minCreativeReach`/`maxCreativeReach`. 551 now stands far enough out that no
   reading reaches, rather than asserting a cutoff. **Finding the exact cutoff
   needs a distance sweep and is unclaimed work.**
2. **A state-shaped verb leaks across scenarios, and it killed a check within
   the hour.** `record` logs in once and reuses one connection; sneak persists
   server-side until another packet clears it. Run together the first time,
   vanilla produced *identical traces for both halves of the pair* — 552 left
   553 sneaking — and the pair distinguished nothing while reporting two
   plausible `diverge`s. Nothing errored, nothing flapped. Fixed in `record.rs`
   beside the `goto` that already resets position: both clients are put into a
   known input state before every scenario. **Any future state-shaped verb
   belongs in that reset.**

Regression probe on the reset, since it now runs before every scenario in the
corpus — seven checks spanning block entities, containers, interaction, entity
tracking and movement, all identical to their baseline entries, 0 vacuous,
0 inert, 0 error: `~/rustmc-private/chain-skipverbs/subset-regression-probe.log`.

#### What is next on this link, in order

1. **`ServerboundContainerClickPacket` — the container-click verb.** Derived as
   far as the class list and left there deliberately: it is much bigger than
   the other two (an `ItemStack` codec, a changed-slots map, a carried-item
   tail) and the standing instruction is to land each verb with its checks
   rather than ship the enum at once. It unlocks storing, retrieving and
   crafting — ranked item 4, and a first-ten-minutes action for every player.
2. **The ~57 remaining skip sites**, per the census above.
3. **`ServerboundSignUpdatePacket` exists** (id 61) and sign *text* is a named
   ZERO. Cheap, and it is the only block-entity content the corpus could reach.
4. The attack-range sweep from refutation 1.
