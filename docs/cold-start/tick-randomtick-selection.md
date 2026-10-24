# Cold start — branch `tick-randomtick-selection`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `tick-randomtick-selection` off `380397f`, 2026-08-06

**The random-tick selection mechanism, derived and gated against vanilla's
own draws — and three corrections to what this project believed about its own
tick layer.** Remote dir `~/rustmc-ticking-0806` on the **Threadripper**;
logs on the laptop under `~/rustmc-logs/ticking-*.log`. Not merged — the
orchestrator gates.

#### What the tick layer actually did before this branch, because the brief was wrong about it

The brief said `tick.rs` "is a clock only — no entities, no mob AI, no random
ticks", and that "earlier work landed entity tracking and motion". **Read from
the tree, one of those is half right and the other is wrong.**

`tick.rs` *is* only a clock. The **game loop is not in it** — it is the
closure `crate::status::serve` hands to `GameClock::spawn`, at
`status.rs:350`, and it has run seven things per tick for some time:

```text
  1  scheduled block ticks     crate::ticks, vanilla's DRAIN_ORDER and
                               UNIQUE_TICK_HASH. Three behaviours reach it:
                               CoralBlock.tick, ButtonBlock.tick, and the
                               container recheck (ChestBlock.tick)
  2  the block broadcast       ChunkHolder.broadcastChanges, coalesced
  3  updateSwingTime           LivingEntity's, every entity, every tick
  4  ChunkMap.tick() entity    crate::tracker — membership on a section
     half                      change, ServerEntity.sendChanges every tick
  5  ServerLevel.runBlockEvents
  6  the acknowledgement flush ServerGamePacketListenerImpl.tick
  7  a tile sweep every 20     this server's own memory management; vanilla
                               has no counterpart
```

So "a clock only" understates it by six items, and a bench refusal resting on
that phrasing rests on a false premise — though its **conclusion** is still
right, for a different reason, below.

**"Entity motion" is the wrong half.** Entity *tracking* landed —
`crate::tracker` is `ChunkMap.TrackedEntity` and `ServerEntity.sendChanges`,
and it is real. Entity **motion did not**: grepping every write to an
entity's position in the server crate returns exactly one non-test site,
`world_send.rs:407-409`, which copies a player's position out of the movement
packet that player's own client sent. There is no gravity, no drag, no
collision, no `Entity.move` anywhere in the tree. `crate::dropitem`'s header
already says so in capitals — "THE ITEM DOES NOT MOVE AFTER IT IS THROWN, AND
THAT IS DELIBERATE" — and argues why faking it is a one-way door.

**And a third thing, which is the one worth knowing:** `rustmc-entity`
contains a careful bytecode derivation of `EntityTickList`'s iteration order
and of the `tickNonPassenger`/`tickPassenger` loop body — `checkDespawn`
before the range gate, `setOldPosAndRot` before `tick`, `rideTick` and not
`tick` for a passenger, all of it. **Nothing calls any of it.**
`grep -rn rustmc_entity` across the workspace returns four lines and all four
import `EntityId`. The server keeps its own entity store in
`crate::entities`, a `BTreeMap` with no tick loop at all. Derived is not
wired, and from the outside the two are indistinguishable.

#### DEFECT CLASS #23 — the caveat that inverted, and it was in a module header

`tick.rs`'s own header said:

> No block updates, no entities, no scheduled ticks, no random ticks — those
> need a world that can change, and this server's world is immutable.

**Every clause of that has been false since `crate::edits` gave the world an
overlay.** Scheduled ticks run, entities are tracked and swing-timed, block
updates are broadcast, and the world changes on every placement.

This is not the same shape as the stale caveat lists already recorded
(`worldgen-dump`'s block, `generate.rs`'s comment). Those *lagged* — they
described an earlier true state. This one **inverted**: it is a header on the
file whose name is the subject, and it says the opposite of what the tree
does. An agent briefed to work on ticking opens `tick.rs` first, and this
paragraph is the first thing in it.

