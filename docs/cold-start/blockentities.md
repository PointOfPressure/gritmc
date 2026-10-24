# Cold start — branch `blockentities`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `blockentities` off `d198028`, 2026-08-05

**The server can now keep an item, and compute one.** Block entities,
chest storage, the whole container click path and a crafting recipe
engine. Not merged — the orchestrator gates. Remote dir
`~/rustmc-blockentities` on the **Threadripper**.

    CORPUS  declared  41/70 -> 43/70 match
                      (21 diverge, 6 absent, 0 vacuous, 0 inert, 0 error)
    GAMEPLAY GREEN, T2-GAMEPLAY GREEN

    560-storing-an-item-in-a-chest                  diverge -> match
    561-clicking-an-empty-chest-slot-changes-...      match -> match  (held)
    562-a-crafting-table-turns-a-log-into-planks    diverge -> match

Full `ci/t2-gameplay.sh` on the Threadripper, ports 25936/25937, log
`~/rustmc-logs/blockentities-gate.log`. Adjudicated against
`ci/checks/gameplay-baseline.toml`: **2 better, 0 worse**, and the two
that moved are exactly the two this link set out to move. 562 is a
**strict** match too. T0 GREEN throughout; the baseline is ratcheted.

**The "before" is 41, not the 38/70 written at line ~732 of this file.**
That figure was recorded by an earlier link and the corpus has grown and
been improved since; 41 is what `gameplay-baseline.toml` itself declares,
measured over the same 70 checks in the same run, and is the only
before/after pair here that is like-for-like.

**The gate ran on ports 25936/25937 because the default 25896 collided.**
The first attempt died at startup — "FAILED TO BIND TO PORT ... Address
already in use" — with two other agents' gates on the box. It is a
one-line crash in `oracle-work/gameplay/vanilla/server.log` and it looks
nothing like a gate failure, so: **when more than one tree is gating,
set `T2_GAMEPLAY_VANILLA_PORT` and `T2_GAMEPLAY_RUSTMC_PORT`.**

**And a crashed vanilla leaks a process that never exits.** When that
attempt died, `ci/vanilla-server.sh stop` was left running for two hours
and had to be killed by hand. It writes `stop` into `console.fifo`, and
a server that has already crashed is not reading it, so the write blocks
for ever — taking the whole `t2-gameplay.sh` shell with it. Nothing shows
up in any log. **After any gate that did not print a `CORPUS` line, check
`pgrep -af vanilla-server.sh` before walking away**, or the next agent
inherits a stuck process. Worth a `timeout` around that write; not done
here because `ci/vanilla-server.sh` is shared and another tree was mid-run
against it.

#### What is here

* `crate::item` — `ItemStack`, an item and a count. The count was read off
  `set_creative_mode_slot` and thrown away for as long as nothing could use
  it; `doClick`'s PICKUP branch is almost entirely arithmetic on counts.
* `crate::blockentity` — a position-keyed store with vanilla's four
  lifecycle events, hooked into `World::set_block` rather than into the
  placement and break call sites, because **every** route that changes a
  block goes through `setBlockState` and a hook on the two obvious paths
  leaks a block entity down every other one.
* `crate::click` — `handleContainerClick` and `doClick`'s PICKUP branch,
  transcribed whole, pure (returns `Vec<Reply>`, touches no socket).
* `crate::recipes` — shapeless and shaped over the extracted table.
* Persistence through `save_to`/`load_from`, with a round-trip test.

#### Four things that are not what an older account would say

1. **`sendCarriedChange` is `ClientboundSetCursorItemPacket`, not a
   `container_set_slot`, and it does NOT call `incrementStateId`.**
   `set_cursor_item` is 96, from Mojang's own `reports/packets.json`. A
   server answering the cursor on the old channel runs one state id ahead
   of vanilla for the rest of that menu's life, and the state id is a
   field every container check compares.
2. **`slotsChanged` is triggered by the storage WRITE, not by
   `setChanged`.** In the pinned jar `TransientCraftingContainer.
   setChanged` and `ResultContainer.setChanged` are both a bare `return`;
   `setItem` calls `menu.slotsChanged(this)` at offset 15 and `removeItem`
   at offset 22 **only when the removed stack is non-empty**. Driving the
   recipe off `setChanged` makes a crafting table answer a click that
   moved nothing.
3. **The harness follows the state id through `container_set_slot` as
   well as `container_set_content`.** So a server that sends a different
   NUMBER of packets than vanilla desynchronises it, and the next click is
   answered with the whole menu instead of a correction. Packet count is
   load-bearing for every later click in the run.
