# Cold start — branch `gameplay-next-three`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `gameplay-next-three`, off `main` (`7ccad044`), 2026-08-10

**Three gameplay conversions, one branch, three commits, because all three
land in `world_send.rs` and would conflict as three branches.** Worktree
`~/grit/work-next3`, remote dir `~/rustmc-next3`. Not merged — the
orchestrator gates. **Inserted above the previous agent's block rather than
replacing it; that block is its own record.**

**WHAT WAS BUILT.** Each is one commit and each is separately revertible.

1. **The sneak flag** (`fd37fd9d`). `ServerboundPlayerInputPacket` (43) was
   read nowhere; `PlayerState` now carries `shift_key_down` and
   `handle_use_item_on` gates the whole block-interaction arm on
   `isSecondaryUseActive() && holdingSomething`
   (`ServerPlayerGameMode.useItemOn` 101-126, the jump at 126 skips **both**
   block calls). Expected to convert `609`, `552`.
2. **The comparator's use sound** (`7682fb66`). Third family in `use_sound`.
   Pitch 0.55/0.5 off `MODE` of the **cycled** state, volume 0.3, and
   `except` = the clicker, which the lever's `aconst_null` is not. Expected to
   convert `420`.
3. **Item-aware `canBeReplaced`** + the merge it enables. `Blocks::replaceable`
   took a position and now takes a `Replacing` context; the default rule gains
   `!item.is(asItem())`, and `SlabBlock`/`SnowLayerBlock` override it.
   `merged_into` is the other half — a slab writes `type=double` and a snow
   layer `min(8, layers+1)`, both built on the state already there. Expected to
   convert `250`, `170`.

**VERIFICATION STATE — READ THIS BEFORE QUOTING ANYTHING.** No corpus run was
taken. **No scenario is claimed converted.** T0 green on `~/rustmc-next3` with
`skipping: no vanilla` at 0, and unit-level evidence per item, each poisoned
and each red at its own assertion. The corpus verdict is the orchestrator's to
sequence.

**FINDINGS THAT ARE NOT CODE.**

* **Four of 90 scenarios observe `sound`** — `420`, `430`, `550`, `551`. So
  `290`, `330`, `470` and `480`, named in the brief as this change's negative
  controls, are **blind on the axis they were named for**. Nothing was edited.
  `docs/DETERMINISM.md` §29.
* **Two test doubles were wrong in the same direction as the code**, both
  found by poisoning: `FakeWorld::replaceable` called a chest replaceable, and
  its `canBeReplaced()` flag omitted `tall_grass` so the new item-aware
  assertion passed for the wrong reason and survived deleting the conjunct it
  was written for. §29.1.

**NULLS — DO NOT RE-RUN THESE.**

* **Do not put the sneak gate inside `crate::useblock::use_without_item`.**
  Vanilla's is above the call and skips both `useItemOn` and `useWithoutItem`;
  inside, a sneaking click looks like a block that declined, and a declining
  block still reaches `open_menu_for`. `609` would still fail.
* **Do not try to reproduce the comparator's sound-before-`setBlock` order.**
  It is not observable: `playSound` reaches `PlayerList.broadcast` unqueued
  and a block change waits for the tick boundary, so the sound leads either
  way. Measured as a reading of both call paths, not assumed.
* **Do not implement item 3 as `canBeReplaced` alone.** Neither `170` nor
  `250` converts without `merged_into` as well: with the gate alone a second
  slab overwrites the first with another bottom slab and the trace carries no
  change at all. Poison E proves the halves are separately necessary.
* **`placement.rs::chest_type`'s `isSecondaryUseActive` branch is still not
  ported and is not a follow-on from item 1.** It needs the flag threaded into
  `state_for_placement` and the table re-sampled. Nothing measures it.

**THE SINGLE NEXT ITEM.** Run the corpus on this branch and read the verdict
for `609`, `552`, `553`, `420`, `250`, `170`, `120`, `020`, `290`, `330`,
`470`, `480`. What closes it: `552`, `553`, `609`, `420`, `250` and `170` all
`match`, and the six controls unmoved from their pre-branch verdicts.
