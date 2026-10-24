//! `BlockState.useWithoutItem` — what right-clicking a block does before any
//! item gets a turn.
//!
//! # The ordering this exists to fix, which was an inversion and not a gap
//!
//! `ServerPlayerGameMode.useItemOn` gives the **block** first refusal and
//! only reaches the placement path if the block declined. This server placed
//! immediately and never consulted the block at all, so right-clicking a
//! lever with a block in hand flipped the lever on vanilla and placed the
//! block here. From the bytecode, in order:
//!
//! ```text
//!   if !block.isEnabled(level.enabledFeatures())            -> FAIL
//!   if gameMode == SPECTATOR: openMenu or PASS
//!
//!   holdingSomething = !mainHand.isEmpty() || !offhand.isEmpty()
//!   skipBlock        = isSecondaryUseActive() && holdingSomething
//!   if skipBlock: goto PLACEMENT
//!
//!   r = state.useItemOn(stack, level, player, hand, hit)
//!   if r.consumesAction()                                   -> r
//!   if r instanceof TryEmptyHandInteraction && hand == MAIN_HAND:
//!       r2 = state.useWithoutItem(level, player, hit)
//!       if r2.consumesAction()                              -> r2
//!
//!   PLACEMENT:
//!   if stack.isEmpty() || cooldowns.isOnCooldown(stack)     -> PASS
//!   ...
//! ```
//!
//! Four things a plausible version gets wrong, and the corpus depends on the
//! first:
//!
//!   * **`useWithoutItem` is not gated on an empty hand.** It runs when the
//!     item-aware `useItemOn` returns the marker `TryEmptyHandInteraction`,
//!     which is `BlockBehaviour`'s default, **and** the hand is `MAIN_HAND`.
//!     Every corpus check that right-clicks a block is holding a *stick* and
//!     still gets the block's behaviour.
//!   * **Sneaking only skips the block if you are also holding something** —
//!     `isSecondaryUseActive() && holdingSomething` is a conjunction, so
//!     sneaking with both hands empty still runs the interaction.
//!   * `holdingSomething` reads **both** hands, not the one being used.
//!   * The empty-stack test sits *after* the block interaction, so an empty
//!     hand still reaches `useWithoutItem` and only then falls out at `PASS`.
//!
//! # The answers come from a table, not from fifty hand-written cases
//!
//! This module shipped with `LeverBlock` ported by hand, and said at the time
//! that adding a second block that way was the wrong next move: 51 classes
//! override `useWithoutItem` in 26.2 and no rule connects a block's name to
//! what it does. `ci/use-golden.sh` and
//! `tools/goldengen/vanilla/UseGolden.java` ask each state instead —
//! **247 blocks with behaviour, 7,718 of 32,366 states with something worth
//! recording**, identical across two runs. It read 184 and 5,941 until
//! 2026-08-18; the difference is 90 blocks the probe's own environment had been
//! hiding and a column that says so, not a change in vanilla — and "writing
//! something" is now the wrong name for the second number, because a cell is
//! kept when it opens a screen, edits a block entity, or answers out of an
//! emptiness this world has, as well as when it writes.
//!
//! Three properties of that table shape this module, all measured rather
//! than assumed:
//!
//!   * **Every write is at offset `[0, 0, 0]`** — all 4,850 of them. No
//!     `useWithoutItem` in the game writes a second block, so an entry is one
//!     optional replacement state and nothing more. A door's *other half* is
//!     not written here; it follows from the neighbour update, which
//!     [`crate::shapes::set_block`] carries **with the flags this table
//!     records** — `DoorBlock.useWithoutItem` writes with 10, not with
//!     `UPDATE_ALL`, and the cascade is gated on them.
//!   * **Nothing is face-sensitive.** Every state was probed against two
//!     faces and none disagreed, so the clicked face is not part of the key.
//!   * **One block is random**: `minecraft:dragon_egg` teleports itself. The
//!     extraction records it with no cells rather than writing one sample of
//!     a die roll into a golden file, so it arrives here as "not in the
//!     table" and declines.
//!
//! **1,516 states of 32,366 refused** — the probe read something its level did
//! not model, or vanilla itself said the call was in the wrong place. Those
//! states are absent from the table and so decline here, which is "not
//! covered" rather than "does nothing".
//!
//! **That is 66 blocks and exactly four causes**, and it is derived rather
//! than transcribed: `ci/refusal-ledger.py` counts them off the table's own
//! cells and off the generator's summary field and refuses if the two
//! disagree.
//!
//! ```text
//! 1,344 states  48 blocks  VanillaPaused: Expected to only call this on server
//!                          every sign; SignBlock bails out of its own rule
//!                          because the harness level is not a ServerLevel,
//!                          and says so through Util.pauseInIde
//!   128 states  16 blocks  UseGolden's level was asked for environmentAttributes
//!                          the 16 beds, 8 states each
//!    32 states   1 block   UseGolden's level was asked for getChunkSource
//!                          minecraft:daylight_detector
//!    12 states   1 block   UseGolden's level was asked for a block entity it
//!                          cannot build — minecraft:moving_piston
//! ```
//!
//! **It read 160 over 17 blocks and two causes until 2026-08-18, and the 1,356
//! that arrived were silence before they were refusals.** The signs and the
//! moving piston had no row at all; the level now supplies a real block entity
//! and arms vanilla's own alarm, so what used to look like "declines a
//! right-click" says "not covered". See §80. **A refusal is not a regression** —
//! this module skipped those 49 blocks before and skips them now, and the
//! difference is that a reader can see why.
//!
//! `getChunkSource` is closed by [`hand_ported`] below — the whole cause, not
//! one state of it.
//!
//! **This paragraph was wrong in three ways at once until 2026-08-18, and how
//! it got that way is the argument for the ledger.** It said 199 in its
//! header, 231 across three cause rows, and described a table carrying 160.
//! Reconstructed: 39 + 128 + 32 = 199 was right when it was written. Then the
//! `Components not bound yet` cause was closed, which took the total to 160 —
//! and the edit put the new **total** into the `environmentAttributes`
//! **cause** row, left the header at 199 and left the closed cause in the
//! table. Three numbers, one table, and nothing in the tree could compare them.
//!
//! # The refusals are the smaller half. 90 blocks decline for no reason at all
//!
//! A refusal carries a reason. An absence carries nothing, and this table's
//! absences are the larger population by a factor of nine.
//!
//! `UseGolden.TableLevel.getBlockState` answered `Blocks.AIR` for a position it
//! was not given and `getBlockEntity` answered `null`. A rule that read either
//! took the empty branch, returned `Pass`, and `interesting()` dropped the
//! state — so the block got **no row**, and this module reads no row as a
//! decline and falls through to placement. Right-clicking it put a block down
//! where vanilla does something else. 90 registry blocks: 48 signs, 17 candle
//! cakes, 13 fences and twelve singletons.
//!
//! `ci/probe-absences.py` counts them from the **jar** rather than from the
//! table, because a class the extraction never reached cannot appear in it:
//! **0 of the 54 `useWithoutItem` overrides have no row, covering 0 registry
//! blocks, and none are the probe's environment.** Closed 2026-08-18, branch
//! `probe-answering-environment`; see `docs/PARITY-ROADMAP.md` §80.
//!
//! **What this module now sees that it did not.** The 48 signs are
//! **refusals**, so they are still declines here and that is correct — a
//! refusal is a row this module skips by design. The 18 cakes and the 5 game
//! master blocks carry no cells (`probeOnly`), and the bell and the chiseled
//! bookshelf carry none either (`faceOnly`), so they are declines here too.
//! What changed for this module is the 13 fences, the decorated pot and the
//! jukebox, which now carry cells — and every cell this module accepts is
//! still one with `consumes: true`, so a `reads: ["no-entities"]` fence row
//! answering `Pass` does not enter the table it builds. **The table grew by 90
//! rows and this module's behaviour is unchanged**, which is the intended
//! shape: the extraction stopped lying before the consumer was asked to act on
//! it.
//! The table can already express what those blocks do — it carries `result`
//! and `consumes` per cell, and `minecraft:dispenser` is a row that consumes
//! and writes nothing. What is missing is a block entity in the harness, not a
//! column in the table.
//! **The middle row said 160 until 2026-08-18 and 39 + 160 + 32 is 231, not
//! 199** — the block above it has always said 199 and the two never agreed.
//! The table's own `refusedBy` map answers directly: 128 / 39 / 32, which sums
//! to 199, and the per-state sweep of `table[*].states[*].refused` agrees
//! block-for-block (16 beds, 39 potted-plus-composter, one daylight detector).
//! What the old figure did was fold the detector's 32 `getChunkSource` states
//! into the `environmentAttributes` bucket *and* leave them in their own row,
//! so one cause was counted twice. Nothing downstream had used the 160: the
//! two numbers that travel are 199 and the per-cause split, and the split is
//! what a reader needs to decide whether a refusal is the probe's debt or the
//! crate's.
//!
//! ## This block said 199 / 39 / 160 / 32 until 2026-08-18, and every one of
//! those four numbers had a different problem
//!
//! It is worth writing out, because three separate mistakes were compounding
//! and only one of them was arithmetic:
//!
//! * **the total, 199, was a stale reading.** `golden-useitemon-axis` bound
//!   item components and item tags on 2026-08-12 and the 39
//!   `NullPointerException: Components not bound yet` refusals — 38 potted
//!   plants and `minecraft:composter` — went away with them. `docs/WORKLIST.md`
//!   records that as "199 -> 160" and this block was never updated;
//! * **the 39 was a row for a cause that no longer exists.** `composter` and
//!   the potted plants are in the table now;
//! * **the 160 was the new TOTAL, sitting in the row for one of its two
//!   parts.** That is why `39 + 160 + 32` came to 231 against a stated 199:
//!   the total was double-counted as a component;
//! * **only 128 and 32 were right**, and `docs/SERVER.md` has said 128 since
//!   2026-08-04 — "8 states each, 16 colours — 128 states, which is exactly
//!   the `environmentAttributes` refusal bucket". Three sources now agree.
//!
//! **And the first correction of this block, earlier the same day, was itself
//! taken against a stale table.** It read `~/rustmc/data/vanilla/`, a shared
//! copy of the extract, and got 199 with 39 component refusals — the exact
//! hazard `docs/AGENT-BRIEF.md` describes when it says a shared `data/vanilla`
//! gates every branch against whichever branch last regenerated it. What
//! caught it was this branch's own `ci/gen-gameplay-tables.sh` run producing a
//! different answer. **Re-derive this from the table your own tree generated,
//! never from another tree's copy and never from this comment.**
//!
//! # THE ROW CAN EXIST AND STILL BE WRONG, AND ONE OF THEM IS THIS MODULE'S BUG
//!
//! An absence is a missing row. This is the other thing: a row that exists,
//! carries cells, and was decided by what the harness **is** rather than by
//! what the block does. 8 probe rules gate on `instanceof ServerLevel` over 82
//! registry blocks; `UseGolden.TableLevel` is not one and cannot be, so the
//! rule returns a plausible value without running a line. `instanceof` is
//! answered by the JVM against the object's class, so the level cannot observe
//! the question, refuse it or log it — the fact is derived from the bytecode by
//! `ci/server-level-gates.py` and stamped as a `gated` column naming the fields
//! of that cell the branch decided. 404 cells over 34 blocks on the two probes
//! this module can reach, and 3,197 over 85 once `useItemOn` is counted — but
//! **nothing under `crates/` opens `use_item_on.json`**, and the defect on that
//! side is the arm `crate::world_send::handle_use_item_on` does not have rather
//! than a row it misreads.
//!
//! **Most of it does not reach this module.** This module reads `refused`,
//! `from`, `consumes` and `writes` and nothing else, so the 362 cells that are
//! false in `opens` — every chest, barrel, shulker box and the ender chest —
//! change no behaviour here. Their rows say `consumes: true, writes: []`, which
//! is what this module wants from them anyway.
//!
//! **`minecraft:sweet_berry_bush` does reach it, and is wrong.** For `age > 1`
//! vanilla drops berries, plays a sound and calls
//! `setBlock(pos, state.setValue(AGE, 1), 2)` — all three inside the gate, with
//! the `return SUCCESS` outside it. So the row reads `consumes: true,
//! writes: []`, this module accepts it on `consumes` and applies an empty
//! `writes`, and **the right-click is eaten and the bush stays at `age=3`.
//! Picking berries does nothing.**
//!
//! Two cells, `age=2` and `age=3` — `age=0` and `age=1` never reach the gate,
//! which is why the ledger's guard is `age>1` and not `always`. It is not
//! fixable from the table, because the table cannot contain what the harness
//! never ran; it wants the same treatment `ButtonBlock` and `CoralBlock` got,
//! a hand port derived from the bytecode. Registered rather than quietly
//! carried: `ci/checks/server-level-gates.toml`, verdict `reachable`, and
//! `docs/PARITY-ROADMAP.md` §87.3.
//!
//! # What is still missing, named rather than left to be found
//!
//! * ~~**`isSecondaryUseActive` is not implemented**~~ — **closed
//!   2026-08-10**, and it was the one bit on the input packet this said it
//!   was. It is deliberately **not** in this module: vanilla's gate is at
//!   offsets 101-126 of `ServerPlayerGameMode.useItemOn`, *above* the call to
//!   `useItemOn`/`useWithoutItem`, and it skips both. Putting it inside
//!   [`use_without_item`] would have made a sneaking click look like a block
//!   that declined, which is a different thing — a declining block still
//!   reaches `open_menu_for`. See `crate::world_send::handle_use_item_on`.
//!   The residue is that `holdingSomething` reads both hands and this server
//!   has no offhand stack.
//! * **A scheduled tick's *effect* is still not in the table**, though *when*
//!   it fires now is. The extraction records the delay, the priority, the
//!   position and **the queue** (`tickDelay`, `tickPriority`, `tickAt`,
//!   `tickKind`), so [`UseTable::tick_for`] can drive `crate::ticks` — 20
//!   ticks for a stone button and 30 for a wooden one, measured rather than
//!   written down. What the tick *does* is still hand-ported per class,
//!   because `Block.tick` takes a `ServerLevel` and there is no cheap oracle
//!   for it, and `ButtonBlock` is the only one ported off this table.
//!
//!   **The queue is why `tickKind` exists.** Of the 34 blocks here that
//!   schedule, 22 are trapdoors, and `TrapDoorBlock` has no `tick` override
//!   at all: `toggle` ends with
//!   `if (WATERLOGGED) scheduleTick(pos, Fluids.WATER, ...)`, so those 640
//!   rows are **fluid** ticks. Reading the delay without the queue puts a
//!   block tick for `oak_trapdoor` into a queue that has no rule for it, and
//!   `UNIQUE_TICK_HASH` then makes that entry block a real block tick at the
//!   same position until it drains. `crate::ticks::on_used` refuses them.
//! * **Sounds are produced from the table since 2026-08-17, for the 1,509
//!   states whose pitch is a constant.**
//!   The `sound` column used to be a boolean — `UseGolden`'s probe counted
//!   the `playSound` call rather than reading its argument — so the table
//!   said *that* 184 blocks sound and never *which* sound, and no rule could
//!   connect the two. It now carries [`TableSound`]: the event id, the
//!   category, the volume, who is excluded, and the pitch when the pitch is
//!   vanilla's.
//!
//!   `crate::world_send::use_sound` still answers first from the four
//!   families whose classes have been transcribed — the lever, the button,
//!   the comparator since 2026-08-10 and the fence gate since 2026-08-11 —
//!   because each of those knows something the table does not, and reaches
//!   the table only after them.
//!
//!   **The event id was necessary and was not the whole gap, which is the
//!   part worth remembering.** Of 4,453 sounding states the extract can
//!   fully describe 1,509 across 34 events; the other 2,944 sound with a
//!   pitch vanilla **draws** — every door, trapdoor and fence gate — and
//!   those stay silent, because a door playing a lever's click is worse than
//!   a door playing nothing and a door playing an invented pitch is no
//!   better. Closing them means reading their class, not regenerating the
//!   table. `docs/PARITY-ROADMAP.md`, the block-sounds rows.
//!
//!   **This said "184 blocks sound" and "the other 180 stay silent", and both
//!   were arithmetic on the wrong number.** 184 was the count of blocks with
//!   *behaviour*, which assumes every block that does something on right-click
//!   also plays a sound. Counted off the `sound` column: **71 blocks have at
//!   least one sounding state**, the four transcribed families cover 28 of
//!   them (1 lever, 14 buttons, 12 fence gates, 1 comparator), and **43 stay
//!   silent** — 16 doors, 16 trapdoors, 4 weathering-copper doors, 4
//!   weathering-copper trapdoors, the note block, the composter and the
//!   decorated pot. `docs/PARITY-GAP.md` carried the identical error as "182
//!   blocks that play a sound" and was corrected the same way on 2026-08-18;
//!   this is the third copy of it. The list is a family and a half, not a long
//!   tail. That table change is the one described above, made on
//!   2026-08-17; the counts in this paragraph are of the boolean-era table
//!   and are kept as the record of what the earlier wording got wrong.
//!
//!   The exclusion the old version of this paragraph declined the whole
//!   feature over is real and is now modelled: `ButtonBlock.playSound` passes
//!   `on ? player : null`, so the press is inaudible to the presser and the
//!   release is not, and `LeverBlock.useWithoutItem` passes `null` outright.
//!   See `crate::sounds`.
//!
//! # Two classes the table cannot answer for, and they fail differently
//!
//! [`hand_ported`] is consulted *before* the table and wins where it answers.
//! It holds exactly the classes whose `useWithoutItem` the state-keyed
//! extract cannot express, and the two in it are there for opposite reasons
//! — which is why they are one function with two arms rather than a list:
//!
//! * **`FenceGateBlock` has a row for every one of its 384 states and the
//!   row is a *reading of the probe's own player*.** Offsets 43-77 are
//!   `Direction d = player.getDirection(); if (state.getValue(FACING) ==
//!   d.getOpposite()) state = state.setValue(FACING, d);` — so what
//!   `ci/use-golden.sh` recorded is what one fixed player standing in one
//!   fixed direction saw. Measured, not inferred: of the 384 writing rows the
//!   twelve fence gates have in `data/vanilla/use_without_item.json`, exactly
//!   **48** change `facing`, and every one of them is a
//!   `facing=north,open=false` row rewritten to `facing=south`. The probe's
//!   player faced **south**, `getOpposite()` was NORTH, and only the
//!   north-facing quarter of the table took the branch. A table keyed on
//!   state alone has nowhere to put the other three quarters, so this is not
//!   a stale table but a shape the table does not have — and the rows it does
//!   carry are right for a player facing south and wrong for the other three.
//!
//!   **And it is the only one, which bounds the defect rather than leaving it
//!   open.** Every class under `net/minecraft/world/level/block/` in the
//!   pinned jar — 405 of them — was disassembled and its `useWithoutItem`
//!   body scanned for calls on the `Player`/`LivingEntity`/`Entity`
//!   parameter. Exactly one names `getDirection`, and none names `getYRot`
//!   or `isSecondaryUseActive`: `FenceGateBlock`. So no *other* row in
//!   `use_without_item.json` silently carries the probe's orientation, and
//!   the 48 rows above are the whole of that class of error. Blocks that
//!   touch the player for some other reason — `openMenu`, `awardStat`,
//!   `Player.eat` — are numerous and are not this: none of them decides the
//!   state that gets written.
//! * **`DaylightDetectorBlock` has no row at all.** All 32 of its states are
//!   `refused: UnsupportedOperationException: UseGolden's level was asked for
//!   getChunkSource`, because `useWithoutItem` ends in `updateSignalStrength`
//!   and that reads `Level.getEffectiveSkyBrightness`. The probe's level
//!   cannot answer, so the block declines here and a right-click does
//!   nothing at all.
//!
//! Both are hand transcriptions off `javap -p -c -constants` on the pinned
//! jar, and both carry `Level.setBlock`'s flags **from the call site** rather
//! than from the table: 10 for the gate (offsets 34 and 96), 2 for the
//! detector (offset 43). The detector's 2 is the one that would be silently
//! wrong if it fell back to this module's `UPDATE_ALL` default — vanilla
//! deliberately does *not* update neighbours there, because
//! `updateSignalStrength` does that half itself with its own flag 3.
//!
//! ## What is NOT ported, said out loud
//!
//! `DaylightDetectorBlock.updateSignalStrength` — offsets 0-148 — is
//! **absent**, not approximated. It is
//!
//! ```text
//!   int b = level.getEffectiveSkyBrightness(pos);
//!   float a = environmentAttributes.getValue(SUN_ANGLE, pos) * 0.017453292f;
//!   if (state.getValue(INVERTED)) b = 15 - b;
//!   else if (b > 0) {
//!       float f = a < PI ? 0.0f : 6.2831855f;
//!       a += (f - a) * 0.2f;
//!       b = Math.round(b * Mth.cos(a));
//!   }
//!   b = Mth.clamp(b, 0, 15);
//!   if (state.getValue(POWER) != b) level.setBlock(pos, state.setValue(POWER, b), 3);
//! ```
//!
//! and every input on its first two lines is a light and sky model this
//! server does not have. Writing one of the two `setBlock` arms and inventing
//! the brightness would be the one-way door `docs/AGENT-BRIEF.md` refuses, so
//! the arithmetic is written here and not run. **The consequence, stated so
//! nobody reads a green check as coverage: this server's daylight detector
//! never changes its `power` — not on the click, not on the block-entity's
//! 20-tick ticker, which is also absent.** It happens to agree with vanilla
//! on the corpus's superflat because vanilla measures `power=0` there both
//! inverted and not; it would disagree the moment a check ran at a sun angle
//! that resolves.
//!
//! ### The same missing model is counted three times, in three units
//!
//! Whoever builds this: it is worth more than the daylight detector, and no
//! single sighting of it says so. `ci/refusal-ledger.py` derives all three
//! from the pinned jar.
//!
//! ```text
//!   use_without_item.json    128 states refused on environmentAttributes
//!                                (the 16 beds, 8 states each)
//!   update_shape.json         96 pairs refused on LevelReader.environmentAttributes
//!                                and 212 on BlockAndLightGetter.getLightEngine,
//!                                which is the light half of the same model
//!   this module                the arithmetic above, written and not run
//! ```
//!
//! The `update_shape.json` figures are the sharper of the two.
//! `ci/checks/shape-refusal-census.json` re-asks every refused pair against a
//! level that answers past the neighbour, and **324 pairs survive that — every
//! one of them one of these three causes and nothing else.** So of the 79,113
//! refusals in that table, all but 324 are a world-shaped problem that a wider
//! harness or a three-position extract can reach, and those 324 are a
//! model-shaped one that nothing but a real light and sky model will close.
//! It is the smallest bounded set in either ledger.
//!
//! (The 324 is not 96 + 212 + 8. Those are the causes at *narrow* breadth;
//! widening the world moves eight pairs off a position refusal and onto
//! `getLightEngine`, so the census counts 220 there. Two numbers for two
//! breadths, and quoting one for the other is the mistake this comment made
//! before `ci/refusal-ledger.py --cite` refused it.)
//!
//! Do not build a second sky model to close the beds. Whichever lane builds
//! one closes all three, and `docs/cold-start/refusal-ledger.md` carries the
//! counts so the next reader does not have to re-derive them to find that out.
//!
//! # The `mayBuild`/`canUseGameMasterBlocks` gate — four blocks, one axis each
//!
//! `golden-probe-audit`'s stance sweep found 17 of 184 rows in
//! `use_without_item.json` that move with the *probe's* player rather than
//! with the block — 12 `FenceGateBlock` states above, and five more:
//! `ComparatorBlock`, `DaylightDetectorBlock`, `RedStoneWireBlock` and
//! `RepeaterBlock` on `Player.mayBuild`, `LightBlock` on
//! `Player.canUseGameMasterBlocks`.
//!
//! **One mechanism closes all four `mayBuild` rows**, checked byte-for-byte
//! rather than assumed: `javap -p -c -constants` on the pinned jar shows the
//! identical leading five instructions in `ComparatorBlock.useWithoutItem`,
//! `RedStoneWireBlock.useWithoutItem` and `RepeaterBlock.useWithoutItem` —
//! `aload player; invokevirtual Player.getAbilities; getfield
//! Abilities.mayBuild; ifne <rest>; getstatic PASS; areturn`.
//! `DaylightDetectorBlock.useWithoutItem` opens with `Player.mayBuild()`
//! instead of the field read directly, but that method is itself only
//! `return this.abilities.mayBuild;`, and its failure branch —
//! `invokespecial BaseEntityBlock.useWithoutItem` — resolves no differently:
//! `BaseEntityBlock` does not override `useWithoutItem`, so the call lands on
//! `BlockBehaviour.useWithoutItem`, which is `return PASS` verbatim. All four
//! classes therefore answer the *same* observable thing, `PASS`, when
//! `mayBuild` is false, and [`use_gate`] is the one gate for it.
//!
//! `LightBlock` reads a different bit and answers differently when it fails:
//! `canUseGameMasterBlocks() == false` returns `CONSUME` with **no write**,
//! not `PASS` — checked against `data/vanilla/use_without_item.json` itself,
//! where all 32 `minecraft:light` rows already read `consumes: true, writes:
//! []`. That is the base stance's own answer (never a game master), so the
//! table already carries the false branch correctly and needs no gate; only
//! the *true* branch — `state.cycle(LEVEL)`, `setBlock(pos, next, 2)` — has no
//! table cell, because the probe that built the table was never a game
//! master. [`use_gate`] supplies that one branch and defers to the table for
//! the other.
//!
//! **`may_build` and `game_master` are hardcoded `true`/`false` at the one
//! call site**, [`crate::world_send::handle_use_item_on`], not read off a
//! connected player: this server has no per-player ability model, the same
//! stated limit `DaylightDetectorBlock`'s own port above already carries
//! ("every player this server serves may build"). So today [`use_gate`]
//! answers `None` on every live call and every one of these five rows is
//! answered exactly as before the gate existed — the port changes nothing
//! reachable, on purpose, until a game mode exists to move the constant.
//! [`Gate::Pass`] and [`Gate::Write`]'s `LightBlock` arm are proven by
//! injection in this module's tests, not by any corpus run, because nothing
//! in the corpus can reach them yet.
//!
//! ## What this does NOT close, checked rather than assumed
//!
//! `ci/checks/use-probe-dependent.toml`'s own header says porting a block
//! this way lets its entry be deleted, "at which point the STALE rule
//! fires". **That claim is wrong, verified by doing it rather than by
//! reading it**: `ci/check-use-probe.py`'s `STALE` branch fires only for a
//! toml entry whose block is absent from a *freshly re-probed* table's
//! `probeDependent`/`probeUnknown` set, and that set is a property of the
//! **pinned jar**, re-derived by `UseGolden.java` with no knowledge of
//! anything in this crate. Porting a class here cannot make vanilla stop
//! reading its player. Deleting `ci/checks/use-probe-dependent.toml`'s
//! `comparator` entry and re-running `ci/check-use-probe.py` against a fresh
//! `data/vanilla/use_without_item.json` produces `NEW`, not `STALE`, and
//! that refusal is `ci/use-golden.sh`'s hard exit — which
//! `ci/gen-gameplay-tables.sh` runs for every tree on the box. So the toml
//! entries for all five blocks this section covers are **left in place**,
//! correctly, not by omission. What would actually let an entry be removed —
//! plausibly a `hand_ported`-aware exemption added to `UseGolden.java` or
//! `ci/check-use-probe.py` — is unbuilt and is generator work, not this
//! item's.

