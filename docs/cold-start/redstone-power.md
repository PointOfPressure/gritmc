# Cold start — branch `redstone-power`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `redstone-power` off `d3b74cc` (was `a573f94`, then `380397f`), 2026-08-06

**Rebased twice while gating, and neither move touches what was measured.**
`main` went `a573f94 -> 380397f` when `chain-signs-eviction-swing` landed, and
`380397f -> d3b74cc` when the trial-chamber worldgen work did. Checked with
`git diff --stat`, not assumed:

* `380397f` adds `ci/selfcheck-aggregate.py` and `docs/WORKLIST.md` only;
* `d3b74cc` adds six commits of **worldgen** — the trial chamber jigsaw,
  `pool_alias`, `structure/place`, `worldgen-assembly`, `worldgen-dump` and
  docs.

Neither touches `crates/rustmc-server/` or `ci/checks/`. The gameplay gate runs
a **superflat world with structures off** — `ci/t2-gameplay.sh` says why: "a
village under the check's feet is a difference between two worlds rather than
between two servers" — so no amount of structure work can reach the corpus
number below. The control is still a control for `main`, and T0 is green on the
rebased tree (exit 0, 0 skips).

**`Level.setBlock` has two branches and this tree only had one.** The shape
half — ask a neighbour what it becomes — has been ported since the stair work.
The redstone half, `updateNeighborsAt`, was a flag carried all the way down to
an empty branch, which is why a torch beside dust changed the dust's *shape*
and never its *power*, and why `100` and `370` sat at `diverge` through every
link of the shape family. Remote dirs `~/rustmc-redstone-power` (subject),
`~/rustmc-redstone-base` (control, a clean `a573f94`) and
`~/rustmc-redstone-gold` (goldengen + T0) on the **Threadripper**; logs on the
laptop under `~/rustmc-logs/redstone-*.log`. Not merged — the orchestrator
gates.

#### The brief was wrong about one thing and the box about another

1. **`main` is at 1,369 commits, not the 1,357 the brief quotes.** Two other
   links landed while this one was briefed. Nothing depended on it; recorded
   because the next brief will quote it.
2. **The Threadripper was at 0 bytes free**, and had been for long enough to
   fail a `cargo` build with `No space left on device` — three concurrent gates
   all writing. That is not this branch's problem and it was every agent's.
   **21 GB reclaimed** by deleting `target/` from 21 trees whose build cache
   had not been touched since 2026-08-04 or earlier, none with a live process.
   A `target/` holds no source, so the worst case is somebody pays for a
   rebuild. Left alone: every tree with a running `cargo`, `t2-gameplay` or
   bench, including `~/rustmc-gate`, which was mid-gate.

#### The trap in `AGENT-BRIEF`'s own setup loop

The link loop skips `gameplay*`, and the stated reason is that
`oracle-work/gameplay/` is live per-run output two trees would overwrite. True
— but the glob also skips **`oracle-work/gameplay-selfcheck/`**, which is 26
saved recordings, read-only, and exactly the expensive shared reference the
loop exists to link in. So `ci/selfcheck-recompare.sh` finds nothing in a
freshly set-up tree and exits 2. Pass it explicitly:

```bash
RECORDINGS=$HOME/rustmc/oracle-work/gameplay-selfcheck ./ci/selfcheck-recompare.sh
```

#### What landed

* **`crate::power`** — `SignalGetter` entire (`getSignal`, `getDirectSignal`,
  `getDirectSignalTo`, `getBestNeighborSignal`, `hasSignal`,
  `hasNeighborSignal`); `CollectingNeighborUpdater` (`addAndRun`, `runUpdates`,
  `MultiNeighborUpdate`); `DefaultRedstoneWireEvaluator`
  (`updatePowerStrength`, `calculateTargetStrength`) and
  `RedstoneWireEvaluator` (`getWireSignal`, `getIncomingWireSignal`); plus
  `RedStoneWireBlock`'s `getBlockSignal`, `checkCornerChangeAt`, `onPlace`,
  `updateNeighborsOfNeighboringWires`, `neighborChanged`, `getSignal` and
  `canSurvive`, and `RedstoneTorchBlock`'s `onPlace`, `notifyNeighbors`,
  `hasNeighborSignal` and `neighborChanged` with the wall override;
* **`data/vanilla/signal.json`**, a thirteenth generated table, from
  `tools/goldengen/vanilla/SignalGolden.java` via `ci/signal-golden.sh`;