It is the sharpest available demonstration of the rule already in
AGENT-BRIEF — *a caveat list that lags the code reads as authoritative and is
worse than none* — because it is the first one measured to have **propagated
into a brief**: the "tick.rs is a clock only" premise this turn was briefed
with is a paraphrase of it. A wrong comment did not merely fail to help; it
became the next turn's specification. Rewritten to what the loop runs and,
more usefully, to what it does not.

#### The mechanism, derived — `javap -p -c -constants`, `oracle-work/vanilla-26.2`

```text
  ServerChunkCache.tickChunks 76-95    speed = gamerule RANDOM_TICK_SPEED, an
                                       INTEGER rule
  ...                        276-288   chunkMap.forEachBlockTickingChunk(
                                         c -> level.tickChunk(c, speed))
  ChunkMap.forEachBlockTickingChunk    one line; delegates to
                                       DistanceManager.forEachEntityTickingChunk
  DistanceManager.forEach...           walks simulationChunkTracker.chunks,
                                       admits on ChunkLevel.isEntityTicking
  ServerLevel.tickChunk 32-76          the ice-and-snow pass, FIRST
  ServerLevel.tickChunk 86-267         the block pass
  Level.getBlockRandomPos 0-55         the position function
```

**Four facts, and every one of them is a way to be wrong at the right rate:**

1. **The chunk set is the ENTITY-ticking set, and the method that selects it
   is named for the block-ticking one.** `forEachBlockTickingChunk`'s whole
   body forwards to `forEachEntityTickingChunk`, which admits on
   `ChunkLevel.isEntityTicking`. `ChunkLevel` carries **both** predicates and
   they are not the same:

   ```text
     ChunkLevel.isBlockTicking(l)   l <= 32    BLOCK_TICKING_LEVEL
     ChunkLevel.isEntityTicking(l)  l <= 31    ENTITY_TICKING_LEVEL
   ```

   So the random-ticking set is a **strict subset** of the block-ticking set,
   one chunk level tighter, and a port that reads the method name and reaches
   for `isBlockTicking` — which is right there in the same class — random
   ticks a whole ring of chunks vanilla leaves alone. At exactly the right
   rate everywhere inside it, so only a count at the boundary sees it. Both
   are simulation distance rather than view distance.
2. **`speed` picks per randomly-ticking SECTION per chunk per tick.** The
   `for (j = 0; j < speed; j++)` is *inside* the per-section loop. Read as
   "per chunk" it is wrong by the number of loaded sections — 24 on the
   overworld, so a rate 24x too low, produced by an implementation that looks
   exactly right.
3. **A quiet section is skipped BEFORE the draw.**
   `LevelChunkSection.isRandomlyTicking()` is
   `tickingBlockCount > 0 || tickingFluidCount > 0`. Skipping after the draw
   gives the right blocks and the wrong stream position for ever after.
4. **The ice-and-snow pass runs first and shares both generators.** It is
   `for (i = 0; i < speed; i++) if (random.nextInt(48) == 0)
   tickPrecipitation(getBlockRandomPos(minX, 0, minZ, 15))` — so it consumes
   `speed` draws from `level.random` per chunk per tick whether or not
   anything freezes, and every success advances the same `randValue` the
   block pass draws from. **Not ported**; carried as
   `Chunk::PRECIPITATION_ATTEMPTS_PER_TICK` so the debt has a name.

`random_tick_speed` is `registerInteger(..., 3, 0)` — **default 3, minimum
0** — read from `GameRules`' static initialiser at offsets 585-590, and the
id is confirmed present in the 59-entry `minecraft:game_rule` registry in
Mojang's own `registries.json`. Not from memory of an older version, per the
standing rule.

#### The position function, and the one line that needed a golden

```text
  Level.getBlockRandomPos(x, y, z, mask), offsets 0-55, entire
     0: randValue = randValue * 3 + 1013904223
    13: int r = randValue >> 2            // ishr — ARITHMETIC
    21: new BlockPos(x + (r       & 15),
                     y + (r >> 16 & mask),
                     z + (r >>  8 & 15))
```