use std::collections::HashMap;
use std::io;
use std::path::Path;

/// The result of offering a right-click to the block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Used {
    /// Nothing in the table consumed the click. The caller goes on to the
    /// placement path.
    Pass,
    /// The block consumed the click. `Some` is the state it wrote at its own
    /// position; `None` is a block that consumed and wrote nothing — an
    /// already-pressed button, and 891 states like it. **The distinction
    /// matters**: vanilla consumed the interaction either way, so the
    /// placement path must not run, and treating this as a `Pass` would
    /// place a block where vanilla places none.
    Consumed(Option<String>),
}

/// One state's answer.
#[derive(Debug, Clone)]
struct Entry {
    to: Option<String>,
    /// The flags vanilla wrote with. **Not `UPDATE_ALL` for everything**:
    /// `DoorBlock.useWithoutItem` uses 10 (`UPDATE_CLIENTS |
    /// UPDATE_IMMEDIATE`) and `ButtonBlock.press` uses 3, and
    /// `Level.setBlock` gates the shape cascade on these. This module used
    /// to hard-code one number for all 5,941 writing states.
    flags: i32,
    schedules_tick: bool,
    /// When and at what priority it asked to be woken. `None` when it did
    /// not ask.
    tick: Option<crate::placedby::Tick>,
    /// The sound this state's rule played, when it played exactly one.
    /// `None` for a silent rule **and** for a rule whose sound the table
    /// cannot name; see [`TableSound::usable`] for the difference.
    sound: Option<TableSound>,
}