* **`Level.setBlock` offsets 140-156**, calling `updateNeighborsAt` with the
  **old** state's block;
* **`UPDATE_SKIP_ON_PLACE`, flag 512**, which `LevelChunk.setBlockState` guards
  `onPlace` on and which nothing sets — the same no-op-that-can-be-measured as
  flag 128, now covering both `onPlace` arms;
* **`RedstoneLampBlock`**, added after the rest because its two directions are
  not symmetric and only one of them needs a tick:

  ```java
  if (lit != level.hasNeighborSignal(pos)) {
      if (lit) level.scheduleTick(pos, this, 4);          // OFF: four ticks late
      else     level.setBlock(pos, state.cycle(LIT), 2);  // ON: immediate
  }
  ```

  So lighting a lamp needs nothing this server does not already have, and
  `360-a-lever-lights-a-lamp`'s own `about` field says it tests "the half of a
  block update this server carries a flag for and does not yet fill in" — which
  is this link. It is also the **first thing to exercise `has_neighbor_signal`
  against real vanilla**: the torch's use of it ends in a tick nobody
  schedules, so until now that function had unit tests and no measurement.
  `getStateForPlacement` is ported and tested but deliberately **not wired into
  `crate::placement`** — it needs a `SignalTable` that `computed_with_neighbours`
  is not handed, and that plumbing is in a file two other links are also in. It
  costs one case nothing in the corpus builds.

#### Why a measured table and not twenty hand ports

`BlockBehaviour` defaults **all three** of `isSignalSource`, `ownSignal` and
`getDirectSignal` to the falsy answer. So a block nobody ported and a block
that genuinely emits nothing are the same value and different facts — the
shape that made `update_shape.json`'s 1,296 all-`DOWN` wire rows read as a
complete table. Twenty classes override one of the three; hand-porting them is
twenty chances to write a silent zero.

`signal.json` is 32,366 states: **1,372 carried, 1,250 refused, 29,744
measured silent**, and the three must sum to the total or the table refuses to
load. The 1,250 are 1,215 wire states — `getConnectionState` reads thirteen
positions — and 35 that keep their output in a block entity (`comparator`,
`jukebox`, `trapped_chest`, `test_block`), which are named as owing a port and
say so on stderr the first time each is asked.

**A refusal is per state and not per direction, deliberately.** The probe stops
at the first throw and marks the whole state unanswerable rather than keeping
the directions that answered first. A port holding five sixths of a rule has
no way to notice.

#### Three things the bytecode said that a summary would have got wrong

* **`NeighborUpdater.UPDATE_ORDER` is `WEST, EAST, DOWN, UP, NORTH, SOUTH`** and
  `BlockBehaviour.UPDATE_SHAPE_ORDER` is `WEST, EAST, NORTH, SOUTH, DOWN, UP`.
  Two branches of one `if` in `setBlock`, two orders, neither of them
  `Direction.values()`. The vertical pair sits third in one and fifth in the
  other;
* **`Level.setBlock` names the OLD block** at offset 150 (`aload 7`), with the
  new one in scope at `aload 6` and used on the very next line for
  `updateNeighbourForOutputSignal`;
* **`getIncomingWireSignal`'s ceiling test reads `pos.above()`** — the wire's
  own; offset 61 is `aload_2` — and not the neighbour's. That is the same
  `canGoUp` gate the connection rule applies, so power and shape agree about
  when dust may climb. Reading it as `npos.above()` lets dust draw power
  through a ceiling it cannot connect through.

#### The one real bug, and what caught it

`RedstoneTorchBlock.neighborChanged` was written `!=` and is `==`. Offset 20 is
`if_icmpne 44` and 44 is the `return`, so the two branch **past** the schedule
when they differ: the tick is asked for when `LIT` and `hasNeighborSignal`
**agree**. It takes a moment because the two mean opposite things — a torch is
on when the block holding it is not powered — so the steady state is
`lit != hasNeighborSignal` and schedules nothing.

Inverted, every torch in the corpus asked for a tick it did not need. It was
found by the "this server has no torch tick" diagnostic firing inside a test
about something else, which is the argument for keeping a diagnostic that names
an unported mechanism instead of letting it return zero quietly.

#### `shouldSignal` is a field on the block and it is load-bearing