**`y` takes bits 16.. and `z` takes bits 8.., and `y` is the only component
that reads `mask`.** That is the reverse of the ordering a transcription from
memory reaches for — this turn wrote it backwards first — and the swap is
invisible to **every property the function has**: the positions stay uniform,
stay inside the section, arrive at the same rate, and advance the generator
by the same amount. `randomtick::tests::every_pick_is_inside_its_own_section`
passes with the swap in place, and its doc comment says so rather than
leaving somebody to discover it.

So it is pinned against vanilla *running the method*.
`ci/random-tick-golden.sh` compiles `tools/goldengen/vanilla/
RandomTickGolden.java` against the pinned jar, allocates a `ServerLevel`
without its constructor — `getBlockRandomPos` touches exactly one field and
calls nothing, so nothing else is reachable and anything that were would
throw — sets `randValue` by reflection, and dumps what vanilla returned.
**48 cases x 24 draws**, plus the `randValue` each case ends on, because a
port can agree on every compared position and still advance by the wrong
amount, which shows on the draw after the last one compared.

**And one claim this branch made and then refuted with its own test.** The
sentence above used to continue "seeds straddle zero on purpose: `>> 2` is
`ishr`, so a `>>>` port agrees with vanilla on every non-negative value and on
none of the negatives". That is **false**, and the test written to defend it
went red on its first run, which is how it was found.

`ishr` versus `iushr` is **not observable through this function at all**. The
two shifted values differ only in bits 30-31; `getBlockRandomPos` reads bits
0-3, 8-11 and 16-19 and `r` is a local, so the disagreement is discarded every
time. Measured over 20,008 seeds: **zero disagreements**. A `>>>` port is
byte-identical, for every input, for ever.

`>>` is still what is written, because `ishr` is what the bytecode says and a
port should not silently disagree with its source — but it is defended by a
comment now instead of by a check that cannot fail. The negative seeds stay
for the other reason: the multiply wraps, and **that** moves `randValue`,
which is what `endValue` compares.

#### What was measured, and at which commit

```text
  T0, the pushed tip, ~/rustmc-ticking-0806b
    exit 0, 0 skips, 18 test binaries                            T0 GREEN

  T0, mid-branch, rebased onto d3b74cc
    exit 0, 0 skips, 18 test binaries                            T0 GREEN

  T0, before the rebase (base 380397f)
    exit 0, 0 skips, 18 test binaries                            T0 GREEN

  the golden
    1,152 vanilla-drawn positions across 48 cases, all reproduced, plus all
    48 endValue transitions — by the Rust, and independently by a python
    transcription of the bytecode reading. Two implementations written from
    the same nine instructions, agreeing with what vanilla itself returned.
    The committed file is byte-identical to a fresh regeneration in a
    separate JVM (four dumps in all, since the script cmp's two per run), so
    it is provably the script's output rather than something edited by hand

  block_props, regenerated with the two new columns
    32,366 states; 1,508 randomly-ticking; 16 randomly-ticking fluids
    the 16 are lava's own level=0..15 and nothing else

  corpus BASELINE, dir ~/rustmc-ticking-0806, at 380397f
    CORPUS  declared  53/86 match  (27 diverge, 6 absent, 0 vacuous, 0 inert, 0 error)
    CORPUS  strict    31/86 match  (51 diverge, 3 absent, 1 vacuous, 0 inert, 0 error)
    GAMEPLAY GREEN    T2-GAMEPLAY GREEN

  corpus BRANCH, same dir, branch content on 380397f
    CORPUS  declared  53/86 match  (27 diverge, 6 absent, 0 vacuous, 0 inert, 0 error)
    CORPUS  strict    32/86 match  (50 diverge, 3 absent, 1 vacuous, 0 inert, 0 error)
    GAMEPLAY GREEN    T2-GAMEPLAY GREEN
    no BETTER, no REGRESSED
```

**Declared is identical, 53/86 to 53/86, which is the result this branch
wanted.** Nothing here is reachable from a running server except the
`block_props.json` loader, so a declared line that moved would have meant the
table change had a side effect nobody predicted.

