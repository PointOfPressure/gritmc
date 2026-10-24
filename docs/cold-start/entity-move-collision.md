# Cold start — branch `entity-move-collision`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `entity-move-collision` off `d3b74cc`, 2026-08-06

**Per-state collision geometry, and `Entity.move`'s geometry on top of it,
gated against 3,710 cases of vanilla running the methods.** Remote dir
`~/rustmc-entitymove-0806` on the **Threadripper**; logs on the laptop under
`~/rustmc-logs/entitymove-*.log`. Working tree `~/grit/work-entitymove`.
Not merged — the orchestrator gates.

#### The one thing to read if you read nothing else

**Nothing calls `crate::collide`, deliberately.** `070`, `500` and `540` do
not move on this branch, and that was written down and committed *before* the
corpus was run — `docs/intel/entity-move-corpus-prediction.md`. The three
checks need the physics *above* `Entity.move`: `ItemEntity.tick`,
`FallingBlockEntity.fall`, `ServerPlayer.doTick`. Every one of them needs
gravity, and `LivingEntity.getDefaultGravity()` is
`getAttributeValue(Attributes.GRAVITY)`, not a constant. Wiring a geometry
engine under three honest `diverge` verdicts before that exists would make
three unmeasured things look measured.

#### How much of the shape space `Entity.move` needs — MEASURED, and the obvious answer is wrong

The cheap representation is one box list per state, `VoxelShape.toAabbs()`,
collided box by box. The argument for it is that the nearest occupied column is
the nearest box face and `collideX`'s two eps guards provably never fire.
**Both halves of that are true and the conclusion is still false.**
`ci/collision-shapes-golden.sh` sweeps every distinct shape, every axis, and
probe boxes built from each shape's own grid coordinates nudged by every
epsilon the algorithm uses:

```text
  16,736,112 probes    345,705 box-list disagreements
                       345,705 of them with the query box already INSIDE the
                               shape — all of them, no second mechanism
                             0 grid disagreements
```

`collideX` starts scanning at the column *after* the one the box's leading face
sits in, so a box embedded in solid reports the gap to the next occupied
column rather than zero — and which column that is depends on where the grid
lines fall, not on the solid's outline. Merging the boxes erases the interior
lines. Worked example, shape 2 on 26.2 (a four-box candle): a box at
x[-1e-7, 0.2499999] sweeping +X by 0.9 is inside the sub-box spanning x[0,1];
vanilla stops it at 0.5625001, the gap to the grid line at x=0.8125, and the
box list stops it nowhere and it travels the full 0.9 through the far wall.

So `data/vanilla/collision_shapes.json` carries the **grid** — three coordinate
lists and an occupancy bitset. **That is not a `VoxelShape` port**: none of
`DiscreteVoxelShape`, the four `IndexMerger`s, `BooleanOp`, `joinUnoptimized`
or `optimize` is needed, because those *construct* shapes and the constructed
grids are what gets extracted. 32,366 states collapse to **318 distinct shapes,
15,845 cells, 273 KB, max grid 10x9x10**.

**The box arm is asserted to stay RED**, and that is not a joke assertion: both
arms share the same probes, so a green box arm would mean the sweep had stopped
probing the overlapping case and the grid arm's zero would mean nothing either.

#### Two more things measured after being read wrongly

* **The admission test is not strict overlap.** `BlockCollisions.computeNext`
  decides membership with `joinIsNotEmpty(entityShape, moved, AND)`, whose
  merged coordinate list comes from `IndirectMerger` and treats two coordinates
  within `1e-7` as one — so an overlap thinner than that does not exist rather
  than being merely small. Strict `<`: **68,551 mismatches in 1,046,007
  probes**, always by admitting a block vanilla does not. With a 1e-7 margin on
  each far face: **0**.
* **A full block takes a different admission test entirely.**
  `shape == Shapes.block()` by *identity* selects a plain `AABB.intersects`
  with **no epsilon**, so a full block IS admitted on a 1e-8 sliver and a slab
  in the same position is not. Both are in the golden.

