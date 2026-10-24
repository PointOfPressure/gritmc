# Cold start — branch `gameplay-611-close-returns-grid`

---

### COLD START — branch `gameplay-611-close-returns-grid` off `main` `7c919d20`, 2026-08-11

**The close path is ported and the two holes the item named are shut. `611`
did not convert, and the reason is a third hole that no amount of work on this
branch can reach.** Remote dir `~/rustmc-close611` on the Threadripper;
worktree `/home/sayer/grit/work-close611`.

#### What was wrong, both of it

1. **There was no serverbound `CONTAINER_CLOSE` dispatch arm.** Confirmed by
   measurement rather than taken from the brief: `CONTAINER_CLOSE` appeared
   *exactly once* in the whole tree, as its own `const` in
   `crates/rustmc-proto/src/ids.rs`. The runner's `close_container` verb was
   landing on a server that read the frame off the wire and dropped it.
2. **`close_open_menu` dropped the menu without walking it** — an
   `Option::take` and an openers decrement, with a doc comment saying the send
   was "not written until something measures it".

Both are fixed in `eaa86bd0`, comment included.

#### The vanilla path, from the pinned jar only

```
handleContainerClose          -> doCloseContainer(); does NOT read the
                                 packet's containerId, unlike
                                 handleContainerClick two methods below it
doCloseContainer              -> containerMenu.removed(player)
                                 inventoryMenu.transferState(containerMenu)
                                 containerMenu = inventoryMenu
AbstractContainerMenu.removed -> hands the CARRIED stack back first, then
                                 setCarried(EMPTY)
CraftingMenu.removed          -> super, then access.execute(... ->
                                 clearContainer(player, craftSlots))
clearContainer                -> dropOrPlaceInInventory over EVERY index,
                                 empty ones included
dropOrPlaceInInventory        -> placeItemBackInInventory unless the player is
                                 removed or has disconnected
placeItemBackInInventory      -> selected slot, then offhand, then a forward
                                 scan; nowhere at all means
                                 player.drop(stack, false)
```

Three things in that chain are worth carrying forward:

* **The cursor is returned before the grid.** Observable: a full inventory
  loses grid contents rather than the cursor.
* **`craftSlots` is the 3x3 and not the result slot.** `ResultContainer` is a
  separate container that no `removed` clears, which is why a crafting result
  vanishes on close and its ingredients do not.
* **A `ChestMenu` reaches the same `removed` and has no `clearContainer` call
  at all.** That falls out here for free — `click::Menu::grid` is empty for a
  container menu — rather than needing a branch.

**The brief's chain was one link off and the check's own `about` text was
right.** `clearContainer` is *not* reached from `AbstractCraftingMenu.removed`
— `AbstractCraftingMenu` has no `removed` at all, and the call is in
`CraftingMenu`'s own `lambda$removed$0`. Nothing downstream changes.

#### THE CENSUS

Full `ci/t2-gameplay.sh` in this worktree, corpus **92** (the run's own
`recorded 92/92`), Threadripper, 2026-08-11 16:44, log
`/home/sayer/grit/work-close611-gate.log`. `T2-GAMEPLAY GREEN`, `GEN=0 T2=0`.

```
CORPUS declared 71/92 match (16 diverge, 5 absent, 0 vacuous, 0 mute, 0 inert, 0 error)
CORPUS strict   67/92 match (23 diverge,  2 absent, 0 vacuous, 0 mute, 0 inert, 0 error)
```

**Identical to `main` `7c919d20` in every field.** `611` did not convert and
**nothing else moved either** — which is the claim that needed the census
behind it, because the close path now runs in every scenario the reset closes
a menu for and any neighbour could have shifted. None did.

The floor prints `7/92` and then `STALE PROJECTION` (measured under
`proj:b385528330fee646`, this run compares under `proj:1e154ef8765b7525`).
**Not quotable, and never subtracted from the score.** Re-measuring it is a
separate item. The `BETTER` advisory on
`container-click-pickup-all-gathers-matching-stacks` is `main`'s and was left
alone.

#### THE FINDING: `611` IS NOT ADJUDICABLE ON `state`, BY EITHER SIDE

Ours now matches vanilla's step 3 in packet, window and slot, and the item
handed back is **byte-identical** — `rest` is `cec2cbd74d50f332` on both
sides. `state` is the only field left:

```
at 5: vanilla {"rest":"cec2cbd74d50f332","slot":36,"state":8,"window":0}
      ours    {"rest":"cec2cbd74d50f332","slot":36,"state":3,"window":0}
```