**Strict moved 31 -> 32 and that is the documented flap, not news.** The
previous link recorded exactly this shape — "53/86 both times, 31/86 against
32/86; one strict verdict flapped between two runs of the same gate on the
same tree" — and vanilla reproduces *itself* under strict on only 34 of 61.
Quote **53/86 declared**; quote strict as "31-32/86, and it moves". A branch
that claimed the +1 as its own would be claiming a coin flip.

`0 vacuous, 0 inert, 0 error` on the declared line, both arms.

**The baseline was re-measured rather than quoted**, and it reproduced the
previous link's line exactly — 53/86 declared, 31/86 strict — which is what
makes it usable as the before arm rather than as a number from a document.

**No check in `ci/checks/gameplay/` was added or modified by this branch, and
`tools/oracle-runner` was not touched**, so there is nothing for
`ci/selfcheck-recompare.sh` to lock: it would re-measure the pre-existing
corpus and attribute the result here. Said explicitly rather than left as a
silently skipped step, because "every check you touch at 25/25" reads as done
or forgotten and this is neither.

**The branch's own corpus arm is taken at the pre-rebase tip** (branch content
on 380397f), from `git archive` rather than an rsync — the falsification sweep
was mutating the working tree in place at the time, and an rsync would have
shipped whichever injected bug happened to be live. That makes the A/B clean
for *this* branch's changes, which is the question; the post-merge run belongs
to the gate.

The only thing here the corpus can see at all is the `block_props.json`
loader, which is now strict about two columns and will refuse a stale table
rather than read every state as non-random-ticking. Nothing else in this
branch is reachable from a running server.

#### The falsification sweep, and it found three holes in its own author's tests

Nine injected bugs, one at a time, the full test set each time, control green
on both sides. `--release` on the Threadripper in `~/rustmc-ticking-0806b`,
2026-08-06. The columns are the three suites: `randomtick` lib / the golden /
`block_props` lib.

```text
  injection                                  lib      golden   props
  CONTROL (before)                           ok 7     ok 1     ok 9
  A  y and z shifts swapped (16 <-> 8)       ok 7     FAILED   ok 9
  C  mask applied to z instead of y          FAILED 1 FAILED   ok 9
  D  add before multiply                     ok       FAILED   ok 9
  E  no `speed <= 0` guard                   ok       ok       ok 9
  F  quiet sections drawn anyway             FAILED 2 ok       ok 9
  G  `speed` picks per chunk, not per        FAILED 2 ok       ok 9
     randomly-ticking section
  H  section y not converted to block y      FAILED 1 ok       ok 9
  I  random tick column wired to false       ok       ok       FAILED 3
  J  x and z swapped (0 <-> 8)               ok 7     FAILED   ok 9
  CONTROL (after)                            ok 7     ok 1     ok 9
```

**A, J and D are the ones the golden alone catches**, and that is the result
this whole apparatus exists for: all three are wrong *positions* — the right
count, in the right sections, uniformly distributed, at the right rate — and
every structural test in the module stays green through them. Only comparing
against values vanilla itself produced separates them.

**And the sweep found three holes, all of them in tests written this turn.**
That is the part worth reading:

* **E is not a bug at all.** Deleting the `speed <= 0` early return changes
  nothing: `for _ in 0..speed` is already an empty range at zero and at every
  negative. The guard is kept because vanilla has the branch and because it
  skips the section walk — a *cost* difference at the speed this server
  ships — but `speed_zero_draws_nothing_and_moves_nothing` cannot tell it
  from its absence, and now says so;
* **H passed everything on the first sweep.** `every_pick_is_inside_its_own_
  section` asks only that a pick land somewhere in the chunk, and a
  24-section chunk is tall enough to swallow a section base left as a section
  index instead of a block coordinate.
  `picks_are_grouped_by_section_in_order_and_land_in_that_sections_band` was
  written for it — grouping, order and the 16-block band in one assertion —
  and H and F both fail it now;
