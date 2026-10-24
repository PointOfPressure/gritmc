# Cold start — branch `shape-family`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `shape-family` off `83f0b6e`, 2026-08-05

**All five of the neighbour-aware shape family are ported.** Not merged; the
orchestrator gates and merges. Remote dir `~/rustmc-chain-shape` on the
**Threadripper**. Logs on the laptop under `~/rustmc-logs/shape-*.log`.

215 lib tests pass (from 174 at `83f0b6e`), clippy clean.

#### The corpus line

`./ci/t2-gameplay.sh` in `~/rustmc-chain-shape`, real vanilla as reference,
67 scenarios recorded and 0 unrunnable. Log:
`~/rustmc-logs/shape-t2-run1.log` on the laptop.

```text
  T2-GAMEPLAY GREEN
  CORPUS  declared  40/67 match   (21 diverge, 6 absent, 0 vacuous, 0 inert, 0 error)
  CORPUS  strict    22/67 match   (41 diverge,  3 absent, 1 vacuous, 0 inert, 0 error)

  BETTER  stairs-make-a-corner-when-two-meet   diverge -> match
  BETTER  chest-pairs-with-its-neighbour       diverge -> match
  BETTER  rail-curves-towards-its-neighbour    diverge -> match
```

**Three BETTERs, no WORSE, and `450` did not move — which is the result the
commit predicted.** `redstone-wire-climbs-the-block-beside-it` is still
`diverge` because its block lands diagonally and the indirect cascade is not
ported. A green there would have been the warning sign, not the reward.

**The two wire checks that share the ported rule did not regress.**
`redstone-torch-powers-wire` and `wire-power-falls-off-with-distance` were
`diverge` before this branch and are `diverge` after it — the placement change
did not disturb them. `ladder-hangs-off-the-wall-behind-it`,
`stairs-and-furnace-face-apart` and `fences-connect-to-each-other` all hold at
`match` as controls.

Locked into `ci/checks/gameplay-baseline.toml`, each with its
self-reproduction rate in the comment.

#### The vanilla self-reproduction lock, measured before any verdict

`ci/selfcheck-recompare.sh` against the 26 corpus-63 recordings at
`~/rustmc-chain-rule26/oracle-work/gameplay-selfcheck`, 25 pairwise
comparisons, **declared** axis — the gated one:

```text
  180 stairs-make-a-corner-when-two-meet     25/25   (strict flaps 1/25)
  240 chest-pairs-with-its-neighbour         25/25   (strict flaps 4/25)
  300 rail-curves-towards-its-neighbour      25/25   no flap either axis
  450 redstone-wire-climbs-the-block-beside  25/25   no flap either axis
  200 ladder            (control)            25/25   reproduces the known lock
```

**This does not need a re-recording run.** The selfcheck measures vanilla
against vanilla, so a *server* change cannot move it and the saved recordings
answer it in seconds. That is the cheap half of `ci/t2-gameplay-selfcheck.sh`
and it is the half that locks a check.

The run reports one `SELFCHECK FAILED` — `540-a-player-that-leaves-the-ground-is-resynced`
— which is the pre-existing, deliberately undeclared instability recorded
below. Nothing to do with this branch.

#### Where each of the five actually lives, which is three hooks and not one

The family was briefed as one shared primitive. It is three, and the criterion
is mechanical — read `updateShape` and ask what it consumes:

```text
  180 stairs   getStairsShape(state, LEVEL, pos); ignores aload 7 entirely
               -> placement REFINEMENT + a new level-aware cascade hook
  240 chest    updateShape reads aload 7 and ONLY aload 7
               -> placement REFINEMENT + the EXISTING typed_update_shape
  300 rail     neither hook. getStateForPlacement reads no neighbour at all
               -> onPlace, a third hook that WRITES and recurses
  450 wire     getConnectionState(LEVEL, ...), thirteen positions, eight
               diagonal -> placement REPLACEMENT; the indirect cascade is NOT
               ported and that is why 450 does not move
  200 ladder   placement REPLACEMENT (landed earlier)
```

A **refinement** is not a second kind of port bolted on: it is the shape
vanilla's own `getStateForPlacement` has for the stair and the chest, which
build a state from click-only inputs the grid already sampled and *then* write
one more property from the level. Re-deriving `FACING` by hand would have been
the furnace/stair inversion this module's docs open with.

**`onPlace` is called from `LevelChunk.setBlockState`, offsets 514-542** —
inside the write, before `sendBlockUpdated` and before `updateNeighbourShapes`,
not after `Level.setBlock` returns.

#### FINDING — five rail tests that could not fail, and Rule 7 fired first

Eight rail tests passed under **four** separate injected bugs. The mutations
were verified present in the source and the harness was sound; the tests were
blind. Every L-shaped scenario — **including check `300`'s own** — bends its
middle rail inside `connectTo`, which runs only the *first* corner pass.
`place`'s second pass at offsets 341-423 needs neighbours on both axes and no
unambiguous corner, and `canConnectTo`'s `||` only bites when a rail already
has two connections. The corpus scenario builds neither.

Five tests added that reach them; the same four bugs now go red 1/1/1/4.

**The same class caught the wire.** "Answer `isRedstoneConductor` from the
collision/sturdy column" — the exact defect the dumper change exists to
prevent — left every wire test green, because both source/conductor cases
reach their answer through `shouldConnectTo` at offset 97 *before* the
conductor test at 106. `dust_runs_under_glass_because_glass_is_not_a_conductor`
is the arrangement that reaches it.

**Generalise:** a test built from a corpus scenario exercises the path that
scenario takes and no other. Porting a method whose branches the scenario
cannot reach means writing tests for the *method*, not for the check.

#### The dumper change, and the table now proves it was needed