**These two integers are only meaningful as a pair, from one named run**, for
the session-global reason below; do not carry either of them anywhere on its
own.

**And vanilla's own value moved between two runs of the same corpus under the
same projection.** The 15:30 gate on `main` recorded vanilla at `state=7`; this
16:44 run recorded it at `state=8`. Our side cannot influence a separate
vanilla process, so that is **the reference moving, not our counter being
behind** — and under ORACLE rule 6 a residual only convicts us if two
independent vanilla runs agree on it. Here they disagree, so `611` cannot be
adjudicated on `state` at all until the offset decoder lands.

**This is n = 2 and it is stated as the two readings it is**, not as a proven
instability — though it is the same field
`ci/checks/gameplay-unstable.toml` already declares this check unstable on,
independently measured at 8/15 pairs over six vanilla runs.

It also kills the tidier story: **the offset is not a fixed 6.** It was 6 in
one run and 5 in the other.

`ClientboundContainerSetSlotPacket`'s state id comes from
`menu.incrementStateId()` and counts **packets actually sent on that menu**,
not changes made. `InventoryMenu` is built once in the `ServerPlayer`
constructor and outlives every container the player opens — and the whole
corpus runs down one connection (`fresh_session` is false for every scenario
after the first). So window-0's counter is **session-global across all 92
scenarios**, and every window-0 packet vanilla sends that we do not leaves us
permanently one behind.

Measured off the recorded traces in `~/rustmc-stack/oracle-work/gameplay/`
rather than derived. **The `state` column below is one 15:30 vanilla run and
is not a property of these checks** — the same column re-read from the 16:44
run has `611` at 8, not 7. What survives re-running is *which* scenarios send
on window 0 and in what order; the integers do not.

| # | scenario | vanilla window-0 sends (15:30 run) | ours |
|---|---|---|---|
| 44 | `a-dropped-item-goes-into-your-inventory` | slot 36, state 2 | none |
| 60 | `swapping-to-the-offhand-shows-on-both-channels` | slot 36 state 3, slot 45 state 4 | none |
| 86 | `closing-a-crafting-table-returns-the-grid-contents` | slot 36, state 7 | **now one; state 3 at 16:44** |
| 91 | `a-flower-pot-empties-when-right-clicked` | slot 38, state 9 | none |

Those are the only four scenarios in the corpus with a window-0
`container_set_slot`, and until this branch **this server emitted none of
them**. The two low sends belong to item pickup and offhand swap, both
baselined `absent`, and the rest are sends outside any observed step. So our
counter is behind for reasons no work on this branch can reach: it needs item
pickup *and* offhand swap to land as well, and neither is this item.

**But that is the weaker half of the finding and it must not be quoted as the
whole of it.** Even with every missing sender ported, `611` would still not be
adjudicable, because the reference does not hold this field still: vanilla
read 7 at 15:30 and 8 at 16:44 on the same corpus under the same projection.
`ci/checks/gameplay-unstable.toml` independently declares the same check
unstable on the same field at 8/15 pairs over six vanilla runs. Chasing our
counter to a value vanilla itself does not reproduce would be work with no
closing condition.

`ci/checks/gameplay-unstable.toml` names the real fix in its own entry for
this check: *a decoder that records the state id as an offset from the first
one seen on that window*, which would retire both this entry and
`swapping-to-the-offhand`'s. That is a projection change — it moves `proj:`,
exactly as the merged `mute` verdict did — and it is a separate item. **It is
now also the only thing standing between `611` and `match`**, which was not
true before this branch: the gap used to be a missing packet.

#### Nulls — do not re-run these

* **Matching vanilla's window-0 `state` by modelling `InventoryMenu` harder is
  a null, twice over.** The counter is session-global across the whole corpus
  and several of its prior increments belong to scenarios this server is
  `absent` on, so no change confined to the close path can reach it — and
  vanilla does not hold the value still between runs anyway, so there is no
  fixed target to reach. Quoting any single vanilla `state` as the number to
  hit is the mistake this bullet exists to stop.
* **`handleSetCreativeModeSlot` is not a window-0 sender**, so the corpus's
  many `hold` verbs do *not* advance the counter. It calls
  `inventoryMenu.setRemoteSlot(slotNum, stack)` — recording what the client
  already believes — *before* `broadcastChanges()`. Checked because a naive
  count predicted dozens of window-0 packets and the recordings show six.
* **`packet:0x6C` is `minecraft:set_player_inventory` = 108**, from Mojang's
  `reports/packets.json`. It is sent from inside
  `placeItemBackInInventory`'s loop, one per slot written, and it carries no
  state id. Ported; do not re-derive it.
