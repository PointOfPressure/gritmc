# Cold start — branch `gameplay-item-use-group`

---

### COLD START — branch `gameplay-item-use-group` off `53cf6424`, 2026-08-11

Four scenarios were briefed as one group. They are **three mechanisms, not
two**, and only one of the three was in scope. Remote dir `~/rustmc-itemuse`
on the Threadripper.

| # | scenario | before | after | what it turned out to be |
|---|---|---|---|---|
| 310 | `hoe-turns-grass-into-farmland` | diverge | see below | the missing `ItemStack.useOn` dispatch — **ported** |
| 320 | `flint-and-steel-lights-a-fire` | diverge | see below | the same missing dispatch — **ported, same commit** |
| 430 | `a-note-block-retunes-when-right-clicked` | diverge | diverge | a `placement.json` extraction defect, **now pinned**, plus two ports — NOT started |
| 070 | `sand-falls-when-unsupported` | diverge | diverge | an entity lifecycle this server does not have — **DROPPED, deliberately** |

## The briefed ordering was backwards, and this is the correction

The brief said `310` and `320` were "an item's own `useOn` running **before**
the block's `useWithoutItem`, which is the ordering vanilla uses". It is not.
`ServerPlayerGameMode.useItemOn` is one straight line and the item is **last**:

```text
  126      if (secondary) goto 223                      // sneak + holding
  145      r = state.useItemOn(stack, level, player, hand, hit)
  152-157  if r.consumesAction()                 -> r
  179-187  if r instanceof TryEmptyHandInteraction && hand == MAIN_HAND
  196      r2 = state.useWithoutItem(level, player, hit)
  203-208  if r2.consumesAction()                -> r2
  223      if stack.isEmpty() || onCooldown      -> PASS
  245      ctx = new UseOnContext(player, hand, hit)
  275/292  r3 = stack.useOn(ctx)
  299-304  if r3.consumesAction()                -> r3
```

`javap -p -c -constants` on the pinned `server-26.2.jar`. The block gets first
refusal and the item gets what is left, which is the order
`crate::useblock`'s header already described and this server already
implemented. **Nothing was wrong with the ordering.** What was missing is that
vanilla reaches the item by *virtual dispatch on the stack* — `BlockItem.useOn`
calls `place(...)`, `Item.useOn` returns `PASS`, and `HoeItem` and
`FlintAndSteelItem` override it — and this server had only the `BlockItem` arm,
inlined into `handle_use_item_on`. A hoe and a flint-and-steel each fell into a
placement that had no block to place and wrote nothing.

So the brief's *conclusion* held — one fix converts both — for a different
reason than the one it gave. `crates/rustmc-server/src/itemuse.rs` is that
dispatch.

## What was checked before starting, because the brief required it

* **`ci/checks/gameplay-unstable.toml`: none of the four is listed.** It
  carries five live `[[unstable]]` entries — `spawn-egg-makes-a-mob`,
  `a-dropped-item-goes-into-your-inventory`, `water-spreads`,
  `dropping-an-item-throws-it-on-the-floor`,
  `a-player-that-leaves-the-ground-is-resynced` — and none of ours. Both
  `sand-falls-when-unsupported` and `a-note-block-retunes-when-right-clicked`
  appear in that file **only in prose**: 070 as the one entry ever removed
  because its defect was *fixed* rather than declared, and 430 as a check that
  walked into the `ClientboundSoundPacket` seed and was rescued by a decoder.
  Neither is a live declaration and neither blocks anything.
* **`docs/PARITY-ROADMAP.md` §19.2** files 310 and 320 under "the whole
  `Item.useOn` dispatch layer", shared with each other. That was right.

## 070 — dropped, and why that is the answer rather than a shortfall

`FallingBlockEntity` needs three things this server does not have, and they are
not one thing in three places:

1. **A `ShapeTable` hole.** `crate::shapes`' own header (offsets read
   2026-08-04) records that `FallingBlock.updateShape` schedules a tick and
   returns its own state, so the extract has **zero `minecraft:sand` rows** —
   discovery drops a `(state, direction)` pair whose answer is itself.