/// One row's sound, as `UseGolden` recorded it.
///
/// # Why this is not just an event id
///
/// A packet needs five things and the event id is one of them. `volume` and
/// the ordinal behind `source` are constants of the rule and come straight
/// off the extract. The two that do not are the interesting ones:
///
/// * **`pitch` is `None` when vanilla drew it.** `DoorBlock.playSound` takes
///   its pitch from `level.getRandom().nextFloat() * 0.1F + 0.9F`, and the
///   extraction harness fixes that seed, so the value it observed is a
///   constant of `RandomSource::create(0)` and not vanilla's. The extract
///   says so in `soundPitchDrawn` and omits the number rather than
///   publishing an artefact, so this is `None` and the rule stays
///   hand-ported.
/// * **`excludes_actor` decides who gets a packet**, not what is in it.
///   `Level.playSound`'s first argument is the *excluded* entity: a button's
///   presser hears nothing and a lever's flipper hears the click.
#[derive(Debug, Clone, PartialEq)]
pub struct TableSound {
    pub event: String,
    /// The wire ordinal, already resolved through
    /// [`crate::sounds::source_ordinal`] — an unknown name is refused at load
    /// rather than defaulted here.
    pub source: i32,
    pub volume: f32,
    /// `None` when vanilla drew the pitch. See the type's own docs.
    pub pitch: Option<f32>,
    pub excludes_actor: bool,
    /// How many sounds the rule played. Anything but 1 means the extract
    /// names only the last of them.
    pub count: u32,
}