`RedStoneWireBlock` carries `private boolean shouldSignal`, `true` from the
constructor and `false` for exactly the length of `getBlockSignal`. While it is
false the wire's `getSignal`, `getDirectSignal` and `isSignalSource` all answer
the falsy value — and because it is a field on the **singleton**, that is true
of every wire in the world, not just the one being evaluated.

That is what stops a dust counting its neighbour at full strength as a *source*
when the decay rule is about to count it at strength minus one. Wire-to-wire is
`getIncomingWireSignal`, which reads the `power` property directly and
subtracts one. Drop the flag and a line of dust never decays.

#### Measured — T0, the unit gates and the 25/25 lock

**T0 GREEN** on the final tree, exit 0, `skipping: no vanilla` count **0**,
705 lib tests, `~/rustmc-redstone-gold`, 2026-08-06.

**33 tests in `crate::power`**, every one aimed at a branch and most with a
degenerate partner that passes under the plausible bug. Two of them are the two
checks' own geometry at the level of the mechanism: a redstone block beside
three dust reads **15, 14, 13**, and a torch placed beside two dust raises them
to **15, 14**.

Two failed first and both were the test being wrong rather than the code:

* **a wire's stored sides do not decide its signal.** `getSignal` calls
  `getConnectionState`, which starts from an all-`none` base and re-derives all
  four from the world — so in an empty world every powered dust is a **cross**
  and signals in all four horizontal directions whatever its stored state says.
  It takes three real blocks to make an asymmetric shape;
* **`onPlace` does not tell the block next door.** It tells the neighbours'
  *neighbours*. The block next door is told by `setBlock`'s own
  `updateNeighborsAt`, and a test calling `torch_on_place` alone watches the
  dust beside the torch stay at 0.

`RECORDINGS=$HOME/rustmc/oracle-work/gameplay-selfcheck ./ci/selfcheck-recompare.sh`,
26 recordings, 25 pairwise comparisons. **61 checks compared and 25 dropped**,
and the five that matter were confirmed present in the reference recording's
own scenario list rather than inferred from not appearing in a failure table:

```text
  100  redstone-torch-powers-wire                 declared 25/25   strict 22/25
  360  a-lever-lights-a-lamp                      declared 25/25   strict 25/25
  370  wire-power-falls-off-with-distance         declared 25/25   strict 22/25
  450  redstone-wire-climbs-the-block-beside-it   declared 25/25   strict 25/25
  290  lever-flips-when-right-clicked             declared 25/25   strict 25/25
  overall: declared 56/61 reproduced, strict 34/61
```

`290` is in the table because flipping a lever now runs a neighbour update
where before it ran none, so it is a check this link could break rather than
one it aims at.

**In fact every check in the redstone family is 25/25 declared**, which is the
stronger and simpler claim: the declared-unstable set has exactly five members
— `a-dropped-item-goes-into-your-inventory`,
`dropping-an-item-throws-it-on-the-floor`, `spawn-egg-makes-a-mob`,
`swapping-to-the-offhand-shows-on-both-channels`, `water-spreads` — and none of
them is redstone. `050`, `330`, `335`, `400`, `410`, `420` and the observer
check were each confirmed present in the reference recording rather than
inferred from absence. So any movement this link produces in that family is a
measurement, not a coin landing the same way twice.

Strict is a different matter and is quoted beside declared, never alone:
`torch-falls-with-its-support` reproduces strict **0/25**, `400` 20/25, `100`
and `370` 22/25 each.

Nothing here changes a check's projection, so these are rates that already
held. They are quoted because a `match` from a check vanilla cannot reproduce
is not evidence.

#### Why `100` and `370` could not have moved before, in mechanism terms

The previous link closed `450` and left `100` and `370` at `diverge`, and
recorded that pair not moving as the *closing condition* rather than a
shortfall. That was right, and this is why.

`Level.setBlock` has two independent branches after the write, and a wire's
state has two independent halves that map onto them exactly:

```text
  updateNeighbourShapes   (UPDATE_KNOWN_SHAPE clear)  -> updateShape
      decides  north/east/south/west : RedstoneSide     <- check 450
  updateNeighborsAt       (UPDATE_NEIGHBORS, flag 1)   -> neighborChanged
      decides  power : 0..15                            <- checks 100, 370
```

`450` is dust climbing the side of a block — a **connection shape**. It closed
when the first branch was completed, because that branch is what computes the
four `RedstoneSide` properties.