* **F was reported as applied when it had not been, and that is a defect in
  the sweep harness itself.** `falsify.sh` verified the injection landed with
  `grep -qF "$to"`. **`grep -F` given a multi-line pattern treats each line as
  a separate alternative**, so a two-line tail of `continue;` / `}` matched
  somewhere else in the file and the check passed against a file nothing had
  been done to. A guard that cannot fail, inside the harness whose only job is
  proving that checks can — the same shape as defect class #20.

  It was caught by **expecting a red and getting a green**, which is the only
  reason it was caught at all: the pattern said `if !ticking` and the source
  said `if !*ticking`, three characters, and the sweep reported the injection
  as live. A and C failed the same way and at least announced it, because
  their replacement was a single line. Sweep 2 does exact literal replacement
  in python and asserts the anchor occurs exactly once.

**Nine bugs, and the honest count is eight** — E is a proven no-op rather than
a survivor. Every other injection moves at least one test, and no test moves
for an injection it was not written for except `picks_are_grouped_...`, which
is written to catch two.

#### The one thing about random ticks that cannot ever be compared, said plainly

`Level`'s constructor sets `randValue` from
`RandomSource.createThreadLocalInstance().nextInt()` — **unseeded**. Two
vanilla servers of this version, on the same world, at the same tick, choose
different random-tick positions, and always will.

So a check that compares random-tick *positions* against a live vanilla can
never pass, vanilla against itself included. The comparable things are the
transition (golden, above), the rate (counting tests), and a distribution.
This is written in `randomtick.rs`, in `random_tick_pos.rs` and here, because
it is exactly the shape of claim that gets re-attempted.

#### THE CORPUS CANNOT OBSERVE A RANDOM TICK, AND THAT IS WHY THIS IS NOT WIRED IN

`ci/t2-gameplay.sh:323` sends `gamerule random_tick_speed 0` to vanilla, and
has since 2026-08-03, deliberately — with random ticks running, grass under
the rigs turned to dirt at unpredictable moments and checks flapped. So **no
check in `ci/checks/gameplay/` can see this mechanism**, and turning it on to
make one would destabilise the other 85.

**And the quieting is one-sided.** All four `gamerule_or_die` calls go to
`$VAN_DIR` and nowhere else. They work today only because this server
implements none of the four — the harness is relying on an absence. The
moment `random_tick_speed` means something here, the two servers are on
different substrates, and a speed of 3 here against vanilla's forced 0 would
show up as a **new divergence** rather than as the parity gain it is.

`randomtick::DEFAULT_SPEED` is therefore **0**, against vanilla's 3, with
that argument written on the constant. The mechanism is verified against
vanilla and is **not** verified to fire; those are different claims and the
module says which one it has.

**This is the item that unblocks it**, and it is a harness item, not a
mechanism item: `ci/t2-gameplay.sh` should deliver the same four gamerules to
*both* servers, or assert that this server's defaults equal the four values
it forces on vanilla. Until then a random tick here can only ever cost the
corpus.

#### The opt-in set is PER STATE, and a table keyed by block name is wrong in both directions

`BlockStateBase.isRandomlyTicking` is a field cached at state construction
from `Block.isRandomlyTicking(state)`, whose default is the block's
`Properties.randomTicks` flag. **Twenty-five classes override it per state** —
counted tree-wide from the 26 classes declaring the method, less
`BlockBehaviour`'s own base declaration.

`block_props.json` grows two columns, both from vanilla's own methods:

```text
  randomlyTicking        state.isRandomlyTicking()
  fluidRandomlyTicking   state.getFluidState().isRandomlyTicking()
```

The second is not implied by the first. `tickChunk` asks them **separately on
the same drawn position** — block first, then fluid — and
`LiquidBlock.isRandomlyTicking(state)` forwards to the fluid while
`Fluid.isRandomlyTicking()` is false and only `LavaFluid` overrides it. So
lava is true on both columns and `randomTick` runs **twice** for one pick
that lands on it; water is false on both.

