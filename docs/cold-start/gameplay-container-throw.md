# Cold start — branch `gameplay-container-throw`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `gameplay-container-throw` off `main` (`cd521254`), 2026-08-09

**Item: `605-container-click-throw-drops-one-item` and `606-…-the-whole-stack`,
S4's named leftover. BOTH CONVERTED.** Worktree `~/grit/work-throw`; box work
in `~/rustmc-throw`, plus `~/rustmc-throwdev` for builds and one throwaway
comparator. Not merged — the orchestrator gates.

**THE NUMBERS, first.** `ci/t2-gameplay.sh`, corpus **88**, vanilla 26.2
protocol 776, superflat, seed 20260803, Threadripper. Both runs taken this
morning on this box:

```
before  (main cd521254,  07:40-08:17):  declared 62/88, strict 60/88   GREEN
after   (this branch,    10:04-10:47):  declared 64/88, strict 60/88   GREEN, no REGRESSED

  container-click-throw-drops-one-item         declared diverge -> match
  container-click-throw-drops-the-whole-stack  declared diverge -> match
  row by row, 88 names, both denominators:     2 rows moved, and they are those two
```

Gate, all on the branch tip: `ci/t0-remote.sh` exit **0**,
`grep -c "skipping: no vanilla"` = **0**, `ci/check-figures.py` FIGURES GREEN
(inside T0), `ci/t2-gameplay.sh` **T2-GAMEPLAY GREEN with no REGRESSED line**.

**The gate's t2 is a THIRD corpus run, 12:29-13:10, taken after the baseline
ratchet and the doc commits**, so it gates the tree being handed over rather
than the tree the 10:04 run measured — declared 64/88, strict 60/88, and the
two throw `BETTER` lines are gone because they are now locked in. Row by row
against the 10:04 run: **0 of 88 moved**, so the conversion is not one lucky
boot of our own server either.

**`070-sand-falls-when-unsupported` HELD**, and visibly rather than by
assertion: `diverge`/`diverge`, still `we never sent: add_entity`, and its
projected packet now reads
`{"at":[1.5,1.0,0.5],"data":118,"kind_id":51,"x_rot":0,"y_head_rot":0,"y_rot":0}`
— **the `y_rot` is still there**, because the exemption tests
`minecraft:item` and 070's entity is `minecraft:falling_block`.
`010-break-block` is `match`/`match`, unmoved. `500` moved from `diverge` to a
*different* `diverge` exactly as §3 S4's negative control predicted: its
`add_entity` now matches and it fails on `we never sent: set_entity_motion`.
`350` `absent`/`diverge` and `050` `match`/`diverge`, both unmoved.