impl TableSound {
    /// Whether this row can drive the emitter on its own.
    ///
    /// **Both halves are refusals to guess, and they fail in the same
    /// direction as today's behaviour** — the emitter stays silent, which is
    /// what it already does for 180 blocks. A `count` above 1 means the
    /// extract kept the last of several sounds and cannot say which the
    /// client should hear; a `pitch` of `None` means the number is not
    /// vanilla's. Emitting either would be worse than silence, because a
    /// wrong sound is indistinguishable from a right one to every check that
    /// does not compare pitch — and no check in this project compares pitch.
    #[must_use]
    pub fn usable(&self) -> bool {
        self.count == 1 && self.pitch.is_some()
    }
}

/// `useWithoutItem` per block state, from `ci/use-golden.sh`.
#[derive(Debug, Default)]
pub struct UseTable {
    by_state: HashMap<String, Entry>,
}

impl UseTable {
    /// Load the extract.
    ///
    /// # Errors
    /// If the file is unreadable, is not JSON, or is not the shape
    /// `UseGolden` writes.
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let doc: serde_json::Value = serde_json::from_str(&text).map_err(io::Error::other)?;
        let table = doc
            .get("table")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| io::Error::other("use_without_item.json has no `table` array"))?;

        let mut by_state = HashMap::new();
        for row in table {
            // A random block carries no cells by construction, so the loop
            // below simply finds none. Nothing special is needed here, and
            // that is the point of the extraction dropping them rather than
            // writing one sample of a die roll.
            let Some(states) = row.get("states").and_then(serde_json::Value::as_array) else {
                continue;
            };
            for cell in states {
                if cell.get("refused").is_some() {
                    continue;
                }
                let Some(from) = cell.get("from").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                if cell.get("consumes") != Some(&serde_json::Value::Bool(true)) {
                    continue;
                }
                let writes = cell
                    .get("writes")
                    .and_then(serde_json::Value::as_array)
                    .map_or(&[][..], Vec::as_slice);
                // Every write in the 26.2 extract is at the clicked block —
                // 4,850 of 4,850. A table that disagrees came from a
                // different generator, and applying its writes at the wrong
                // position would be silent, so it is refused instead.
                if writes.len() > 1 {
                    return Err(io::Error::other(format!(
                        "{from}: {} writes, but every useWithoutItem in 26.2 writes at most one",
                        writes.len()
                    )));
                }
                let mut to = None;
                let mut flags = 0;
                if let Some(w) = writes.first() {
                    let at = w.get("at").and_then(serde_json::Value::as_array);
                    let origin =
                        at.is_some_and(|a| a.len() == 3 && a.iter().all(|v| v.as_i64() == Some(0)));
                    if !origin {
                        return Err(io::Error::other(format!(
                            "{from}: a write away from the clicked block, but every write in \
                             the extract is at [0,0,0]"
                        )));
                    }
                    to = w
                        .get("state")
                        .and_then(serde_json::Value::as_str)
                        .map(ToString::to_string);
                    // Required, not defaulted, for the same reason as in
                    // `crate::placedby`: a table dumped before flags were
                    // recorded would otherwise load and cascade — or not —
                    // according to whatever number looked reasonable here.
                    flags = w
                        .get("flags")
                        .and_then(serde_json::Value::as_i64)
                        .and_then(|v| i32::try_from(v).ok())
                        .ok_or_else(|| {
                            io::Error::other(format!(
                                "{from}: a write has no `flags` — regenerate with ci/use-golden.sh"
                            ))
                        })?;
                }
                let schedules_tick =
                    cell.get("schedulesTick") == Some(&serde_json::Value::Bool(true));
                // The delay is only meaningful beside `schedulesTick`, and the
                // two must not be able to disagree: a row that says a tick
                // exists but carries no delay is refused rather than woken at
                // some invented moment.
                let tick = if schedules_tick {
                    let delay = cell
                        .get("tickDelay")
                        .and_then(serde_json::Value::as_i64)
                        .ok_or_else(|| {
                            io::Error::other(format!(
                                "{from}: schedulesTick with no `tickDelay` — regenerate with \
                                 ci/use-golden.sh"
                            ))
                        })?;
                    let name = cell
                        .get("tickPriority")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("NORMAL");
                    let kind = cell.get("tickKind").and_then(serde_json::Value::as_str);
                    Some(crate::placedby::Tick {
                        delay,
                        priority: crate::placedby::priority_ordinal(name).ok_or_else(|| {
                            io::Error::other(format!("{from}: unknown TickPriority {name}"))
                        })?,
                        is_block: kind != Some("fluid"),
                    })
                } else {
                    None
                };
                // A row that says it sounded must say what it played. Before
                // 2026-08-17 `sound` was a lone boolean and this block did
                // not exist at all; a table still carrying only the boolean
                // is an old extract, and reading it as "silent" would be a
                // silent downgrade of every block below. It is refused with
                // the command that fixes it, exactly as a missing `flags` or
                // a missing `tickDelay` is.
                let sound = if cell.get("sound") == Some(&serde_json::Value::Bool(true)) {
                    let event = cell
                        .get("soundEvent")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| {
                            io::Error::other(format!(
                                "{from}: sound with no `soundEvent` — regenerate with \
                                 ci/use-golden.sh"
                            ))
                        })?;
                    let name = cell
                        .get("soundSource")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| {
                            io::Error::other(format!(
                                "{from}: sound with no `soundSource` — regenerate with \
                                 ci/use-golden.sh"
                            ))
                        })?;
                    let source = crate::sounds::source_ordinal(name).ok_or_else(|| {
                        io::Error::other(format!("{from}: unknown SoundSource {name}"))
                    })?;
                    Some(TableSound {
                        event: event.to_string(),
                        source,
                        #[allow(clippy::cast_possible_truncation)]
                        volume: cell
                            .get("soundVolume")
                            .and_then(serde_json::Value::as_f64)
                            .unwrap_or(1.0) as f32,
                        // Absent by design when `soundPitchDrawn`, so this is
                        // `None` rather than a defaulted 1.0 — a defaulted
                        // pitch is the wrong pitch wearing a right one's
                        // clothes.
                        #[allow(clippy::cast_possible_truncation)]
                        pitch: cell
                            .get("soundPitch")
                            .and_then(serde_json::Value::as_f64)
                            .map(|p| p as f32),
                        excludes_actor: cell.get("soundExcludesActor")
                            == Some(&serde_json::Value::Bool(true)),
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        count: cell
                            .get("soundCount")
                            .and_then(serde_json::Value::as_i64)
                            .unwrap_or(1) as u32,
                    })
                } else {
                    None
                };
                by_state.insert(
                    from.to_string(),
                    Entry {
                        to,
                        flags,
                        schedules_tick,
                        tick,
                        sound,
                    },
                );
            }
        }
        Ok(Self { by_state })
    }

    /// The one table this process serves, loaded once.
    ///
    /// Shared rather than loaded per join, like the shape table: this is 1.7
    /// megabytes of JSON and it was being parsed again for every connection.
    /// The clock thread needs it too now — a button's tick fires there and
    /// has to ask what class the block is — and two independent copies would
    /// be two chances to disagree.
    ///
    /// # Errors
    /// Whatever [`Self::load`] returned the first time, repeated.
    pub fn shared(path: &Path) -> Result<&'static Self, String> {
        static TABLE: std::sync::OnceLock<Result<UseTable, String>> = std::sync::OnceLock::new();
        TABLE
            .get_or_init(|| Self::load(path).map_err(|e| e.to_string()))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// How many states have an answer. Zero means the file was empty, which
    /// is a different situation from every block declining and is worth
    /// being able to tell apart.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_state.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_state.is_empty()
    }

    /// Whether this state asked to be woken later.
    #[must_use]
    pub fn schedules_tick(&self, state: &str) -> bool {
        self.by_state.get(state).is_some_and(|e| e.schedules_tick)
    }

    /// When and at what priority it asked to be woken.
    ///
    /// **The delay is the whole point.** The extract used to record only
    /// *that* a tick was scheduled, so every button said `true` and none
    /// could say 20 ticks or 30 — and a button's entire observable behaviour
    /// is that it un-presses a fixed number of ticks later. Measured from
    /// the table: stone and polished blackstone **20**, every wooden button
    /// **30**, every trapdoor **5**.
    #[must_use]
    pub fn tick_for(&self, state: &str) -> Option<crate::placedby::Tick> {
        self.by_state.get(state).and_then(|e| e.tick)
    }

    /// The flags vanilla wrote this state's replacement with.
    #[must_use]
    pub fn write_flags(&self, state: &str) -> Option<i32> {
        self.by_state.get(state).map(|e| e.flags)
    }

    /// The sound this state's rule played, whatever the table knows about it.
    ///
    /// Keyed on the state that was **clicked**, not the one written: the
    /// extract probes `useWithoutItem` on a state and records what that call
    /// did, so the row belongs to the input. Callers that key a sound on the
    /// written state — `use_sound`'s lever and comparator arms both do, and
    /// correctly, because vanilla reads the *cycled* value for its pitch —
    /// are reading a different fact and must keep doing it their way.
    #[must_use]
    pub fn sound_for(&self, state: &str) -> Option<&TableSound> {
        self.by_state.get(state).and_then(|e| e.sound.as_ref())
    }

    /// How many states the table can emit a sound for unaided, and how many
    /// sound but cannot be emitted.
    ///
    /// Exists to be asserted on. A table-driven emitter whose coverage silently
    /// fell to zero would look exactly like the hand-ported server it
    /// replaced — every check still green, every sound still absent — which
    /// is this project's signature defect. The number is the evidence that
    /// the fallback is wired to something.
    #[must_use]
    pub fn sound_coverage(&self) -> (usize, usize) {
        // Iterator chains rather than a `for`, because `by_state` is a
        // `HashMap` and clippy's `iter_over_hash_type` is denied here. Both
        // results are order-invariant — a count and, at the call site, a
        // `BTreeSet` — so nothing is lost by not sorting first.
        let usable = self
            .by_state
            .values()
            .filter(|e| e.sound.as_ref().is_some_and(TableSound::usable))
            .count();
        let unusable = self
            .by_state
            .values()
            .filter(|e| e.sound.as_ref().is_some_and(|s| !s.usable()))
            .count();
        (usable, unusable)
    }
}