* **The declared projection filters on the check's `observe` list**
  (`compare::project`, the `kinds` filter), so `set_player_inventory` is not
  compared for `611` under `declared`. It *is* under `strict`, which passes
  `kinds = None`.

#### Named gaps, deliberately not invented

* `getSlotWithRemainingSpace`'s **offhand probe** (`getItem(40)` —
  `SLOT_OFFHAND`, outside `Inventory.items`, resolved through
  `EQUIPMENT_SLOT_MAPPING`) is absent: this server models no equipment.
* The **no-room drop** routes to `dropitem::spawn_thrown_item`, which
  transcribes `traceItem = true`; vanilla's call here is `drop(stack, false)`,
  a different velocity branch. Inventing the other branch off no bytecode
  would be worse, and destroying the stack would be worse than both.
* `openMenu` reaches `closeContainer`, not `doCloseContainer`, so vanilla
  *does* send a `ClientboundContainerClosePacket` when a second menu opens
  over a first. Still unwritten: nothing in the corpus opens two menus without
  a disconnect, so nothing has measured it.
* **This server never sweeps window 0.** Vanilla's `ServerPlayer.tick` calls
  `containerMenu.broadcastChanges()`, and `containerMenu` *is* the inventory
  menu whenever nothing else is open. The only thing that advances
  `MenuState::inventory_state_id` here is a menu close.

#### One transcription that could not be literal

`addResource`'s `copyWithCount(0)` cannot be ported as written:
`ItemStack::new` collapses a zero count to `EMPTY` **on purpose**, so that
`is_empty` and `item` can never disagree. The literal port produced
`ItemStack { item: "", count: 1 }` — caught by the free-slot unit test, which
is the whole reason it was written before the corpus ran.

#### Next item that closes this

**`611` converts when the state-id decoder lands**, not before. The check now
diverges on one integer where it used to diverge on a missing packet, and the
item handed back is byte-identical to vanilla's. Whoever takes the decoder
item should expect `611` and `swapping-to-the-offhand-shows-on-both-channels`
to come out of `ci/checks/gameplay-unstable.toml` together, as that file
already says.

**Do not re-run the corpus hoping for a different answer, and do not move
`611`'s baseline.** It stays `diverge`, with the real reason: the only field
still differing is one two independent vanilla runs disagree on.

#### Nulls added by the corpus run

* **"Fix our window-0 counter so `state` matches" is a null.** n = 2 vanilla
  readings, 7 and 8, on the same corpus and projection. There is no value to
  match.
* **"The offset is a constant 6" is a null.** 6 at 15:30, 5 at 16:44.
* **"The close path might have moved a neighbour" is a null**, and it was
  worth the census to say so: declared and strict are identical to `main`
  `7c919d20` in every field.

---

### SUPERSEDED — 2026-08-11, by branch `corpus-stateid-decoder`

**Appended by a later branch rather than edited, because this file is this
branch's append-only history and the entry above was honest when it was
written.** What follows is only what stopped being true.

The entry above concludes that `611` **"is not adjudicable on `state`, by
either side"** and that it **"converts when the state-id decoder lands, not
before"**. The decoder has landed — `compare::rebase_session_counters` now
rebases `container_set_slot.state` onto the scenario, scoped per
`(connection, window)` — so every forward-looking instruction in that entry is
discharged:

* **`ci/checks/gameplay-unstable.toml` no longer declares this check.** Its
  entry, and the two others on the same field
  (`swapping-to-the-offhand-shows-on-both-channels`,
  `a-flower-pot-empties-when-right-clicked`), were retired in one commit. The
  sentence "as that file already says" no longer describes that file.
* **"Do not re-run the corpus hoping for a different answer, and do not move
  `611`'s baseline" is spent.** It was correct advice *for a tree without the
  decoder*. The measurement that supersedes it is a corpus run on the branch
  named above; whatever that run says about `611` is the current answer, and
  the baseline moves only if that run — not this note — says so.
* The `state` field is adjudicable now. It is compared as each packet's
  distance from the first one on that menu **in the check's own projection**,
  so a uniform offset inherited from earlier scenarios is discarded while the
  gap between two packets on one menu is kept.

Paired evidence, same six recordings, one variable: this check went **9/15
pairs to 0/15** and the corpus floor **7/92 to 4/92** over ensemble
`coverage92-selfcheck-n6-20260811`. Projection `proj:1e154ef8765b7525` ->
`proj:7cefde0fb56d9f3e`.

Full write-up: `docs/cold-start/corpus-stateid-decoder.md`.