`100` and `370` are signal strength. They could not have moved on that link
however exact it was, because **nothing in the first branch ever writes
`power`**. `getConnectionState` takes the old state's `POWER` and carries it
across untouched — `crate::wire`'s own doc says so: "this function never
computes power, and porting it as if it did would put a redstone signal in a
placement rule". A wire's power is written in exactly one place in the game,
`DefaultRedstoneWireEvaluator.updatePowerStrength`, and the only way to reach
it is `neighborChanged`, and the only way to reach *that* is the branch this
link adds.

So the pair holding still was evidence: it said the shape port had not leaked
into power. Movement there would have been the failure mode, not the success
one.

What this link adds is the second branch and the arithmetic behind it:

```text
  power := calculateTargetStrength(level, pos)
         = getBlockSignal == 15 ? 15
         : max(getBlockSignal, getIncomingWireSignal)

  getBlockSignal       = getBestNeighborSignal(pos) with EVERY wire in the
                         world silenced by the `shouldSignal` field
  getIncomingWireSignal= max over the four horizontals of a neighbouring
                         wire's own `power`, minus one, clamped at zero
```

The two terms are why a dust beside a redstone block reads 15 and the next one
along reads 14: the block is a *source* and is seen by the first term, and the
dust beside it is a *wire* and is invisible to the first term and seen only by
the second, which costs one.

#### The corpus — the control, the subject, and the prediction

**The control reproduces `main` exactly, and that convicts the first
measurement rather than the tree.**

```text
  ~/rustmc-redstone-base, a573f94, clean box, 2026-08-06 11:43
  CORPUS  declared  53/86 match   (27 diverge, 6 absent, 0 vacuous, 0 inert, 0 error)
  CORPUS  strict    30/86 match   (52 diverge, 3 absent, 1 vacuous, 0 inert, 0 error)
  GAMEPLAY GREEN    no BETTER, no REGRESSED
  T2-GAMEPLAY GREEN                                              exit 0
```

**A first baseline run of the same commit came back 48/86 declared with five
REGRESSED.** That run straddled the disk filling to 0 bytes with two other
gates running. Re-taken on a clean box it is 53/86 with nothing regressed, so
the five were **box-induced and not code-induced** — Rule 7 exactly: when a
measurement convicts, suspect the measurement. The five were:

```text
  a-block-in-hand-does-not-stop-a-lever-flipping   match -> diverge
  typing-on-a-sign-you-placed-sets-its-text        match -> absent
  a-second-edit-of-the-same-sign-is-ignored        match -> absent
  a-sign-update-for-the-back-face-is-carried       match -> absent
  a-second-swing-inside-the-animation-is-swallowed match -> diverge
```

Four of those five are now *proven* to reproduce 25/25 vanilla against vanilla
(`380397f` measured them), which is what makes "the box, not the checks" the
only reading left: a check that reproduces itself in 25 of 25 comparisons did
not flap on its own.

**Strict is 30/86 here against the 31-32/86 the previous link quoted**, which
is the documented behaviour of the strict denominator rather than news, and is
why strict is quoted beside declared and never alone.

**The prediction, written before the run, and the outcome beside it.**

```text
  predicted   three BETTERs, 53 -> 56 declared, no REGRESSED, 450 and 290 hold
  measured    three BETTERs, 53 -> 56 declared, no REGRESSED, 450 and 290 held
```

`~/rustmc-redstone-power`, whole corpus, fresh vanilla world, 13 tables current
and stamped for that tree, ports 26760/26761, 2026-08-06. Log mirrored to
`~/rustmc-logs/redstone-subject-gate.log`:

```text
  CORPUS  declared  56/86 match   (24 diverge, 6 absent, 0 vacuous, 0 inert, 0 error)
  CORPUS  strict    34/86 match   (48 diverge, 3 absent, 1 vacuous, 0 inert, 0 error)
  BETTER  redstone-torch-powers-wire:         was diverge, is now match
  BETTER  a-lever-lights-a-lamp:              was diverge, is now match
  BETTER  wire-power-falls-off-with-distance: was diverge, is now match
  GAMEPLAY GREEN
  T2-GAMEPLAY GREEN
```

Against the control taken on the same box, same corpus, same baseline:
**53 -> 56 declared and 30 -> 34 strict**, and the delta is exactly the three
checks this link exists to close.