/// `BlockState.useWithoutItem`.
///
/// `None` for the table means no extract was loaded and every block
/// declines — which is what this server did before the table existed, said
/// out loud rather than shown as right-clicks that quietly do nothing.
#[must_use]
pub fn use_without_item(table: Option<&UseTable>, state: &str) -> Used {
    // The corpus positive control: the server before any `useWithoutItem` was
    // ported, which answered `Pass` to every right-click on every block. A
    // lever that does not flip, a door that does not open, a detector that
    // does not invert. See `crate::sabotage`.
    if crate::sabotage::fires_here(crate::sabotage::Kind::Use) {
        return Used::Pass;
    }
    match table.and_then(|t| t.by_state.get(state)) {
        Some(entry) => Used::Consumed(entry.to.clone()),
        None => Used::Pass,
    }
}

/// `FenceGateBlock`, from `data/vanilla/set_placed_by.json`'s class column.
///
/// Twelve blocks, and none of them a subclass of another — the wood type is a
/// constructor argument, exactly as it is for `ButtonBlock`.
pub const FENCE_GATE_BLOCK: &str = "FenceGateBlock";

/// The two classes whose sound is the table's event and one shared pitch.
///
/// `DoorBlock.playSound` and `TrapDoorBlock.playSound` are the same four
/// arguments in the pinned 26.2 jar: the player (so the actor is excluded),
/// `SoundSource.BLOCKS`, volume `fconst_1`, and a pitch of
/// `level.getRandom().nextFloat() * 0.1F + 0.9F` — offsets 25-44 of
/// `TrapDoorBlock.playSound`, 25-46 of `DoorBlock`'s. `FenceGateBlock` is the
/// third and was hand-ported on 2026-08-11 before the table could name events.
///
/// **What varies between them is only the event**, and the event is exactly
/// what the extract now carries: `BlockSetType.trapdoorOpen()` /
/// `trapdoorClose()` per block set, which is why a hand-port would have
/// needed 2,560 constants and the table needs none.
pub const DOOR_BLOCK: &str = "DoorBlock";
pub const TRAPDOOR_BLOCK: &str = "TrapDoorBlock";

/// `DaylightDetectorBlock`. One block.
pub const DAYLIGHT_DETECTOR_BLOCK: &str = "DaylightDetectorBlock";

/// What a hand-ported `useWithoutItem` wrote, and with which flags.
///
/// The flags are carried rather than looked up because the two blocks here
/// disagree with each other *and* one of them is absent from the table that
/// would otherwise supply them. See this module's header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ported {
    /// The state written at the clicked block. `None` would be a consume
    /// that writes nothing; neither class here has that branch.
    pub to: Option<String>,
    /// `Level.setBlock`'s third argument at that call site.
    pub flags: i32,
}

/// `useWithoutItem` for the classes [`UseTable`] cannot answer for.
///
/// `None` means "not one of mine" and the caller falls through to the table,
/// which is the answer for the other 182 blocks in it.
///
/// `player_facing` is `Entity.getDirection()` — `Direction.fromYRot(getYRot())`
/// — as one of `north`/`south`/`west`/`east`, which is
/// [`crate::placement::from_y_rot`] indexed into the same SOUTH/WEST/NORTH/EAST
/// order `from2DDataValue` uses.
#[must_use]
pub fn hand_ported(
    classes: Option<&crate::placedby::PlacedByTable>,
    state: &str,
    player_facing: &str,
) -> Option<Ported> {
    let classes = classes?;
    let block = state.split('[').next().unwrap_or(state);
    if classes.is_a(block, FENCE_GATE_BLOCK) {
        return Some(fence_gate(state, player_facing));
    }
    if classes.is_a(block, DAYLIGHT_DETECTOR_BLOCK) {
        return Some(daylight_detector(state));
    }
    None
}

/// `FenceGateBlock.useWithoutItem`, offsets 0-101.
///
/// ```text
///    0-13   if (state.getValue(OPEN)) {
///   16-39       state = state.setValue(OPEN, false);
///               level.setBlock(pos, state, 10);
///          } else {
///   43-48       Direction d = player.getDirection();
///   50-62       if (state.getValue(FACING) == d.getOpposite())
///   65-77           state = state.setValue(FACING, d);
///   78-98       state = state.setValue(OPEN, true);
///               level.setBlock(pos, state, 10);
///          }
/// ```
///
/// **The rewrite is on the opening branch only.** Closing a gate never turns
/// it, which is what makes `701` and `700` a pair rather than a duplicate:
/// both click a `facing=south` gate once, `700` from the south and `701` from
/// the north, and only the second reaches offset 65.
fn fence_gate(state: &str, player_facing: &str) -> Ported {
    let next = if crate::placement::property_of(state, "open") == Some("true") {
        crate::placement::with_property(state, "open", "false")
    } else {
        let turned = if crate::placement::property_of(state, "facing")
            == Some(opposite_horizontal(player_facing))
        {
            crate::placement::with_property(state, "facing", player_facing)
        } else {
            state.to_string()
        };
        crate::placement::with_property(&turned, "open", "true")
    };
    Ported {
        to: Some(next),
        flags: FENCE_GATE_SET_BLOCK_FLAGS,
    }
}

/// `bipush 10` at offsets 34 and 96 — `UPDATE_CLIENTS | UPDATE_IMMEDIATE`,
/// the same pair `DoorBlock.useWithoutItem` writes with and **not**
/// `UPDATE_ALL`.
const FENCE_GATE_SET_BLOCK_FLAGS: i32 = 10;

/// `Direction.getOpposite()`, over the four horizontals.
///
/// Total for this call site rather than in general: both arguments it is ever
/// reached with are horizontal — `FACING` on a fence gate is a
/// `HORIZONTAL_FACING` property, and `Direction.fromYRot` returns one of the
/// four by construction. A vertical would fall through to the input, which
/// makes the comparison it feeds fail closed.
fn opposite_horizontal(facing: &str) -> &str {
    match facing {
        "north" => "south",
        "south" => "north",
        "west" => "east",
        "east" => "west",
        other => other,
    }
}

/// `DaylightDetectorBlock.useWithoutItem`, offsets 20-67.
///
/// ```text
///   20-24  if (level.isClientSide) return SUCCESS;
///   27-37  BlockState next = state.cycle(INVERTED);
///   39-47  level.setBlock(pos, next, 2);
///   48-60  level.gameEvent(BLOCK_CHANGE, pos, Context.of(player, next));
///      67  updateSignalStrength(next, level, pos);
/// ```
///
/// **Offset 0 is not ported and that is deliberate.** `if (!player.mayBuild())
/// return super.useWithoutItem(...)` falls into `BaseEntityBlock`'s menu path;
/// every player this server serves may build, and the corpus's actor is in
/// creative. The day a game mode exists this is the line that grows.
///
/// Offset 67 is not ported either, and that one is a real gap — see this
/// module's header for the arithmetic and for what it costs.
fn daylight_detector(state: &str) -> Ported {
    let inverted = crate::placement::property_of(state, "inverted") == Some("true");
    Ported {
        to: Some(crate::placement::with_property(
            state,
            "inverted",
            if inverted { "false" } else { "true" },
        )),
        flags: DAYLIGHT_DETECTOR_SET_BLOCK_FLAGS,
    }
}

/// `iconst_2` at offset 43 — `UPDATE_CLIENTS` **without** `UPDATE_NEIGHBORS`.
///
/// The neighbour half of this mechanic is done separately, by
/// `updateSignalStrength`'s own `setBlock(..., 3)`, and only when the computed
/// signal differs from the one already on the block. Falling back to
/// `UPDATE_ALL` here would push a neighbour update vanilla does not send.
const DAYLIGHT_DETECTOR_SET_BLOCK_FLAGS: i32 = 2;

/// The four classes whose `useWithoutItem` opens with vanilla's own
/// `if (!mayBuild) return PASS;` — see this module's header for the
/// byte-for-byte comparison and for why `DaylightDetectorBlock`'s differently
/// shaped bytecode still resolves to the same `PASS`.
const MAY_BUILD_GATED: [&str; 4] = [
    "ComparatorBlock",
    DAYLIGHT_DETECTOR_BLOCK,
    "RedStoneWireBlock",
    "RepeaterBlock",
];

