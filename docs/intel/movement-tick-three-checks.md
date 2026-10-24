# The three checks the movement tick was supposed to close — measured first

Branch `movement-tick`, off `entity-move-collision` (`65ced03`), 2026-08-06.
Written **before** any server code was changed on this branch, so that nothing
below is a rationalisation of a result.

The item was: wire `crate::collide` into the tick and close
`070-sand-falls-when-unsupported`, `500-dropping-an-item-throws-it-on-the-floor`
and `540-a-player-that-leaves-the-ground-is-resynced`. Two of the three cannot
close, for two different reasons, and both reasons are measurements rather than
readings. That is the first thing this branch has to say.

## Baseline, measured on this tree

`ci/t2-gameplay-subset.sh` over exactly those three, `~/rustmc-movetick`,
2026-08-06:

```text
  diverge  sand-falls-when-unsupported              we never sent: add_entity
  diverge  dropping-an-item-throws-it-on-the-floor  we never sent: set_entity_motion
  diverge  a-player-that-leaves-the-ground-is-resynced
                                                    we never sent: move_entity
                                                    we also sent:  entity_position
  CORPUS  declared  0/3 match   (3 diverge, 0 absent, 0 vacuous, 0 inert, 0 error)
```

**A subset run is not a corpus figure** — `ci/t2-gameplay-subset.sh` says so in
its own header and its denominator is whatever was passed on the command line.
It is quoted here as the *shape* of each divergence, never as a score.

## 070 — closable, and the only one of the three that is

Vanilla's arm is deterministic. The same five projected observations, with the
same digests, appear in `~/rustmc-entitymove-0806`'s full-corpus recording of
2026-08-06 and in this tree's subset run:

```text
  block_update [1,0,0] air            the dig
  add_entity   [1.5,1.0,0.5] kind 51  rest dc54fa7f9dce24f3   <- identical in both runs
  block_update [1,1,0] air            the sand removed by FallingBlockEntity.fall
  block_update [1,0,0] sand           sendToTrackingPlayers, from the entity's own tick
  block_update [1,0,0] sand           the deferred section broadcast
```

Nothing in it is drawn from a random source. `FallingBlockEntity.fall` puts the
entity at `(pos.getX()+0.5, pos.getY(), pos.getZ()+0.5)` and `FallingBlock.tick`
fires on a scheduled tick with a fixed delay of 2. This is a check that can be
closed by writing the mechanism, and it is the one this branch aims at.

## 500 — cannot close, and no server can close it

`set_entity_motion`'s velocity **is inside the observation's `rest` digest** and
is deliberately left there. `crate::trace`'s decoder reads the `LpVec3` beside
the digest rather than instead of it, and its own comment gives the reason: the
packing clamps at 32766, so 32766 and 32767 are different bytes that decode to
the same double, and a decoder that consumed them would make two different wire
messages compare equal.

The item's velocity is four unseeded `RandomSource.nextFloat()` draws in
`LivingEntity.createItemStackToDrop`. Its spawn rotation is a fifth. So:

* `add_entity`'s `rest` differs on every run — measured again here, vanilla's
  own digest for that packet was `7a53ea802245effe` in this tree's subset run
  and `05e78c9e174f33c4` in an older one, with the position identical to the
  last bit in both;
* every `set_entity_motion` and every position that follows is a deterministic
  function of that velocity, so **implementing `ItemEntity.tick` correctly adds
  observations that cannot match rather than removing ones that do not.**

`ci/checks/gameplay-unstable.toml` already carries this check at **25 flaps in
25 comparisons at N = 26** — twenty-six cold vanilla servers disagreeing with
themselves. That file's header states the consequence plainly: such a check
"can never come back `match` for anybody — not for us, and not for vanilla
against itself."

**`ItemEntity.tick` is ported on this branch anyway.** Fidelity is not
conditional on a check being able to see it: a thrown item that hangs in the air
is a defect a player sees immediately, and `docs/SERVER.md` has carried the
absence as a known one since `dropitem.rs` landed. What must not happen is 500
being reported as closed, or its `diverge` being read as a defect this branch
failed to fix.

## 540 — cannot close: **the reference is not reproducible**

This is the finding worth the most, and it refutes the premise the item was
written on — that 540 is waiting on server-side player physics.

### Vanilla does not simulate a player's movement on the server

Read from the pinned jar, and every link in it checked rather than assumed:

```text
  LivingEntity.aiStep      600: canSimulateMovement()  ifeq -> skip travel
                           607: isEffectiveAi()        ifeq -> skip travel
                           614: travel(Vec3(xxa, yya, zza))
  Entity.canSimulateMovement   -> isLocalInstanceAuthoritative()
  Entity.isLocalInstanceAuthoritative
        (server side)          -> !isClientAuthoritative()
  Player.isClientAuthoritative -> iconst_1        (true, unconditionally)
```