`block_props.json` gained `redstoneConductor` and `signalSource`, regenerated
from the pinned jar, identical across the script's own two runs, 32,366 states.

**`minecraft:glass` has `collisionFullFaces` `111111` and `redstoneConductor`
false.** A block full on every collision face that redstone treats as empty.
That is the measurement that settles "no existing column can stand in for it"
— it was an argument before and is a number now. 2,874 states are conductors;
2,621 are signal sources.

**One briefed claim was wrong and it would have shaped the port.**
`shouldConnectTo` needs *one* column, not "a redstone power source including
repeater and comparator facing": the wire, repeater and observer are decided
by block identity plus their own `facing`, and **a comparator has no facing
rule there at all** — it reaches `isSignalSource()` like any other source.

#### Nulls and traps — do not pay for these twice

* **The self-reproduction lock does not need a recording run.** Five hours of
  `ci/t2-gameplay-selfcheck.sh` buys nothing a `selfcheck-recompare` against
  the saved corpus-63 set does not already answer, for a server-side change.
* **`Direction.getClockWise()` THROWS on UP/DOWN.** It does not return `this`;
  that is `getClockWise(Axis.Y)`, a different method. `Face::clockwise`
  returns `Option` for that reason.
* **The chest's un-pairing branch is already in `update_shape.json`** — all 32
  rows of it, every LEFT/RIGHT half losing its partner along its own connected
  direction. Testing it through `set_block` measures the extraction, not the
  port. Only the *pairing* half was missing.
* **The stale-binary trap, again.** After restoring a mutated file, `rsync -a`
  puts the older local mtime back and cargo re-runs the binary built from the
  bug. Four tests "failed" on a clean tree.
  `find crates tools -name '*.rs' -exec touch {} +` after every rsync.
* **`ci/block-props-golden.sh` does not stamp.** Running it directly makes
  `t2-gameplay` refuse with "changed since it was stamped". Regenerate through
  `ci/gen-gameplay-tables.sh`.

#### The single next item, and what closes it

**`RedStoneWireBlock.updateIndirectNeighbourShapes`, and it is the whole of
what `450` is waiting on.** The check places its block *diagonally* from the
wire it expects to change; `set_block` walks six faces and the override walks
out to exactly those diagonals. The connection rule underneath it is already
ported and unit-tested, so this is one cascade and not a rule.

Closing condition: `450` moves `diverge -> match` on a corpus run **and** the
`100`/`370` wire checks do not move, since all three read the same rule.

**Transcribed here so that link is a port and not a re-derivation.**
`RedStoneWireBlock.updateIndirectNeighbourShapes`, offsets 0-233:

```java
MutableBlockPos m = new MutableBlockPos();                       //   0-7
for (Direction d : Plane.HORIZONTAL) {                           //   9-37
    if (state.getValue(PROPERTY_BY_DIRECTION.get(d)) == NONE)    //  39-66
        continue;
    m.setWithOffset(pos, d);
    if (level.getBlockState(m).is(this)) continue;               //  78-87
    m.move(Direction.DOWN);                                      //  90-98
    if (level.getBlockState(m).is(this)) {                       //  99-115
        BlockPos from = m.relative(d.getOpposite());             // 118-128
        level.neighborShapeChanged(d.getOpposite(), m, from,     // 130-152
            level.getBlockState(from), flags, recursion);
    }
    m.setWithOffset(pos, d).move(Direction.UP);                  // 157-171
    if (level.getBlockState(m).is(this)) {                       // 172-188
        BlockPos from = m.relative(d.getOpposite());             // 191-201
        level.neighborShapeChanged(d.getOpposite(), m, from,     // 203-225
            level.getBlockState(from), flags, recursion);
    }
}
```

Three things in it that a summary would lose:

* **The diagonals are only probed when the orthogonal neighbour is NOT a
  wire** (offset 87, `ifne` straight to the next direction). A wire beside a
  wire is already served by the six-face cascade, so this override adds
  exactly the cases that cascade cannot reach and no others.
* **It is gated on this wire's own connection being non-`NONE`.** A dust
  reaches out along the sides it claims to use, not along all four.
* It notifies **the diagonal**, from `d.getOpposite()`, with the state at the
  position one step back from the diagonal — which is the block that changed,
  not this one.

For check `450` that is: dust placed at `[2,1,0]`, its west neighbour
`[1,1,0]` is air rather than wire, so the DOWN probe finds the dust at
`[1,0,0]` and hands it a shape update from the east. Which is the whole
observation the check makes.

**And `Level.setBlock` calls it TWICE, around the cascade we already have** —
offsets 177-229, established while the corpus ran:

```java
if ((flags & 16) == 0 && recursionLeft > 0) {              // 177-186
    int inner = flags & -34;                               // 189-193
    oldState.updateIndirectNeighbourShapes(this, pos, inner, recursionLeft-1);  // 195-205
    newState.updateNeighbourShapes     (this, pos, inner, recursionLeft-1);     // 208-217
    newState.updateIndirectNeighbourShapes(this, pos, inner, recursionLeft-1);  // 220-229
}
```

`aload 7` is the **old** state and `aload_2` the new one, so the sequence is
old-indirect, new-direct, new-indirect. `crate::shapes::set_block` implements
only the middle call, under the same `flags & 16` and `recursion > 0` guard
and the same `flags & -34` inner flags — so the next link is **two calls
either side of a loop that already exists**, not a new traversal.

The old state's call is not symmetry for its own sake: it is how a wire that
has just been *removed* tells the diagonals that were connected to it. A port
that added only the new state's call would fix placing and not breaking.

Second item, smaller: the wire's own `updateShape` (three branches, offsets
0-117 read; the `isCross`/`crossState` fallback at 118+ is not yet
transcribed).