**The loader `ok_or_else`s on both columns where every other column in that
file is `unwrap_or(false)`, and the asymmetry is the point.** A missing
`solidRender` makes something behave oddly where a gate can see it. A missing
`randomlyTicking` makes **every state inert at once**, and the output of that
is silence — which is also the output of a correct server on a world where
nothing happens to be growing. No gate can separate those, so the stale table
has to refuse instead of defaulting.

Other trees cannot be silently caught by this: `prov_gen_digest` hashes
`BlockPropsGolden.java` along with its script, so a tree with the old table
exits 4 at the preflight rather than loading it.

#### Every tick-driven divergence in the corpus is ONE missing subsystem

This is the answer to "which divergences are tick-driven rather than
packet-driven, take those first", and it is not the list the question
suggests. Three checks are tick-driven, and all three bottom out in the same
place:

```text
  070 sand-falls-when-unsupported       diverge
  500 dropping-an-item-throws-it...     diverge
  540 a-player-that-leaves-the-ground   diverge
```

* **070** needs `FallingBlock.tick` -> `FallingBlockEntity.fall`, and
  `FallingBlockEntity.tick`'s own call list is `applyGravity`,
  `getDeltaMovement`, **`move(MoverType, Vec3)`**, `Level.clip(ClipContext)`
  and `onGround`;
* **500** is `ItemEntity.tick` — gravity, drag, `move`;
* **540**'s divergence was already diagnosed in its own check file:
  "`handleMovePlayer` never calls `setDeltaMovement` — it calls
  `player.move(MoverType.PLAYER, vec)`", and the cause is written there as
  "**no server-side player physics**".

So they are **one item, seen from three places: `Entity.move`.** Taking "the
tick-driven divergences first" does not mean three items of work; it means
one, and it is not small.

**Its prerequisite does not exist in this tree and that is the honest next
step.** `Entity.move` resolves a swept AABB against every intersecting
block's collision shape, and there is no per-state collision *geometry*
anywhere here. `block_props.json` carries `collisionFullFaces` — six
booleans — which cannot express a slab, a stair, a fence post or a soul
sand's 0.875. Note that `crate::shapes` is **not** this: it is `updateShape`,
the neighbour-state cascade, and the shared word has already misled once in
this file's history.

Deliberately not built here. A collision-shape table with no consumer
falsifies nothing, and a floor-finder that fakes the fall is the one-way door
`crate::dropitem` already refuses in writing.

#### Tick timing under load — REFUSED, and the previous refusal's reason was wrong

The bench pass declined to time the tick loop because "`tick.rs` is a clock
only". That premise is false — the loop runs seven things — and the
**conclusion is still right**, for two reasons that survive the correction:

1. **A vanilla tick's load is dominated by work this server does not do at
   all.** Entity ticking with `Entity.move`, mob AI, `NaturalSpawner
   .createState` over every spawning chunk *every tick*, random ticks, fluid
   ticks, and the block-entity tickers. This server does none of the six. A
   ratio would be a statement about coverage, and it would flatter us: our
   tick is faster because it is emptier. That is precisely the number an
   honest gap beats.
2. **Even with all six ported, the substrate is not available.** The only
   two-server harness in the tree quiets vanilla with `spawn_mobs false`,
   `random_tick_speed 0`, `advance_weather false` and `advance_time false`,
   and sends none of them to this server. A tick bench on that substrate
   times a vanilla with its three largest tick loads switched off, against a
   server that never had them. **The substrate is the work, not the
   measurement.**

So it is left unmeasured, and the closing condition is written rather than
the number: a tick bench becomes honest when both servers run the same four
gamerules at the same values and `Entity.move` exists. Nothing was run and
nothing is quoted.

#### Nulls and traps — do not pay for these twice

* **`Level.getBlockRandomPos`'s y and z are NOT in the order you remember.**
  y is bits 16.., z is bits 8... Confirmed twice: from the bytecode, and by
  reproducing vanilla's own 72 dumped draws and 3 end values arithmetically
  before any Rust existed. The swapped version differs on **24 of 24** draws,
  so the golden catches it instantly — and nothing else does.