/// `LightBlock.useWithoutItem`, the one row on `game_master` rather than
/// `no_build`.
const GAME_MASTER_GATED: &str = "LightBlock";

/// `iconst_2` at `LightBlock.useWithoutItem` offset 27 — `UPDATE_CLIENTS`
/// alone, the same constant `DaylightDetectorBlock`'s own cycle writes with
/// and not `UPDATE_ALL`.
const LIGHT_SET_BLOCK_FLAGS: i32 = 2;

/// What [`use_gate`] answers when it answers at all.
///
/// Two arms, not a bare `Option<Ported>`, because the whole point of the
/// `mayBuild` gate is a branch [`Ported`] cannot express: [`Used`] already
/// distinguishes "consumed, wrote nothing" from "consumed, wrote something",
/// but every row in `use_without_item.json` is `consumes: true` — the probe
/// that built the table could always build, so nothing in it was ever
/// refused outright. `Pass` is that missing third answer, and it takes
/// priority over everything else `useWithoutItem` could otherwise say,
/// exactly as it does in the bytecode: offset 0, before any state is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    /// `mayBuild` was false. The click is refused before any state is read,
    /// so the caller falls through to the placement path exactly as a
    /// table-driven `Used::Pass` does.
    Pass,
    /// `canUseGameMasterBlocks` was true and `LightBlock`'s `LEVEL` cycled —
    /// the one branch the table has no cell for.
    Write(Ported),
}

/// The `mayBuild`/`canUseGameMasterBlocks` gate, consulted **before**
/// [`hand_ported`] and the table both — see this module's header for the
/// bytecode this ports and for why `may_build`/`game_master` are constants
/// at the one call site rather than read off a player.
///
/// `None` means "this state's answer is unaffected by either bit", which is
/// every call in production today (`may_build = true, game_master = false`
/// hardcoded), and the caller proceeds to [`hand_ported`] and the table
/// exactly as if this function did not exist.
#[must_use]
pub fn use_gate(
    classes: Option<&crate::placedby::PlacedByTable>,
    state: &str,
    may_build: bool,
    game_master: bool,
) -> Option<Gate> {
    let classes = classes?;
    let block = state.split('[').next().unwrap_or(state);
    if !may_build && MAY_BUILD_GATED.iter().any(|c| classes.is_a(block, c)) {
        return Some(Gate::Pass);
    }
    if game_master && classes.is_a(block, GAME_MASTER_GATED) {
        return Some(Gate::Write(Ported {
            to: Some(light_cycle_level(state)),
            flags: LIGHT_SET_BLOCK_FLAGS,
        }));
    }
    None
}

/// `LightBlock.useWithoutItem` offsets 15-24: `state.cycle(LEVEL)`.
/// `IntegerProperty.cycle` walks its declared values in order and wraps;
/// `LEVEL`'s are the contiguous `0..=15` the golden table's own 32
/// `minecraft:light` rows enumerate (16 levels times two `waterlogged`
/// values), so the wrap is `(level + 1) % 16`. `waterlogged` is not read by
/// this method, so it is carried through untouched.
fn light_cycle_level(state: &str) -> String {
    let level: u8 = crate::placement::property_of(state, "level")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    crate::placement::with_property(state, "level", &((level + 1) % 16).to_string())
}

#[cfg(test)]
mod tests {
    use super::{use_gate, use_without_item, Gate, UseTable, Used};

