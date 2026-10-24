# Cold start — branch `gameplay-block-interactions`

Append-only. A new `### COLD START` entry per turn on this branch; nothing
above is edited.

---

### COLD START — branch `gameplay-block-interactions`, off `main` (`7fa6b7d8`), 2026-08-11

**Item: three block-interaction checks vanilla answers and we do not — `701`
the fence gate turning to face you, `710` the daylight detector inverting,
`790` the flower pot emptying.** Worktree `~/grit/work-blocks`, remote dir
`~/rustmc-blocks`. Not merged — the orchestrator gates.

**THE BRIEF WAS REFUTED ON TWO OF THE THREE AND THE REFUTATIONS ARE THE
FINDING.** Both came off one artefact, `~/rustmc-stack/oracle-work/gameplay/`
of 2026-08-11 19:22 — the recordings and the `report.json` of the gate that
produced `declared 72/92, strict 68/92` — read before any code was written.
Neither cost a corpus run to discover.

1. **The daylight detector does not diverge on `power` and vanilla does not
   read 7.** Briefed as "ours `power=0` where vanilla has `power=7`". The
   recording says both of vanilla's `block_update`s at the detector's own
   position `[1,0,0]` are
   `minecraft:daylight_detector[inverted=true,power=0]` — the third
   `block_update` on that stream is `minecraft:air` at `[1,1,0]`, the resync of
   the position a placement would have taken — and ours is
   `[inverted=false,power=0]`. **The divergence is `inverted`, and `power` is
   0 on both sides.** That matters for scope: the briefed reading implies
   `updateSignalStrength` has to be ported to convert the check, and it does
   not — vanilla's own recomputation returns 0 here for both `inverted`
   values, so it writes nothing and the check is decided entirely by the
   `INVERTED` cycle.

   **Three independent cold-vanilla recordings agree, which is ORACLE rule 6's
   bar and not one lucky run.** `~/rustmc-stack`, `~/rustmc-stateid` and
   `~/rustmc-cov3`'s `oracle-work/gameplay/vanilla.json` each put exactly
   `minecraft:daylight_detector[inverted=true,power=0]` on both of the actor's
   `block_update`s at `[1,0,0]`, and each puts
   `oak_fence_gate[facing=north,in_wall=false,open=true,powered=false]` on
   `701`'s. No recording on the box carries `power=7`.
2. **The fence gate's missing `sound` is not a second defect.** Briefed as
   "two separate defects on one scenario … fixing the facing alone will not
   convert it". `700-a-fence-gate-opens-when-right-clicked` is `match` on
   **both** the declared and the strict projection in that same report and
   carries the identical `missing_kinds: ["sound"]`. So the advisory is
   one-sided by construction — `FenceGateBlock` plays through
   `Level.playSound(player, …)`, which excludes the actor, and both checks
   `observe_from = "actor"` — exactly as `701`'s own header already says.
   `701` converts on `facing` alone.

   **And the tree already said so, in two places, before this branch existed.**
   `docs/PARITY-ROADMAP.md` §20.7 records the same finding from `700`'s own
   falsification run — "It cannot see that sound", with the offsets — and
   `701`'s check file carries the paragraph. This is the shape
   `ci/checks/gameplay-unstable.toml`'s header warns about: something briefed
   as a live defect that the tree had already measured and written down. The
   reading above is an independent second one; the credit is the roadmap's.
   §20.7 also gives `710` as "we emit one fewer `block_update` than vanilla
   (2 against 3)" — the same fact as `reference_obs 4, subject_obs 3` counted
   without the ack — and it does **not** say `power=7` anywhere, so that
   figure entered with the brief rather than from the tree.

   §20.7's `701` bullet is also the diagnosis this branch acts on, and it was
   right: *"not merely unimplemented, it is **inexpressible in the table's
   shape**, and what the generated `use_without_item.json` holds instead is the
   probe player's own direction baked in as a constant. That constant is
   `south`."* This branch adds the count — 48 of 384 rows, and
   `FenceGateBlock` is the only class in the game it can happen to.

**`ci/checks/gameplay-unstable.toml`, checked first as the brief requires.**
The fence gate is **not listed** and never has been (the only `fence` string
in the file is `fences-connect-to-each-other`, a different check, in an
archived block). The daylight detector is **deliberately not listed**, with a
measurement saying why — 0/15 pairs on the corpus ensemble against 5/15 on a
five-check subset, and the entry that was drafted from the subset would have
been red the day it landed. The flower pot **was** listed and was retired the
same day (`d6112fd9`) with the state-id decoder; its verdict is still
`diverge` for an unrelated reason.