#### Five things the bytecode says that a reading of it does not

1. **`axisStepOrder` resolves the LARGER horizontal axis first.**
   `abs(x) < abs(z)` selects `YZX_AXIS_ORDER` and `YZX` is `of(Y, Z, X)`, so
   `|x| < |z|` puts the bigger one first after Y. The scoping note further down
   this file says "then the smaller horizontal magnitude" and it is the wrong
   way round; corrected there.
2. **`Cursor3D` varies x fastest and z slowest** — `x = index % width`,
   `z = index / width / height`. Load-bearing, not cosmetic: `Shapes.collide`
   folds a clamp-to-zero across the list and the clamp does not commute.
3. **`hasLargeCollisionShape()` returns TRUE when the cache is null**, so every
   dynamic-shape block passes the face-cell filter and vanilla goes on to
   compute its shape. A face cell holding a shulker box is consulted.
4. **`VoxelShape.move` offsets at READ time**, through `OffsetDoubleList`, so
   every coordinate is `c + offset` and a collide result is `(c + o) - h`.
   Transcribed that way — see the null below; it is not observable, and it is
   still what the bytecode does.
5. **The step-up branch reads `resolved.y`, not `vec.y`**, both for the step box
   and for the `startY` that filters candidate heights.

#### What is gated, and how hard

`crates/rustmc-server/tests/entity_move.rs` against
`crates/rustmc-server/golden/entity_move.json`, produced by
`ci/entity-move-golden.sh` calling the real methods on a real `Entity` in a
real `Level` — `Entity.collide` itself by reflection, because it is private and
reimplementing its arrangement in the dumper would gate one reading of the
bytecode against another.

```text
  35 layouts x 106 probes = 3,710 cases, bit-exact
  1,656 saw a collider    1,353 clamped    133 moved by auto-step
  0 entity collisions     0 border shapes   (both asserted, not observed)
```

Three assertions per case — the collider list **in order**,
`collideBoundingBox`, and `collide` — because they fail separately and a port
can sweep perfectly against the wrong blocks.

#### The falsification sweep, which is where the real work was

`ci/collide-falsify.sh`. The gate went green on its first run, which rule 7
says is when to suspect the instrument. **First sweep: 14 of 24 injections not
caught.** Three real holes, all in the fixture rather than the port:

* the sliver probes measured the wrong box — admission is decided on the box
  expanded by the delta, not the entity's own;
* `admission_strict` was still missed after that because the **injection** was
  wrong, not the gate: it removed the margin from one of six conditions and the
  deciding one was a different one. `Shape::overlaps` now calls one
  `span_overlaps` helper so the margin exists once;
* **every step-up probe had a horizontal delta short of the obstacle** — the
  box face sits 0.7 from the step block and the deltas were 0.4–0.5, so
  `collide` returned before reaching auto-step at all.
  `casesWhereStepUpMoved` was non-zero from other probes the whole time, which
  is exactly how it hid.

Final: **14 caught, 0 holes, 0 wrong arguments, 1 open.**

Nine injections are recorded `unobservable` with a written argument, and **the
script fails if the gate ever catches one**, because that would mean the
argument is wrong. It caught none.

#### Nulls — do not pay for these twice

* **The `Mth.equal` epsilon's exact value is not observable through `collide`.**
  `-9.999999747378752E-6` against `-1.0E-5` differ by 2.5e-13; the only thing
  that value reaches is step-up collider membership, and every test that list is
  then put through carries its own 1e-7 margin. Transcribed correctly anyway.
  Do not go looking for a scenario; one was looked for.
* **`(c + o) - h` and `c - (h - o)` are the same double inside the world
  border.** |o| < 2^25 and shape coordinates are multiples of 1/8, so `c + o` is
  exact; a block only collides with an adjacent box, so `h - o` is exact by
  Sterbenz. Layouts a million blocks out are in the golden and do not separate
  them.