**No REGRESSED is the other half and it was the thing at risk.** Flipping a
lever, pressing a button, breaking a torch's support and placing a block beside
dust all now run a neighbour update where before they ran none. `290`, `330`,
`335`, `050`, `150`, `190`, `220`, `450` and `553` all held at `match`.

Still `diverge`, all for named reasons rather than unknown ones: `380`/`390`
(piston, no `neighborChanged`), `400` (the torch tick, the next item), `410`
(`DiodeBlock`, not ported), `420` (comparator — `signal.json` refuses all 35 of
its states because the output lives in a block entity) and the observer.

`ci/checks/gameplay-baseline.toml` is ratcheted to 56/86 **from this
full-corpus run**, never from a subset — the adjudicator's subset false-red is
`380397f`'s finding and this run predates nothing it applies to.

#### Nulls and traps — do not pay for these twice

* **Nineteen of the 25 checks the selfcheck recordings predate still have no
  reproducibility figure**, and the set is worth reading: every container-click
  check, both crafting checks, the two attack checks, the two sneak checks and
  `a-block-in-hand-does-not-stop-a-lever-flipping`. A `match` on any of them is
  a match under a projection nobody has shown to be deterministic. That is not
  a reason to distrust them; it is a reason not to quote them as stable.

  **It was 25 when this block was first written and `380397f` measured six of
  them** — `570`, `571`, `572`, `580`, `581`, `582`, all 25/25 on both
  denominators — using the `SELFCHECK_CHECKS` subset path that commit adds.
  Corrected here rather than left, because a caveat that lags the tree reads as
  authoritative.

  **And that correction changes what a bad measurement of mine meant.** Four of
  the five checks that moved in the disk-starved baseline run below — `570`,
  `571`, `572`, `581` — are now *proven* to reproduce 25/25 vanilla against
  vanilla. So their movement was not check flakiness. It was this server or the
  box, and the clean control run is what tells the two apart.
* **`selfcheck-recompare.sh` needs `RECORDINGS=`** in any tree set up by the
  `AGENT-BRIEF` loop. See above.
* **A `target/` directory holds no source.** When the box is full, stale ones
  are tens of gigabytes and the only cost is somebody's rebuild. Check for a
  live process first.
* **`Around` cannot be returned from a function.** It holds `&dyn Fn` and the
  four closures are temporaries that cannot outlive the statement that makes
  them. Hand it to a callback; every other construction site in the crate
  builds it inline as a local for the same reason.
* **A lib test's working directory is the package directory**, so the bare
  `data/vanilla/...` the server itself opens resolves to nothing. There is a
  `repo()` helper in `crates/rustmc-server/tests/*` and now one in `power.rs`.
* **`java.util.HashSet`'s iteration order is reproduced** in
  `power::hash_set_order`, for the reason `shortset.rs` reproduces fastutil's:
  `updatePowerStrength` tells seven positions in that order and a wire told
  before its neighbour settles gets a different answer. Seven entries never
  resize a 16-slot table, so it is three lines. The expected orders are
  **measured against OpenJDK 25 on the box**, not derived from the port's own
  algorithm — a test that recomputed the expectation would agree with any bug
  in it.
* **This port allocates where vanilla does not, and the fan-out is large
  enough that it is worth writing down before somebody profiles it and thinks
  it is a bug.** `Blocks::state_at` returns an owned `String`; vanilla's
  `getBlockState` returns an interned reference. `crate::power` calls it in
  fourteen places and every one is on the hot path — `execute_update` reads the
  state of each of six neighbours per `updateNeighborsAt`.

  The fan-out is vanilla's own and it is not small. Placing one dust runs
  `updateNeighborsOfNeighboringWires`, which is up to eight
  `checkCornerChangeAt`, each of which is seven `updateNeighborsAt`, each of
  which is six notifications: **336 state reads for one placement**, plus
  seven more per wire whose power actually moved. In vanilla those are pointer
  loads. Here they are 336 heap allocations.

  Nothing in the corpus is slow enough to notice and no measurement here says
  it matters — it is named because "parity mode must itself be fast" is a
  standing rule and this is the first mechanism in the server with a
  three-hundred-fold amplification on a single block placement. The fix, when
  it is wanted, is a `block_at`-style read that compares the block name without
  materialising the state, which `crate::world` already has internally.