**WHAT WAS BUILT** — one commit, `891617de`.

`crate::useblock::hand_ported` is a new arm consulted **ahead of** the
state-keyed table, holding exactly the classes `data/vanilla/use_without_item.json`
cannot express. Two are in it and they fail in opposite directions:

* **`FenceGateBlock` has a row for all 384 of its states and every row is a
  reading of the probe's own player.** Offsets 43-77 of `useWithoutItem` are
  `Direction d = player.getDirection(); if (state.getValue(FACING) ==
  d.getOpposite()) state = state.setValue(FACING, d);`. Measured off the
  checked-in table rather than inferred: of those 384 writing rows exactly
  **48** change `facing`, and all 48 are `facing=north,open=false ->
  facing=south`. The probe faced south. The table is right for a player facing
  south and wrong for the other three quarters — a shape the table does not
  have, not a stale table. `setBlock` flags 10.

  **The defect is bounded and that is measured, not assumed.** All 405 classes
  under `net/minecraft/world/level/block/` in the pinned jar were disassembled
  and each `useWithoutItem` body scanned for calls on its `Player` parameter:
  exactly one names `getDirection`, and none names `getYRot` or
  `isSecondaryUseActive`. It is `FenceGateBlock`. So those 48 rows are the
  whole of "a table row that is really a reading of the probe's own player",
  and no other block in `use_without_item.json` needs re-examining for it.
  (Many classes touch the player for `openMenu`, `awardStat` or `eat`; none of
  those decides the state written, which is why the scan is on the
  orientation getters and not on `aload 4`.)
* **`DaylightDetectorBlock` has no row at all.** All 32 states are
  `refused: UnsupportedOperationException … getChunkSource`. The `INVERTED`
  cycle at offsets 27-47 is ported, with `setBlock` flags **2** — the flag
  that a fall-through to this module's `UPDATE_ALL` default would get wrong,
  because vanilla deliberately does not update neighbours there.

`use_sound` gains a fourth family, `FenceGateBlock` (offsets 102-162), with
the `WoodType.<clinit>` split read off the jar: cherry, nether for crimson and
warped, bamboo, and plain for the other eight. It excludes the clicker and is
**expected to buy no verdict** — see refutation 2. `chest_pitch` became
`nine_tenths_pitch` because the gate writes `nextFloat() * 0.1f + 0.9f`
character for character.

**WHAT WAS NOT PORTED, DELIBERATELY.**
`DaylightDetectorBlock.updateSignalStrength` (offsets 0-148) is written out in
`crate::useblock`'s header and **not run**. Every input on its first two lines
— `Level.getEffectiveSkyBrightness` and the `SUN_ANGLE` environment attribute
— is a light and sky model this server does not have, and inventing one is the
one-way door `docs/AGENT-BRIEF.md` refuses. The consequence is stated in the
module: **this server's daylight detector never changes its `power`**, on the
click or on the block entity's 20-tick ticker, which is also absent. It agrees
with vanilla on the corpus's superflat because vanilla measures 0 there both
inverted and not. A check at a resolving sun angle would separate them.

**790 WAS NOT ATTEMPTED, AND THE REASON IS A CORRECTION TO THE BRIEF, NOT A
SHORTFALL OF EFFORT.** The brief names `FlowerPotBlock.useWithoutItem` as the
unported mechanism. **Porting it alone converts nothing**, and the measured
reason is in the check's own setup:

* offset 0 of `useWithoutItem` is `if (isEmpty()) return CONSUME`, and
  `isEmpty()` is `this.potted == Blocks.AIR`;
* our pot is **empty at the moment of the click**, because the setup's potting
  step is `FlowerPotBlock.useItemOn` — a different method — and this server has
  **no `useItemOn` extraction of any kind**. `data/vanilla/` has 60-odd tables
  and none of them is a `use_item_on.json`; `use_without_item.json`'s row for
  `minecraft:flower_pot` is `consumes: true, writes: []`, so our server
  consumes the dandelion click and writes nothing;
* which is why `790`'s first difference is at index **2** and not index 0. Our
  first two observations agree with vanilla's by coincidence: vanilla's pot
  ends as `minecraft:flower_pot` after emptying and ours was never anything
  else.

So `790` is a **new golden axis plus two ports plus `Player.addItem`**, not a
port. Sized, so the next agent does not re-derive it:

**AND THE REFUSED SET IS SMALLER AND SHARPER THAN "199 STATES" SUGGESTS.**
Counted off `use_without_item.json` **as it stood when this was written**, the
199 refused states were **56 blocks and exactly three causes**:

```
   39 states   39 blocks  NullPointerException: Components not bound yet
                          38 potted flower pots + minecraft:composter
  128 states   16 blocks  UseGolden's level was asked for environmentAttributes
                          the 16 beds
   32 states    1 block   UseGolden's level was asked for getChunkSource
                          minecraft:daylight_detector
```

> **BOTH OF THOSE ARE CLOSED. Re-derived 2026-08-18 by
> `ci/refusal-ledger.py`: 160 refusals, 17 blocks, two causes** — 128
> `environmentAttributes` (the 16 beds) and 32 `getChunkSource`
> (`daylight_detector`, itself closed on the consumer side by
> `useblock::hand_ported`). The 39 `Components not bound yet` states are gone:
> **all 39 `FlowerPotBlock` blocks and `minecraft:composter` are in the table
> now**, so the probe fix named below as "the cheapest thing left in this
> module" has been made and there is nothing there to do.
>
> The middle row of the block above was also **printed as 160 rather than
> 128** for a week, in this file and in two others. 39 + 128 + 32 = 199, which
> is the header this table sat under; when the first cause closed, the new
> total was written into the `environmentAttributes` row and the header was
> left alone, so three numbers described one table and nothing could compare
> them. The row is restored to 128 here and `ci/refusal-ledger.py` now counts
> every population off the rows *and* off the generator's summary and refuses
> when the two disagree.
>
> **The cheapest thing left in this module is not a refusal at all.**
> 13 of the jar's 54 `useWithoutItem` overrides have **no row**, covering 90
> registry blocks (48 signs, 17 candle cakes, 13 fences), and all 13 are the
> harness answering air and `null` rather than throwing. See
> `ci/probe-absences.py` and `docs/cold-start/refusal-ledger.md`.

This branch closed the `getChunkSource` cause **entirely** — one block, all 32
states, by hand.

1. `POTTED_BY_CONTENT` — `FlowerPotBlock`'s constructor is
   `this.potted = content; POTTED_BY_CONTENT.put(content, this)`, so one map
   serves both directions. Of the **39** `FlowerPotBlock` blocks, **38 are
   `refused` in `use_without_item.json` and all 38 for the same reason**,
   `NullPointerException: Components not bound yet` — thrown by
   `new ItemStack(this.potted)` at offset 11. Only `minecraft:flower_pot`
   itself is in the table. Fixing the component binding in
   `tools/goldengen/vanilla/UseGolden.java` would put the *emptying* block
   write into the existing table for all 38 at once; it would still not give
   the item handed back, nor the potting direction.

   **And a table already in the tree recovers the map, which may mean no new
   golden at all.** `data/vanilla/loot_blocks.json` gives every potted block a
   loot table of exactly two item pools, `minecraft:flower_pot` first and the
   content second — checked, **38 of 38, zero exceptions**, and `flower_pot`
   itself drops only `minecraft:flower_pot`, which is `isEmpty()` falling out
   the same way. That is a datapack correspondence and `POTTED_BY_CONTENT` is a
   code map, so agreeing on 38 of 38 is evidence and not identity; the cheap
   cross-check is that once the probe's components bind, every potted row in
   `use_without_item.json` must read `writes: [{state: minecraft:flower_pot}]`,
   which pins the other half of the same pair. Do that check before trusting
   the loot route, and say which one you used.
2. `FlowerPotBlock.useItemOn` (offsets 0-113) and `useWithoutItem`
   (offsets 0-66). Both small and both fully read.
3. `Player.addItem` — vanilla's `container_set_slot` here is window 0,
   **slot 38**, item digest `64371cd9cc8ac4c2`, and it arrives with no
   `set_player_inventory` beside it, so it is `Inventory.add` picked up by the
   next `broadcastChanges` sweep and **not** `placeItemBackInInventory`, which
   is the only inventory-write path this server has and which sends
   `set_player_inventory` from inside its own loop.

Of the 39 blocks that share the implementation, this branch covered **none**.