* **`forEachBlockTickingChunk` selects the ENTITY-ticking set** — it is one
  line calling `forEachEntityTickingChunk`, and `ChunkLevel.isEntityTicking`
  is `l <= 31` where `isBlockTicking` is `l <= 32`. Both predicates exist in
  that class. The name is not merely redundant, it points at the wrong one of
  the two, and the difference is a ring of chunks.
* **`ci/t2-gameplay.sh` quiets vanilla only.** Four `gamerule_or_die` calls,
  all `$VAN_DIR`. Any mechanic ported here that one of those four controls
  becomes a divergence on its first gate. This is the trap to check *before*
  porting weather, daylight, mob spawning or random ticks — all four.
* **`rustmc-entity`'s `tick.rs` and `tick_list.rs` are derived and unwired.**
  ~740 lines of correct bytecode reading that nothing calls. Before writing
  an entity tick loop, read them — the work is largely done and the gap is
  the wiring, not the derivation.
* **`--add-exports java.base/sun.misc=ALL-UNNAMED` is wrong** and the JVM
  says so on stderr, where a golden script's `2>/dev/null` swallows it.
  `sun.misc` is in `jdk.unsupported` and needs no flag.
* **`javap` over every class to find an override times out** — 14,883 classes.
  `grep -rla '<methodName>' net/minecraft/` then `javap` the hits is seconds,
  and it is how the 25-override count was taken.
* **`grep -F` with a MULTI-LINE pattern matches any one of its lines.** So
  `grep -qF "$replacement"` is not a test that a multi-line replacement
  landed: a `continue;` / `}` tail matches somewhere else in any Rust file and
  the check passes against a file nothing was done to. This silently voided
  an injection in this turn's own falsification sweep. Replace literally in
  python and assert the anchor occurs exactly once.
* **A falsification sweep must expect a specific red, not just "some red".**
  All three holes this turn were found by an injection coming back green
  where a named test should have failed. A sweep that only checked "did the
  suite go red somewhere" would have reported nine for nine.
* **`--release` for a falsification sweep costs about ten minutes per
  injection** on this workspace (`lto = "thin"`, `codegen-units = 1`, full
  relink each time). Nine injections is most of an afternoon. For a sweep
  over pure logic a debug build answers the same question; `--release` is the
  gate's requirement, not the sweep's.

#### The single next item

**Give `ci/t2-gameplay.sh` a symmetric substrate.** It is the smallest change
that unblocks the largest amount: it is what lets a random tick be observed
at all, it is what stops the next ported gamerule mechanic reading as a
regression, and it is a precondition for any honest tick bench. It closes
when the four rules are delivered to both servers — or asserted equal on both
— and the corpus line is unchanged by the change itself.

Only then is the second item worth starting: **one randomly-ticking behaviour
with a deterministic body**, and the shortlist is **one entry, not two**.

`SugarCaneBlock.randomTick` ignores its `RandomSource` argument entirely —
`isEmptyBlock(above)`, count the column, `age == 15 ? grow : age + 1` — needs
no light engine, and everything it reads this server already has.

**`CactusBlock.randomTick` was on that list until it was read, and it does not
belong there.** It draws `RandomSource.nextDouble()` against 0.25 (column
height >= 3) or 0.1 (below) and places a `CACTUS_FLOWER` above on success —
offsets 91-133. The growth branch beneath it is the sugar-cane shape, but the
flower branch is a fresh unseeded draw, so a check on a cactus is a
distribution and not an event. Written down because "cactus and sugar cane are
the same rule" is the natural assumption and it is a 26.2 addition that breaks
it.

Note before writing a check for sugar cane: the age increments are written
with flag `sipush 260`, which is `Block.UPDATE_NONE` — `UPDATE_INVISIBLE |
UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS` — so **fifteen of the sixteen random
ticks a growth takes put nothing on the wire.** A check counting packets
counts one event per sixteen ticks, not sixteen, and one that expected sixteen
would read a correct server as broken by a factor of 16.