So a `ServerPlayer` never reaches `travel`, never applies gravity, and never
runs the friction step. The same predicate gates two more limbs of
`Entity.move`: `restituteMovementAfterCollisions` (offset 563) and
`checkFallDamage` (offset 525) are both skipped for a player, and
`handleMovePlayer` calls `doCheckFallDamage` itself instead.

A player's position on the server is the client's claim run through
`Entity.move` from `handleMovePlayer`, and the ground flag is written from
`packet.isOnGround()` at offset 1160. **There is no player gravity to port**,
and `getDefaultGravity()` being an attribute read is therefore not what 540 was
blocked on. (It is still an attribute read, and the attribute path is ported
here — see below for what actually needs it.)

### The measurement

`ci/.flap540.sh 5` — five back-to-back runs of `ci/t2-gameplay-subset.sh` over
540 and two probes that differ from it only in the ground bit, same jar, same
tree, plus the two earlier subset runs of 540 itself. Kinds on the observer's
stream, `@` is the step, `S` = `entity_position` (a forced sync), `d` =
`move_entity` (a delta):

```text
  scenario                 bits    run1   run2   run3   run4   run5   (earlier)
  540                      0 then 1  SS@0 d@1  SS@0 d@1  SS@0 d@1  d@0 d@1  d@0 d@1   d@0 d@1  (x2)
  probe-ground-flag-clear  0 then 0  d@0 SS@1  SS@0 d@1  SS@0 d@1  d@0 SS@1 d@0 SS@1  SS@0 SS@1
  probe-ground-flag-set    1 then 1  d@0 d@1   d@0 d@1   d@0 d@1   d@0 d@1  d@0 d@1   d@0 d@1
```

With the ground bit **set**, vanilla is stable: a delta each step, **6 runs for
6**. With the bit **clear**, the same scenario against the same jar produces a
different *packet kind* on different runs: 540 came back `SS@0 d@1` on three
runs and `d@0 d@1` on four, and `SS@0 d@1` is exactly what the check file
recorded on 2026-08-04 — so both observations are real and neither is a stale
artefact. The clear/clear probe produced **three** distinct sequences across
six runs.

`3/7` and `4/7` on one check is not a rate anybody should quote as a
probability; what it establishes is only that the sequence is **not a function
of the input**, which is all that is needed to say the check cannot be closed.

Two syncs at one position with **different** `rest` digests means the flag
crossed the predicate twice inside one step: false, then true again, with no
packet in between. Vanilla restores the flag on its own within a tick or two,
and whether `sendChanges` — which for a player runs every second tick,
`updateInterval` 2 — lands inside that window is a phase coin flip against the
arrival time of the movement packet.

This is the same mechanism the corpus already has a name for. `compare::project`
filters the 60-tick heartbeat out for exactly this reason, with the arithmetic
written out: one fair coin per run predicts the disagreement rate that was
measured. 540 is a second instance of it, on a different packet.

### What this does and does not license

It does **not** license deleting `|| self.was_on_ground != e.on_ground` from
`crate::tracker`. That disjunct is real, it is `ServerEntity.sendChanges`
offsets 680..691, and it has a unit test. Removing it to make a red check go
green would be manufacturing a pass out of a measurement — the exact thing this
corpus has found sixteen instances of.

It does mean 540 belongs in `ci/checks/gameplay-unstable.toml`, alongside the
entries whose reference draws a fresh random value. **This branch does not add
it there**, because that file is verified in both directions by
`ci/t2-gameplay-selfcheck.sh` and the evidence here is five subset runs rather
than a selfcheck at N ≥ 12. The measurement is recorded; the declaration is the
orchestrator's, and `SELFCHECK_RUNS=12 ./ci/t2-gameplay-selfcheck.sh` is the
run that would settle it.

The two probe files that produced the table above are deliberately **not**
committed: they are 540 with one field changed, they would be two more entries
in a corpus whose denominator is quoted, and their whole content is in this
document. `ci/.flap540.sh` is likewise untracked.

## What actually needs the attribute system

Not gravity for a player — nothing reads it, because nothing calls `travel`.
What does need it is `Entity.maxUpStep()`, which `crate::collide::collide` takes
as an argument and which is `getAttributeValue(Attributes.STEP_HEIGHT)` for any
`LivingEntity`. A player walking into a slab auto-steps by 0.6 and that 0.6 is
an attribute with a range, not a constant. `LivingEntity.getAirDrag()` and
`getEntityBounciness()` are two more attribute reads on the same path.

So the attribute path is ported here — `data/vanilla/attributes.json` from
`ci/attributes-golden.sh`, and the modifier arithmetic in
`crates/rustmc-server/src/attributes.rs` — and the two entity kinds that
actually fall under their own power do not touch it:
`ItemEntity.getDefaultGravity()` and `FallingBlockEntity.getDefaultGravity()`
are each `ldc2_w 0.04d`, a literal, in the bytecode.