* **28 blocks override `neighborChanged` and two are ported.**
  `power::execute_update` names the other 26. Not reaching them is not a
  regression — the whole path was dead before — but it is now a *reachable*
  gap, and `RedstoneLampBlock` (check `360`) and `DiodeBlock` (check `410`) are
  the two the corpus can already see.

#### The single next item

**The torch tick, and the lamp's with it — one arm each in a dispatch that
already has two.** They close
`400-a-redstone-torch-turns-off-when-its-block-is-powered` and the *off* half
of `360`, and every piece already exists:

* the two conditions are ported and unit-tested —
  `power::torch_should_schedule` (`lit == hasNeighborSignal`, delay 2) and the
  lamp's `lit != hasNeighborSignal` (delay 4). Both currently call a
  `*_wants_a_tick` diagnostic that prints instead of scheduling, so **grep for
  those two functions and you have both call sites**;
* `crate::ticks::ScheduledTicks::shared().schedule(pos, block, now, delay,
  priority)` is the call;
* `crate::ticks` already dispatches a drained tick to `ButtonBlock` and
  `CoralBlock` by block class, and a torch arm is the same shape. Read the
  button arm first — it is the one that shows the flags matter (`3`, not the
  coral's `2`).

The bodies, from the jar 2026-08-06:

```java
// RedstoneTorchBlock.tick, offsets 76-190
if (lit &&  hasNeighborSignal) { setBlock(pos, state.setValue(LIT,false), 3);
                                 if (isToggledTooFrequently(level,pos,true)) {
                                     levelEvent(1502,pos,0);
                                     scheduleTick(pos, getBlockState(pos).getBlock(), 160); } }
if (!lit && !hasNeighborSignal && !isToggledTooFrequently(level,pos,false))
                                 setBlock(pos, state.setValue(LIT,true), 3);
// RedstoneLampBlock.tick, offsets 0-41
if (lit && !hasNeighborSignal) setBlock(pos, state.cycle(LIT), 2);
```

`isToggledTooFrequently` and the 160-tick burnout are a separate, later
question — the torch closes `400` without them, because `400` toggles once.
**Note the lamp's tick re-checks `hasNeighborSignal` rather than trusting the
schedule**: four ticks is long enough for the lever to have been flipped back,
and a tick that wrote unconditionally would put out a lamp that is powered
again.

It closes when `400` moves, `360` still matches, and neither block prints that
it wanted a tick nobody gave it.

**The shape family after that is already derived and is slabs and snow**, which
are one mechanism seen twice. `SlabBlock.canBeReplaced` offsets 0-150 and
`getStateForPlacement` offsets 0-156, and `SnowLayerBlock`'s pair, both read
from the jar 2026-08-06:

```java
// SlabBlock.canBeReplaced
if (type != DOUBLE && held.is(asItem())) {
    if (!ctx.replacingClickedOnBlock()) return true;
    boolean upper = ctx.getClickLocation().y - ctx.getClickedPos().getY() > 0.5;
    Direction face = ctx.getClickedFace();
    return type == BOTTOM ? face == UP   || (upper  && face.getAxis().isHorizontal())
                          : face == DOWN || (!upper && face.getAxis().isHorizontal());
}
return false;
// SlabBlock.getStateForPlacement
if (level.getBlockState(clicked).is(this))
    return that.setValue(TYPE, DOUBLE).setValue(WATERLOGGED, false);
// else BOTTOM, or TOP when face == DOWN or (face != UP and cursor y > 0.5)
// SnowLayerBlock.canBeReplaced
if (held.is(asItem()) && layers < 8)
    return ctx.replacingClickedOnBlock() ? ctx.getClickedFace() == UP : true;
return layers == 1;
```

It closes `slab-stacks-into-a-double-slab` and `snow-layers-stack`, and **the
cost is not the rules — it is that both need
`BlockPlaceContext.replacingClickedOnBlock()` and the item in hand**, neither
of which `crate::placement::Click` carries today. That plumbing runs through
`useblock.rs` and `placement.rs`, which the sign and container links are also
in; check who is holding them before starting.

*(Merge note, rebase driver, 2026-08-06: this cold-start and the one below it
were rewritten by two branches sharing this block and reconciled here rather
than by taking one side. Author dates overlap — `redstone-power`'s cold-start
commits span 11:16:32-12:49:35, `tick-randomtick-selection`'s single cold-start
commit lands at 11:57:28, inside that span — so the two were written
concurrently and there is no clean total order. Kept in full, ordered by each
block's own opening commit.)*