**Both halves were necessary, and it is measured and not argued.** The
after-run's own recordings were re-compared by a comparator with the exemption
removed and nothing else changed (patched in `~/rustmc-throwdev`, never
committed, as §11's experiment was):

```
A  old decoder, no exemption, old server   62/88  60/88
D  new decoder, no exemption, NEW server   62/88  60/88   0 of 88 rows vs A
C  new decoder, exemption,    new server   64/88  60/88   2 of 88 rows vs D
```

**A -> D is zero**: the decoder change is output-neutral over the whole corpus
and the server work alone converts nothing. **D -> C is two rows**: the
exemption's entire effect is the two scenarios it was built for. The other
direction needs no run — before the server work we emitted no `add_entity` at
all for a container throw, and no field exemption rescues an absent packet.

**THE EXEMPTION IS ONE BYTE, AND THAT IS THE PART TO READ.** §11's experiment
and this file's previous cold-start block both said to drop `at` **and**
`rest` from an item `add_entity`. Neither was dropped.

* **`at` must NOT be dropped**, and the recordings say so: vanilla's item and
  ours are both at `[0.0, 1.3199999928474426, 0.0]` relative, identical to the
  last bit. It is the one deterministic thing about a thrown item and it is
  what catches `1.62` written where the widened `1.62f` belongs.
* **`rest` is not dropped either.** `tools/oracle-runner/src/trace.rs` now
  **names** `xRot`, `yRot`, `yHeadRot` and `data` — the four values
  `ClientboundAddEntityPacket.write` ends with — and `compare::project`'s map
  drops `y_rot` alone, for `minecraft:item` read out of
  `data/vanilla/entity_types.json` (71, asserted in a unit test, never
  hardcoded). Reversing the FNV of the four recorded tails shows why: three of
  the four bytes are constant `0` for every item vanilla spawns, so dropping
  `rest` would have stopped comparing three real values to reach one that
  cannot be compared.

**They are no longer unstable on the gated denominator, and that was tested in
both directions.** The two entries were removed from
`ci/checks/gameplay-unstable.toml` **before** the run, the convention `070`'s
removal set, so a failed exemption would have printed `NEWLY UNSTABLE`.
`SELFCHECK_RUNS=2 ci/t2-gameplay-selfcheck.sh` then
`ci/selfcheck-recompare.sh` over three recordings (`r1`, `r2`, and the corpus
run's own `vanilla.json` as `r3`), N = 3, 2 comparisons:

```
605 / 606  declared 0/2   (was 2/2)
605 / 606  strict   2/2   — set_entity_motion and move_entity, the item's own
                            physics, which vanilla does not reproduce either
3 runs, declared 5, measured 7, 81/88 reproduced; strict 77/88
```

`ci/checks/gameplay-baseline.toml` is ratcheted to `match` for these two.

**WHAT I KILLED OR CORRECTED — do not re-derive.**

1. **`click.rs`'s dispatch-offset comment was wrong and is fixed.** It said
   "`PICKUP` first (offset 0), `QUICK_MOVE` at 663"; those are offsets from
   *inside* the shared arm. The real dispatch is `QUICK_CRAFT` 0-11,
   `PICKUP`/`QUICK_MOVE` 547-572, `SWAP` 1080, `CLONE` 1386, `THROW` 1449,
   `PICKUP_ALL` 1612. The behaviour was right; the map was not.
2. **`trace.rs` claimed vanilla "structurally never emits" `set_entity_motion`
   for an ITEM.** `ITEM` is not one of the ten `trackDeltas() == false` types,
   so it emits one every time — and 500's own first difference in this run is
   that packet. Corrected in place with the receipt. **So a check does compare
   an entity's movement value on the wire**, exactly one does, and it is four
   `nextFloat()` draws.
3. **`n` in the THROW arm is read once, at 1491-1499, and the loop re-uses
   it.** A `button = 1` throw of a stack of two is **one** `safeTake(2, MAX)`
   and **one** item entity of two — not two entities of one. A unit test
   asserts the count of dropped stacks, which is the only thing that catches
   it; the corpus cannot.
4. **`Player.canDropItems()` is `iconst_1; ireturn`** and
   `handleCreativeModeItemDrop` is a bare `return`, neither overridden by
   `ServerPlayer`. The two `return`s at 1509 and 1574 are unreachable and are
   omitted rather than transcribed as `if (true)`.
5. **Not a defect, checked: the step window.** 605/606 are
   `container_click` steps, which return **250 ms**, so the `Step::Hold` /
   `Step::Look` 60 ms coin flip of §12 cannot reach them. Vanilla answers at
   1295 ms into a step opened at ~1283 ms with a boundary at ~1533.

**Known gaps this leaves, named rather than left to be found.**

* **`Slot.onTake` is not called.** It is a bare `setChanged()` for every slot
  here except `ResultSlot`, which consumes the crafting grid — so a `THROW` on
  a crafting result empties the grid in vanilla and does not here. No scenario
  writes that click.
* **605/606 stay strict-`diverge`** and their whole residual is the item's own
  tracking traffic: two `set_entity_data` (`ItemEntity`'s `DATA_ITEM`, which
  this server does not carry), `set_entity_motion`, `move_entity`,
  `entity_position`. The last three are `ItemEntity.tick`.
* **Two `NEWLY UNSTABLE` names are not mine and reproduce**:
  `a-player-that-leaves-the-ground-is-resynced` (1/2) and
  `closing-a-crafting-table-returns-the-grid-contents` (1/2), which §11
  reported at 2/2 and 1/2 on 2026-08-08 from different recordings under a
  different comparator. Two sessions, two days, same two names. Not absorbed —
  neither mechanism has been read out of the jar and that is
  `gameplay-unstable.toml`'s own test for absorbing. This is why
  `ci/t2-gameplay-selfcheck.sh` exits non-zero here; it is not part of the
  gate.
* **The five inherited `BETTER container-click-*` lines are still not locked
  in — a fourth round.** Each agent has correctly declined to ratchet somebody
  else's conversion, and the cost is now that five real conversions have no
  ratchet under them.

**THE SINGLE NEXT ITEM, AND THE RESULT THAT CLOSES IT.**
**`ItemEntity.tick` — gravity, drag, the swept-AABB move, the 40-tick pickup
delay, the 6000-tick despawn.** It is the one mechanism behind the whole
remaining item family at once: `500`'s declared `set_entity_motion`, `605` and
`606`'s entire strict residual, and half of what `350` needs. It is the
largest single thing left in the gameplay corpus that is not an instrument
question, and `entity-move-collision` landed a collision grid that nothing
calls. **Closed by `500-dropping-an-item-throws-it-on-the-floor` moving from
declared `diverge` to declared `match` in a full corpus run** — which is a
real target rather than a hopeful one now, because 500's `add_entity` already
matches and `set_entity_motion` is the only kind it reports missing. Note
before starting that `gameplay-unstable.toml` declares 500 at 25/25 for the
velocity's four `nextFloat()` draws, so a `set_entity_motion` whose `v` is
compared byte-exactly can never match: **the honest closing condition is that
500 reaches the same shape 605/606 reached — the packets exist, the fields
that can be compared agree, and the one field that cannot is exempted as
narrowly as the yaw was.** Read `docs/PARITY-ROADMAP.md` §14 for how narrow
that has to be to be honest, and do not reach for `set_entity_motion`'s whole
`rest`.