2. **A `FallingBlock.tick` behaviour.** `crate::ticks` ports exactly one
   block's tick (`ButtonBlock`).
3. **An entity that exists between two ticks.** `crate::entities` can *encode*
   an `add_entity`, and `crate::dropitem` spawns one, but only as a player's
   own throw with no lifetime: nothing in this server gives an entity gravity,
   collides it with the ground, and turns it back into a block.

**Item 3 is the whole item, and the roadmap already says so in a figure worth
quoting rather than re-taking:** 070's blocker is "`FallingBlockEntity` plus
**eight `block_update` against our two**" (§ the S4 negative-control block).
So an `add_entity` on its own converts nothing — the six extra block updates
are the sand travelling and landing. And 070's own `add_entity` is
byte-identical across three vanilla recordings (`kind_id 51`,
`at [1.5, 1.0, 0.5]`), so this is a scenario that *is* gradeable and simply is
not cheap.

That is an entity lifecycle, and the brief's instruction was to say so and drop
rather than grow the branch. **Dropped.** The next agent should not read this
as "nearly done": item 3 is the item, and `crate::physics::Profile` already
knowing `minecraft:falling_block`'s gravity is groundwork, not progress.

## 430 — not started, but the blocking defect is now pinned rather than named

The roadmap said 430's first difference is "a `placement.json` instrument
defect (`basedrum` vs `harp`)". That was a description; this is the
measurement, taken by decoding the table's own cells:

```text
minecraft:note_block  cells by face [DOWN UP NORTH SOUTH WEST EAST]
                      = [0, 1, 0, 0, 0, 0]
                      states[0] = note_block[instrument=harp,...]
                      states[1] = note_block[instrument=basedrum,...]
                      neighbourSensitive = false
```

**The instrument is a function of the block below the placed position, and the
table has recorded it as a function of the clicked face.** `PlacementGolden`'s
`TableLevel.reset()` puts one stone at the clicked position and air everywhere
else, so the UP face is the only one that puts the note block on top of that
stone — `NoteBlock.setInstrument` reads `pos.below()`, gets `stone`, and
answers `BASEDRUM`. Every other face lands above air and answers `HARP`. In the
corpus the block below is the superflat surface, so vanilla says `harp` and
this server says `basedrum` for the UP face regardless of what is under it.

**And the sensitivity detector cannot see it.** `neighbourSensitive` is
detected by re-running one cell with a **stone ring** — horizontal — so a
`pos.below()` or `pos.above()` read is invisible to it. Swept the whole table
for the signature (cells constant within every face, and exactly one of UP or
DOWN differing from the other five, with `neighbourSensitive = false`) and it
matches **two blocks: `minecraft:note_block` and `minecraft:scaffolding`.**
Every other block with that shape — `farmland`, `dirt_path`, the seven tall
plants — is already flagged sensitive. So this is a two-row defect with a
one-line cause, not a systemic one.

Converting 430 is therefore **three** pieces of work and none of them is the
one §4 files it under:

1. **The instrument.** `BlockState.instrument()` is a per-block property; a
   column in `block_props.json` (`BlockPropsGolden.java`, beside
   `redstoneConductor`) is the cheap half. Then `NoteBlock.setInstrument` —
   above first, then below, `worksAboveNoteBlock()` deciding both — has to run
   instead of the table row, and `NoteBlock.updateShape` has to re-run it on
   any Y-axis neighbour change.
2. **`playNote` → `block_event`.** `NoteBlock.playNote` is
   `if (INSTRUMENT.worksAboveNoteBlock() || level.getBlockState(pos.above()).isAir())
   level.blockEvent(pos, this, 0, 0)`. Both event bytes are `iconst_0`.
   `crate::containers::BlockEvents` already carries this packet for chests.