* **`startY`'s source (`resolved.y` vs `v.y`) is not observable.** A candidate
  can only improve the result by lifting the box higher than the y sweep already
  put it (h > resolved.y), but `collideWithShapes` re-runs the y sweep so h is
  only reachable when h <= headroom — and when y is clamped upward the headroom
  IS resolved.y. When y is clamped downward, resolved.y < 0 <= h and the filter
  never fires.
* **`break` vs `continue` in `collectCandidateStepUpHeights` is a no-op** —
  `getCoords` is ascending.
* **The `hasLargeCollisionShape` face-cell filter cannot change a result** — a
  face cell lies outside the query box by construction, so a shape confined to
  its own cell cannot reach it. It is an optimisation.
* **`ci/entity-move-golden.sh` was derived from `ci/collision-shapes-golden.sh`
  with sed, and the sed did not rewrite `OUT`.** Its first successful run wrote
  movement cases over `data/vanilla/collision_shapes.json`, announcing itself
  only in a summary line naming the file it had written. Both are regenerated
  and re-verified. If you derive a third script this way, check `OUT` first.

#### Open, and reported on every sweep run

`grounded_ignores_direction` — dropping `v.y < 0.0` from the `grounded` test
leaves the suite green and there is no proof that it must. The hypothesis and
the weak link in it are written beside the injection in
`ci/collide-falsify.sh`.

#### What this does NOT cover, said out loud

* **23 blocks have `hasDynamicShape()` and get no row** — seventeen shulker
  boxes, `bamboo`, `moving_piston`, `pointed_dripstone`, `powder_snow`,
  `scaffolding`, `sulfur_spike`. `collide` raises `NoShapeFor` rather than
  guessing: air would drop an entity through a shulker box, a full block would
  stand it on powder snow, and both look like working physics. Two tests pin the
  refusal *and* pin that it does not over-fire — a shulker box in a `TYPE_EDGE`
  or `TYPE_CORNER` cell must be skipped, because vanilla never asks.
* **22 classes declare the four-argument `getCollisionShape`**, by reflection
  census in the dumper so the list cannot go stale. A state-keyed table is right
  for all of them today because vanilla's own `BlockStateBase.Cache` is
  state-keyed, but that is where it would first be wrong.
* **Entity-vs-entity collision is absent from both sides.** The golden records
  vanilla returning an empty entity-collision list for every case and asserts
  it, rather than assuming it.
* **No chunk boundaries.** The harness level is its own chunk, so
  `getChunkForCollisions` never returns null and the "skip the whole column"
  path is unexercised.

#### Next item, and the result that closes it

`Entity.move` proper — the ~200 instructions after `collide`: `setPos`, the
four collision flags (`horizontalCollision`, `verticalCollision`,
`verticalCollisionBelow`, `minorHorizontalCollision`), `checkSupportingBlock`
and `setOnGroundWithMovement`. It needs nothing this branch does not already
have and it is the last piece before an entity tick can call any of it. Closes
when a golden of the four flags plus the resulting position reproduces vanilla
over the same layouts.

**Do not start `ItemEntity.tick` or `FallingBlockEntity.fall` before the
attribute system.** `getDefaultGravity` is an attribute read for every living
entity, and hard-coding 0.04 is the one-way door `crate::dropitem` already
refuses in writing.


*(Merge note, rebase driver, 2026-08-06: entity-move-collision's cold-start (authored 14:10:37) is chronologically after both redstone-power's (11:16:32-12:49:35) and tick-randomtick-selection's (11:57:28) — no overlap this time. Placed at the top per the insert-newest-first convention entity-move-collision's own commit already used against chain-signs-eviction-swing. The single already-demoted `chain-signs-eviction-swing` heading below is kept once, not duplicated — entity-move-collision's own un-rebased copy repeated it undemoted; dropped as a literal duplicate of already-present shared text, not as a narrative.)*