**THE CORPUS RUN AND WHERE TO READ IT.** `ci/t2-gameplay.sh` in
`~/rustmc-blocks`, log at `~/rustmc-blocks/t2.log`, artefacts at
`~/rustmc-blocks/oracle-work/gameplay/{vanilla,rustmc,report}.json`. It queued
behind two other trees' runs for most of the evening — `~/rustmc-stack`'s merge
gate and then `~/rustmc-swap`'s run — which is the box-wide corpus lock working
as designed. **The first attempt died at 92/92 vanilla + 8/92 rustmc when its
ssh channel was killed**, taking the remote job with it; the relaunch runs
under `setsid nohup` on the box precisely so the next channel death cannot do
it again. Read the verdict out of `t2.log`'s `CORPUS declared` / `CORPUS strict`
lines and the per-scenario `first_difference` out of `report.json`.

**The three verdicts to read, and what each one means:**

* `701` — expect `diverge -> match`. It converts on `facing` alone. If it is
  still `diverge`, read the field: `facing=north` means the port did not run
  (check `hand_ported` is reached before the table); anything else is a second
  defect this branch did not find.
* `710` — expect `diverge -> match`, and expect **four** actor observations
  where we had three, the new one being the `setBlock(flags 2)` broadcast.
* `790` — expect `diverge`, unchanged, at index 2. It is not touched by this
  branch and the reason is above.
* `700` and every other check — expect **no** `WORSE` line. The fence-gate
  sound is new and goes only to the observer, and both fence-gate checks read
  the actor.

Only `700`, `701` and `710` name a fence gate or a daylight detector anywhere
in `ci/checks/gameplay/`, so the blast radius of this branch is exactly three
scenarios; a `WORSE` on a fourth would be a finding about `use_sound`'s
routing, not about these ports.

**THE FLOOR IS STALE AND WAS NOT TOUCHED.** `ci/checks/gameplay-floor.toml`
stamps `coverage92-selfcheck-n6-20260811` at `proj:b385528330fee646`; this
branch compares under `proj:7cefde0fb56d9f3e`, so every run printed
`STALE PROJECTION`. `ci/corpus-floor.sh --update` was not run — another agent
owns the re-measurement.

**THE RIDER WAS NOT TAKEN, AND THAT IS THE BRIEF'S OWN CONDITION.**
`ci/checks/gameplay-baseline.toml` still reads
`container-click-pickup-all-gathers-matching-stacks = "diverge"`, and it is
**not** locked in on this branch. Both gate logs do say `BETTER … was diverge,
is now match` — 15:30 `gate-sulfur-cinnabar-r19-20260811-143633.log:585`
(declared 71/92) and 20:05 `gate-corpus-stateid-decoder-20260811-184803.log:581`
(declared 72/92) — so the two-independent-runs bar is met by those two. The
brief's condition is that **this branch's own run must also read it as
`match`**, and this branch's run had not produced a verdict by handoff. Locking
it in on two other trees' evidence is exactly the substitution the rider was
written to prevent. Read it out of `~/rustmc-blocks/oracle-work/gameplay/report.json`
and lock it in **in its own commit**, quoting all three logs.

(For the next reader: the 20:05 gate carries a *second* `BETTER` line,
`closing-a-crafting-table-returns-the-grid-contents`, which the brief did not
mention. That one has **one** run behind it, not two, so it does not clear
rule 6's bar and was left alone.)

**T0 PROVENANCE OF THE TIP, STATED EXACTLY.** `T0 GREEN` was measured on the
Threadripper in `~/rustmc-blocks` at commit `891617de` — the only commit on
this branch that changes behaviour — 787 lib+bins tests, 0 `skipping: no
vanilla`. Every commit after it is doc-comment or `#[cfg(test)]` only. Those
were checked in a throwaway tree (`~/rustmc-blockschk`, since removed) rather
than in `~/rustmc-blocks`, because the corpus run owns that directory and an
rsync into it would have replaced a running job's tree: `cargo fmt --check`
clean, all 13 `useblock::tests` pass including the two new table-pinned ones,
539 `rustmc-server` lib tests pass. The one failure there was
`anvil::real_vanilla_chunks_survive_a_rewrite`, which needs an `oracle-work`
the throwaway tree did not have. **A full T0 on the tip still has to be run**,
and the merge gate runs one.

**NEXT ITEM.** `790`, in the order above, and the result that closes it is
`a-flower-pot-empties-when-right-clicked` reading `match` with five actor
observations, the last two being `container_set_slot` window 0 slot 38 and
`block_changed_ack`. The thing that would make it look closed while it is not:
implementing `useWithoutItem` first and reading the unchanged verdict as "the
port did not work" — it cannot work until `useItemOn` pots.
