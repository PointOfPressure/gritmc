# Pre-registered prediction — branch `entity-move-collision`, 2026-08-06

Written and committed **before** `ci/t2-gameplay.sh` was run on this branch, so
that the outcome cannot be read backwards into the prediction. The result is
recorded underneath, unedited, whatever it says.

Baseline: `ci/checks/gameplay-baseline.toml`, 86 entries, **53 match, 27
diverge, 6 absent**.

## The prediction

**Nothing moves. 53 / 27 / 6, identical to baseline, in both directions.**

In particular the three checks this branch's subsystem exists for stay exactly
where they are:

| check | baseline | predicted |
|---|---|---|
| `sand-falls-when-unsupported` | diverge | **diverge** |
| `dropping-an-item-throws-it-on-the-floor` | diverge | **diverge** |
| `a-player-that-leaves-the-ground-is-resynced` | diverge | **diverge** |

And the property that matters more than the count must hold: **0 vacuous, 0
inert, 0 error.**

## Why nothing should move, stated so it can be wrong

`crates/rustmc-server/src/collide.rs` is reachable from no runtime path. It is
`pub`, it is compiled, it is tested, and it is called by exactly one thing in
the tree — `crates/rustmc-server/tests/entity_move.rs`. There is no gravity, no
drag, no entity tick and no caller in `status::serve`'s game loop, so no packet
this server sends can be different because of this branch.

That is deliberate and it is the reason the prediction is "nothing". The three
checks above need the physics **above** `Entity.move` — `ItemEntity.tick`,
`FallingBlockEntity.fall`, `ServerPlayer.doTick` — and every one of those needs
gravity, which is `getAttributeValue(Attributes.GRAVITY)` for a living entity
and therefore the attribute system. Wiring a geometry engine underneath them
before that exists would put *some* motion under three checks that currently
carry an honest `diverge`, and make three unmeasured things look measured.

## What would falsify it, and what each would mean

1. **Any check moves at all.** The branch would be doing something at runtime
   that this note says it cannot, and the first thing to check is whether
   `collide` acquired a caller.
2. **The run reports nothing and exits 4.** `ci/t2-gameplay.sh` refuses a
   `data/vanilla` table that is absent, foreign, or older than its generator,
   and this branch adds one — `collision_shapes.json`. That is a tooling
   failure, not a movement, and it must not be reported as either.
   `./ci/gen-gameplay-tables.sh` was run on the remote first and reported
   **13 tables current**, the new one generated and stamped through the
   existing derived-path mechanism, so this is expected to be closed already.
3. **A check moves and re-running does not reproduce it.** Then it is flap, not
   signal, and the corpus has a stability problem this branch did not cause.
   A single-run movement on any of the three above should be re-run before it
   is believed in either direction.
4. **Any vacuous / inert / error appears.** Those say nothing about the server
   and fail the gate regardless of the count.

## Result

Run finished 2026-08-06, `~/rustmc-entitymove-0806` on the Threadripper, full
corpus, log `~/rustmc-logs/entitymove-t2.log`. `T2-GAMEPLAY GREEN`, exit 0.

```text
CORPUS  declared  53/86 match   (27 diverge, 6 absent, 0 vacuous, 0 inert, 0 error)
CORPUS  strict    30/86 match   (52 diverge, 3 absent, 1 vacuous, 0 inert, 0 error)
```

**The prediction is matched, exactly, on every figure it named.** 53 / 27 / 6
declared, 0 vacuous, 0 inert, 0 error. And the three checks:

| check | baseline | predicted | measured |
|---|---|---|---|
| `sand-falls-when-unsupported` | diverge | diverge | **diverge** |
| `dropping-an-item-throws-it-on-the-floor` | diverge | diverge | **diverge** |
| `a-player-that-leaves-the-ground-is-resynced` | diverge | diverge | **diverge** |

A matched prediction is worth less than a missed one, and this one was cheap to
make: the branch has no runtime caller, so "nothing moves" was close to
arithmetic. What it does buy is the second falsifier being closed — the run
did not exit 4. `ci/gen-gameplay-tables.sh` picked up
`data/vanilla/collision_shapes.json` through the existing derived-path grep with
no edit to the gate, generated it, stamped it, and `t2-gameplay`'s preflight
reported **13 tables current and stamped**. A branch bringing new generated data
went through the corpus without touching the mechanism, which is the thing this
branch most needed to know and could not have learned any other way.

### One thing measured that the prediction did not name

The **strict** arm reports **1 vacuous**, on
`a-standing-player-is-rebroadcast-on-a-timer`. The declared arm — the one the
baseline ratchets and the gate reads — is 0 vacuous, so this is not a gate
failure and not a regression against anything recorded.

It is named here rather than passed over because "0 vacuous" is quoted as a
corpus property and that quote is about the declared arm only. This branch
cannot have caused it: nothing calls `crate::collide`, so no packet this server
sends differs. Whoever owns `530` next should know the strict projection of it
observes nothing, and should find out whether that predates this run — **this
run cannot say, because no strict-arm baseline exists to compare against.**

### A caveat on the conditions, since it would have mattered had anything moved

Another agent's corpus (`~/rustmc-gate-tables`) and a worldgen server were
running on the box throughout; load average was 7–9 rather than idle, and the
vanilla arm ran at roughly 4.6 scenarios a minute. Several checks carry
`Timing::Bucket` with 500 ms buckets. Nothing moved, so the contention cost
nothing here — but a movement measured under that load would have needed a
re-run on a quiet box before being believed.
