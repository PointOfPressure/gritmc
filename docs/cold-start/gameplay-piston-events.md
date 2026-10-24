# Cold start — branch `gameplay-piston-events`

### COLD START — branch `gameplay-piston-events`, 2026-08-17

**Area: pistons — the block-event behaviour behind corpus scenarios 380 and
390.** Branched from `208e34ef`.

## Two things the brief said that the tree does not agree with

Both checked against `208e34ef`, both worth knowing before anybody re-reads
the same files:

1. **`containers::run_block_events` does not return a `Drained` tally.** It
   returns `()` and still has its four bare `continue`s at
   `containers.rs:397-411`. The tally is on `origin/block-events`, which is
   unmerged. Nothing on main counts a vanished event.
2. **No Rust test anywhere points at `minecraft:piston`.** `grep -rn
   'minecraft:piston'` over the tree matches `placedby.rs::a_beds_head_offset_follows_its_facing_and_nothing_else` (`:509`) (a fixture
   about something else) and the two scenario TOMLs, and nothing in
   `containers.rs`. What exists is a *doc comment* at `containers.rs:343`
   predicting that a piston would answer `trigger_event` yes and
   `has_openers_counter` no. That prediction is right, but it is prose, and
   "make the existing discard test go green" had no test to go green. One was
   written; see below.

## What the jar says

Read clean-room from `server-26.2.jar`. The whole design turns on one thing,
and it is written where the code is (`crates/rustmc-server/src/piston.rs`
module header) rather than repeated here — but the headline is:

**`setBlock`'s flags split the piston's writes into the ones a client is told
about and the ones it is not, and every `moving_piston` placement is in the
second group.** `moveBlocks` places them at flag **324** and the retraction at
**276**; neither carries `Block.UPDATE_CLIENTS` (bit 2). The
`PistonMovingBlockEntity` is created with `Level.setBlockEntity`, which sends
nothing. What *is* broadcast is the piston's own `extended=true` (flag 67),
destroyed blocks (18), the vacated source positions (82) and `finalTick`'s
arrival (3).

So **the client predicts the entire two-tick animation from the `block_event`
packet alone.** This is a packet-and-ordering port, not a block-entity one.
An implementation that modelled `PistonMovingBlockEntity` faithfully and
broadcast it would put packets on the wire vanilla never sends *and still not
move the piston on screen*.

The other three findings worth not re-deriving:

* **`PistonBaseBlock.triggerEvent` re-reads the world.** It calls
  `getNeighborSignal` and returns `false` in three cases — re-powered before a
  contract drains, unpowered before an extend drains, and `moveBlocks`
  failing. The chest's `triggerEvent` is an unconditional `return true`. That
  difference is why `containers::trigger_event(block, b0, b1)` **cannot** be
  widened to cover pistons: a constant `true` sends the packet in the two
  cases vanilla suppresses.
* **`getNeighborSignal` is three passes**, and the two easy ones to lose are
  `hasSignal(pos, DOWN)` at the piston's *own* position, and the quasi-
  connectivity sweep around `pos.above()` that skips `DOWN`.
* **`isPushable`'s `allowDestroy` differs by call site** — `false` at the head
  of a line, `true` inside `addBlockLine`'s forward walk. A `DESTROY` block in
  front of the piston refuses the push; the same block further along is broken.
  And `addBlockLine`'s four early returns are `true` (line finished, push is
  fine), not refusals — reading them as refusals inverts every ordinary piston.

## The subset built, and its boundary

**Built** — the event layer, end to end:

* `checkIfExtend` → what to queue, with `b1 = facing.get3DDataValue()`;
* `triggerEvent` → the three refusals and the write plan, with vanilla's flags;
* `getNeighborSignal`, `isPushable`, and `PistonStructureResolver.resolve`
  restricted to its **non-sticky** arm;
* the dispatch: `power::execute_update` gains the piston arm (queue only,
  never act), and `containers::run_block_events` routes pistons to
  `piston::trigger_event` instead of the constant predicate;
* `ci/piston-golden.sh` + `PistonGolden.java` → `data/vanilla/piston_push.json`,
  the push-reaction / unbreakable / block-entity table `isPushable` needs and
  no other table carried.

**Not built, deliberately:**

* `PistonMovingBlockEntity` — its tick, `progress`/`progressO`, `finalTick`.
  **This is the biggest gap and it has a visible consequence:** nothing places
  the pushed block at its destination, because that is `finalTick`'s `setBlock
  (…, 3)` two ticks later. Scenario 390 measures exactly that arrival.
* entity movement — `moveCollidedEntities`, `moveStuckEntities`, `PistonMath`;
* the sticky pull-back on retraction;
* `addBranchingBlocks` / `reorderListAtCollision` — slime and honey only.
  `resolve` returns `Unresolved::StickyUnported` for them, kept **distinct**
  from `Unresolved::Refused` so an unported branch cannot read as a vanilla
  refusal;
* `MovingPistonBlock`'s shapes, drops and clone item;
* the `TRIGGER_DROP` arm of `checkIfExtend`. It needs a
  `PistonMovingBlockEntity` to interrogate and this server places none, so
  `b0` is always `TRIGGER_CONTRACT` on a retraction. **A divergence, not a
  simplification**, and named in the code.

## Nulls — do not re-run these

* **Widening `containers::trigger_event`'s `matches!` to include
  `minecraft:piston`.** It makes the discard symptom vanish and is wrong; see
  above. There is now a test asserting the arm's *absence*
  (`the_constant_predicate_does_not_answer_for_a_piston`).
* **Defaulting a missing push-reaction row to `NORMAL`.** It makes every
  unknown block pushable, which is correct for most of the registry and
  produces a piston that works in every scenario written first. `LevelPiston`
  answers `PushReaction::Block` for an unknown row instead, so an absent table
  makes every piston refuse rather than over-push.
* **Editing 380/390's `observe` or steps.** Both are already registered at
  tier 2 in `ci/checks/gameplay-falsifiable.toml` and editing either would go
  STALE and cost a re-sweep. `semantic_digest` excludes `about`, so the `about`
  correction landed on 390 needs none — `--check` verified green at 108/108,
  0 unswept.

## The single next item

**Port `PistonMovingBlockEntity.finalTick` and the two-tick timer**, and close
390. Everything else in the extend path is in place: the structure resolves,
the `moving_piston` markers are written server-side with the right flags, and
the `block_event` reaches the client. What is missing is the write that makes
the pushed block *arrive* — `finalTick`'s `setBlock(worldPosition, movedState,
3)`, which is broadcast, at `TICKS_TO_EXTEND = 2`. The result that closes it:
390 reporting `match` rather than `diverge` on `block_update` at the
destination position.