4. `slotChangedCraftingGrid`'s `setRemoteSlot(0, result)` at offset 118 is
   what stops `broadcastChanges` sending slot 0 a second time.

#### Nulls — killed explicitly, do not re-run

* **`selfcheck-recompare.sh` cannot measure 560, 561 or 562.** The
  recordings under `oracle-work/gameplay-selfcheck` hold **61** scenarios
  and predate all three; the script reports "9 not present in the
  recordings" and silently drops them. A 25/25 figure for these checks
  does not exist and cannot be produced without a fresh
  `ci/t2-gameplay-selfcheck.sh` run (~12 min x 26). The recompare was run
  anyway and is clean — 5 declared, 5 measured, 56/61 reproduced — which
  says this branch did not perturb the stability of any check that *is*
  in the recordings.
* **560 and 561 report `strict: diverge`, and it is not this branch.**
  561 was already declared-match/strict-diverge before any of this work.
  The strict comparison covers every channel, and the same recompare run
  shows `chest-opens-a-screen` and `a-bystander-sees-a-chest-open` strict-
  unstable at **25/25** — vanilla cannot reproduce the chest's
  `block_event` lid cadence against itself. Both of these scenarios open a
  chest. 562 opens a crafting table, has no lid, and is a strict match.
  Do not chase the chest strict divergence as a defect of this port.
* **`crate::containers::Openers` was deliberately NOT folded onto the
  block entity.** Vanilla hangs the counter off `ChestBlockEntity`, so
  merging would be more faithful — and would rewrite the mechanism behind
  four checks currently at match to buy nothing any check can see. The
  divergence it leaves is written down in `blockentity.rs`: a chest broken
  while open loses its block entity and keeps its openers count.
* **Two equivalent mutants**, found by `ci/container-mutation-sweep.sh`
  and replaced rather than waved through: `from_nbt`'s slot-range check is
  redundant with `set_item`'s own bounds guard, and `ItemStack::new`'s
  normalisation is invisible to `is_empty` because `is_empty` re-derives
  it from the count.

#### The instrument that lied three times, and is now the best evidence here

`ci/container-mutation-sweep.sh` injects 22 plausible bugs into named
branches and requires each to turn a named test red. **22 caught, 0
missed**, against a control of 258 passing tests. It reported `0 caught, 16 missed` twice before that, for two
different reasons, and its control went red once with a mutation that had
been restored in the source and was still alive in the binary:

* `cargo test` prints `error: test failed, ...` when a test FAILS, and the
  compile-error detector was `^error(\[|:)`;
* `cargo test` writes `test <name> ... FAILED`, and the failure scraper was
  anchored `^[a-z_]+::`;
* `cp` + `mv` restores an mtime EARLIER than the mutated file rsync had
  already pushed, so cargo skipped the rebuild.

Rule 7, three times in one script.

Two real defects came out of it, both the same shape — an item silently
ceasing to exist. A placement into a container whose block entity had gone,
and a click on a slot index outside the menu, both reached `safeInsert`,
split the stack off the cursor and handed it to a `set` with nowhere to
write. `may_place` now refuses both. The test that should have caught the
second one clicked with an **empty** cursor, where every arm of `doClick`
is a no-op anyway — defect class #19 exactly.

#### Not in T0, and say so rather than let a green imply coverage

`ci/t0.sh` is `--lib --bins`, so nothing under `tests/` runs in it. The
round trip must be run by hand:

    cargo test --release -p rustmc-server --test block_entity_round_trip

Red with the save side disabled, red with the load side disabled, green
with both — measured, in that order.

#### The single next item

**A furnace that smelts.** `crate::blockentity` already models
`Kind::Furnace` at three slots and `crate::menu` already gives it four
data slots, and every one of them is hardcoded 0 — lit time, lit total,
cooking progress, cooking total. Nothing drives them, so a furnace is a
three-slot chest today. It closes when a scenario puts a raw item and a
fuel in one and the `container_set_data` values move on both servers.

The other unclaimed piece is `AbstractContainerMenu.stillValid`: vanilla
closes a container menu the moment the player walks more than eight blocks
from it, with a `container_close`. This server keeps it open until the next
`openMenu`. It **cannot shift the window-id counter** — `doCloseContainer`
does not touch `containerCounter` — which is the only reason it is safe to
defer, and `menu.rs` carries the note.