3. **The sound**, out of `NoteBlock.triggerEvent`:
   `getPitchFromNote(note) = (float) Math.pow(2.0, (note - 12) / 12.0)`, and
   the event off `NoteBlockInstrument.getSoundEvent()` — a 27-member enum.
   Gated on `isTunable()`; an untunable instrument pitches at `fconst_1`.

Start it from a `--json` diff, not from this list.

## Named gaps inside what *was* ported

All in `crates/rustmc-server/src/itemuse.rs`'s header, so they are not
re-derived: `FlintAndSteelItem`'s campfire/candle/candle-cake lighting branch
(needs `BlockTags.CAMPFIRES` / `CANDLES` / `CANDLE_CAKES` — see the warning
below); `FireBlock.getStateForPlacement`'s connection walk
(needs `getIgniteOdds`, a 175-entry map from `FireBlock.bootStrap()`);
`BaseFireBlock.isPortal`; `rooted_dirt`'s `hanging_roots` pop; `hurtAndBreak`.

**The flint-and-steel sound is deliberately not emitted.** Its pitch is
`level.getRandom().nextFloat() * 0.4F + 0.8F`, a draw from a level-wide
`RandomSource` this server does not model. `ctx.getPlayer()` is excluded from
it — `Level.playSound`'s player-excluding overload — so no scenario in this
corpus can hear it either way, and a plausible-but-wrong number on the wire is
the failure mode `crate::useblock` argues against for its 181 silent blocks.
The hoe's is emitted: `fconst_1`, `fconst_1`, exact.

## An unstamped table is sitting in the shared `data/vanilla`

Found while sizing the lighting branch, and it is not this branch's to fix:
**`data/vanilla/tags_block.json` exists on the box, carries `campfires`,
`candles`, `candle_cakes` and `soul_fire_base_blocks` among 265 tags, and is
produced by nothing in this tree.** No file under `ci/`, `tools/` or `crates/`
mentions it; `ci/gen-gameplay-tables.sh`'s `generator_for` has no arm for it,
so it is neither generated nor required nor provenance-checked; and it carries
no stamp key. It reached `~/rustmc-itemuse/data/vanilla` through the
`cp -rn ~/rustmc/data/vanilla/.` that every tree's setup runs.

Reading it would have made this branch's server depend on whichever branch
produced it — the 2026-08-03 shared-`oracle-work` failure with a different
path. **Not read.** Whoever lands the branch that wants block tags should add
the `generator_for` arm in the same commit as the first `crates/` reference,
or the provenance preflight will keep passing on a table it cannot check.

**Seven hoes, not six.** `copper_hoe` is new in 26.2. Counted twice — seven
`new HoeItem` sites in `Items`' disassembly, seven ids ending `_hoe` in
vanilla's own registry report — because a hardcoded list is exactly the thing
that silently loses a member.

**`onlyIfAirAbove` is on `rooted_dirt` and nowhere else.** The disassembly
prints both `TILLABLES` predicates as `InvokeDynamic #N:test`; only the
`BootstrapMethods` table separates `HoeItem::onlyIfAirAbove` from
`lambda$static$0`, whose whole body is `iconst_1; ireturn`. Reading `javap -c`
without `-v` makes all five rows unconditional, and a hoe would then till grass
through a block. The unit test
`only_rooted_dirt_carries_the_air_above_predicate` is that reading pinned.

## Nulls — do not re-run these

* **The ordering hypothesis.** `ItemStack.useOn` before `useWithoutItem` is not
  vanilla's order and never was. Offsets above.
* **`gameplay-unstable.toml` for any of the four.** Checked, none listed, and
  the two prose mentions are history.
* **`FireBlock`'s flammability table for `320`.** Not needed. A sturdy face
  below satisfies both `canSurvive` and the connection guard without any
  `canBurn` call, and `320` puts fire on top of stone.
* **`neighbourSensitive` as a whole-table problem.** Swept: two rows.

## The single next item

**430**, in the order above — the `block_props.json` instrument column first,
because until it exists the check's *first* differing observation is a setup
step and any work on `block_event` is unmeasurable behind it.