    /// `UseTable::load` takes a path, so each test writes one and removes it.
    struct Temp(std::path::PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    fn table_from(json: &str) -> (UseTable, Temp) {
        let p = std::env::temp_dir().join(format!(
            "rustmc-use-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&p, json).expect("write");
        let t = UseTable::load(&p).expect("loads");
        (t, Temp(p))
    }

    const LEVER: &str = r#"{"table": [
      {"block": "minecraft:lever", "random": false, "faceSensitive": false, "states": [
        {"from": "minecraft:lever[face=floor,facing=south,powered=false]", "result": "Success",
         "consumes": true, "schedulesTick": false, "sound": true,
         "soundEvent": "minecraft:block.lever.click", "soundSource": "block",
         "soundVolume": 0.3, "soundCount": 1, "soundPitchDrawn": false,
         "soundExcludesActor": false, "soundPitch": 0.6,
         "writes": [{"at": [0, 0, 0],
                     "state": "minecraft:lever[face=floor,facing=south,powered=true]", "flags": 3}]},
        {"from": "minecraft:lever[face=floor,facing=south,powered=true]", "result": "Success",
         "consumes": true, "schedulesTick": false, "sound": true,
         "soundEvent": "minecraft:block.lever.click", "soundSource": "block",
         "soundVolume": 0.3, "soundCount": 1, "soundPitchDrawn": false,
         "soundExcludesActor": false, "soundPitch": 0.5,
         "writes": [{"at": [0, 0, 0],
                     "state": "minecraft:lever[face=floor,facing=south,powered=false]", "flags": 3}]}
      ]}
    ]}"#;

    /// The corpus's own rig, and the block the whole table is checked
    /// against: `lever-flips-when-right-clicked` went `diverge -> match`
    /// against real vanilla on the hand port, and `ci/use-golden.sh` refuses
    /// to write a table that disagrees with it.
    #[test]
    fn a_lever_toggles_powered_in_place() {
        let (t, _g) = table_from(LEVER);
        assert_eq!(t.len(), 2);
        assert_eq!(
            use_without_item(
                Some(&t),
                "minecraft:lever[face=floor,facing=south,powered=false]"
            ),
            Used::Consumed(Some(
                "minecraft:lever[face=floor,facing=south,powered=true]".into()
            ))
        );
        assert_eq!(
            use_without_item(
                Some(&t),
                "minecraft:lever[face=floor,facing=south,powered=true]"
            ),
            Used::Consumed(Some(
                "minecraft:lever[face=floor,facing=south,powered=false]".into()
            ))
        );
    }

    /// A state the table does not carry declines, and so does every state
    /// when there is no table at all. The second half is what a checkout
    /// without `data/vanilla` gets, and it must decline rather than panic.
    #[test]
    fn an_unknown_state_and_a_missing_table_both_pass() {
        let (t, _g) = table_from(LEVER);
        assert_eq!(use_without_item(Some(&t), "minecraft:stone"), Used::Pass);
        assert_eq!(use_without_item(None, "minecraft:stone"), Used::Pass);
        assert_eq!(
            use_without_item(
                None,
                "minecraft:lever[face=floor,facing=south,powered=false]"
            ),
            Used::Pass
        );
    }

    /// 891 states in the real extract consume the click and write nothing —
    /// pressing a button that is already pressed. That is **not** the same as
    /// declining: vanilla consumed the interaction, so the placement path
    /// must not run, and a `Pass` here would place a block where vanilla
    /// places none.
    #[test]
    fn consuming_without_writing_is_not_the_same_as_passing() {
        let (t, _g) = table_from(
            r#"{"table": [{"block": "minecraft:stone_button", "random": false,
                 "faceSensitive": false, "states": [
              {"from": "minecraft:stone_button[face=floor,facing=north,powered=true]",
               "result": "Success", "consumes": true, "schedulesTick": true,
               "tickDelay": 20, "tickPriority": "NORMAL",
               "sound": false, "writes": []}]}]}"#,
        );
        assert_eq!(
            use_without_item(
                Some(&t),
                "minecraft:stone_button[face=floor,facing=north,powered=true]"
            ),
            Used::Consumed(None)
        );
        assert!(t.schedules_tick("minecraft:stone_button[face=floor,facing=north,powered=true]"));
    }

    /// The loader refuses a table from a different generator rather than
    /// applying its writes at the wrong position. Every write in the 26.2
    /// extract is at the clicked block; one at an offset means the
    /// assumption this module is built on has stopped holding, and a silent
    /// misplacement is exactly the failure this project keeps finding.
    #[test]
    fn a_write_away_from_the_clicked_block_is_refused() {
        let p = std::env::temp_dir().join(format!("rustmc-use-bad-{}.json", std::process::id()));
        std::fs::write(
            &p,
            r#"{"table": [{"block": "minecraft:x", "random": false, "faceSensitive": false,
              "states": [{"from": "minecraft:x", "result": "Success", "consumes": true,
              "schedulesTick": false, "sound": false,
              "writes": [{"at": [0, 1, 0], "state": "minecraft:y", "flags": 3}]}]}]}"#,
        )
        .expect("write");
        let err = UseTable::load(&p).expect_err("a write off the origin is refused");
        let _ = std::fs::remove_file(&p);
        assert!(err.to_string().contains("[0,0,0]"), "{err}");
    }

    // -------------------------------------------------------------------
    // `hand_ported` — the two classes the state-keyed table cannot answer
    // for. See the module header for why each is here.
    // -------------------------------------------------------------------

    /// A `PlacedByTable` carrying only the class columns these tests read.
    fn classes() -> (crate::placedby::PlacedByTable, Temp) {
        let p = std::env::temp_dir().join(format!(
            "rustmc-useport-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(
            &p,
            r#"{"classes": {"minecraft:oak_fence_gate": "FenceGateBlock",
                            "minecraft:bamboo_fence_gate": "FenceGateBlock",
                            "minecraft:daylight_detector": "DaylightDetectorBlock",
                            "minecraft:lever": "LeverBlock"},
                "superclasses": {"FenceGateBlock": ["Block"],
                                 "DaylightDetectorBlock": ["BaseEntityBlock", "Block"],
                                 "LeverBlock": ["Block"]},
                "table": []}"#,
        )
        .expect("write");
        let t = crate::placedby::PlacedByTable::load(&p).expect("load");
        (t, Temp(p))
    }

    /// **The whole of `701`.** A south-facing gate clicked by a player facing
    /// north opens *and turns*, because `getOpposite()` of north is south and
    /// that is the gate's own facing. The table's row for this state says
    /// `facing=south` — the probe player's direction — so a server reading it
    /// keeps south and diverges on one property.
    #[test]
    fn a_closed_gate_turns_to_face_a_player_standing_behind_it() {
        let (c, _g) = classes();
        let was = "minecraft:oak_fence_gate[facing=south,in_wall=false,open=false,powered=false]";
        assert_eq!(
            super::hand_ported(Some(&c), was, "north"),
            Some(super::Ported {
                to: Some(
                    "minecraft:oak_fence_gate[facing=north,in_wall=false,open=true,powered=false]"
                        .into()
                ),
                flags: 10,
            })
        );
    }

    /// **The whole of `700`, and the control for the test above.** The same
    /// gate and the same click from the *front* opens without turning: the
    /// branch at offset 50 compares `FACING` against the opposite of the
    /// player's direction, and south is not the opposite of south.
    #[test]
    fn a_closed_gate_opened_from_the_front_keeps_its_facing() {
        let (c, _g) = classes();
        let was = "minecraft:oak_fence_gate[facing=south,in_wall=false,open=false,powered=false]";
        assert_eq!(
            super::hand_ported(Some(&c), was, "south"),
            Some(super::Ported {
                to: Some(
                    "minecraft:oak_fence_gate[facing=south,in_wall=false,open=true,powered=false]"
                        .into()
                ),
                flags: 10,
            })
        );
    }

    /// Closing never turns. Offsets 16-39 are the whole of that branch and
    /// `player` is not read on it — a gate shut from behind stays where it
    /// was, and a port that cycled `OPEN` and applied the rewrite either way
    /// would pass both tests above and fail this one.
    #[test]
    fn an_open_gate_closes_without_turning() {
        let (c, _g) = classes();
        let was = "minecraft:bamboo_fence_gate[facing=south,in_wall=true,open=true,powered=true]";
        assert_eq!(
            super::hand_ported(Some(&c), was, "north"),
            Some(super::Ported {
                to: Some(
                    "minecraft:bamboo_fence_gate[facing=south,in_wall=true,open=false,powered=true]"
                        .into()
                ),
                flags: 10,
            })
        );
    }

    /// The detector cycles `INVERTED` and writes with **2**, not 3. The flag
    /// is the half of this that no packet in the corpus distinguishes and
    /// that a fall-through to `UPDATE_ALL` would get wrong.
    #[test]
    fn a_daylight_detector_cycles_inverted_and_writes_without_neighbours() {
        let (c, _g) = classes();
        assert_eq!(
            super::hand_ported(
                Some(&c),
                "minecraft:daylight_detector[inverted=false,power=0]",
                "south"
            ),
            Some(super::Ported {
                to: Some("minecraft:daylight_detector[inverted=true,power=0]".into()),
                flags: 2,
            })
        );
        assert_eq!(
            super::hand_ported(
                Some(&c),
                "minecraft:daylight_detector[inverted=true,power=11]",
                "south"
            ),
            Some(super::Ported {
                to: Some("minecraft:daylight_detector[inverted=false,power=11]".into()),
                flags: 2,
            })
        );
    }

    /// **`power` is carried, never computed**, and this test exists to fail
    /// the day somebody adds a light model and forgets this module.
    /// `state.cycle(INVERTED)` preserves every other property; recomputing it
    /// is `updateSignalStrength`'s job and that method is not ported.
    #[test]
    fn the_detectors_power_is_carried_through_the_cycle_untouched() {
        let (c, _g) = classes();
        for power in 0..=15 {
            let was = format!("minecraft:daylight_detector[inverted=false,power={power}]");
            let got = super::hand_ported(Some(&c), &was, "west").expect("ported");
            assert_eq!(
                got.to,
                Some(format!(
                    "minecraft:daylight_detector[inverted=true,power={power}]"
                ))
            );
        }
    }

    /// Everything else falls through to the table, and so does every block
    /// when there is no class table at all — a checkout without
    /// `data/vanilla` must not lose the lever.
    #[test]
    fn a_block_of_neither_class_is_not_hand_ported() {
        let (c, _g) = classes();
        assert_eq!(
            super::hand_ported(
                Some(&c),
                "minecraft:lever[face=floor,powered=false]",
                "south"
            ),
            None
        );
        assert_eq!(
            super::hand_ported(Some(&c), "minecraft:stone", "south"),
            None
        );
        assert_eq!(
            super::hand_ported(
                None,
                "minecraft:oak_fence_gate[facing=south,open=false]",
                "north"
            ),
            None
        );
    }

    /// The workspace root, so a test can read a generated table.
    /// `crate::commands`' copy, and the same reasoning: a missing table is a
    /// **loud failure and not a skip**, because a skipped test is a gate that
    /// cannot fail.
    fn repo(relative: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative)
    }

    /// **The shipped table IS a south-facing player's answer, and this is what
    /// would notice if it stopped being one.**
    ///
    /// The module header's central claim about `FenceGateBlock` is a claim
    /// about `data/vanilla/use_without_item.json`, not about our code — and a
    /// claim about a generated file is exactly the kind nobody re-checks after
    /// the generator changes. So it is asserted against the real table:
    ///
    ///  * every one of the twelve gates' 384 writing rows agrees, state for
    ///    state, with [`hand_ported`] called with `player_facing = "south"`,
    ///    flags included;
    ///  * exactly **48** of them change `facing`, and every one of those is
    ///    `facing=north -> facing=south`, which is what "the probe faced
    ///    south" means in the data.
    ///
    /// If `ci/use-golden.sh` is ever re-run with a probe standing somewhere
    /// else, this fails and names the row. Without it the header would quietly
    /// become false and `hand_ported` would still be right, which is the worse
    /// of the two ways round.
    /// What the shipped extract can and cannot say about sounds, pinned.
    ///
    /// # Two different failures, and only one of them is a regression
    ///
    /// **A row that sounds and names no event** is an extract generated
    /// before 2026-08-17, and it makes every block below it silently silent.
    /// `load` already refuses it; this says so against the real file rather
    /// than a fixture, so a stale `data/vanilla` is caught here rather than
    /// as a missing packet nine steps downstream.
    ///
    /// **The 2,944 that sound and cannot be emitted** are not a regression
    /// and must not be read as one. Their pitch is a draw — every door,
    /// trapdoor and fence gate — and the extract deliberately omits a number
    /// it measured under a fixed seed rather than publish a constant of
    /// `RandomSource::create(0)` as vanilla's. Closing them means reading
    /// their class, not regenerating the table.
    ///
    /// **The table describes 1,525 and the use path can sound 1,509 of them
    /// (re-extraction of 2026-09-04, `stack-gameplay`).** The 247-block
    /// probe-environment table describes sixteen states — decorated pots
    /// and their kin — whose rule consumes the click and writes nothing, and
    /// `world_send` only calls `use_sound` after a write, so they never
    /// sound. This method counts what the table says; `ci/sound-coverage.py`
    /// counts what the server can emit and lists the sixteen as `unreached`.
    /// The two numbers are different claims and both are pinned.
    ///
    /// **The figure that matters is 1,509 against a floor of 208.** 208 is
    /// what this same table yielded while `soundPitchDrawn` was *inferred*
    /// from "did the rule touch the RNG" instead of measured across `SEEDS`,
    /// and 208 is exactly the four families that were already hand-ported —
    /// i.e. an event-id column that measured the wrong thing would have
    /// added nothing at all and looked like it worked.
    #[test]
    fn the_shipped_table_says_which_sound_and_admits_when_it_cannot() {
        let path = repo("data/vanilla/use_without_item.json");
        let table = UseTable::load(&path)
            .unwrap_or_else(|e| panic!("{} — run ci/gen-gameplay-tables.sh ({e})", path.display()));

        let (usable, unusable) = table.sound_coverage();
        assert_eq!(
            (usable, unusable),
            (1525, 2944),
            "sound coverage moved — if this is a re-extraction, say so in the message"
        );

        // The routing in `world_send::use_sound` is worth nothing if the
        // events are all one event, and a table that collapsed to a single
        // id would still satisfy the count above.
        let events: std::collections::BTreeSet<String> = table
            .by_state
            .values()
            .filter_map(|e| e.sound.as_ref())
            .filter(|s| s.usable())
            .map(|s| s.event.clone())
            .collect();
        // **35, not 34, and the difference is the whole point of the
        // paragraph above.** This set is taken over what the TABLE describes
        // — the same 1,525 the assertion above pins — and 1,525 - 1,509 is
        // exactly the sixteen `decorated_pot` states whose rule consumes the
        // click and writes nothing, which are the only states in the table
        // carrying `block.decorated_pot.insert_fail`. Dropping them drops
        // exactly one event name, so `ci/sound-coverage.py`'s `events 34` and
        // this 35 are the same table read under the two different claims the
        // paragraph above says are both pinned. 34 here was the census's
        // number in the test that counts the other quantity: the first
        // assertion carried the same confusion as `(1509, 2944)` and was
        // corrected to `(1525, 2944)` one line up without this one moving.
        assert_eq!(
            events.len(),
            35,
            "distinct sound events the table describes"
        );
        assert!(events.contains("minecraft:block.note_block.harp"));
        assert!(events.contains("minecraft:block.lever.click"));
        // The sixteen, by name, so the two readings stay separable: this one
        // is here and `ci/sound-coverage.py`'s is not.
        assert!(events.contains("minecraft:block.decorated_pot.insert_fail"));
        let emittable: std::collections::BTreeSet<&String> = events
            .iter()
            .filter(|e| *e != "minecraft:block.decorated_pot.insert_fail")
            .collect();
        assert_eq!(
            emittable.len(),
            34,
            "what the use path can actually sound — ci/sound-coverage.py's reading"
        );
    }

    #[test]
    fn the_shipped_fence_gate_rows_are_exactly_a_south_facing_players_answer() {
        let path = repo("data/vanilla/use_without_item.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} — run ci/gen-gameplay-tables.sh ({e})", path.display()));
        let doc: serde_json::Value = serde_json::from_str(&text).expect("the golden is json");
        let table = doc["table"].as_array().expect("a table array");

        let mut rows = 0_usize;
        let mut turned = 0_usize;
        for block in table {
            let name = block["block"].as_str().unwrap_or_default();
            if !name.ends_with("_fence_gate") {
                continue;
            }
            for cell in block["states"].as_array().expect("states") {
                let Some(from) = cell["from"].as_str() else {
                    continue;
                };
                let writes = cell["writes"].as_array().expect("writes");
                let Some(w) = writes.first() else { continue };
                let to = w["state"].as_str().expect("a written state");
                rows += 1;
                let ported = super::fence_gate(from, "south");
                assert_eq!(
                    ported.to.as_deref(),
                    Some(to),
                    "{from}: the table and the port disagree for a player facing south"
                );
                assert_eq!(w["flags"].as_i64(), Some(10), "{from}: setBlock flags");
                if crate::placement::property_of(from, "facing")
                    != crate::placement::property_of(to, "facing")
                {
                    turned += 1;
                    assert_eq!(crate::placement::property_of(from, "facing"), Some("north"));
                    assert_eq!(crate::placement::property_of(to, "facing"), Some("south"));
                }
            }
        }
        assert_eq!(rows, 384, "twelve fence gates, 32 states each");
        assert_eq!(
            turned, 48,
            "the north-facing closed quarter, and only it, takes the rewrite"
        );
    }

    /// **The detector is absent from the table, which is why it is hand
    /// ported at all**, and it is absent for one named reason across all 32
    /// states. If a probe fix ever puts it in, this fails and whoever is
    /// looking gets to decide whether the hand port should go.
    #[test]
    fn the_daylight_detector_is_refused_in_every_one_of_its_states() {
        let path = repo("data/vanilla/use_without_item.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} — run ci/gen-gameplay-tables.sh ({e})", path.display()));
        let doc: serde_json::Value = serde_json::from_str(&text).expect("the golden is json");
        let row = doc["table"]
            .as_array()
            .expect("a table array")
            .iter()
            .find(|b| b["block"].as_str() == Some("minecraft:daylight_detector"))
            .expect("the detector has a row");
        let states = row["states"].as_array().expect("states");
        assert_eq!(states.len(), 32, "inverted x power");
        for cell in states {
            let refused = cell["refused"]
                .as_str()
                .unwrap_or_else(|| panic!("{cell} is not refused"));
            assert!(
                refused.contains("getChunkSource"),
                "the refusal reason changed: {refused}"
            );
        }
    }

    /// A state vanilla declined is not in the table, so it must not be
    /// keyed in by accident — `consumes: false` rows are dropped at load.
    #[test]
    fn a_declined_state_is_not_loaded() {
        let (t, _g) = table_from(
            r#"{"table": [{"block": "minecraft:x", "random": false, "faceSensitive": false,
              "states": [{"from": "minecraft:x", "result": "Pass", "consumes": false,
              "schedulesTick": false, "sound": false, "writes": []}]}]}"#,
        );
        assert!(t.is_empty());
        assert_eq!(use_without_item(Some(&t), "minecraft:x"), Used::Pass);
    }

    // -------------------------------------------------------------------
    // `use_gate` — the `mayBuild`/`canUseGameMasterBlocks` gate. See this
    // module's header for the bytecode each arm ports.
    // -------------------------------------------------------------------

    /// A `PlacedByTable` carrying the five gated classes plus one ungated
    /// control (`LeverBlock`).
    fn gate_classes() -> (crate::placedby::PlacedByTable, Temp) {
        let p = std::env::temp_dir().join(format!(
            "rustmc-usegate-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(
            &p,
            r#"{"classes": {"minecraft:comparator": "ComparatorBlock",
                            "minecraft:daylight_detector": "DaylightDetectorBlock",
                            "minecraft:redstone_wire": "RedStoneWireBlock",
                            "minecraft:repeater": "RepeaterBlock",
                            "minecraft:light": "LightBlock",
                            "minecraft:lever": "LeverBlock"},
                "superclasses": {"ComparatorBlock": ["Block"],
                                 "DaylightDetectorBlock": ["BaseEntityBlock", "Block"],
                                 "RedStoneWireBlock": ["Block"],
                                 "RepeaterBlock": ["Block"],
                                 "LightBlock": ["Block"],
                                 "LeverBlock": ["Block"]},
                "table": []}"#,
        )
        .expect("write");
        let t = crate::placedby::PlacedByTable::load(&p).expect("load");
        (t, Temp(p))
    }

    /// **The failure this whole item exists to close.** Before `use_gate`
    /// existed, nothing stood ahead of the table, so a comparator click from
    /// a player who may not build fell straight through to
    /// `use_without_item` — whose one recorded cell is the golden probe's own
    /// player, who could always build — and consumed the interaction anyway.
    /// Witnessed by temporarily changing `use_gate`'s first `if` to
    /// `if false &&` and re-running: this test and the two below it went
    /// `Some(Gate::Pass) == None` / `Some(Gate::Write(..)) == None`, three
    /// failures, restored after.
    #[test]
    fn a_comparator_click_from_a_player_who_may_not_build_passes() {
        let (c, _g) = gate_classes();
        assert_eq!(
            use_gate(
                Some(&c),
                "minecraft:comparator[facing=north,mode=compare,powered=false]",
                false,
                false,
            ),
            Some(Gate::Pass)
        );
    }

    /// The same field, the same branch shape, on the other three classes —
    /// `DaylightDetectorBlock` included, whose bytecode reaches `PASS` by a
    /// different route (`Player.mayBuild()` then a delegated
    /// `BaseEntityBlock.useWithoutItem` that is not overridden) but lands on
    /// it just the same. "One mechanism, not four" checked rather than
    /// asserted.
    #[test]
    fn the_may_build_gate_is_the_same_on_all_four_classes() {
        let (c, _g) = gate_classes();
        for state in [
            "minecraft:daylight_detector[inverted=false,power=0]",
            "minecraft:redstone_wire[east=none,north=none,power=0,south=none,west=none]",
            "minecraft:repeater[delay=1,facing=north,locked=false,powered=false]",
        ] {
            assert_eq!(use_gate(Some(&c), state, false, false), Some(Gate::Pass));
        }
    }

    /// `may_build = true` defers to whatever would otherwise answer — the
    /// table for three of these, [`hand_ported`]'s own `daylight_detector`
    /// arm for the fourth — on every one of them. The golden table's one
    /// recorded player could always build, so its cells are already this
    /// branch's byte-identical answer and `use_gate` must not shadow them.
    #[test]
    fn a_player_who_may_build_is_not_intercepted() {
        let (c, _g) = gate_classes();
        for state in [
            "minecraft:comparator[facing=north,mode=compare,powered=false]",
            "minecraft:daylight_detector[inverted=false,power=0]",
            "minecraft:redstone_wire[east=none,north=none,power=0,south=none,west=none]",
            "minecraft:repeater[delay=1,facing=north,locked=false,powered=false]",
        ] {
            assert_eq!(use_gate(Some(&c), state, true, false), None);
        }
    }

    /// `LightBlock` reads a different bit and a player who is not a game
    /// master gets no intercept at all — the table's own `consumes: true,
    /// writes: []` on every one of its 32 rows is already this branch's
    /// answer, and `use_gate` defers rather than duplicating a `Pass` this
    /// class's bytecode never takes.
    #[test]
    fn a_light_click_from_a_non_game_master_defers_to_the_table() {
        let (c, _g) = gate_classes();
        assert_eq!(
            use_gate(
                Some(&c),
                "minecraft:light[level=0,waterlogged=false]",
                true,
                false
            ),
            None
        );
    }

    /// **The write the table has no cell for.** The probe's player was never
    /// a game master, so every `minecraft:light` row is the `Consumed(None)`
    /// above; a game master's click cycles `LEVEL` and this is the only place
    /// that branch is answered from. Failing this before the port existed —
    /// `None` where a write was wanted — is the whole reason `game_master` is
    /// in `ci/checks/use-probe-dependent.toml` at all.
    #[test]
    fn a_game_masters_light_click_cycles_the_level() {
        let (c, _g) = gate_classes();
        assert_eq!(
            use_gate(
                Some(&c),
                "minecraft:light[level=0,waterlogged=false]",
                true,
                true
            ),
            Some(Gate::Write(super::Ported {
                to: Some("minecraft:light[level=1,waterlogged=false]".into()),
                flags: 2,
            }))
        );
        // Wraps, and `waterlogged` is carried through untouched — `LEVEL` is
        // the only property `LightBlock.useWithoutItem` ever reads or writes.
        assert_eq!(
            use_gate(
                Some(&c),
                "minecraft:light[level=15,waterlogged=true]",
                true,
                true
            ),
            Some(Gate::Write(super::Ported {
                to: Some("minecraft:light[level=0,waterlogged=true]".into()),
                flags: 2,
            }))
        );
    }

    /// A block of none of the five gated classes is never intercepted, may
    /// build or not — and neither is anything at all when there is no class
    /// table, which is what a checkout without `data/vanilla` looks like.
    #[test]
    fn a_block_of_none_of_the_gated_classes_is_not_intercepted() {
        let (c, _g) = gate_classes();
        assert_eq!(
            use_gate(
                Some(&c),
                "minecraft:lever[face=floor,powered=false]",
                false,
                true
            ),
            None
        );
        assert_eq!(use_gate(Some(&c), "minecraft:stone", false, true), None);
        assert_eq!(
            use_gate(
                None,
                "minecraft:comparator[facing=north,mode=compare,powered=false]",
                false,
                false
            ),
            None
        );
    }
}
