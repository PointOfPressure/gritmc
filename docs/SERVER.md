# The server: what it does, and what it does not

`rustmc run` carries a client from a TCP connect to standing in a
rustmc-generated world: real terrain, shaped by the surface rules, with caves
carved and trees planted. This is the record of how far that goes and where it
stops, because a list of implemented packets is not the same as a working
server and the difference is where the surprises live.

## What works, and what proves it

Every claim below is checked against **vanilla**, not against our own codec.
Testing an encoder with its own decoder passes whatever number you put in —
that is how protocol 775 shipped from memory and passed three unit tests when
the answer is 776.

| state | proof |
|---|---|
| status | protocol 776 read back off the wire; ping echoed exactly |
| login | offline UUID matches vanilla's `OfflinePlayer:<name>` scheme byte for byte |
| configuration | all 29 registries, cross-checked against `RegistryDataLoader.SYNCHRONIZED_REGISTRIES` |
| join packet | **byte-identical** to a captured real-server packet, 109 bytes |
| chunk packet | **117 of 117** captured packets round-trip exactly |
| play | full sequence to a client: join, game event, chunks, teleport |
| compression | ~8:1 measured on the wire, 20,326 bytes against ~160,000 |
| the world | a client joins, is sent 25 decorated chunks and stands on the surface |
| the world **grows** | walking east out of the spawn tile makes a chunk the client has never seen arrive — `chunk (6, 0) arrived 13.7s after walking out`, generated on its own thread while the player stood still |
| the world **changes** | a broken block is gone from the *next* encode of that chunk, its section count follows, and so does its heightmap column — `decorated_world.rs`, run against the real generated world |
| break and place | over a socket, against a running server: `ci/dig-probe.py` joins, digs, places and reads back the `block_update` and the `block_changed_ack` for each — including a log clicked on its east face coming back as `oak_log[axis=x]` |
| the state a placement puts down | a stair and a furnace placed from **one** yaw come back facing opposite ways, over the same socket — `oak_stairs[facing=east]` and `furnace[facing=west]`. Neither is a rule; both are vanilla's own answer, looked up per block |
| a command is answered | `/list`'s reply is **byte-identical** to real vanilla's 539 bytes, recorded off the wire with `RUSTMC_DUMP_PACKET=chat` and held verbatim as `crate::commands`'s test vector. `chat`'s projection digests the whole body, so there is no partial credit — and the `commands.list.players` translation takes **three** arguments in 26.2 (online, max, names), which reading `ListPlayersCommand` alone would have got wrong |
| the command tree is declared | The `commands` packet (16) is built from a literal node tree and filtered per player by `fillUsableCommands`, and its bytes are gated at **all five** permission levels against bytes vanilla's own encoder produced for the same tree (`ci/commands-golden.sh`). `/seed` is absent below GAMEMASTERS and carries flags `0x25` above, which is what makes the permission half a gate that can fail rather than a constant |
| a player says something | `ServerboundChatPacket` (9) is read whole and broadcast as `player_chat` (65) to every recipient whose `chatVisibility` is FULL, each with its own `globalIndex`. The clientbound body carries vanilla's `Instant.now()`, so it is the one packet in the corpus that cannot be digested whole — `tools/oracle-runner`'s `player_chat` arm consumes exactly to the timestamp and leaves the rest exact |
| a block update **cascades** | a torch stood on a block is gone from the wire the moment the block under it is broken — two `block_update`s from one dig, `ci/dig-probe.py` against a running server |
| something **ticks** | a coral placed out of water turns into its dead block a few seconds later, with the probe sending nothing in between — the first `block_update` this server has ever emitted that no packet asked for |

The last row is the one that took the most correcting, and all of it was
about *when* rather than what.

A chunk cannot be generated on demand and answered immediately. Carvers read
a 17x17 neighbourhood and decoration writes one chunk outwards, so a chunk is
only finished once every chunk that can reach it has been; answering straight
away hands out a chunk whose trees have not been planted, and the world ends
up depending on the order players walked through it. `pipeline` already
states the rule — generate a margin, offer only the inside — so the server
generates a square up front, and grows by generating more of them.

That square costs **24 seconds** for 25 chunks through every stage — a
figure measured when the margin was one ring, so 49 chunks generated.
`DECORATION_MARGIN` became **2** on 2026-07-27, because one ring was
measurably wrong (ORACLE.md rule 14), and the same request is now 81 chunks.
So treat the 24s as a floor rather than a current reading: it is left
unscaled on purpose, because a number this document asserts should come from
a run and not from arithmetic.

Generating it during login put those seconds on the client's
clock and the join failed at exactly twenty, which is the client's read
timeout and not a fault in any packet it was waiting for. It is now built
before the socket opens, which is what vanilla means by "Preparing spawn
area", and one world is shared by every player rather than one generated
each.

The pattern worth keeping: **capture vanilla's bytes and reproduce them.**
`javap` gives a record's fields, not its wire order or widths. Three fields
of the join packet would have been wrong from the field list alone — a
`Holder` that is really a bare registry id, a *signed* byte holding −1
between two booleans, and an `i32` where varints are the norm — and each
desyncs everything after it while the client blames some later field.

## What it does not do

Named here rather than left to be discovered from a client that hangs.

- **All seventeen overworld structure sets of the twenty, and this bullet said
  "No structures" until 2026-09-05, "seven" until 2026-10-01, "fourteen" until
  woodland mansions landed, "fifteen" until ancient cities did and "sixteen"
  until ocean monuments did.**
  `World::generate_through` calls `worldgen_assembly::generate::generate` — the
  same function `tools/worldgen-dump` drives — so every overworld set that call
  site lays out runs here exactly as it runs in the dump, and the dump and the
  server are one configuration rather than two.

  Served: mineshafts, villages, trial chambers, ocean ruins (warm and cold),
  shipwrecks (submerged and beached), ruined portals, buried treasure,
  igloos, swamp huts, jungle temples, desert pyramids, pillager outposts,
  trail ruins, strongholds, woodland mansions, ancient cities (whose sculk
  patches run on the city's own piece stream) and ocean monuments. Nether
  and End structures follow their own dimensions' caveats.

  The entities generation spawns are in the level too
  (`crate::structure_entities`): a mansion's evokers, vindicators and
  allays, a monument's elder guardians, an end city's shulkers and elytra
  frame, a mineshaft's chest minecarts, a swamp hut's witch and cat and an
  ocean ruin's drowned.
  They are tracked and sent to clients, saved into `entities/` with the
  chunk, and have no AI: they do not move, attack, take damage or despawn.
  Template entities (igloo villagers, outpost cage allays, village
  villagers) are not spawned.

  Each served set has a window in `crates/rustmc-server/tests/served_structures.rs`
  that decodes the `LevelChunkWithLight` the packet writer is handed and finds
  that set's marker blocks in it, against a `Through::Carvers` control over the
  same window that must find none. The block-level parity of the sets ported
  on `structures-live` is in `docs/cold-start/structures-live.md`, measured in
  each structure's own saved piece boxes against clamped twin vanilla
  harvests: every one of them reads at or under its own twin floor.
- **All forty-nine block-entity types are registered; what each one *does*
  is a separate and much longer list.** This bullet said "No block entities"
  until 2026-08-06 and "Three block-entity types, out of upwards of forty"
  until 2026-09-05, and it is corrected in place rather than deleted because
  the shape of what is missing keeps changing while the fact of it does not.

  What is true as of 2026-09-05: every block vanilla attaches a block entity
  to — 186 of them, checked against the bootstrapped registry by
  `ci/block-entity-golden.sh` — gets one here, with the right type and the
  right container size, and **the compound a freshly placed one saves is
  vanilla's, key for key, tag width for tag width and value for value** on all
  47 types the extract can construct. A chest keeps what is put in it, across
  a restart and across the retirement of the tile it stands on, and so now can
  a barrel, a hopper, a dispenser and the other thirteen containers.

  The two the extract cannot construct are `minecraft:piston` and
  `minecraft:vault`, which throw in a JVM with no server, and both are
  declared as writing nothing beyond the metadata — a stated unknown rather
  than a transcription. `PistonMovingBlockEntity` in particular does save
  state and none of it is written here.

  What is **not** true: every one of them ticks. The state of the rest is
  there and saved; the mechanism that changes it is not.

  **The furnace family and the brewing stand do, as of 2026-10-01**
  (`be-furnace-brewing`). A furnace, blast furnace or smoker lights, burns
  its fuel (a lava bucket leaves its bucket), cooks against the recipe table
  and writes `lit`; a brewing stand takes blaze powder as twenty brews, shows
  its bottles on the block, brews for 400 ticks and sends `level_event 1035`.
  An open menu over either is swept every tick (`crate::click::
  sweep_container`), so the client sees the fuel go, the result arrive and
  the progress bar move as `container_set_data`. The menus are their own:
  `FurnaceFuelSlot`, `FurnaceResultSlot`, the three brewing slot classes and
  both menus' `quickMoveStack` are transcribed (`crate::menuslots`). Brewing
  is exact for every stack this server can hold — a component-less potion has
  `PotionContents.EMPTY`, which `PotionBrewing.mix` returns unchanged — and
  nothing more: an awkward potion or a water bottle cannot be represented.
  **Not here:** the smelting experience. `FurnaceResultSlot.onTake` awards it
  as experience orbs, and this server has no orb entity, so taking an ingot
  out of a furnace drops no XP. Both block entities' counters are saved
  from the live ticker state and reloaded onto it, so a burning furnace
  survives a restart or a retired tile still burning.

  **Hoppers, droppers, dispensers and crafters do, since branch
  `be-hopper-dispense`.** A hopper pushes into the container it faces and
  pulls from the one above — or, with nothing above, absorbs the item
  entities in `Hopper.SUCK_AABB` — on vanilla's eight-tick cooldown, and a
  lever beside it writes `enabled=false`. A dispenser or dropper triggers
  on a rising signal (the block above counts), fires four ticks later, and
  sends 1000/2000 or the dry 1001; a dropper fills the container in front
  instead of dropping. A crafter crafts its 3x3 into an item in front (or
  the container there) with 1049/2010, or clicks 1050. Of the
  `DispenseItemBehavior` registry only the flint and steel is ported: every
  other registered item — arrows, buckets, bone meal, shulker boxes,
  minecarts, boats, armour — is dropped as an item where vanilla fires,
  pours, grows or places it. `crate::dispenser`'s module doc has the rest,
  including what a dispensed item's unseeded draws do to a comparison. A block whose type is missing keeps no state and every later
  assertion about it measures an empty position — that hazard is closed. The
  one that replaces it is narrower and should be read as narrower: the state
  is correct and static.

  A chest that generation placed with a loot table now opens stocked
  (branch `loot`, 2026-10-01): the worldgen side channel's record of
  `setBlockEntityLootTable` reaches the block-entity store
  (`crate::lootroll::adopt_generated`), and the first open runs
  `RandomizableContainer.unpackLootTable` -> `LootTable.fill` on the
  record's seed. A chest a creative player places with `block_entity_data`
  carrying `LootTable`/`LootTableSeed` does the same, and a seedless one draws
  from the table's server-wide random sequence (`--seed`). What it is not:
  the rolled stacks carry no components (an enchanted book arrives without
  its enchantment), a hopper or comparator reading an unopened chest does not
  unpack it, and only chests and trapped chests have a menu to open.

  Twelve of the twenty types that override `getUpdatePacket` put a
  `ClientboundBlockEntityDataPacket` on the wire here; the other eight are
  named in `crate::blockentity::BlockEntity::update_tag` with the reason each
  is refused rather than guessed. Neither `ChestBlockEntity` nor
  `AbstractFurnaceBlockEntity` overrides `getUpdatePacket` in the pinned jar,
  so its base `return null` stands and a chest changing state broadcasts a
  `block_update` and nothing more — as do the barrel, the hopper, the lectern
  and the other twenty-six types in that position.

  **That sentence stopped being a prediction on 2026-08-05.** It named the
  observation that would expose this and there was no verb able to make it:
  the scenario language could open a container and could not click inside one,
  so four checks put a screen on the wire and none could put an item anywhere.
  `Step::ContainerClick` — `ServerboundContainerClickPacket`, packet 18 — is
  that verb, and the checks are `ci/checks/gameplay/560` and `562`. Vanilla
  answers each click with a `container_set_slot` correcting the slot that
  moved; this server answers neither, and both diverge at the **first click**
  rather than at the open. A crafting table is the sharper of the two: vanilla
  fills menu slot 0 with four `oak_planks` because `CraftingMenu.slotsChanged`
  ran the recipe, and `data/vanilla/recipes.json` is in this tree and read by
  nothing.

  What makes those checks able to fail at all is a property of the verb rather
  than of the files. `changedSlots` and `carriedItem` in that packet are the
  *client's prediction*, hashed; `handleContainerClick` writes them into the
  menu's remote-belief table and `broadcastChanges` then sends a correction
  **only where the truth differs from what the client is believed to think**.
  A harness that predicted correctly would be answered with silence by
  vanilla, and every storing and crafting check would report `match` against a
  server that has no containers. The harness claims nothing — an empty map and
  an empty carried item, which is the only honest claim something with no
  inventory model can make — so the server's answer is maximal.

  The degenerate half is `561`, which is `560` with one number changed so that
  both clicks land on an empty slot with an empty cursor. Vanilla runs the
  whole click path and sends nothing, and so do we, so it is `match`. It
  forbids the cheap wrong fix — correct the clicked slot on every click — and
  that was watched failing before it was locked: with such an arm in
  `world_send.rs`, `561` went `match` -> `diverge` ("we also sent:
  container_set_slot") while `560` and `562` stayed `diverge`. The arm was
  reverted.
- **A command tree of literals, and no argument nodes.** As of 2026-08-06
  there *is* a registry (`crate::commandtree`, `crate::commands`): a literal
  node tree with per-node permission levels, vanilla's `fillUsableCommands`
  filtering, the `RESTRICTED` flag, the breadth-first `enumerateNodes`
  numbering and the `commands` packet itself, sent on join after the
  `24 + level` permission entity event. `list`, `list uuids` and `seed` are
  registered, each dispatchable and each answering bytes gated against
  vanilla — `list` against 539 bytes of real wire, the other two against
  bytes from vanilla's own component codec (`ci/commands-golden.sh`).
  **What is still missing:** argument nodes, and with them the ~50
  `ArgumentTypeInfo` serialisers, redirects and custom suggestions — `Node`
  has no argument variant, so an argument command fails to compile rather
  than mis-serialising one. There is no `chat_command_signed`, no command
  spam throttle, and an unknown command is still answered with **silence**
  rather than the two components `Commands.finishParsing` sends, because
  inventing bytes for a channel the oracle digests whole is worse than
  sending none.
- **Chat exists as of 2026-08-06, and did not before.** There was no
  serverbound chat packet id at all, so a chat message was not merely
  unhandled but unrepresentable. `crate::chat` reads
  `ServerboundChatPacket` (9) whole and broadcasts `player_chat` (65) — not
  `disguised_chat`, which is for system-sender messages.
  **What is deliberately absent:** the entire signed-chat path. It is
  unreachable on this server, not skipped: the listener installs
  `SignedMessageChain.Decoder.unsigned(uuid, server::enforceSecureProfile)`,
  that decoder discards the signature when the supplier is false, and
  `enforceSecureProfile()` is false unless `online-mode` is on — which it is
  not here. `docs/intel/chat-and-commands.md` §2 lists what an online-mode
  server would still owe.
- **A player cannot die, and the whole death and respawn path is transcribed
  rather than served.** There is no health value, no damage source and no
  `/kill`, so nothing on this server can take a player to zero; `crate::death`
  and `crate::respawn` hold the 26.2 path from the pinned jar — the
  `player_combat_kill` packet and its message ladder, the team-visibility
  fan-out, the experience cap, the death-drop scatter, `restoreFrom`'s
  thirty-one-field copy list, the respawn packet order, and the bed/anchor
  position ladder — with unit tests and one byte-exact control, and no live
  call site.

  **The corpus cannot reach any of it either**, and that is measured rather
  than assumed: no `Step` verb damages anybody, the per-scenario console arm
  refuses anything but `gamerule <rule> <value>`, `Step::Command` runs as a
  non-operator so a level-2 `/kill` is refused by vanilla too, and nothing
  anywhere constructs `minecraft:client_command` (12), so `PERFORM_RESPAWN`
  cannot be sent and vanilla would sit on its death screen for the rest of a
  recording. So **no death or respawn check is registered**; one written today
  would compare two silences.

  One thing did move on the wire: `ClientboundLoginPacket`'s `showDeathScreen`
  is the **negation** of the `immediate_respawn` game rule, and it was a
  literal `true` here — along with `reducedDebugInfo` and `doLimitedCrafting`
  — so `/gamerule immediate_respawn true` set a value nothing read. All three
  come off `GameRules::shared()` now. `docs/cold-start/death-respawn.md`.
- **`neighborChanged` exists, and so does `affectNeighborsAfterRemoval`.**
  This bullet said "no `neighborChanged`" until 2026-09-05, a month after
  `crate::power` landed `CollectingNeighborUpdater`, the wire's default
  evaluator, the torch, the repeater, the observer and the lamp. What was
  still missing on that date and is now in: the removed state's
  `affectNeighborsAfterRemoval` (`crate::removal`, `LevelChunk.setBlockState`
  offsets 440-490), the lamp's off tick, the comparator and its analog reads
  (`crate::comparator`), the piston's `moveBlocks` with its
  `PistonMovingBlockEntity` tick and `finalTick` (`crate::pistonmove`), the
  tick queue's `willTickThisTick`/`hasScheduledTick`, the repeater's LOCKED
  arm of `updateShape`, `DiodeBlock.setPlacedBy`'s tick and
  `Level.updateNeighbourForOutputSignal`. Still absent, each named where it
  would go: pressure plates (nothing can stand on one), the daylight
  detector's ticker (`EnvironmentAttributes` and sky light), entity pushing
  by pistons, item frames as comparator inputs.
- **Right-clicking a block reaches the block, but only one block answers.**
  The *ordering* is now vanilla's: `ServerPlayerGameMode.useItemOn` offers
  the click to the block first and only falls through to placement if the
  block declines, and `crate::useblock` carries that. Until 2026-08-03 this
  server placed immediately and never consulted the block at all, which was
  an inversion rather than a gap — right-clicking a lever with a block in
  hand flips the lever on vanilla and placed the block here.

  **The answers come from an extraction, not from hand ports.**
  `ci/use-golden.sh` asks every state of every block what a right-click does
  and writes `data/vanilla/use_without_item.json`: **247 blocks with
  behaviour, 7,718 of 32,366 states with something worth recording**, identical
  across two runs. Every write in it is at the clicked block — 4,850 of 4,850 —
  so a door's second half is not written by `useWithoutItem` at all and comes
  from the neighbour update instead.

  **The item-aware half exists as of 2026-08-12, for one class.**
  `ServerPlayerGameMode.useItemOn` asks the block *twice* — offset 145 with
  the stack, offset 196 without — and only the second had a table.
  `data/vanilla/use_item_on.json` is the first: 1,537 items over 4,537
  states, 92.1M probes, 163 blocks with behaviour. `crate::useitemon` reads
  it and answers for the classes in its `PORTED_CLASSES`, which is
  `FlowerPotBlock` and nothing else — **39 blocks**, the empty pot and its 38
  potted forms. Applying all 163 at once would turn 162 unmeasured blocks on
  in one commit, so the list is explicit and widening it is a deliberate act.
  Everything not in it answers `TryEmptyHandInteraction`, which is
  `BlockBehaviour`'s own default and exactly what this server did before.

  With it, a flower pot fills and empties, and `FlowerPotBlock.useWithoutItem`
  hands the flower back through `Player.addItem`. That last part is not in
  either table — `use_without_item.json` records the block write and has no
  column for an item — and the pairing is the inverse of `POTTED_BY_CONTENT`,
  read off the empty pot's own 38 `except` groups and corroborated 38 of 38
  by `loot_blocks.json`. The slot reaches the client from the **next tick's**
  menu sweep rather than from the handler, which is where vanilla's own
  recording puts it: block updates, then `container_set_slot`, then the ack.

  **This is the commit that made `use_item_on.json` a required table** — the
  first `data/vanilla/` literal under `crates/` to name it — so every tree
  from here regenerates it and pays about 95 s more setup.

  **Where the `mayBuild` gate sits, once there is a step in front of it
  (2026-08-16).** `crate::useblock::use_gate` is at the head of
  `use_without_item_step` and not at the head of the handler, because it is
  offset 0 of the *block's* `useWithoutItem` — `ComparatorBlock`:
  `getAbilities`, `mayBuild`, `ifne`, `PASS` — and `ServerPlayerGameMode`
  reaches that only after asking `BlockState.useItemOn` at 145 and being handed
  a `TryEmptyHandInteraction` at 176. Hoisting it into the caller would gate a
  call vanilla makes unconditionally. **Nothing measures this yet:**
  `may_build` and `game_master` are hardcoded `true`/`false` while this server
  has no per-player ability model, so `use_gate` returns `None` on every call
  and no corpus check can tell the composed order from the wrong one. Whoever
  makes those flags real is the first person who can, and should expect to
  write the check that does.

  It read **184 and 5,941** until 2026-08-18, and the difference is not vanilla
  moving. 90 registry blocks had no row because the probe's own level answered
  air and `null` and the rule took the empty branch; see §80 and
  `docs/cold-start/probe-answering-environment.md`. "States writing something"
  is also the wrong name for the second number now: a cell is kept when it
  opens a screen, edits a block entity, or answers out of an emptiness this
  world has, as well as when it writes.

  **1,516 states refused** during extraction, in four causes: 1,344 because
  `SignBlock` bails out with vanilla's own `Expected to only call this on
  server` (48 signs), 128 for wanting `environmentAttributes` (the 16 beds, 8
  states each), 32 for `getChunkSource` (`minecraft:daylight_detector`) and 12
  for a block entity this harness cannot build (`minecraft:moving_piston`).
  Those are absent from the table and decline, which is "not covered" rather
  than "does nothing".
  Re-derived by `ci/refusal-ledger.py`, which counts them off the table's cells
  *and* off the generator's summary and refuses if the two disagree.

  **This paragraph has now been wrong twice, in the two different ways a
  figure goes wrong, and both are worth keeping.** It named a chest and a
  lectern as its examples of blocks the table cannot answer for and both were
  wrong — found 2026-08-04 by the run that built the container channel;
  `minecraft:chest` is in the table with **24 of 24 states consuming and none
  refused** and `minecraft:lectern` with 16 of 16, which is what made the menu
  channel possible at all. Then it said **199 refusals over 56 blocks and
  three causes**, which was true when written and stopped being true when the
  `Components not bound yet` cause was closed. The count is 160 over 17
  blocks, and nothing noticed for a week.

  **The refusals used to be the smaller half.** 13 of the jar's 54
  `useWithoutItem` overrides had no row in this table at all, covering 90
  registry blocks — 48 signs, 17 candle cakes, 13 fences — every one of them
  the probe's environment rather than the jar, and `crate::useblock` reads no
  row as a decline, so those 90 blocks placed a block on right-click where
  vanilla rings a bell, eats a cake or opens a sign.

  **0 of the jar's 54 `useWithoutItem`
  overrides have no row in this table now, covering **0 registry blocks** —
  every one of the 90 is a row or a refusal.** The level builds the real block
  entity, refuses when vanilla raises its own alarm, records what it answered
  out of an empty world on the row itself, and sweeps two more player stances
  and the second clicked face; see `docs/PARITY-ROADMAP.md` §80. The table went
  184 rows to 274 and lost none. `ci/probe-absences.py` counts what is left from
  the jar, because a class the extraction never reached cannot appear in the
  table, and what is left is **3 overrides over 3 blocks, 0 of them the
  environment**, all on the `setPlacedBy` side.

  **And a row can exist and still be wrong, which neither of those counts
  reaches.** 8 of the jar's probe rules gate on `instanceof ServerLevel`, over
  82 registry blocks; `TableLevel` is not one and cannot be, so the branch is
  taken for every state under them and the rule returns a plausible value
  without running a line. `SignBlock` is the only one vanilla announces, through
  `Util.pauseInIde`, which is the sole reason its 1,344 cells are refusals. The
  other seven are silent: `instanceof` is answered by the JVM against the
  object's class, and a `Level` subclass cannot observe the question, refuse it
  or log it.

  Those cells carry a `gated` column now, listing the fields of that very cell
  the branch decided rather than the block, derived from the bytecode by
  `ci/server-level-gates.py` and stamped by the dumpers, which count it
  independently and must agree. `minecraft:ender_chest` reads
  `"reads": ["air-world"], "gated": ["opens"]`: one cell naming two different
  limitations of the same harness.

  **`useItemOn` is swept too, and it is the larger half.** 15 rules over 138
  registry blocks, 14 silent over 90, and **3,197 cells false in a column the
  table has** — 404 on `useWithoutItem` and `setPlacedBy`, 2,793 on
  `useItemOn`. 2,784 of the second are signs, because `SignBlock.useItemOn`
  does **not** call `Util.pauseInIde`: the same 48 blocks are loud refusals on
  one half of the right-click and silent false rows on the other. See
  `## 90.` in `docs/PARITY-ROADMAP.md`.

  **One of them is a live bug in this server and not a documentation gap.**
  `SweetBerryBushBlock` for `age > 1` drops berries, plays a sound and writes
  `age=1`, all inside the gate, with the `return SUCCESS` outside it. So the row
  says `consumes: true, writes: []`, `crate::useblock` filters on exactly that
  and applies `writes`, and **picking berries eats the click and leaves the bush
  at `age=3`**. Two cells. It is the only one of the eight whose false column
  this module reads — the 362 `opens` cells change no behaviour today, because
  `useblock` consumes `refused`, `from`, `consumes` and `writes` and nothing
  else. See `docs/PARITY-ROADMAP.md` §87.

  Two things the table cannot give, both named rather than left to be found.
  **A scheduled tick's *effect*** — `Block.tick` takes a `ServerLevel` for
  which there is no cheap oracle, so the effect is hand-ported per class and
  only `ButtonBlock` and `CoralBlock` are. *When* the tick fires is no longer
  a gap: the extract carries `tickDelay` and `tickPriority`, so a stone
  button waits 20 and a wooden one 30 without either number being written
  down here.

  **`isSecondaryUseActive` was the other one and is closed as of
  2026-08-10.** It read, until that date: "because there is no sneak flag, a
  sneaking player holding a block flips a lever here and places the block on
  vanilla." That sentence was prose with nothing able to check it until
  2026-08-05, when `ci/checks/gameplay/552` and `553` — one rig, one line
  different — made it a measured pair. The flag arrives in
  `ServerboundPlayerInputPacket` (id 43, `FLAG_SHIFT` = 32), not in
  `ServerboundPlayerCommandPacket`, whose 26.2 `Action` enum has no shift
  entry at all; `crate::player::input_shift` reads it and
  `crate::player::PlayerState::shift_key_down` holds it.

  The gate it feeds is one line of `ServerPlayerGameMode.useItemOn`, offsets
  101-126: `isSecondaryUseActive() && holdingSomething` jumps **past both**
  the item-aware `useItemOn` and `useWithoutItem` straight to the placement
  path, so a sneaking placement never asks the block anything. What is still
  approximate is the conjunct, not the branch: `holdingSomething` reads both
  hands and this server has no offhand stack, so an empty main hand with a
  full offhand is the one case where the two still disagree.

  `placement.rs`'s `chest_type` has a **second** `isSecondaryUseActive`
  branch, on a different method, and that one is still not ported — see its
  own doc comment.
- **A placement writes the blocks `setPlacedBy` writes too.** A door goes down
  with both halves and a bed with its head, from
  `data/vanilla/set_placed_by.json` — 59 blocks, 2,046 of 32,366 states. See
  "And what else it puts down". The same file carries the **class map** for
  the whole registry, which is how every rule here asks what kind of block
  something is without keying on its id.
- **The world grows a tile at a time, and each tile is as finished as
  `DECORATION_MARGIN` makes it — which at seed 0 does not converge.** See
  "The world grows" below. Nothing retires a tile, so a long walk grows
  memory without bound.
- **The clock ticks, and since 2026-09-06 the weather does too.** `tick.rs`
  runs a 20 Hz game clock shared by every connection, and `set_time` goes out
  once a second with the real game time. `crate::weather` is
  `ServerLevel.advanceWeatherCycle` in full — the clear/rain/thunder timers
  with their four `UniformInt` providers, the two level ramps at 0.01f a
  tick, `isRaining`'s `> 0.2` threshold on the interpolated level, and the
  `game_event` sends (START_RAINING 1, STOP_RAINING 2, RAIN_LEVEL_CHANGE 7,
  THUNDER_LEVEL_CHANGE 8) — run at the top of the tick because it is the
  second item in `ServerLevel.tick` and this server has no world border to
  be the first. `/weather clear|rain|thunder [duration]` reaches it through
  the console (`crate::console`), not through chat, because
  `WeatherCommand.register` gates it on permission 2 and the harness's
  players are not operators. A player joining a raining world gets
  `PlayerList.sendLevelInfo`'s three packets before the chunks.
  **What does not run is `tickPrecipitation`** — ice, snow layers and
  cauldron fill — and that is unreachable rather than unported: both ice and
  snow need `getTemperature(pos, seaLevel) < 0.15f` and this server's
  superflat biome is `minecraft:plains` at 0.8. See
  `docs/cold-start/weather.md`. The deadline comes from a fixed start rather than "now plus
  50ms", so a slow tick is absorbed by the next one instead of pushing every
  future tick back, and a tick that misses its slot entirely is skipped
  rather than run late — which is what "Can't keep up" means.

  The clock-update map is filled from the captured registries: the
  `minecraft:overworld` entry of `world_clock` is driven at rate 1.0, so the
  sun moves. The holder is a **plain varint registry id** —
  `WorldClock.STREAM_CODEC` is `holderRegistry`, which resolves through
  `registry(...)` to `idMapper` and writes the id as-is. It is *not* the
  `holder(...)` form with `id + 1` and a zero meaning an inline value; those
  are one byte apart and only one of them is this packet.

  **Scheduled ticks exist** — see "Something ticks" below — but they serve
  one block family, because `Block.tick` takes a `ServerLevel` and there is
  no cheap oracle for it. **Entities are tracked and broadcast as of
  2026-08-04** — see "Entities exist" below — but nothing gives them a
  per-tick behaviour for players and items, so neither *moves* under its
  own power. **Two mobs do, as of 2026-09-05: the pig and the bat.** This
  server declares **five** `EntityKind` constants — `PLAYER`, `ITEM`, `PIG`,
  `BAT` and `EXPERIENCE_ORB` — and `crate::mob` is the `Mob`/`LivingEntity`
  host that drives
  `crates/rustmc-entity`'s goals, controls and navigation from the tiled
  level: `SpawnEggItem.useOn` → `EntityType.create` → the `finalizeSpawn`
  chain puts one in the world, `Mob.tick` → `LivingEntity.aiStep` →
  `travel` → `Entity.move` ticks it, `Mob.checkDespawn` discards it, and
  `ServerEntity.sendDirtyEntityData` carries its data and attributes. The
  other 84 of the 86 types the pinned jar puts in a spawning category — which
  hold **114 subtypes of `Mob`** — are still unreachable by construction, and
  `NaturalSpawner` is absent; the goals, controls and navigation themselves
  (`GoalSelector`, `PathNavigation`, `registerGoals`, `serverAiStep`) are
  main `f4fa28ff8`'s, in `crates/rustmc-entity/src/ai` — see `docs/cold-start/mob-in-world.md` for
  what is deferred inside the two that exist (fluid travel, inside-block
  effects, the light engine). **Damage left that list on 2026-09-05**: a mob
  can be hit, knocked back, killed, and drops its loot and its experience —
  `LivingEntity.hurtServer` (shared with the player's own transcription),
  `dealDefaultKnockback`, `die`, `dropAllDeathLoot` through
  `crates/rustmc-loot`, `dropExperience` through `ExperienceOrb.award`, and
  `tickDeath`'s twenty ticks. `docs/cold-start/mob-combat.md`. An orb and a
  dropped stack still do not tick, and nothing picks either up. For scale, the jar's `Mob` hierarchy is 115 classes
  declaring **3,870 methods**, of which 54 declare their own `registerGoals`
  and 38 their own `customServerAiStep`; `ci/mob-tick-census.py` derives all
  of it from the jar and gates it, and `docs/PARITY-ROADMAP.md` §94 is why
  the crate's own marker set reports that as *one* gap. There are and there are
  no **random** ticks *running*, though the selection layer now exists and is
  gated: `crate::randomtick` reproduces `Level.getBlockRandomPos` against
  vanilla's own draws (`crates/rustmc-server/golden/random_tick_pos.json`,
  1,152 positions) and carries `ServerLevel.tickChunk`'s loop structure.
  **The rate is `random_tick_speed` picks per randomly-ticking SECTION per
  block-ticking chunk per tick — not per chunk** — and the gamerule's default
  is 3, so "3 positions per section per tick" is right only for a chunk whose
  every section is randomly ticking. That module's docs have the three
  reasons it is not wired to the clock, the first being that no
  `randomTick` override is ported: every behaviour behind them (crops, fire,
  ice, leaf decay) is still the same `ServerLevel` problem. Block updates **do** now go out at a tick
  boundary, coalesced per section, with the `block_changed_ack` after them —
  see "The tick boundary" below; that caveat was true until 2026-08-03 and is
  not any more. The `RegionMessage` enum in `lib.rs` describes where the
  threading eventually goes. Five blocks — all five corals — schedule a tick from
  `getStateForPlacement`, and `update_shape.json` marks every carried row
  that asks for one, so the list a first `LevelTicks` has to serve already
  exists.
- **Movement is validated, collided and rubber-banded.** `player.rs` decodes
  all four `ServerboundMovePlayerPacket` forms once (`decode_move`, which
  `apply` is written on top of); `move_player.rs` is
  `ServerGamePacketListenerImpl.handleMovePlayer`'s state machine —
  `firstGood`/`lastGood`, the packet counters, the per-tick `100.0f * i`
  speed budget, `containsInvalidValues`, the clamps, `updateAwaitingTeleport`
  and the teleport counter; `playermove.rs` runs the claimed delta through
  `physics::move_entity(MoverType::Player)` into `collide.rs`'s transcription
  of vanilla's own sweep, applies `isEntityCollidingWithAnythingNew`, and
  answers a refused move with `ClientboundPlayerPositionPacket`; `fall.rs`
  accumulates `fallDistance` and turns a landing into
  `calculateFallDamage`'s number. `ChunkView` still moves the cache centre,
  sends the chunks the player is walking into nearest-first, and tells the
  client to forget the ones behind.

  **Still missing, and each is named where it is refused rather than left to
  be discovered:** `clientIsFloating` and the flying kick (five of its nine
  terms are subsystems this server has no field for); `checkSupportingBlock`
  and therefore `mainSupportingBlockPos`, which costs the block a step sound
  and a landing read on a slab; `isInWater`, so a player descending through
  water accumulates fall distance here and none in vanilla; the eight blocks
  that override `Block.fallOn`; `tryResetCurrentImpulseContext`;
  `checkMovementStatistics`, which needs a stats subsystem. The damage a
  landing produces is computed and is not yet applied — that is
  `origin/player-systems`' `hurt`, and this is the input it said the server
  could not produce.

  The whole path is **opt-in**: `playermove::shared()` returns `None` without
  `data/vanilla/{collision_shapes,movement_props,attributes}.json` and a
  session with `None` writes the client's position onto the player and
  believes it, exactly as this server did before.
- **Offline mode only.** No encryption, no session-server lookup. Deliberate:
  it is network plumbing with a hard external dependency and none of what
  this project is trying to prove.
- **Light is on the wire** — every `World::chunk` packet carries the
  incremental engine's own classification, default-on since 2026-09-05, with
  `RUSTMC_SECTION_LIGHT=0` for a bisect. The reading that bought the default
  is vanilla's own saved nibbles on three generated worlds: **0 of 6,787,072
  differ in the overworld, 0 of 7,409,664 in the nether, 5,609 of 1,863,680
  in the End** (all sky, all in section -1), controls 1,611,341 / 4,686,430 /
  63,837. `crates/rustmc-server/tests/light_harvest.rs`. The other chunk
  producers — `world_send::flat_chunk` and `Terrain::chunk` — still send empty
  masks and are named in their own comments.
- **One connection at a time** — fixed. Each accepted connection gets a
  thread, and the server list reports a live player count rather than a
  constant, counted only once a connection reaches play so a list refresh
  does not make the number flicker. Verified by holding one client's socket
  open and timing a second: answered in 0.00s where it would previously have
  waited out the first session.

  The world is shared and the **generated canvas** is still immutable
  (`World::shared`); what a player changes lives in a lock beside it. So the
  contention is real now and is handled where the data is rather than by
  serialising whole sessions — see "The world is mutable" below.

## The world is mutable

A block a player breaks is gone: for them, for everyone else connected, and
for the next encode of that chunk — which is what a player who walks away and
back, or a player who joins later, actually receives.

**The generated canvas is never written to.** Player writes go into an
overlay (`edits.rs`) and a read is "the overlay, else the canvas". The reason
is not tidiness: the canvas is the object every parity number in this project
is measured against, and if a client and a region diff read the same object
then the first hole anyone dug would move the number. Vanilla draws the same
line — a chunk is generated once and saved thereafter.

What that costs and what it buys, all of it measured by
`crates/rustmc-server/tests/decorated_world.rs` against a real generated
world rather than a fixture:

- a broken block is air in the re-encoded chunk;
- the section's `nonEmptyBlockCount` follows it — a section that miscounts
  renders as thin air or as solid rock, so this is not bookkeeping;
- the column's `MOTION_BLOCKING` heightmap drops by one. The canvas primes
  its heightmaps during generation and they stay right for every untouched
  column; an edited column is walked over the merged view instead, because
  the canvas cannot know about a write it never saw.
- a write outside the served square is refused. The decoration margin is
  generated so the inside can be finished; it is not itself finished and is
  never sent to anyone, so an edit there would be invisible and permanent.

**Which dig packet destroys the block depends on the game mode, and the two
are not interchangeable.** A creative client destroys on
`START_DESTROY_BLOCK` and never sends a `STOP`; a survival client sends
`START` when it begins mining and `STOP` when its own progress reaches one.
**The server does not believe the survival client's STOP** (2026-09-30,
`crate::mining`): it keeps its own clock from the START, destroys at STOP only
when its own progress is at 0.7 or more, and otherwise finishes the dig itself
on the tick its progress reaches one. Until then this server destroyed on any
survival STOP, which made survival mining instant; `004` recorded vanilla
acknowledging an early STOP at once and breaking the block ~520 ms later.

**The sequence number is the part that is easy to skip and fatal to skip.**
Every serverbound block action carries one, and the client holds its own
*predicted* result until the matching `block_changed_ack` comes back. A
server that never acks leaves the prediction standing forever — the block
looks broken to the player who broke it and to nobody else, and no later
correction can dislodge it, because the client is still treating its own
guess as authoritative. So the ack goes out whether or not anything changed;
a **refused** dig is precisely the case the prediction exists for. Order is
vanilla's: the `block_update` first, the ack after, because vanilla records
the sequence during the handler and sends the ack at the end of the tick.

**Players join in creative** (`status::GAME_TYPE`). That is the world's
constraint rather than a preference: the world has no
light data of its own, so a survival player walks off into a void they cannot
fly back out of, and the only way a client can hold a block to place is the
creative inventory, because there is no survival inventory to put one in.
`JoinInfo::default()` stays survival, because the captured vanilla join
packet it is checked byte-for-byte against is.

**Writing to another player's socket needed a lock, and not an obvious one.**
Until a block could change, no thread ever wrote to a socket it did not own.
Two threads writing frames to one `TcpStream` do not produce two packets in
an unexpected order — they produce one packet made of both, and the client
disconnects with a protocol error several frames later, blaming whatever it
was reading at the time. So a connection's outbound half is a mutex around a
writer (`players.rs`) and every packet after the join goes through it; the
inbound half needs no lock, because exactly one thread ever reads a socket.
`players.rs` has a test that hammers one connection from four threads and
asserts every frame arrived whole.

### Placing, and the thing that blocked it

`use_item_on` says *where* and never *what*. The client sends it whether or
not the hand is empty, so a server that does not track the hand knows a
placement was attempted and not what to place — which is why placing came
after breaking rather than with it.

What tracks it is nine slots and a selection (`player.rs`), fed by
`set_creative_mode_slot` and `set_carried_item`. Not an inventory: creative
is the only way this server puts anything in one. Two traps, both read from
the jar:

- **the creative slot is a `InventoryMenu` index, not a hotbar index.**
  `USE_ROW_SLOT_START` is 36, so hotbar slot 0 arrives as 36. Reading it as a
  hotbar index puts everything a player picks into a slot they can never
  select, and every placement afterwards silently does nothing.
- **the item id needs a registry the extract did not have.** `Item.STREAM_CODEC`
  is `holderRegistry`, so the wire carries a plain varint id and nothing
  else. `ci/dump-items.sh` pulls the 1,537 `minecraft:item` entries out of
  vanilla's own registry report into `data/vanilla/items.json` — the same
  category as every other file there, extracted rather than transcribed.
  Without it a player can dig and not build, and the server says so on the
  connection rather than leaving it to be inferred from placements that do
  nothing.

Where the block goes is `BlockPlaceContext`'s rule, cut to the part that
decides position: click a **replaceable** block — tall grass, water, snow —
and the placement takes that block's own position; otherwise it goes one step
along the clicked face's normal, and that position must itself be
replaceable. Without the first case, a player who clicks the grass they are
standing in gets a block one step from where they aimed.

### What state a placement puts down

Not always the default, and **not a rule**. `getStateForPlacement` is per
block *class* and `block_props.json` carries block *states*, with no idea what
class produced them. Every block with a horizontal `facing` looks identical in
that table, and

```text
  StairBlock            FACING = context.getHorizontalDirection()
  AbstractFurnaceBlock  FACING = context.getHorizontalDirection().getOpposite()
```

are opposites. A generic "set `facing` from the player's yaw" would be
*backwards* for half the blocks that have it — wrong-and-confident rather than
uniformly unfinished, which is the worse of the two. So the answer is
extracted per block, the way every other vanilla behaviour here is.

**`tools/goldengen/vanilla/PlacementGolden.java`** clicks every block in
`BuiltInRegistries.BLOCK` over a grid — six clicked faces x four yaws x three
pitches x two cursor heights, 144 cells — and writes down what
`getStateForPlacement` returned. `ci/placement-golden.sh` runs it twice,
demands the two runs agree, and produces `data/vanilla/placement.json`.
`crate::placement::PlacementTable` reads it; the server logs the row count on
every connection, because "not implemented" and "the extract is missing" look
identical from a client.

Measured 2026-07-28 against the 26.2 jar:

| | |
|---|---|
| blocks asked | 1,196 |
| rows in the file | 666 — the other 535 answer their own default for every cell and are summarised, not spelled out |
| yaw-only, so the table is exact | 1,183 |
| pitch-sensitive, so it is not | 10 |
| random | 3 |
| neighbour-sensitive | 195 |
| schedule a tick while computing a state | 5 |
| refuse at least one cell (`null`) | 126 |

**The axes are derived vanilla quantities, not raw angles**, and that is what
makes 144 cells exact rather than a sample. The yaw column is
`Direction.fromYRot(yaw)` — the same rounding-to-nearest-cardinal
`getHorizontalDirection()` does — so a real yaw of 47 degrees lands in the
column sampled at 90 and the column is the answer for both. The cursor column
is `clickLocation.y - getClickedPos().getY() > 0.5`, and that threshold was
read rather than assumed: of the 117 classes implementing
`getStateForPlacement`, exactly three touch the click location — `SlabBlock`,
`StairBlock`, `TrapDoorBlock` — and all three compare against 0.5.

**Three kinds of block the table cannot describe, each detected and each
falling back to the default rather than guessing:**

- **pitch-sensitive** (10: dispensers, droppers, both pistons, observers,
  barrels, crafters, the three command blocks). These read
  `getNearestLookingDirection()`, whose boundary between "looking level" and
  "looking up" depends on the yaw as well as the pitch, so no column indexed
  by a rounded yaw can hold it. The pitch rows are in the file for whoever
  ports `Direction.orderedByNearest`.
- **random** (3: weeping, twisting and cave vines).
  `GrowingPlantHeadBlock.getStateForPlacement` is
  `AGE = level.getRandom().nextInt(25)`. A table cannot hold a die roll, and
  **the two-run check is what found them** — the file refused to agree with
  itself, which is exactly what that check is there for.
- **clicked into open air.** Every cell was measured with one stone at the
  clicked position and air everywhere else, so the 195 blocks whose answer
  moves once they have neighbours — fences, walls, panes, redstone, the second
  half of a double chest — get the isolated answer. That is the right answer
  in the open and the same one the default gives, so it costs nothing yet.

Two properties are still computed rather than read. `waterlogged` depends on
the *world* and the grid's world has no water in it, so it is applied on top of
whatever the table returned; every `SimpleWaterloggedBlock` sets it from the
fluid at the position being placed into, and only a **source** counts — the
test is `getType() == Fluids.WATER`, and flowing water's type is
`FLOWING_WATER`. `axis` is in the table and also still computed, which is
idempotent, and is what answers a block the file does not carry:
`RotatedPillarBlock.getStateForPlacement` is exactly
`defaultBlockState().setValue(AXIS, context.getClickedFace().getAxis())`.

**A `null` return is now carried through as a refusal.** Vanilla returns null
for a door with no room above or a crop on no farmland; the server places
nothing and still sends the ack, because a refusal is precisely the case the
client's prediction exists for.

The guard that makes the computed properties safe: a property is only
overridden if the default state already has it, and the result is only used if
the block table knows it. So `minecraft:nether_portal`, whose axis takes x or z
and never y, falls back to its default rather than becoming a state with no
network id — which would fail the *next* encode of the whole chunk rather than
the one placement that caused it.

#### And what else it puts down: `setPlacedBy`

`getStateForPlacement` describes **one** block, and for 1,040 of the 1,054
block items that is the whole placement. It is not for a door or a bed.
`BlockItem.place` does not stop at the write:

```text
  state = level.getBlockState(clickedPos)        // re-read after placeBlock
  if state.is(placementState.getBlock()):
      state.getBlock().setPlacedBy(level, clickedPos, state, player, stack)
```

and 16 classes override that call. Four of them write a *second* block the
placement state never mentioned:

```text
  DoorBlock.setPlacedBy   setBlockAndUpdate(pos.above(),
                                            state.setValue(HALF, UPPER))
  BedBlock.setPlacedBy    setBlockAndUpdate(pos.relative(FACING),
                                            state.setValue(PART, HEAD))
```

`tools/goldengen/vanilla/PlacedGolden.java` and `ci/placed-golden.sh` ask each
state what `setBlock` calls it makes, against **`UseGolden`'s level** rather
than a second one — both dumpers ask the same question of the same world, so
two levels that answered differently would be a difference between the tables
rather than between the mechanics. Measured 2026-08-03: **50 blocks with
behaviour, 1,684 of 32,366 states doing something**, 12 refusals (both
pistons, on `getChunkSource`), 0 random, identical across two runs. Those three
counts still hold on 2026-08-18, re-derived rather than re-quoted.

**What no count in that sentence can see is the nine classes that produce
nothing.** The jar declares sixteen `setPlacedBy` overrides; this table carries
rows for six of them and a refusal for a seventh. Of the remaining nine, one is
a real no-op — `PitcherCropBlock` is `0: return`, cancelling
`DoublePlantBlock`'s upper half — one is out of scope (`DriedGhastBlock` plays
a sound and writes nothing), and **seven are the probe's environment**:

```text
  CommandBlock (3)   getBlockEntity -> null, the instanceof pattern fails
  DiodeBlock (2)     shouldTurnOn reads the input side of an air world
  JukeboxBlock       ItemStack.EMPTY.get(BLOCK_ENTITY_DATA) -> null
  MossyCarpetBlock   createTopperWithSideChance finds no sides
  StructureBlock     getBlockEntity -> null
  WitherSkullBlock   checkSpawn finds no wither pattern
  WitherWallSkullBlock  the wall form of the same call
```

`DiodeBlock` is the one that matters and it is the reason
`ci/probe-absences.py` exists. Its whole body is `if (shouldTurnOn(...))
level.scheduleTick(pos, this, 1)`. `UseGolden.TableLevel.getBlockState` answers
air for a position it was not given rather than throwing, so `shouldTurnOn` is
false, nothing is scheduled, and there is **no row for `minecraft:repeater` or
`minecraft:comparator`**. A server driven by "no row, no tick" puts down a
repeater that never wakes when it is placed already powered — and the table and
the server then hold the same absent input, so they agree, and agreeing is all
any check between them can measure. That is why the override set is derived
from the **jar** and never from the table.

The tick rows are the other half of the same reading. There are **24 and all of
them are `crafter`** — `crafting=true, triggered=true`, delay 4, `NORMAL`, at
the block's own position. Not because the crafter is the only block that
schedules on placement, but because it is the only one whose condition an empty
world can satisfy.

**The offset is not a constant, and that is the trap the module is built
around.** A door's second block is always `above`; a bed's is
`pos.relative(FACING)`, four different offsets. A consumer that hard-coded
`[0, 1, 0]` would pass every door test that will ever be written and be wrong
about every bed, so the offset is carried per state and both the shell oracle
and the unit test require two states differing *only* in `facing` to produce
two different offsets.

The **flags** are carried per write too. `Level.setBlock` gates the shape
cascade on `UPDATE_KNOWN_SHAPE` being clear and the neighbour path on
`UPDATE_NEIGHBORS` being set, so the same write with different flags is a
different event: `setBlockAndUpdate` is 3, `DoorBlock.useWithoutItem` writes
with 10. A write with no `flags` is refused at load rather than defaulted.

One limit, named rather than papered over: **`minecraft:small_dripleaf` is
`worldSensitive`**. `DoublePlantBlock.copyWaterloggedFrom` reads the fluid at
the position it is about to write, so the extract's `waterlogged=false` is
right for a placement into air and wrong for one into water. Every corpus
placement is into air. The detector that found it is itself worth a line —
see "a check that could not fail" below.

#### The upper half, and a gap that reads as silence

Placing both halves is not enough for the door checks that *interact* with
one. `update_shape.json` carries **32 rows for `oak_door`, every one
`half=lower, dir=DOWN`, and zero for `half=upper`.** A row is one `to` per
neighbour class, and the upper half's rule is

```text
  if (dir.getAxis() == Y && (half == LOWER) == (dir == UP)):
      return nb.getBlock() instanceof DoorBlock && nb.getValue(HALF) != half
           ? nb.setValue(HALF, half)          // the WHOLE neighbour state
           : AIR
```

— an answer that is a *function of the neighbour*, which one row cannot hold.
The pair is not in the dump's `refused` list either: eleven buckets, none
naming a door. **Nothing in the extract's own diagnostics says the gap is
there.** So it is a typed rule in `crate::shapes`, consulted only where the
table has no answer — that order matters, because a hand port that shadowed a
row would be an extraction quietly overridden.

**One rule per class and not a family rule, because the family disagrees.**
Read rather than assumed:

```text
  DoorBlock         nb.setValue(HALF, half)     the neighbour's whole state
  BedBlock          state.setValue(OCCUPIED, nb.getValue(OCCUPIED)), else AIR
  DoublePlantBlock  AIR if the other half is gone, else no change
```

A single "double block" rule would be right about one of the three.
`DoorBlock` and `BedBlock` are both ported as typed rules; `DoublePlantBlock`
needs none, because its only non-self answer *is* air and a row holds that —
`update_shape.json` carries 2 rows for `sunflower`, both halves at `dir=DOWN`.

**One gap in that, named rather than left to be found.** Those rows are
`dir=DOWN` only, so digging the *lower* half of a tall flower removes the
upper — which is what `237-digging-a-tall-flower-takes-both-halves` measures —
and digging the **upper** half leaves the lower standing, where vanilla
removes it. The rule is symmetric in the bytecode
(`(half == LOWER) == (direction == UP)`) and the extract is not. No check
covers the upper-half dig; adding one would report `diverge` today.

> **2026-08-04 — the check was added, and the prediction above was right
> about the verdict and wrong about the mechanism.**
> `238-digging-the-top-of-a-tall-flower-takes-the-bottom` was written first
> and observed `diverge` before anything was fixed, which is the only order
> that proves a check can fail. But a **dig never reaches `updateShape`**.
> `ServerPlayerGameMode.destroyBlock` calls `playerWillDestroy` *before*
> `Level.removeBlock`, and `DoublePlantBlock.playerWillDestroy` sends a
> creative player — `player.preventsBlockDrops()` — into
> `preventDropFromBottomPart`, which clears the lower half itself with
> `setBlock(below, .., 35)` and never consults a shape rule at all. So the
> missing row and the failing check are **two different defects that happen
> to share a symptom**. The fix is `crate::shapes::player_will_destroy`,
> ported from that method; the `dir=UP` gap in the extract is still open and
> a dig cannot reach it. Anything that removes a double plant's lower half
> *without* being a player break — a piston, a fluid, the support going —
> still leaves the upper standing, and no check covers that yet because the
> corpus has no way to set it up.

The bed is the one worth reading twice, because it differs from the door in
**three** independent ways and a rule generalised across the pair gets each of
them wrong:

  * the direction is **horizontal and per state** —
    `getNeighbourDirection(part, facing)` is `part == FOOT ? facing :
    facing.getOpposite()` — where the door's is the Y axis;
  * the answer is the **subject's** state with `OCCUPIED` copied, where the
    door returns the *neighbour's* state with `HALF` put back;
  * the match is `nb.is(this)`, the **same block**, where the door's is
    `instanceof`. A red bed's foot beside a blue bed's head becomes air rather
    than inheriting anything.

#### Building the harness: three things the jar made non-obvious

Each cost a run, and each is the kind of thing that only reading the bytecode
settles.

- **`LivingEntity.getViewYRot(1.0F)` returns `yHeadRot`, not `yRot`.**
  `getHorizontalDirection()` is `player.getDirection()`, which reads `yRot`;
  `Direction.orderedByNearest` reads `getViewYRot`. Aiming the harness player
  with `setYRot` alone left every `getNearestLookingDirection()` block
  answering as though the player faced south from all four yaws — while
  stairs and furnaces looked perfect, because they read the other field. A
  real player's head and body are both driven from the move packet; a
  synthetic one has to be told twice.
- **An `ItemStack` cannot be built after `Bootstrap.bootStrap()`.** Item data
  components are bound by `DataComponentInitializers` during a *datapack*
  load, so `new ItemStack(item)` throws "Components not bound yet". The
  context carries `ItemStack.EMPTY` instead, and that is safe because of the
  117 classes mentioning `getStateForPlacement`, exactly two touch the stack
  inside it — `LecternBlock` and `TestBlock` — and both treat an absent
  component as the ordinary case.
- **The `Level` throws for everything it does not model, and that is the
  design** (the same argument `GenLevel` makes for worldgen). Three of its 29
  abstract methods turned out to be on the placement path, and each announced
  itself by name instead of quietly returning a plausible blank:
  `BedBlock` asks for the `WorldBorder`, `DirtPathBlock` sweeps the entity
  index through `Block.pushEntitiesUp`, and `CoralBlock` **schedules a tick**
  before returning a state. That last one is where the "nothing ticks" item
  has to start looking.

~~**Item to block is by name, and that is a heuristic.**~~ **Fixed in
`a8aca42`**: the placement dump now carries `BlockItem.getBlock()` for every
item — 1,054 place a block and 14 do it under another name, so
`minecraft:redstone` places `redstone_wire`. An item absent from the map
places nothing, which is the right answer for a sword and is now a statement
rather than an accident of the name not matching.

Still missing, and named: no block *entities*, so a chest is still a chest
shape with nothing in it — it opens a screen and moves its lid as of
2026-08-04, and its twenty-seven slots are empty every time, because the
openers counter that drives the lid is per-position state and not a
container; no drops; a door places only its lower half, because
`DoorBlock` places the upper one itself with a second `setBlock` that
`getStateForPlacement` does not describe; no entity collision beyond refusing
to place into your own two-block column.

**The overlay no longer dies with the process — but only if you ask.** Pass
`--world <dir>` and the server writes every chunk a player has changed into
`<dir>/dimensions/minecraft/overworld/region` as real Anvil, on a timer
(`--autosave-seconds`, default 300, vanilla's own 6000 ticks), and loads them
back before a tile is ever served. Without the flag nothing is written, which
is deliberate: every gate here boots this server in a scratch directory
against a vanilla one, and a subject that remembered the previous trial would
make a differential harness measure the wrong thing.

What lands on disk is a world **vanilla can open**, and that is a measured
claim rather than an inherited one — `ci/t2-vanilla-loads-ours.sh` writes
through our writer, boots a real vanilla server on the result, and reads the
answer out of vanilla's own console. **Block entities are saved as of 2026-08-05** and this paragraph said
otherwise until 2026-08-06: `World::save_to` walks the union of the edited
chunks and the chunks holding block entities — the union rather than the
first alone, because a chest *loaded* from disk and then filled has no block
edit at all and would be lost on the second restart rather than the first —
and writes each one's compound into the column. So a chest survives a restart
as a chest **and** as its contents, and a sign as its two texts.

**The world directory is a world vanilla opens, as of 2026-09-05, and that is
also measured rather than inherited.** `--world` used to write region files
into a directory with no `level.dat`, which `LevelStorageSource` treats as a
world that does not exist — so the sentence above was true of the *file* and
false of the *world*. It now writes `level.dat`, `session.lock` (three bytes,
U+2603, plus vanilla's own advisory lock, so the two servers exclude each
other) and the nine `data/minecraft/*.dat` files that 26.2 split out of
`level.dat`: the seed lives in `world_gen_settings.dat` now, the game rules in
`game_rules.dat`, the time of day in `world_clocks.dat`.
`ci/t2-vanilla-opens-our-world.sh` boots a real vanilla server on a directory
our writer produced and makes it report the seed back — with
`server.properties` carrying a *different* one, so the number can only have
come out of our file — and checks `ServerBrands` afterwards reads
`rustmc,vanilla`. Its `--negative-control` deletes that file and requires the
decoy instead.

**A player comes back where they left**, in `players/data/<uuid>.dat` — 26.2's
path, not `playerdata/`. Position, rotation and the ground flag, and nothing
else, because there is nothing else: this server has no per-player inventory,
health, hunger or experience. A save merges onto the existing file rather than
replacing it, so the other 24 keys of a player who has also played on vanilla
survive.

**Block ticks are saved and fluid ticks are not.** The queue's pending entries
for a chunk go into `block_ticks` as `SavedTick` compounds — six mandatory
keys, `t` a *delay* rather than a trigger tick and `p` the `TickPriority`
*value* rather than its ordinal, so NORMAL writes as 0 and not 3.
`fluid_ticks` is written empty and honestly so: there is no fluid tick queue
to write. `InhabitedTime` is left at 0 for the same reason — vanilla
accumulates it and changes local difficulty and mob spawning by it, so a made-up
value would be worse than none — and `isLightOn` stays 0, which makes vanilla
relight the chunk, which is the right answer for a server that computes no
light.

**Entities are still not saved, and the reason is that there is nothing to
save.** The format is implemented and has a golden
(`rustmc_world::entitystore`, against a real vanilla
`entities/r.0.0.mca`); what is missing is content. This server has two entity
kinds. `EntityTypes.PLAYER` is built with `noSave()`, so vanilla never writes a
player into the entity region either — players go to `players/data`, which is
done. `EntityTypes.ITEM` is the other, and `crate::entities::Entity` carries no
item stack, so a saved item entity would have no `Item` compound and vanilla
discards one of those on load. Writing the region today would produce a file
whose only honest content is an empty list.

## Nothing floats any more: a block update cascades

Break the block under a torch and the torch goes with it, for everyone
connected, in the same packet burst. Measured over a socket by
`ci/dig-probe.py`:

```text
  rig:        block_update minecraft:stone at (9, 62, 8), ack 12
  torch:      block_update minecraft:torch at (9, 63, 8), ack 13
  cascade:    block_update minecraft:air at (9, 62, 8) took the torch
              at (9, 63, 8), ack 14
```

### The two things called "a block update", and which one this is

`Level.setBlock` does two different things after it writes, and conflating
them is the trap:

  * **`updateNeighborsAt`**, on `UPDATE_NEIGHBORS` (flag 1) — `neighborChanged`
    on the six neighbours. That is the *redstone* path: signal strength,
    comparators, observers, pistons. **Not ported.** The flag is carried and
    honoured at every level so that porting it is filling in one branch
    rather than rediscovering where it went.
  * **`updateNeighbourShapes`**, on `UPDATE_KNOWN_SHAPE` (flag 16) being
    *clear* — ask each neighbour what it becomes now. That is the one a torch
    falls down, and it is what `crate::shapes` implements, together with
    `Block.updateOrDestroy`, `Level.destroyBlock` and `Level.removeBlock`.

Three details from the bytecode that a plausible reimplementation gets wrong:

  * **`UPDATE_SHAPE_ORDER` is `WEST, EAST, NORTH, SOUTH, DOWN, UP`**, not
    `Direction.values()`. It is observable, so it is copied.
  * **the direction argument points back.** `updateNeighbourShapes` calls
    `neighborShapeChanged(d.getOpposite(), pos.relative(d), pos, ...)`, so
    the block being asked is handed the direction that points *at* the block
    that changed. Reversed, a wall torch falls off the block it is attached
    to and survives when its own support goes.
  * **a dig does not write air.** `Level.removeBlock` writes
    `getFluidState(pos).createLegacyBlock()`, so breaking a waterlogged fence
    leaves water and breaking water leaves water — the same state, so nothing
    changes and nobody is told. Writing air would drain an ocean a block at a
    time.

`recursionLeft` is vanilla's 512 and is decremented per level, so a column of
torches collapses one block per level and the limit is a real bound rather
than a safety net.

### The table, and what it refuses

`updateShape` is per block *class*, so it is the same problem
`getStateForPlacement` was and it gets the same answer: ask vanilla.
`tools/goldengen/vanilla/ShapeGolden.java` puts every one of the 32,366 block
states at a position whose level answers **two** positions — itself and one
neighbour — and throws for a third, then sweeps the neighbour over every
state in the game. Two passes, and since 2026-08-18 **both of them read all
32,366 states**: discovery stops at the first neighbour that moves the answer,
then an exhaustive sweep resolves whatever discovery found. Discovery used to
read the 1,196 block defaults, and that was a hole rather than a sampling
decision — see the section below. `ci/shape-golden.sh` runs it twice and
demands the two agree.

```text
  32366 states, 103482 sensitive (state, direction) pairs
  12748 carried, 11621 connective, 79113 refused
  135 distinct neighbour sets
```

  * **12,748 carried** — the pairs whose sweep produced the state itself and
    exactly **one** other. That is the destruction family, and the neighbour
    sets are bitmaps over the global state id, deduplicated to 135 across the
    whole table: a torch, a rail and a sapling that agree about what can hold
    them share one entry. Loaded once per process.
  * **11,621 connective** — several alternatives, which is a *connection*
    property: a fence choosing which way to join, a stair its corner. Those
    change how a block looks and never whether it exists. Not carried, and
    it is the same gap `placement.json` names from the other side as its 195
    neighbour-sensitive blocks — the two are one piece of machinery seen from
    opposite ends.
  * **79,113 refused** — the rule read more of the world than the neighbour
    that changed, so this harness cannot answer it and says so instead of
    handing back a plausible air. By cause, and these eleven rows **sum to
    79,113**; the five-way row this table used to carry summed to 79,093 and
    silently dropped two:

    | pairs | what it read | e.g. |
    |---|---|---|
    | 48,995 | the block **above**, `0,1,0` | walls, fire, vine, redstone_wire |
    | 6,976 | `0,0,-1` | stairs, cactus |
    | 6,629 | `0,-1,0` | saplings |
    | 5,412 | `1,0,0` | stairs, cactus |
    | 5,386 | `0,0,1` | stairs |
    | 5,381 | `-1,0,0` | stairs |
    | 212 | `getLightEngine` | wheat, carrots, mushrooms |
    | 96 | `environmentAttributes` | lava |
    | 16 | `0,-1,-1` | sugar cane |
    | 8 | `registryAccess` | attached melon and pumpkin stems |
    | 2 | `0,-2,0` | nether portal |

    A two-position level is the whole limit and it is worth stating in the
    positive: everything in the carried set is *exact*, over every neighbour
    state that exists, rather than sampled.

### 79,113 refused is not 79,113 owed

The number stood uncharacterised until 2026-08-18 and was read as debt. It is
not, and two properties of the **discovery pass** rather than of the game are
why: it breaks out of the candidate loop on the first throw, so a pair is
recorded as refused before anything establishes that its answer ever moves;
and it does not distinguish a rule that reads one fixed extra position from
one that walks the world.

`tools/goldengen/vanilla/ShapeReadsGolden.java` re-derives the refusal set with
`ShapeGolden`'s own narrow probe — `ci/shape-reads.sh` asserts all four of
`sensitive`, `carried`, `connective` and `refusedPairs` against this tree's
table, so the two cannot drift — then re-asks every refused pair against a
level that answers **every** position with a filler. Air, stone and water:
nothing, a solid, a fluid. Answers that agree across all three mean the rule is
not reading the world in a way that changes its output; answers that disagree
mean no widening of a fixed-neighbourhood harness will ever help.

```text
  48081  60.8%  world         real debt; needs a world, not a wider harness
  22010  27.8%  recoverable   one alternative, invariant under all three fillers
   5609   7.1%  connective    the family this table already excludes by design
   3089   3.9%  inert         confirmed over ALL 32,366 neighbour states
    324   0.4%  refused       getLightEngine 220, environmentAttributes 96,
                              registryAccess 8 — a real level, not a wider one
```

**The debt is 48,405 pairs.** The other 30,708 are a pair with no rule at all,
the connection family counted a second time, or a destruction rule that a
three-position extract would carry outright.

**It is one read and four classes, not many.** 48,995 of the refusals — 62% —
are the block above, and 43,021 pairs read `0,1,0` and nothing else; 54,724 of
79,113 read exactly one position past their own neighbour, which is what says a
three-position table is the shape of the fix rather than a real level. By
class, `WallBlock` alone is 41,472 pairs and every one is `world`; `StairBlock`
is 19,200 of which 14,400 are recoverable and 1,920 inert; with `FireBlock` and
`NoteBlock` that is **84% of the whole refusal population in four classes**.

`data/vanilla/update_shape3.json` is that extract, and the sentence that keeps
it apart from the census is that **its level answers three positions and throws
for a fourth**. The census's level answers a filler for every position and so
invents the world; nothing under `crates/` may read it. The extract invents
nothing, so a port may read it the day one exists. See
`docs/cold-start/shape-three-position-extract.md`.

### The 164 that were in no count at all, and are now in three of them

The three counts above partition what discovery **found**. Until 2026-08-18
they did not partition the game, and `carried + connective + refused ==
sensitive` balanced anyway — which is worse than a partition that visibly does
not add up, because there is nothing to notice.

Discovery swept the **1,196 block defaults**; a pair it found nothing for was
dropped as inert, never reached the exhaustive pass, and ended up with no row,
no refusal and no place in any count. `ShapeGolden`'s header had always named
that limit — "a subject that reacts to some non-default state of a block and
to no block's default state is missed by pass 1" — and nobody had put a number
on it. `ShapeReadsGolden` did: **164 pairs move that the 1,196-default breadth
drops**, and it reports that as `missedAtDefaults`.

```text
  CopperChestBlock            64
  WeatheringCopperChestBlock  64
  ChestBlock                  16
  TrappedChestBlock           16
  PointedDripstoneBlock        2
  SulfurSpikeBlock             2
```

160 of the 164 are chests, and that is a mechanic a player sees: two chests
placed side by side become one double chest, and the pair that decides it fires
only against a neighbour state no block's default carries.

**Discovery reads all 32,366 states now**, and the 164 landed in the three
counts rather than beside them. The split is a subtraction between two
re-derived tables and not a fourth measurement: `sensitive` 103,318 -> 103,482,
`carried` 12,716 -> 12,748, `connective` 11,491 -> 11,621, `refusedPairs`
79,111 -> 79,113. **32 carried, 130 connective, 2 refused.**

The widening is nearly free because discovery early-exits: a pair anything
moves stops at the candidate it always stopped at, so only the genuinely inert
pairs read 32,366 instead of 1,196. `insensitive` is **90,714** — the pairs
nothing in the game moves, rather than the pairs no *default* moves.

#### The 2 are the finding, not the 164

`discoveryLateRefusal` was **0** before the widening — no non-default neighbour
reaches past the harness where no default did — and that was read as a
prediction that the refusal set would not move at all. It moved by 2.

`discoveryLateRefusal` is a statement about **discovery** and not about
**resolution**, and the two passes refuse at different moments. Discovery stops
at the first candidate that moves the answer; resolution then sweeps every
state from the beginning and can meet a throw discovery stopped short of. A
pair discovery never found cannot reach resolution at all, so no measurement of
discovery could have said what resolution would do with it. Both of the 2 read
`0,1,0`, which is why that row went 48,993 -> 48,995.

Stated generally, because it is not about this table: **a count of what one
pass refuses is not a bound on what a later pass will refuse.**

#### The measurement outlives the fix, deliberately

`missedAtDefaults` is what the 1,196-default breadth would still be dropping.
It is a property of the jar, not of a loop bound, so it stays 164.
`discoveryMissed` is what the **current** breadth drops, is structurally 0, and
`ci/refusal-ledger.py` checks that it is 0. A number that can only ever be zero
once its cause is fixed is a check that has stopped being able to fail, so both
questions are asked on every pair and both answers are published.

The census lives in `ci/checks/shape-refusal-census.json` and is committed
while `data/vanilla` is not, deliberately. **Nothing under `crates/` may read
it**: a widened level invents the world past the neighbour, and a row derived
from an invented world is exactly the plausible answer this project refuses. It
is a claim about the jar, committed so that a later run can be made to disagree
with it — `ci/refusal-ledger.py` carries the cheap half of that disagreement on
every gate.

One thing the harness had to be corrected on, and it is the standing lesson
about oracles: it first refused 10,563 pairs for reading
`BlockGetter.getBlockEntity`, which made a torch unanswerable. The torch was
not the problem — a **shulker box** as the candidate ground was, because
`ShulkerBoxBlock` reads its own block entity to decide whether its face is
solid, and a `null` there is a shulker box with no shulker box in it. The
harness now hands back `EntityBlock.newBlockEntity`, which is what the world
holds the moment the block is placed. Carried rows went from 2,510 to 12,716.

### What still does not cascade

Redstone cascades since `crate::power` (this sentence said otherwise until
2026-09-05; see the `neighborChanged` bullet above). No drops —
`dropResources` has nowhere to put an item without entities. No connection properties, so a fence next to
a new fence does not join it until the chunk is re-encoded from the overlay
(and it will not join then either, because the overlay stores what was
placed). Multi-block blocks — doors, beds, tall flowers — are all in the
refused set, so breaking the bottom of a door leaves the top.

## Something ticks: scheduled block ticks

The clock has run at 20 Hz since before the socket opened and nothing was
ever scheduled on it. Now one thing is. Over a socket:

```text
  coral:      block_update minecraft:fire_coral_block at (9, 63, 8), ack 17
  tick:       minecraft:dead_fire_coral_block at (9, 63, 8) 3.2s later,
              unprompted
```

"Unprompted" is the whole claim: between those two lines `ci/dig-probe.py`
sends nothing but keep-alive replies.

### Why coral, and why only coral

`Block.tick` takes a **`ServerLevel`** — a class, not an interface — so the
trick that made `getStateForPlacement` and `updateShape` extractable, a
`java.lang.reflect.Proxy` over a level that answers two positions and throws
for a third, does not work on it. There is no cheap oracle for "what does
this block do when its tick fires".

What there is, already measured, is a list: **five blocks schedule a tick
from inside `getStateForPlacement`**, carried by `placement.json` as
`schedulesTickBlocks`. All five are `CoralBlock`, and `CoralBlock` is small
enough to read whole:

```text
  getStateForPlacement: if (!scanForWater(level, clickedPos))
                            level.scheduleTick(pos, this, 60 + random.nextInt(40));
  tick:                 if (!scanForWater(level, pos))
                            level.setBlock(pos, deadBlock.defaultBlockState(), 2);
  scanForWater:         any of Direction.values() whose fluid is in #water
```

`deadBlock` is a constructor argument rather than a naming rule, so it was
read rather than derived: reflecting `CoralBlock.deadBlock` over
`BuiltInRegistries.BLOCK` returns exactly five pairs, each `X_coral_block` to
`dead_X_coral_block`, and no others.

So `crate::ticks` is the **machinery** — a `LevelTicks` with vanilla's
ordering, drained by the clock thread — plus the behaviours whose whole rule
fits in a few lines of bytecode. Everything else that ticks is blocked on an
oracle, and that is written down rather than worked around.

### The second one is the button, and it moved where the delay comes from

`ButtonBlock` joined the coral on 2026-08-03. Its rule is just as short:

```text
  tick:          if (!state.getValue(POWERED)) return;
                 checkPressed(state, level, pos);
  checkPressed:  hasArrow = type.canButtonBeActivatedByArrows()
                            && an AbstractArrow is inside the button's shape
                 if (hasArrow != state.getValue(POWERED)):
                     setBlock(pos, state.setValue(POWERED, hasArrow), 3)
                 if (hasArrow): scheduleTick(pos, this, ticksToStayPressed)
```

`hasArrow` is false because nothing in this world is an arrow. That used to
read "this world holds no entities at all", which stopped being true on
2026-08-04: players are entities and are tracked, and a thrown item is an
entity too. Neither is an `AbstractArrow`, so the *rule's* answer is unchanged
— but the reason is now about the entity kinds present rather than about
there being none.

What is new is the **delay**. The coral's is `60 + nextInt(40)`, written into
`crate::ticks` as a constant range. A button's is `ticksToStayPressed`, a
**constructor argument** — 20 for stone and polished blackstone, 30 for the
**twelve** wooden ones — with nothing connecting it to the block's name.
Counted from the bytecode 2026-08-03, not recalled: `Blocks` declares 14
`_BUTTON` fields and the `ButtonBlock` constructor is invoked with `bipush 20`
twice and `bipush 30` twelve times. This paragraph and check 335's `about`
both said eleven, which predates pale oak. So it is
data: `UseGolden`'s `TickSink` used to count scheduled ticks and drop them, so
the table could say *that* a button schedules and never *when*, and a column
that says a tick exists but not when it fires cannot drive a tick queue. It
now records `tickDelay`, `tickPriority` and `tickAt`, and the loader refuses a
row claiming `schedulesTick` without a delay so the two cannot disagree.

**Which blocks a rule applies to is read from vanilla's own class name**, not
from the id. `set_placed_by.json` carries `classes` for the whole registry —
all ~1,196 blocks, not only the ones with behaviour, because a rule also asks
what class a block's *neighbour* is and a partial map answers "not that class"
for everything it omits. "The id ends in `_button`" is a naming rule that
holds until one block breaks it, and the break is silent.

### The machinery, from `ScheduledTick`

`DRAIN_ORDER` is `triggerTick`, then `priority`, then `subTickOrder` — a
monotonic counter stamped at schedule time, which is what makes two ticks
scheduled for the same tick at the same priority run in the order they were
*asked for* rather than in whatever order a map iterates. Priority is the
enum's own ordinal, so `EXTREMELY_HIGH` (0) sorts first. The queue is keyed
by all three, so iterating it *is* the drain order.

A second schedule for a position that already has one pending is **dropped**
— `LevelChunkTicks.schedule` guards on a set hashed by
`ScheduledTick.UNIQUE_TICK_HASH`, which is `(type, pos)`. Without it a block
that reschedules on every neighbour change accumulates one tick per change
and then fires all of them.

A tick is scheduled for a *position*, so `run` re-reads the state and does
nothing if the block moved on. A player who breaks a coral before it dries
out leaves a tick behind, and it must not turn whatever replaced it into
dead coral.

One thing here is **not** vanilla and is named rather than hidden: the delay
is `60 + nextInt(40)` drawn from a source of our own. Vanilla draws from
`level.getRandom()`, the server level's shared stream, and reproducing that
stream would mean inventing every other draw that shares it. The *range* is
vanilla's; the particular delay is not. Nothing a client can see about the
tick — that it fires, and what it does — depends on which.

### The instrument, and what it caught immediately

`RUSTMC_TRACE_TICKS=1` prints every schedule and every fire, with what the
position actually held when the tick ran. It earned itself on the first run:
the coral was placed and nothing happened, and the trace said

```text
  placed minecraft:fire_coral_block at (9, 62, 8) ... water true —
    Down=dirt Up=air North=water[level=0] South=grass_block
    West=stone East=water[level=0]
```

The seed-0 spawn is a **shoreline** and the probe's rig square is in it, so
the server was right and the probe was standing in the sea. The check now
puts the coral one block up, above the water line. Worth the detour twice
over: a tick that never fires and a tick that fires and finds the wrong block
are the same silence from a client, and the first version of the trace
printed neither because it sat after the early return it needed to explain.

## Entities exist (2026-08-04)

Until this date **no `add_entity` broadcast existed anywhere in the server**.
Not a weak one — none. A client was told about the world and about block
changes and about nothing else, so a second player was invisible to the first
and a dropped item did not appear.

Five of the twelve `absent` checks in the gameplay corpus are entity-shaped,
and this was the floor under all five — but "absent for that one reason" would
be too strong for three of them and is worth not writing. `080-other-player-
is-an-entity` and `520-a-standing-player-is-rebroadcast-on-a-timer` needed
**only** tracking, and both went `match` the moment it existed.
`500-dropping-an-item`, `350-a-dropped-item-goes-into-your-inventory` and
`080-spawn-egg-makes-a-mob` each additionally need something that *creates*
the entity, which is a separate mechanism per check.

Three modules, matching vanilla's own split:

* `crate::entities` — the level's entity list, the id allocator, and the wire
  encoders (`add_entity`, `remove_entities`, the three `move_entity` shapes,
  `rotate_head`, `entity_position_sync`, `set_entity_motion`), plus `LpVec3`
  and `Mth.packDegrees`;
* `crate::tracker` — `ChunkMap.TrackedEntity` (*who* can see an entity) and
  `ServerEntity` (*what* has been sent about it);
* `crate::dropitem` — `LivingEntity.createItemStackToDrop`'s aimed branch,
  reached from **two** callers as it is in vanilla: the DROP_ITEM arm of
  `handlePlayerAction`, and `AbstractContainerMenu.doClick`'s `THROW` arm
  (offsets 1449-1608, added 2026-08-09). `Player.drop(ItemStack, boolean)`
  is `drop(itemStack, false, bl)`, so both sites arrive with
  `dropAround = false` and `traceItem = true` and there is one function
  behind them here too — `world_send::spawn_and_announce_thrown_item`.

### The two cadences, which are easy to conflate

`ChunkMap.tick()` re-runs the all-players range scan **only when an entity
crosses a `SectionPos`**, and runs `ServerEntity.sendChanges` **every tick**.
Scanning every player every tick gives identical packets and the wrong
performance. Keying the scan on a `ChunkPos` instead misses every vertical
crossing.

`updatePlayer`'s range test is the part a reasonable implementation gets
wrong, in three separate ways:

```java
double limit = Math.min(getEffectiveRange(), getPlayerViewDistance(player) * 16);
boolean visible = d.x*d.x + d.z*d.z <= limit*limit
                  && entity.broadcastToPlayer(player)
                  && isChunkTracked(player, entity.chunkPosition());
```

It is **horizontal** — `d.y` never appears, so an entity 300 blocks overhead
is in range; it is **`<=`** where the `isChunkTracked` test beside it is `<`;
and the range is the *smaller* of the type's and the **player's view**, so in
a 4-chunk test rig a player is tracked to 64 blocks and not to its type's 512.

### The heartbeat is not a special case

`sendPos = moved || tickCount % 60 == 0` is one boolean with two ways to
become true, so a standing entity's three-second re-broadcast takes the
ordinary path and carries deltas that happen to be zero. Two consequences:

* `tickCount++` sits **outside** the `updateInterval` gate. A player's
  interval is 2, so advancing the counter inside the gate halves the
  heartbeat rate to one per 120 ticks — and looks entirely plausible.
* the counter starts when the entity begins being tracked, not at any world
  time, so the phase is per-entity.

### The defect this exposed, which nothing could previously observe

`JoinInfo::default().player_id` is 1 and no call site overrode it, so **every
player was told its own entity id was 1**. Invisible while no client was ever
told about another entity; fatal the moment one is. Ids now come from the
level's single allocator, so a player and an item cannot collide either.

### What is tracked and what is still absent

Tracked: players, and item entities from a `DROP_ITEM` action.

Not modelled, and each is a limb of a vanilla condition that is currently
constant here: passengers and vehicles, minecart behaviour, item frames,
equipment, `syncPosition` and `getRequiresPrecisePosition`. **`needsSync`,
`hurtMarked` and attributes left this list**: a mob's `AttributeMap` syncs
through `sendDirtyEntityData` (2026-09-05, `mob-in-world`), and `knockback`
sets `needsSync` while `markHurt` sets `hurtMarked`, both of which
`send_changes_with` now reads — the second from outside the interval gate, at
`sendChanges` offset 1202, which is why a hit mob is told its velocity twice on
the next tick. Both are still constant for a **player**, which has no
server-side physics to move. `ChunkMap.tick`'s `inEntityTickingRange`
limb has no ticking-range manager behind it, so it reads as true whenever
anyone can see the entity.

**`SynchedEntityData` is now partly modelled, and the part that is missing is
not the part that used to be** (2026-08-08). `crate::synched` carries the two
accessors a player is away from its definitions on — `LivingEntity`'s health
at 9 and `Avatar`'s model customisation at 16 — so a player pairing now
carries the `set_entity_data` it always should have, with the payload
confirmed byte-for-byte against a vanilla recording. What is still absent is
an **item's**: `ItemEntity`'s stack lives at its own accessor and this server
does not carry it, so vanilla sends a `set_entity_data` for a dropped item
and we send none. That belongs with the rest of the drop chain.

**It is now the named reason two `match`es are only declared ones**
(2026-08-09). `605-container-click-throw-drops-one-item` and `606` convert on
the declared denominator and stay strict-`diverge`, and their whole remaining
strict residual is the item's own tracking traffic: two `set_entity_data`,
`set_entity_motion`, `move_entity`, `entity_position`. The first of those is
this paragraph; the rest are `ItemEntity.tick`, below.

**`update_attributes` is a real gap in the pairing bundle, and it is a gap
with no matchable target.** Vanilla sends one for any `LivingEntity` whose
`AttributeMap.getSyncableAttributes()` is non-empty, which for a player it
always is. Its payload is the `values()` of a lazily populated
`Object2ObjectOpenHashMap` keyed by `Holder<Attribute>` — so it is the set of
attributes the reference happened to have read, in an order derived from JVM
identity hashes. **Vanilla does not reproduce it between runs**: over the two
full-corpus vanilla recordings of 2026-08-08,
`ci/selfcheck-kind-stability.py` reports `update_attributes` 1 identical and
1 differing, and the differing one is `530`. `set_entity_data` over the same
pair is 8 identical and 0 differing. See `docs/PARITY-ROADMAP.md` §10 for why
that means no payload this server could send would help, and why the fix — if
there is one — is a projection decision and not server work.

`set_equipment` is a different
case and must not be counted with it: vanilla skips empty stacks and sends
nothing when the list is empty, so an empty-handed player produces no packet
in either server — verified in `530`'s reference recording, whose pairing
bundle is `add_entity`, `set_entity_data`, `update_attributes` and nothing
else.

**A thrown item falls, merges, expires and is picked up (2026-10-01, branch
`item-pickup`).** `ItemEntity.tick` — gravity, friction, `Entity.move`, the
pickup delay, `mergeWithNeighbours`, the 6000-tick despawn — is
`crate::itementity`; `Player.aiStep`'s touch loop and `ItemEntity.playerTouch`
are `crate::pickup`. Until then this paragraph said the item hung in the air,
and argued that faking the fall with a floor-finder would be a one-way door;
`crate::physics`'s swept-AABB `Entity.move` was ported first and the tick
stands on it. Fluids, `applyEffectsFromBlocks` and experience orbs are not
ported and are named in those modules' headers.

**The velocity limb's sign was inverted, and it is fixed (2026-08-05).**
`ServerEntity.sendChanges` enters that limb on
`needsSync || trackDelta || (LivingEntity && isFallFlying)` — `javap -p -c
-constants`, offsets 874–919, with `ifne` on `trackDelta` at 887. This server
tested `!trackDelta`, and the comment justifying it reasoned that a client
which extrapolates needs no help from the server. That is backwards: a client
extrapolates *from* a velocity, so `trackDeltas` true is exactly when the
server must send one. Vanilla sends `set_entity_motion` for an item and not
for a standing player; we did the opposite.

It was not a latent bug. `dropitem.rs` gives a thrown item a real
`throw_velocity`, and a player's movement is always zero here for want of
physics — so the inverted test took a silent branch in both directions and
this server sent **no `set_entity_motion` at all, for anything**. It was found
as `dropping-an-item-throws-it-on-the-floor`'s
`missing_kinds: ["set_entity_motion"]` in the 2026-08-05 corpus run, and the
bytecode was read afterwards to find out why. `needsSync` and fall-flying stay
unmodelled; both are disjuncts, so their absence can only make this server
send less than vanilla, and for a player it is now the whole of the limb.

**And the fix moves no verdict today, which is worth writing down so nobody
re-measures it.** Corpus runs 1 and 2 on this branch, either side of the sign
change, are identical: 35/63 declared, 24 diverge, 4 absent, 0 vacuous, 0
inert, 0 error, with the same single `BETTER`.
`dropping-an-item-throws-it-on-the-floor` still reports
`missing_kinds: ["set_entity_motion"]`. The reason is the paragraph above
this one: **the limb is guarded by a delta test, and our item's velocity never
changes.** `ServerEntity`'s constructor seeds `lastSentMovement` from
`entity.getDeltaMovement()` — verified at offsets 62–65, and ours already did
the same — so a thrown item starts with its throw velocity already recorded as
sent. Vanilla's item then falls, `ItemEntity.tick` changes the velocity every
tick, and `distanceToSqr > 1.0E-7` fires. Ours hangs in the air at a constant
velocity, so the delta stays exactly zero for ever.

So under the old sign this server sent no `set_entity_motion` because it
skipped the only entity that had a velocity; under the new one it sends none
because that velocity is frozen. **The sign was still wrong and is still worth
fixing** — it is a one-way trap that would have silently mis-broadcast the
moment `ItemEntity.tick` lands — but the packet does not appear until item
physics does, and it is item physics, not this sign, that closes 500's missing
kind.

Nothing spawns a mob, so `080-spawn-egg-makes-a-mob` is untouched.

### The tracking range was a fact about the generator (2026-08-04)

`TrackedEntity.updatePlayer` bounds visibility by
`min(getEffectiveRange(), getPlayerViewDistance(player) * 16)`, and
`ChunkMap.getPlayerViewDistance` is
`Mth.clamp(requestedViewDistance, 2, serverViewDistance)`. **This server had
no `serverViewDistance`.** `status.rs` set the join packet's chunk radius to
`(area.max_chunk_x - area.min_chunk_x) / 2` — the radius of the *generated
spawn tile* — and handed the same number to `PlayerIdentity.view_distance`,
which is what reaches the clamp. So how much world had been generated decided
how far a player could see another player: 2 under `--radius 2`, a 32-block
tracking limit against vanilla's 64 at `view-distance=4`. The client's own
requested view distance was read during configuration and dropped on the
floor.

Three quantities now read one number, as vanilla's do — the login packet's
chunk radius, the chunk sender's radius, and the clamp — and `--view-distance`
defaults to 10 because that is `server.properties`' own default.
`ci/t2-gameplay.sh` passes 4 to match `ci/vanilla-server.sh`, which is
substrate parity in the same sense as `difficulty=peaceful`. `--radius` is
untouched and still means tile size.

It was found by `530-walking-away-drops-a-standing-player-from-view`, and the
red/green pair is the whole evidence: with the server unfixed, vanilla emits
`remove_entities` at step 2 and this server at step 0; with it fixed, both at
step 2 and `add_entity` at step 3. That difference *is* 64 blocks against 32.

**Three doc comments in our own source were wrong about this, and one of them
wrote a prediction.** `JoinInfo::chunk_radius` said "the client's own view
distance, echoed back" — a third answer, matching neither the code beside it
nor vanilla — and the first version of check 530 predicted, from that line,
that this server would track at 160 blocks. It tracked at 32.

### The first run of that check measured the rig, not the server

Worth its own heading because the verdict *agreed with the prediction* and
was still meaningless. Check 530 came back `diverge` with our extra removal
at step 0, exactly as written down beforehand — and vanilla had emitted
nothing at all, on that run and the one after it, because `Session::hop_to`
walked 8 blocks a packet:

```text
  [Server thread/WARN]: oracle moved too quickly! 0.0,0.0,16.0
```

Sixteen, not eight: hops leave every 30 ms and a tick is 50, so two reach the
server against one `lastGood` and `handleMovePlayer` tests their sum against
`100.0f * i`. The actor was teleported back three times and never got further
than 24 blocks from the observer — and because a measured step deliberately
never reads the socket, the `accept_teleportation` never went out, so the
first refusal pinned it for the rest of the walk.

`Session::goto` beside it had already been through this and fixed it at 0.5
blocks a packet, with the story written above its own `HOP`. The correction
lived in the function it corrected and did not travel.

**And the repair overshot, which is the other half of the lesson.** The bound
gives a window, not a point, and the first fix took the small end of it — one
block a packet. The walk became legal and `080-other-player-is-an-entity`
fell from `match` to `diverge` on the full gate, with vanilla's first
`move_entity` delta at 12288 wire units against our 8192: three blocks against
two. 080 walks four blocks, which at eight blocks a packet was **one** packet
that neither server could split, and at one block a packet is four that their
`sendChanges` phases divided differently. Nothing about either server changed.
**More packets for the same walk buy no fidelity and make every observation of
it phase-sensitive**, so inside the legal window the hop wants to be as large
as it can be. `hop_to` is now four blocks a packet: legal at every routine
fold, and a single hop for 080.

The general form, for the next check that moves anything: **a scenario step
that walks is only as real as vanilla's willingness to accept it, and vanilla
records its refusals in its own log rather than on the wire.** `grep -c
'moved too quickly' <vanilla>/logs/latest.log` is a one-line check that any
walking scenario is measuring the server.

### The rig can walk and cannot jump, and the cost of jumping is one bit

**`entity_position_sync` is reached by nothing in this corpus, and the walk
fix is what settled that rather than what caused it.** It has four triggers:
a delta that leaves `i16`, the 400-tick forced teleport, a dismount, and
`wasOnGround != entity.onGround()`.

Measured, from the one-check recordings of `530-...` on this branch: with
`hop_to` at 8 blocks a packet the trace carried `entity_position` and with it
at one block it carries none, on either server — because the arm needs more
than 8 blocks of travel between two `ServerEntity.sendChanges` body runs, and
a player's `updateInterval` is 2 ticks. Eight blocks per 30 ms is 26 blocks
per body run; one block per 30 ms is 3.3.

So the coverage the 8-block hop appeared to give was real and it was also
fiction, because vanilla was refusing those very packets — the recording
carried our `entity_position` and not vanilla's, which is a divergence and
not coverage. Nothing else in the corpus moves fast enough: the setup walk is
`goto` at 0.5 blocks a packet and `080-other-player-is-an-entity` covers its
four blocks in a single hop.

The ground-flag arm has never been reachable at all — `hop_to` writes a
literal `1` for the on-ground byte of every `MOVE_PLAYER_POS` — so no
scenario has ever told a server it was in the air. Only the unit test
`a_change_of_ground_flag_forces_a_position_sync` covers it.

**Both arms want the same one change to the rig**, which is why they are
scoped together below: a `Step::Walk` that can choose its pace and its
on-ground bit. The pace has a window rather than a point — `100 * i` allows a
claimed 10 blocks between ticks and `i16` needs more than 8 between body runs
— and hitting a window is exactly the kind of thing that must be measured
against vanilla before it is believed, not reasoned into a constant.

**Scoped, not built.** The cost is smaller than it looks and the reason is
worth writing down, because the expensive answer is the plausible one. A
jump looks like it needs the client to model vanilla's jump arc — 0.42
initial, gravity 0.08, drag 0.98, one packet per tick — because
`ServerGamePacketListenerImpl.handleMovePlayer` runs the client's claimed
delta through `player.move(MoverType.PLAYER, ...)` and then compares where
its own collision put the player against where the client said, rejecting a
disagreement over 0.0625 with "moved wrongly!" and a rubber-band.

It does not, in this substrate. That comparison is skipped outright for a
creative player:

```text
  handleMovePlayer, offsets 785..835
    785: dload 25 / 787: ldc2_w 0.0625d / 791: ifle 855      past the tolerance
    798: ServerPlayer.isSleeping  : ifne 855
    808: ServerPlayer.isCreative  : ifne 855   <-- the corpus substrate
    818: ServerPlayer.isSpectator : ifne 855
    837: LOGGER "{} moved wrongly!"
```

and the corpus is `gamemode=creative` by construction. What remains is the
"moved too quickly" bound, which the existing 8-block hops already satisfy,
and vanilla's own jump detector immediately above:

```text
  handleMovePlayer, offsets 631..657
    if (player.onGround() && !packet.isOnGround() && <local 29, the
        upward-movement flag>) player.jumpFromGround();
```

So the rig needs to send **one byte differently** — the on-ground flag — and a
target above the ground, and vanilla will treat it as a jump, call
`jumpFromGround`, and produce the ground-flag change that
`ServerEntity.sendChanges` turns into `entity_position_sync`. Concretely:
an `on_ground` field on `Step::Walk` (or a `Step::Jump`) threaded into
`Session::hop_to`, which today hardcodes it. No physics, no tick alignment,
no new packet type.

**Read from the jar, not measured**, which is the caveat that matters: the
0.0625 branch was read at the offsets above and no probe has yet sent a
`MOVE_PLAYER_POS` with the flag clear. The measurement that would settle it
is one recording of a two-step scenario against real vanilla, and it belongs
with the change rather than before it.

### Three of the entity-shaped `absent` checks can never reach `match`

Measured, not argued — `ci/checks/gameplay-unstable.toml` at N = 26, twenty-six
cold vanilla servers compared against each other:

| check | vanilla against itself | why |
|---|---|---|
| `dropping-an-item-throws-it-on-the-floor` | 25 flaps / 25 | four `nextFloat()` draws in the throw velocity |
| `a-dropped-item-goes-into-your-inventory` | 25 / 25 | `popResource` scatters the position |
| `spawn-egg-makes-a-mob` | 25 / 25 | the spawn rotation |

The reachable verdict for all three is `absent -> diverge`. **This corrects a
premise worth correcting**: "twelve checks where we send nothing" reads as
twelve checks with headroom, and among the *absent* ones on the entity axis
there were exactly two — `520-a-standing-player-is-rebroadcast-on-a-timer`
and `080-other-player-is-an-entity`. Both are now `match`.

**One entity-shaped check with a reachable `match` does remain, and it is not
in the `absent` column at all.** `070-sand-falls-when-unsupported` is
`diverge`, reports `we never sent: add_entity` against vanilla's
`kind_id 51` — `minecraft:falling_block` — and does not appear in
`gameplay-unstable.toml`. Looking only at `absent` is what hid it.

**Two more reached `match` on 2026-08-09 and they were in this table's own
family** — `605-container-click-throw-drops-one-item` and `606`, which were
added to `gameplay-unstable.toml` on 2026-08-08 for the item's spawn yaw and
removed again the next morning. Nothing about vanilla changed. What changed is
that the yaw stopped being one anonymous byte inside `add_entity`'s `rest`
digest: `tools/oracle-runner/src/trace.rs` now names `xRot`, `yRot`,
`yHeadRot` and `data`, and `compare::project` drops `y_rot` for
`minecraft:item` alone. The other three are constant `0` for every item
vanilla spawns and are still compared, and so is the position. So "can never
reach `match`" is a statement about a *projection* as much as about vanilla,
and it is worth re-asking of the three rows above whenever the field they name
is separable from the fields around it. It is not separable for those three:
`dropping-an-item-throws-it-on-the-floor` observes `set_entity_motion`, whose
whole payload is four `nextFloat()` draws;
`a-dropped-item-goes-into-your-inventory` and
`torch-falls-with-its-support` turn on `Block.popResource` scattering the
position, and `050` on whether the item is picked up at all — which decides
*which packets exist*, and no projection reaches that
(`docs/PARITY-ROADMAP.md` §11).

The split within an unreproducible check is still sharp and still worth
holding to. A thrown item's **position** is deterministic and vanilla records
it identical to the last bit on all twenty-six runs; the velocity moves, and
so does the **spawn yaw** — `ItemEntity(EntityType, Level)` offsets 39-52 are
`setYRot(this.random.nextFloat() * 360.0F)`, from the entity's own unseeded
generator. This server wrote zero there until 2026-08-08 and `dropitem.rs`
carried a comment denying the call existed; both are fixed and the yaw is now
drawn, in vanilla's order, out of a generator seeded the way
`RandomSupport.generateUniqueSeed` seeds one. It converts nothing — two
unseeded draws never agree — and it is the difference between a server that
does what vanilla does and one that writes a constant.
Writing `PLAYER_EYE_HEIGHT` as the `f64` literal `1.62` rather than the
widened `float` `1.6200000047683716` put the drop 4.8e-9 of a block out — and
the recorded vanilla number is the only reason that was caught.

## The world grows

`rustmc run` used to generate one square before opening the socket and serve
out of it for ever. Past its edge there was nothing, and making the square
bigger only moved the edge — while charging the whole of it to the first
player to join.

It is **tiles** now (`crate::tiles`). Each tile is its own full generation of
a square, margin and all, and a chunk is served by whichever tile owns it.
Tiles never overlap: `tile_of` is a Euclidean division, because truncation
would put chunk −1 in the origin's tile and make every tile west or north of
spawn overlap its neighbour by a row.

A tile takes seconds, and a client's read timeout is 20 — the same 20 that
made the *first* version of this server fail its join — so a tile a player is
walking into is generated on a background thread and the chunks arrive when
they are ready. Requests are deduplicated, so two players walking the same way
start one generation.

**The retry is the part that is easy to miss.** Chunks were offered only when
the player crossed a chunk border, which was right when a chunk was either
there or never would be. With a tile that takes seconds, a player who walks to
the edge and *stops* would be told about the new tile never. So the once-a-
second clock that already runs re-offers whatever the view is still missing.

Measured 2026-07-28 on the Threadripper, tile radius 2 (which was
the default then and is 10 now — see below):

```text
  spawn tile: 25 chunks (81 with the margin) in 10.6s
  generating tile (1, 0) — 25 chunks
  generating tile (2, 0) — 25 chunks
  grew: chunk (6, 0) arrived 13.7s after walking out
```

### What tiling costs, and the seam it leaves

Each tile regenerates its own margin: a tile of side *n* generates `(n + 4)^2`
chunks to deliver `n^2`. At radius 2 that is **81 generated for 25 delivered,
69% thrown away**, and the neighbouring tile pays it again for the chunks they
share. A generator that tracked a per-chunk *status* — promoting a chunk to
decorated once its neighbours had reached the previous stage — would generate
each chunk once. That is the real fix, it changes `pipeline`, and `pipeline`
is also where item 2's concurrency work has to go, so it is deliberately not
started from the server side.

**`status::WORLD_RADIUS` is 10 as of 2026-08-06, and this discard is why.**
The measurements on this page were taken at radius 2, which is what the
figures above mean; the default moved because at radius 10 the same
arithmetic gives 625 generated for 441 delivered — 29% thrown away rather
than 69%, or 4.7x the useful chunks per second. `docs/BENCH.md` has the
curve across 2..12 and the boot time it costs. The status-based generator
above remains the real fix; a bigger tile only makes the waste smaller.

**Tiling makes the world unbounded. Status-based generation makes it cheap.
They are independent.**

### The finding: `DECORATION_MARGIN` does not converge at seed 0

Tiling is only sound if the chunks either side of a tile boundary hold what
one generation covering both would have produced.
`crates/rustmc-server/tests/tiled_world.rs` measures exactly that — two
adjacent 1x1 tiles against one pass over both — and gets **0 differences over
196,608 blocks** at seed 20260722, the seed every parity number in this
project is measured at.

Then a client disagreed with itself. `ci/dig-probe.py` spawned at y **75**
with a tile radius of 1 and y **63** with a radius of 2 — same world, same
column (8,8), twelve blocks apart, which is about a tree. So
`tests/margin_probe.rs` widened the context at seed 0 and regenerated chunk
(0,0) inside areas of 2 to 7 rings:

```text
  context 2 rings: top at (8,8) = 74
  context 3 rings: top at (8,8) = 74, blocks differing from previous =  24
  context 4 rings: top at (8,8) = 62, blocks differing from previous = 332
  context 5 rings: top at (8,8) = 74, blocks differing from previous = 332
  context 6 rings: top at (8,8) = 62, blocks differing from previous = 334
  context 7 rings: top at (8,8) = 62, blocks differing from previous =  61
```

It does not converge. It **oscillates** — which is not the signature of a
margin that is merely too small, because that would settle once the widest
reaching feature was inside it.

Three things follow, and the third is the one to act on:

1. **This is not a tiling bug.** The fixed square the server served before was
   also one area chosen at one radius, and had exactly the same exposure.
   Tiling does not make any chunk less finished; it makes the problem visible
   in more places, and now measured rather than assumed.
2. **A tile is exactly as finished as that single square was, and no more.**
   That is the whole claim `crate::tiles` is allowed to make, and it is what
   its module docs say.
3. **It belongs to worldgen, not to the server.** Whatever reaches a chunk
   from more than two rings away has to be found before `DECORATION_MARGIN`
   can be raised with any confidence — and since every region diff is measured
   over some area, item 1's numbers rest on the same premise. Run
   `RUSTMC_MARGIN_PROBE=1 RUSTMC_MARGIN_SEED=<seed>` to ask it of another seed.

**Answered by the worldgen track, 2026-07-28** — full write-up in
`docs/WORKLIST.md`, "CLOSED this turn: `DECORATION_MARGIN` at seed 0". Nothing
reaches the chunk: `RUSTMC_TRACE_WRITE=8,74,8` says the writer is chunk
(0,0)'s **own** `trees_birch_and_oak_leaf_litter`, deciding differently
because decoration reads the blocks its already-decorated predecessors left.
The x-then-z walk carries that chain inward from the edge of the generated
area without limit, so `DECORATION_MARGIN` — which bounds a feature's direct
*write* reach — has no fixed point to converge to and **must not be raised to
3 in the hope that it does**. Terrain is per-chunk pure, and seeds 20260722
and 31415926 are byte-stable through decoration too (0 blocks over 49
delivered chunks, contexts 2 to 6), so this is confined to seeds where the
chain is live. For the server that means point 2 above stands exactly as
written, and generate-once status-based generation is the fix, not a bigger
margin.

## The gameplay oracle

`region-differ` answers *does our world contain vanilla's blocks*. This
answers the other half: **does our server behave as vanilla behaves, as seen
by a client.** `tools/oracle-runner` runs a scripted scenario against real
vanilla and against rustmc and diffs what each said on the wire.

```sh
./ci/gen-gameplay-tables.sh             # once per tree — the gate refuses without it
./ci/t2-gameplay.sh                     # boot both, record both, diff
T2_GAMEPLAY_KEEP=1 ./ci/t2-gameplay.sh  # keep the traces under oracle-work/
```

**A tree cannot gate T2 until it has generated its own tables.** Everything
under `data/vanilla/` is gitignored and derived from this tree's own dumpers,
so it is branch-dependent: `set_placed_by.json` exists on one branch and not
another, and `use_without_item.json` changes whenever `UseGolden.java` does.
A directory that borrows another tree's copy gates that tree. On 2026-08-03
`~/rustmc-gate` had `data/vanilla` symlinked into a checkout sitting on main
and reported **12/55 declared with 11 regressed** for a branch whose real
reading was clean — the missing `set_placed_by.json` meant no bed, door or
tall flower wrote its second block, and the 133 KB-stale `use_without_item.json`
meant no lever, button or repeater carried its scheduled tick. Every trace
agreed and every trace was wrong.

`ci/gen-gameplay-tables.sh` derives the required list from the
`data/vanilla/…` paths the server crate actually opens, regenerates what is
missing or stale, and stamps each file with the tree that made it and the
digest of its generator. `ci/t2-gameplay.sh` exits **4** — never 0, never a
number — on a table that is absent, foreign or older than its dumper, and on
an `oracle-work` or `oracle-work/gameplay` that is a symlink out of the tree.
`./ci/handoff.sh <branch>` does the whole setup and both gates in one command.

A check is a TOML file of *actions* and nothing else — it never states what
should happen, because the expectation is whatever vanilla did. Adding one
is writing down a sequence of steps, which is what makes a corpus affordable
to grow.

**Corpus measured 2026-08-03 against vanilla 26.2, protocol 776, on
superflat, on the Threadripper.** These are the first numbers taken with a
client that stands where the harness thinks it does, acknowledges its chunk
batches, and works in a world whose gamerules were actually applied — see the
sections below for what each of those three cost. All three were wrong at
once until today, and each one hid the next.

| | declared | strict |
|---|---|---|
| match | 23 / 55 | 10 / 55 |
| diverge | 21 | 41 |
| absent | 11 | 4 |
| inert | 0 | 0 |
| vacuous / error | 0 | 0 |

**14/51 -> 23/55 on 2026-08-03 (evening)**, from four mechanisms:
`setPlacedBy` (`door-places-both-halves`, `bed-places-two-blocks`, and the new
`a-bed-facing-north-puts-its-head-the-other-way`), the door's upper-half
`updateShape` (`digging-a-door-takes-both-halves`,
`door-opens-when-right-clicked`), and `ButtonBlock.tick` with a delay read from
the table (`button-unpresses-itself` and the new
`a-wooden-button-stays-pressed-longer`), and `BedBlock.updateShape` (the new
`digging-a-bed-takes-both-halves`). The new
`digging-a-tall-flower-takes-both-halves` needed no new mechanism at all —
`DoublePlantBlock`'s rule was already in the shape table and only wanted a
second half to cascade to, which is exactly what it exists to say.

Nine checks. Every one was predicted from the recorded traces or from the
table before any code was written. **Read `strict` as noise**: it moved
6/53 -> 10/55 across runs whose server change was two typed rules, which
cannot account for four, and it is not ratcheted.

Two denominators, always. **Declared** applies the projection each scenario
asked for; **strict** keeps the scenario's `within` radius and its
`observe_from` but drops the `observe` kind filter, so it also counts the
packets no decoder has been written for, whose digests make them exact by
construction. The gap between them is how much of what the two servers said
is not being looked at.

**Read the zeros before the 23.** Every one of the 55 can fail: 0 vacuous,
0 inert, 0 error. That property matters more than the match count, and it has
now survived the corpus going 37 -> 51 -> 55 and nine checks closing.

### A denominator whose own reference scores 8% is not a denominator

**This is the most important thing on this page, and it invalidated a number
this document used to publish.**

`strict` was described here as "the honest measure of how much verification
has been deferred". It was not. It used to drop the scenario's `within`
radius and its `observe_from` as well as its `observe` kind filter, so every
check's strict verdict compared **the whole session, everywhere in the
world, on both clients** — every other workspace, every entity, every
unprompted thing that happened anywhere within view.

The first run of `ci/t2-gameplay-selfcheck.sh` priced that, by asking the
only question that could:

```text
  vanilla against ITSELF, old strict     3 / 37     8%
  vanilla against ITSELF, new strict    16 / 37    43%
  rustmc,                 old strict     0 / 51
  rustmc,                 new strict     5 / 51
```

**A denominator its own reference fails 92% of the time is measuring the
reference, not the subject.** Our 0/51 was indistinguishable from vanilla's
own noise floor, and the three scenarios that did survive it —
`sand-falls-when-unsupported`, `server-answers-a-command`,
`digging-a-door-takes-both-halves` — have nothing in common except that
nothing random happened near them.

Keeping the scenario's bounds and dropping only the kind filter is what the
number was always meant to say: *inside the box this check is about, and on
the client it is about, did the two servers send the same bytes — including
the bytes no decoder reads.* That rebases the ceiling from 8% to 43%.

**And 43% was itself an artefact of N = 2, which is the correction of
2026-08-04.** It counted checks that matched strict in **one** comparison of a
population that mostly flips. Asked properly — does a check reproduce under
strict in *all* eleven comparisons of a twelve-run selfcheck — the answer for
this corpus is:

```text
  vanilla against ITSELF, strict, N = 12     0 / 55
```

Not one check in the corpus. Counted from the eleven reports rather than read
off a printout: **55 of 55 unstable, 13 at 11 of 11, 21 at 10 or 11 of 11, and
the minimum anywhere is 5 of 11** — nine checks sit at that floor, led by
`place-log-axis`, `snow-layers-stack` and `bed-places-two-blocks`.
**So `strict` has no reference
ceiling to score against at all yet**, and the mechanism above says why the
*floor* of that distribution sits at about a half: a 60-tick re-broadcast
against a sub-3-second window is a coin flip under every check at once, and
5/11 is what a coin flip looks like. Until entity tracking lands, a strict
figure is a statement about which side of that flip a particular run fell.

> **Both halves of that last sentence have since been overtaken, in opposite
> directions, and it is left standing because the pair is the lesson.** The
> coin flip closed on 2026-08-04 at the *projection* layer with no entity
> tracking written at all — see the block below — so the "until" was never the
> gate it claimed to be. Entity tracking then landed later the same day
> (`## Entities exist`), which means this server now emits the heartbeat the
> paragraph is about. Neither event is the one the sentence predicted would
> matter.

**This is the same class of error as an untwinned worldgen residual**, and
the correction is the same shape. ORACLE rule 6 exists because a residual
counts against us only if two independently generated vanilla worlds agree
on it; the twin mask subtracts vanilla's disagreement with itself before any
number is quoted. `strict` had no twin, so it was quoting vanilla's
nondeterminism as our defect. The general rule, which applies to any number
this project publishes:

> **Measure the reference against itself before quoting a score against it,
> and quote the gap against that ceiling — never against 100%.**

Two consequences worth carrying:

* **`strict` is not ratcheted and must not be read as progress.** It moved
  5/51 -> 3/51 between two runs whose only server change was one lever,
  which cannot account for two checks. That is the residual 57% of the
  ceiling showing through. Only `declared` is ratcheted by
  `ci/checks/gameplay-baseline.toml`.
* **And it moves on a tree that has not changed at all, for a reason that is
  in vanilla's bytecode.** Measured 2026-08-04: two corpus runs on the *same
  commit*, 26 minutes apart, gave `declared 22/55` both times — identical
  check by check — and `strict 5/55` then `strict 7/55`. The two names that
  moved were `break-block` and `place-log-axis`, and the cause is one packet.

  `ServerEntity.sendChanges` decides whether to broadcast a position with

  ```java
      moved = positionCodec.delta(pos).lengthSqr() >= 7.62939453125E-6
              || this.tickCount % 60 == 0;
  ```

  — offsets 497–541 — so **every tracked entity is re-broadcast every 60
  ticks, three seconds, whether or not it moved.** Every scenario here has a
  second player standing five blocks away and every measured window is
  shorter than three seconds, so that broadcast lands inside the window on a
  coin flip. `declared` filters `move_entity` away; `strict` does not; and
  this server has no entities to move, so strict can only match on a run
  where vanilla stayed quiet.

  A 3 s period against a ~1.2–1.5 s window predicts 40–50%. Twelve cold
  vanilla servers measured **6 of 11 and 5 of 11 comparisons, vanilla against
  itself**, with arrivals spread across the window at 0, 194, 195, 788, 859
  and 984 ms — the rate and the uniformity both. That reproduces the two gate
  readings exactly.

  **So no strict figure may be quoted from a single run.** The coin flip sits
  under all 55 checks and shows up only in the two whose strict projection is
  otherwise clean enough for one packet to decide the verdict.

  > **"It closes when entity tracking is implemented and not before" was the
  > sentence here, and it was wrong — measured 2026-08-04.** It closed at the
  > *projection* layer, with no entity tracking written at all. The packet is
  > filed as `move_entity_heartbeat` and dropped unless a check names it,
  > because `dx² + dy² + dz² < 128` in wire units is exactly
  > `delta.lengthSqr() < 2⁻¹⁷` in blocks, which is the test `sendChanges`
  > itself applies before deciding the entity moved. A packet under it cleared
  > no movement test. **Quantity A went 0 of 55 to 35 of 60, both at N = 12.**
  > (Superseded as the headline by 34 of 61 at N = 26 — see the ceiling bullet
  > below for why a *lower* number at a higher N is the better one.)
  >
  > Worth keeping as a method note rather than just a correction: the fix was
  > available for the price of reading one predicate, and it was deferred
  > behind a subsystem nobody had started. "This needs feature X first" is a
  > claim that deserves the same evidence as any other.
* **`strict` has a ceiling, and it is 34 of 61 at N = 26.** This bullet has now
  been wrong three times and every version is left standing. It first read "the
  43% ceiling is itself a worklist ... and the ceiling is the honest target";
  that was 16 of 37 in *one* comparison at N = 2, and it was withdrawn. It then
  read "there is no ceiling to score against, so `strict` is not a metric at
  all", on a measured 0 of 55 — true of the instrument as it stood, and wrong
  about why. It then read 35 of 60 without saying at what N, which is the
  mistake this version exists to stop repeating.

  **This is quantity A: vanilla against itself, self-reproduction. Never call
  it "strict" bare, and never quote it as parity.** It says how reproducible
  the *reference* is, which is the ceiling any score against it could reach,
  and nothing whatever about this server.

  **A IS A FUNCTION OF N AND IS COMPARABLE ONLY AT EQUAL N.** A check counts
  as reproduced only if it matched in *every* comparison, so A is
  **monotonically non-increasing in N by construction**: 26 runs give 25
  comparisons and 25 chances for a flaky check to flap, against 11 at N = 12.
  A figure quoted without its N is meaningless, and **a fall from a lower N to
  a higher one is not a regression** — it is the earlier figure having been
  optimistic. Anyone who "fixes" a drop by quoting the highest number has
  quoted the worst-sampled one.

  **The ceiling is 60 of 61, not 61 of 61.**
  `520-a-standing-player-is-rebroadcast-on-a-timer` is permanently `vacuous`
  under strict — its only observations are heartbeats and strict drops those
  by design, so both projections are empty — and
  `ci/selfcheck-aggregate.py` counts any non-`match` as unstable and cannot
  tell a permanently vacuous check from a flapping one. It costs A a fixed 1
  for as long as that is true. A reader who does not know this reads the gap
  as instability that isn't there.

  **The sequence, with its Ns, because the shape of it is the evidence:**

  ```text
    0 of 55            the instrument as it stood
    35 of 60  N = 12   heartbeat exemption; pre-registered floor of 30, CLEARED
    37 of 60  N = 12   `within` leak fix; registered floor of 42, MISSED —
                       shipped as a correctness fix on a different basis, not
                       as a strict-recovery win
    34 of 61  N = 26   best sampled, and the figure to quote
  ```

  34 of 61 supersedes 37 of 60 because it is better sampled, **not because
  anything got worse**. Someone reading only the last number learns nothing;
  the sequence shows that the mechanism was derived from bytecode before it was
  measured every time, and that one of the two hypotheses was refuted by its
  own pre-registered floor and said so.

  Still **not ratcheted and still not a parity claim.** The residual is
  characterised rather than mysterious, and one earlier characterisation of it
  was wrong: `set_entity_motion` was never an unstable predicate but
  `compare::near` unable to bound an observation carrying no position. See
  `docs/WORKLIST.md` for the attribution and `ci/checks/gameplay-unstable.toml`
  for the named causes.

**Do not read 13/51 against the 9/12 this table used to show.** That number
was taken while the harness was holding nine chunks, so almost every check
was comparing two silences, and it was withdrawn along with the finding it
supported. The corpus has also more than doubled since, and the new checks
were chosen to *fail* — a corpus that only contains what already works is a
scoreboard nobody can lose.

**Every remaining divergence names a specific missing mechanic**, which is
what a corpus is for: a falling-block entity for unsupported sand, redstone
power on a wire, a door and a bed writing their second half, a slab merging
into a double, a stair taking a corner shape, chests pairing, snow counting
layers, and a door's two halves breaking together.

**The fourteen added on 2026-08-03 say it more sharply**, because each one
reports which observation *kinds* we never sent:

```text
  block_event            4 checks — both pistons, the note block, a chest lid
  open_screen            2 — a furnace menu and a crafting table menu
  section_blocks_update  2 — one packet where we send two
  add_entity             1 — an item entity for a dropped stack
  container_set_slot     1 — the offhand swap
  open_sign_editor       1 — placing a sign opens its editor
```

`block_event` is the single highest-leverage gap in that list: it is one
small packet (`Level.blockEvent` recorded, `ServerLevel.runBlockEvents`
flushed at the tick boundary) and four checks are waiting on it.

**Three of the new checks are missing no kinds at all** and diverge only on
content, which puts them closest to green: the redstone-torch inverter (5
observations against our 3), the observer pulse (6 against 4), and the
repeater delay at **8 against 7** — one observation short.

**That shape is a reliable predictor, and it has now paid out twice.**
`lever-flips-when-right-clicked` had exactly it — no missing kinds, one
observation short, vanilla sending `lever[powered=true]` where this server
sent `powered=false` — and porting `LeverBlock.useWithoutItem` moved it
`diverge -> match`. Extracting the whole `useWithoutItem` family then closed
`a-repeater-delays-the-signal-it-passes-on` as well.

**And that second one falsified the prediction written beside it.** The
repeater check was expected to need redstone signal propagation, because its
rig puts dust in front of the repeater and the point of a repeater is what
comes out the other side. It did not: cycling `DELAY` was the whole gap, and
the check went to `match` at 8 observations against 8. The lesson is the one
this page already carries in the other direction — read the trace, not the
summary — and it cuts both ways.

**Read `strict` as noise between runs, not as progress.** It moved 5/51 ->
3/51 across two runs whose only server change was that lever, which cannot
account for two checks. Strict carries the reference's own run-to-run
nondeterminism inside each scenario's box. Only `declared` is ratcheted by
`gameplay-baseline.toml`.

> This paragraph used to give "the ceiling measured by the selfcheck — 16/37
> for vanilla against *itself*" as the reason. **Corrected 2026-08-04: there
> is no ceiling.** 16/37 was one comparison at N = 2; at N = 12 vanilla
> reproduces itself under strict on **0 of 55**. The conclusion is unchanged
> and stronger — strict is noise between runs — but it is not noise around a
> 43% ceiling, it is noise all the way down until the 60-tick re-broadcast is
> dealt with.

**`coral-dries-out-unprompted` matches.** It was the last divergence on the
old table and it was never the coral rule: the scheduled tick had been right
all along and the broadcast that carried it was going out at the wrong
moment.

### The verdict that does the work

From a client, "did it differently" and "never implemented it" look
identical — the packet you waited for did not arrive. So:

  * **diverge** — we produced observations of the kinds vanilla did, and they
    differ. The mechanism exists and is wrong.
  * **absent** — we produced *nothing*. The mechanism is not implemented.
  * **vacuous** — **vanilla** produced nothing. The scenario proves nothing
    and is a fault in the check. This is the project's own rule mechanised:
    a negative result is only evidence if the mechanism was demonstrably
    live, and vanilla's trace is the proof that it was. A vacuous check can
    never pass and never fail; it fails the *gate*, as a broken instrument.

`ci/checks/gameplay-baseline.toml` is a ratchet, not a pass list: a check may
not go backwards, a new check must state where it starts, and `vacuous` and
`error` are refused outright.

### What it found on its first run — and the withdrawal

The first run reported that **we tell clients about block changes vanilla
does not**: that vanilla says almost nothing to the acting player, resyncs
two positions after a `use_item_on` where we send one, and tells a nearby
player nothing at all.

**That finding is withdrawn. It was measured against a client that was not
where the harness thought it was.**

Vanilla's own console, asked directly, put the acting player at
`[-0.5, -60.0, -1.5]` — still at spawn — while the harness believed it was
32 blocks away at its workspace. Every dig and placement in the corpus had
been aimed at empty air a workspace from the player. Vanilla was not
declining to send block updates; **nothing was happening at all.**

The cause is one packet. `minecraft:player_loaded` is serverbound 44, an
empty record, and `ServerGamePacketListenerImpl` gates **eight** handlers on
`hasClientLoaded()` — movement, block actions and item use among them. Each
returns immediately if the client has never sent it. There is no error, no
acknowledgement, no log line. A `clientLoadedTimeoutTimer` eventually marks
a silent client loaded anyway, which is why the harness did not fail
outright but failed *intermittently* — the worst of both.

Reading the bytecode also settled what the earlier run could not explain,
and settled it against the earlier conclusion:

  * `ChunkHolder.broadcastChanges` sends to `getPlayers(pos, false)` — **all**
    players tracking the chunk, one change as a `block_update` and several
    in a section as a `section_blocks_update`. Vanilla broadcasts block
    changes to nearby players, so **this server is right to** and must not
    be changed to match the empty streams the broken harness recorded.
  * `handleUseItemOn` silently drops an interaction that fails
    `isWithinBlockInteractionRange`, and it acknowledges the sequence
    *before* that check. An out-of-reach placement therefore produces an ack
    and nothing else — which is exactly what the corpus recorded and
    misread as a difference in how block updates are sent.
  * With the packet sent, vanilla's console-driven `setblock` and `summon`
    both reached **both** clients: a `block_update` for the block and an
    `add_entity` of type 100, `minecraft:pig`. Broadcasting and entity
    tracking were never broken.

So the pig mystery and the block-update mystery were one bug in the harness,
and neither was a fact about either server. The corpus numbers this
document quotes are the ones measured **after** that fix.

### The intermittent channel, and what it actually was

For a day this harness had an unreliable half. Whether a block change reached
a given client varied run to run with no change to either server, on *both*
clients: `break-block` went `match` -> `diverge` because vanilla sent the
digging client the block update in one run and only the ack in the two
before it. The reliable half was the two `handleUseItemOn` resyncs and the
`block_changed_ack`, and every check that matched rested only on those.

**The split was real and the explanation was wrong. It was never actor
against observer, and it was never a race in either server: it was direct
sends against broadcasts, and the broadcasts were being withheld from
everybody because this harness was not a conforming client.**

`PlayerChunkSender` is constructed with `maxUnacknowledgedBatches = 1` — the
`MAX_UNACKNOWLEDGED_BATCHES = 10` constant is only ever assigned inside
`onChunkBatchReceivedByClient`. `sendNextChunks` returns immediately while
`unacknowledgedBatches >= maxUnacknowledgedBatches`. So a client that never
sends the serverbound `chunk_batch_received` packet (id 11, one big-endian
`f32`) is sent one batch of `floor(START_CHUNKS_PER_TICK)` = **9 chunks** and
then nothing at all, for the rest of the session.

That reaches far past chunk loading because of one predicate:

```java
ChunkMap.isChunkTracked(player, x, z)
    = player.getChunkTrackingView().contains(x, z)
      && !player.connection.chunkSender.isPending(ChunkPos.pack(x, z))
```

`ChunkHolder.broadcastChanges` sends to `getPlayers(pos, false)`, whose only
filter is exactly that, and so does entity pairing in
`ChunkMap.TrackedEntity.updatePlayer`. **A chunk still queued for delivery is
a chunk whose block changes and entities are silently withheld.** Direct
sends go to `connection.send` with no such gate — which is precisely the
reliable/unreliable split, mechanised.

Measured, `ci/chunk-batch-probe.sh`, vanilla 26.2 superflat, view-distance 4,
8 s after login, Threadripper:

```text
  without the acknowledgement    chunks=9    batches=1
  with the acknowledgement       chunks=117  batches=4
```

Every scenario walks 32(n+1) blocks to a workspace outside those nine chunks,
so at the workspace every chunk in the tracking view had been re-marked
pending and no broadcast could reach either client. What made it
*intermittent* rather than simply absent is the narrow window after
`ChunkMap.move` widens the tracking view and before the new chunks are marked
pending: `contains` is already true and `isPending` is not yet, so a change
landing in that window *is* broadcast.

**What this falsifies**, stated plainly because it was written down as a
finding twice: "vanilla never sends the acting player a broadcast
`block_update`; its actor-facing protocol only ever corrects a wrong
prediction". No. `ChunkHolder.broadcast` is
`list.forEach(p -> p.connection.send(packet))` with no exclusion of anyone.
The actor was missing from the *list*. `Players::broadcast_except` has been
removed and a placement now puts four things on the actor's socket: resync of
the clicked block, resync of the block placed into, the broadcast, then the
ack.

**And what it retires.** The three unexplained results in
`ci/checks/gameplay/open/README.md` — a pig five blocks away never announced,
two players five blocks apart never told the other moved, `add_entity`
arriving only after a 24-block round trip — are all this same predicate and
need no probe of their own. So is the observer channel's apparent
untrustworthiness. Nothing about who was watching ever mattered.

**Two parity gaps this opened in our own server**, named rather than left to
be found. We do not implement `PlayerChunkSender` at all — no per-tick quota,
no batch acknowledgement, no `pendingChunks`, and **no gating of broadcasts
or entity pairing on chunk delivery** — so a client that never acknowledges a
batch gets nine chunks from vanilla and the whole world from us. And
`ChunkHolder.broadcastChanges` coalesces more than one change in one section
in one tick into a `section_blocks_update`, where we send one `block_update`
each.

### The tick boundary, and the two things immediate broadcasts got wrong

Fixing the chunk-batch stall made the broadcast channel visible for the first
time, and the first thing it showed was that this server was broadcasting at
the wrong moment and in the wrong shape. Measured against real vanilla on
2026-08-03 over 23 scenarios: **13 divergences and 8 `inert` verdicts, and
between them they are these two facts.**

Block changes used to go out the instant the world was written, from the
connection thread that caused it. Vanilla does not. `Level.setBlock` reaches
`ChunkHolder.blockChanged`, which only **records** the position into a
per-section `ShortOpenHashSet`. The packets are a tick-boundary event.

**Order.** One placement puts four things on the acting client's socket:

```text
  block_update       resync of the clicked block          handleUseItemOn
  block_update       resync of the block placed into      handleUseItemOn
  block_update       the broadcast                        ServerChunkCache.tick
  block_changed_ack                                       ServerGamePacketListenerImpl.tick
```

The ack is last because `ackBlockChangesUpTo` only records
`max(seq, ackBlockChangesUpTo)` and the connection's own `tick()` sends it.
Broadcasting immediately put ours **first** instead of third, and all 13
divergences were at index 0.

**Coalescing.** More than one change in one section in one tick is one
`section_blocks_update`, not several `block_update`s. Vanilla sent 18 of them
across the corpus and this server sent none — which is why every scenario
that moves two blocks at once came back `inert`: a door, a bed, a pair of
chests, a fence connecting to its neighbour, a torch falling off its support.
The harness had been recording every one as `packet:0x54`.

`crate::broadcast` is that boundary and `crate::shortset` is the detail
underneath it. **A hash set's internal order is a wire format here**:
`ClientboundSectionBlocksUpdatePacket` writes the positions in the set's own
iteration order, which is neither insertion order nor sorted order. fastutil
places a key at `mix(k) & 31` in a 32-slot table, iterates the table
**downward**, and never stores zero in it at all — the zero key lives in a
`containsNull` flag and comes out first. All three are reproduced, and
`sectionRelativePos` is `0` at a section's own corner, so the third is
reachable rather than theoretical.

`broadcast::tick_lock` stands in for vanilla's single game thread: a
connection thread holds it for the whole of a block handler and the clock
thread holds it across the flush, so a handler can never interleave between a
tick's broadcasts and its acknowledgements. It is a coarse global
serialisation point, and making it finer is a later optimisation that owes
the usual proof of being output-identical.

#### The `shortset` model was checked against three independent orderings

Worth recording because it is the rare case where a reproduction was
*confirmed* rather than merely not contradicted, and because the confirmation
came from something that first looked like a bug.

The fence pair produced a two-block section update on both servers with
identical contents and, on one run, **opposite order**. The order is a
function of `SectionPos.sectionRelativePos`, which is built from the low four
bits of the **absolute** x, y and z — and vanilla's superflat spawn moves from
world to world while this server's does not, so the two servers were standing
13 blocks apart and hashing different numbers. `crate::shortset` predicts all
three observed orders correctly: vanilla's spawn in one run, vanilla's spawn
in the next, and this server's. Descending table iteration, `mix(k) & 31`, no
adjustment.

So the model is right and the *comparison* was wrong. `WORKSPACE_SPACING` now
places every workspace at an absolute coordinate rather than a spawn-relative
one, which is the only way a position-dependent packet can be compared at
all. **No run before 2026-08-03 could have caught a wrong `shortset`, and
none should be trusted on ordering.**

### A scenario is bounded to its workspace

Fixing the stall broke an invariant it had been hiding. Scenarios share one
world and march 32 blocks apart, which isolated them only because each client
was holding nine chunks for a whole session. With chunk delivery working, a
client at one workspace can see several previous ones, and the first
corrected run recorded two `block_update`s for a `grass_block` becoming
`dirt` 21 and 71 blocks away — positions no scenario ever touched, in a world
with `randomTickSpeed 0`. **That remains unexplained** and is written down
here rather than quietly filtered.

Scenarios therefore have a `within` radius, default 16 blocks, Chebyshev and
horizontal only. It is a *projection*, applied at compare time exactly like
`observe` and `observe_from`; the strict denominator still reads every
observation recorded, so whatever it hides is still counted in the gap
between the two numbers. Widening `WORKSPACE_SPACING` instead would have cost
about four times the corpus's running time, because `goto` hops half a block
at a time.

### A check has to be able to fail: the `inert` verdict

`vacuous` catches a reference that produced nothing. Once the two
unconditional resyncs were reproduced, **every scenario built on `place` has
a non-empty vanilla trace by construction**, and vacuous alone stopped being
able to tell a live check from a dead one. `water-spreads` is the worked
example: it went `absent` -> `match` on two servers agreeing to do nothing,
with three observations on the reference side, every one of them forced by
the scenario's own click.

`inert` subtracts what the actions force — the ack, and one `block_update` at
each of the two positions a `use_item_on` resyncs — and refuses the check if
nothing is left. What survives is the `ChunkHolder` broadcast, which
`Level.setBlock` only reaches when the state really changed. A dig produces
no resyncs, so a dig check is live on its own. Like `vacuous`, it is never
baselineable and fails the gate rather than the ratchet.

### And a check has to be able to *pass*: eight that cannot, ever

`ci/t2-gameplay-selfcheck.sh` was written on 2026-08-03 and first run on
2026-08-03. **34 of 37**, not all-match, and all three failures are vanilla
disagreeing with **itself** across two cold-booted servers on the same seed.
They are permanent, and they are the mirror image of everything above: not a
check that cannot fail, a check that cannot pass.

Re-run against the grown corpus later the same day it came back **48 of 55**:
four more, none of them noticed when those checks were written and baselined,
because the corpus went 37 -> 55 without the selfcheck being re-run. They are
`a-bystander-sees-a-chest-open`, `dropping-an-item-throws-it-on-the-floor`,
`other-player-is-an-entity` and `swapping-to-the-offhand-shows-on-both-
channels`, and every one of them is on the entity or container channel the
next tranche of work is about. Their mechanisms are in
`ci/checks/gameplay-unstable.toml` with the bytecode each was derived from —
a chest re-broadcasting its viewer count every 5 ticks forever, four
`nextFloat()` draws in the throw vector of a dropped item, a walk divided
into per-tick broadcasts, and a container state id that counts packets rather
than changes.

Raising the instrument then found two more that two runs had never seen:
`chest-opens-a-screen`, which is the same chest re-broadcast on the acting
connection, and `a-lever-lights-a-lamp` — which turned out not to be a vanilla
instability at all but a **rig straddling a chunk boundary**, the fault check
235 was closed for, and was fixed rather than declared. The workspace origin
sits at absolute z 0, so a rig writing at relative z = -1 and z = 0 is writing
in two chunks and measuring vanilla's inter-chunk send order. The rule is now
in `ci/checks/gameplay/README.md` rather than in the three check files that
have each had to rediscover it.

**Then the instrument ran at its configured N for the first time, and moved
the list in both directions.** Every attempt before this one was killed by its
launcher at about 65 minutes — twice, both in run 6 at `recorded 25/55` — so
the twelve-run design had never executed and the names above were assembled by
hand from the traces that survived. Run to completion, detached, 2026-08-03,
twelve cold vanilla servers, **47 of 55**:

```text
  water-spreads                                  11/11 comparisons
  a-dropped-item-goes-into-your-inventory         11/11
  dropping-an-item-throws-it-on-the-floor         11/11
  spawn-egg-makes-a-mob                           11/11
  other-player-is-an-entity                       10/11
  swapping-to-the-offhand-shows-on-both-channels   9/11
  a-bystander-sees-a-chest-open                    2/11
  sand-falls-when-unsupported                      1/11   NEW
  chest-opens-a-screen                             0/11   WITHDRAWN
```

Two corrections fall out of that column, and the spread is the point: a
two-run design cannot tell 11/11 from 1/11, and five of these eight are not
near-certain.

`chest-opens-a-screen` **comes out**. Its declared mechanism was real — an open
chest emits the same `block_event(1, 1)` every 250 ms forever — but it was
declared by analogy with the 2000 ms window that does flap, from a run that
never produced a comparison. Its own 1000 ms window is four whole cadences and
lands clear. A real mechanism is not a measured instability. (0 of 11 is not
proof of stability either, at the rate the new entry was caught at; the
withdrawal says so.)

`sand-falls-when-unsupported` **goes in, and it is the only entry in that file
that is ours rather than vanilla's.** One field differs, at 1 of 11:
`add_entity` with the same position, the same entity type and the same FNV
digest of every remaining byte — and `entity: 0` against `entity: 1`. That is
`Normalizer` assigning a different index to the same falling block, because it
renumbers in `decode` over every packet rather than in `project` over the
packets a check observes, through one normalizer shared between two sockets
read in turn. It was written up as an armed-but-unsprung trap earlier the same
day, from the code, before any run produced it; this is it sprung, **vanilla
against vanilla**, where there is no server difference to confuse it with. The
entry comes out when the renumbering moves.

**The instrument that found them was itself under-sampled.** Two runs, flagged
unstable when they disagree, catches a two-outcome check at `2p(1-p)` — 50% at
best. Check 235 came back 4:2 across six runs, which a two-run selfcheck had a
44% chance of ever seeing. It now records the corpus twelve times and compares
every run against the first: `1 - p^12 - (1-p)^12` is 99.2% on a 4:2 split and
99.95% on an even one, and the honest limit — a one-in-six flap still getting
through about one run in nine — is written into the script beside the
arithmetic.

Two are unmatchable *in principle*, by any implementation including vanilla,
because the value on the wire is drawn fresh from the level's shared,
unseeded RNG:

* **`spawn-egg-makes-a-mob`.** `EntityType.spawn` calls
  `snapTo(x, y, z, Mth.wrapDegrees(level.getRandom().nextFloat() * 360.0F), 0)`.
  A spawned mob's yaw is a random draw. The position matched exactly on both
  runs — `[1.5, 0.0, 0.5]` — and only the digest of the packet's tail moved,
  which is where pitch, yaw and head-yaw live.
* **`a-dropped-item-goes-into-your-inventory`.** `Block.popResource` places
  the item at `pos + 0.5 + Mth.nextDouble(level.getRandom(), -0.25, 0.25)` on
  each axis. Confirmed numerically as well as structurally: the two runs put
  the torch's drop at y 1.2427 and y 1.3222, and the torch was at
  `[1, 1, 0]`, so both fall inside `1.5 - ITEM.getHeight()/2 ± 0.25`. The
  consequence is worse than a moved field — the item lands inside the pickup
  radius in one run and outside it in the other, so `container_set_slot`,
  `take_item_entity` and `remove_entities` are present in one trace and
  absent from the other.

The third is fixable but is not fixed:

* **`water-spreads`.** Not RNG. One run sent a `block_update`, the other a
  `section_blocks_update` carrying two positions — the same changes,
  packaged differently, because how many scheduled fluid ticks land inside
  one broadcast depends on where in the tick the placement arrived. A
  projection comparing `(position, final state)` as a set would be stable,
  but the packaging is itself a property this corpus checks and
  `crate::shortset` exists because of it, so a corpus-wide change costs more
  than the check is worth. A per-scenario opt-out is the shape it should
  take.

**This matters more as the corpus grows into entities**, because the field
that moves is on `add_entity` and every entity check carries one. It is a
fact about the target, not about this server: there exist client-observable
fields that no implementation can match.

The error direction is at least the safe one — an unstable check makes this
server look *worse* than it is, never better, and the ratchet still works
because the verdict is stable at `diverge`. So it is a labelling problem,
and the label is `ci/checks/gameplay-unstable.toml`: eight declared with
their cause, **verified by the selfcheck in both directions**, so a newly
unstable check is a failure and a no-longer-unstable one is a stale file.
The selfcheck used to print numbers and ask the reader to think about them;
three permanent failures printed every run become wallpaper within a week,
and the next new instability hides behind them.

### The substrate is superflat, and that is deliberate

Both servers run vanilla's own superflat preset, in creative, with structures
off and `difficulty=peaceful`. Every column is identical, so a scenario
written relative to where the player stands is the same situation on both
servers *by construction* rather than by an argument about terrain parity.
The layers are **extracted** by `ci/dump-flat-preset.sh` from Mojang's data
generator into `data/vanilla/flat_preset.json`, never transcribed, and
`rustmc run --level-type flat` refuses to start without them.

`difficulty=peaceful` is substrate, not tidiness: superflat is famously full
of slimes, and one scenario recorded **32 sound observations** that had
nothing to do with it before that landed.

**The four gamerules this harness sets were a silent no-op from the day the
corpus was written until 2026-08-03**, and that is the most expensive thing
on this page after the chunk-batch stall. Game rules became a **registry** in
26.2 — `minecraft:game_rule`, 59 entries — with snake_case namespaced ids, so
`gamerule randomTickSpeed 0` answers `Incorrect argument for command` and the
server carries on:

```text
  doDaylightCycle -> advance_time        doMobSpawning   -> spawn_mobs
  doWeatherCycle  -> advance_weather     randomTickSpeed -> random_tick_speed
```

Random ticks were therefore running for every measurement this document has
ever quoted. That is what turned the grass under a freshly placed rig block
into `dirt` at unpredictable moments — the "unexplained" observation that had
been chased twice, first as far-away noise and then as a bug in
`SpreadingSnowyBlock`, which has exactly one block-writing method and it is
`randomTick`. The bytecode was right and the world was not the one this file
described.

`ci/t2-gameplay.sh` now runs each rule through `gamerule_or_die`, which greps
the server log and **exits** on `Incorrect argument` or `Unknown or
incomplete`. A console command that fails must not be able to fail quietly,
and a substrate a script claims and does not have is worse than one it never
claimed.

So are `level-seed` and `spawn-radius=0`, added 2026-08-03, and the reason is
the sentence above about "relative to where the player stands" being not
quite enough. The seed was empty, so every run generated a fresh world and
the shared spawn moved with it; `spawn-radius` was vanilla's default 10, and
`PlayerList.placeNewPlayer` calls `ServerPlayer.fudgeSpawnLocation`, which
picks a random position inside that radius on **every join**. Three
consecutive runs put the reference player at (-4.5, 2.5), (-3.5, -2.5) and
(3.5, 6.5).

**Not every packet is position-invariant, which is what makes that a
substrate problem rather than an aesthetic one.** The block order inside a
`section_blocks_update` is the iteration order of a `ShortOpenHashSet` keyed
on `SectionPos.sectionRelativePos` — the low four bits of the *absolute* x, y
and z — so with a moving spawn the reference disagreed with itself between
runs on that packet's bytes. Workspaces are absolute now for the same reason:
`(-(n+1) * 32, spawn_y, 0)` on both servers, so the two are never comparing
situations 13 blocks apart.

### Seven instrument bugs, and why they are written down

Every one produced *silence*, and silence is the one thing this harness must
never report by accident. Between them they invalidated two whole runs'
conclusions, which is the argument for writing them down rather than fixing
them quietly. **Two of them were mistaken for findings about vanilla and
written into this server's code**, which is the strongest argument of all.

  * **`chunk_batch_received` was never sent**, so vanilla delivered nine
    chunks and stopped, and `ChunkMap.isChunkTracked` withheld every
    broadcast and every entity from both clients wherever they worked. It
    produced an *intermittent* silence, which is worse than a total one: it
    read as a race in the servers. It cost the whole observer channel, three
    parked checks, and a wrong exclusion rule in `Players::broadcast`. See
    "The intermittent channel, and what it actually was".
  * **`player_loaded` was never sent**, so vanilla dropped movement and every
    block interaction — see the withdrawal above. Invisible from the client,
    and the reason a whole corpus was measured against a player standing 32
    blocks from where the harness believed it was.

  * **A short socket read timeout can fire inside a frame.** Treating the
    resulting `WouldBlock` as "nothing arrived" reads the middle of that
    frame as the next frame's length, and everything after it is garbage. It
    took out all eleven scenarios against rustmc while passing against
    vanilla, purely because our chunk packets are bigger. `Conn::recv_poll`
    now applies the short timeout to the **first byte only**.
  * **A decoder that ran off the end dropped the packet.** Vanilla's own
    `add_entity` went unrecorded and a mob that had demonstrably spawned
    looked like one that had not. A failed decode is now recorded as
    `packet:0x..` with a digest of the whole body.
  * **Vanilla ignores the first block action of a session.** Seq 1 was never
    acknowledged; seq 2 onward always was. The harness now spends one dig at
    a scratch workspace before any scenario runs.
  * **A measured step must not consume the socket**, because whatever it
    consumed it also discarded — and the packets a walk causes are the ones a
    walk exists to provoke.
  * **A client that is not where it thinks it is observes nothing, and does
    nothing.** The walk sent four blocks every 40ms — about a hundred blocks
    a second — and vanilla simply did not move the player, without always
    saying so. The first arrival check missed it because the correction
    arrived *after* the check had already taken the anchor from the
    harness's own belief. `goto` now walks at a speed vanilla accepts, reads
    the socket between every step so a correction is applied during the walk,
    settles **before** deciding it has arrived, and fails by name otherwise.

`ci/checks/gameplay/open/` holds checks that were written, measured, and
proved to demonstrate nothing — with what is already ruled out, so nobody
re-runs it. All 45 packet ids this harness uses were cross-checked against
Mojang's report and all 45 are correct.

### Three more, and none of them were in the harness (2026-08-03)

The same class, found in the *extractors* rather than in the corpus. Each one
reported a clean number that was a fact about the instrument.

  * **A water probe that could not detect water.** `PlacedGolden` probes each
    `setPlacedBy` twice — once in air, once in water — and flags any whose
    answer moves, so a row conditioned on a world the harness does not have
    is marked rather than shipped. It filled the map with `Blocks.WATER` and
    flagged **zero**. The only path that asks is `LevelReader.isWaterAt`,
    which is `getFluidState(pos).is(FluidTags.WATER)` — a **tag** lookup, and
    `Bootstrap.bootStrap()` binds no tags, so it answered false in water
    exactly as in air. `TableLevel.isWaterAt` is overridden now and the oracle
    *requires* `small_dripleaf` to come back sensitive; it reports 1 of 50,
    which is the one block whose bytecode reads water.
  * **A randomness detector that was a coin flip.** `ci/use-golden.sh` decided
    a rule was random by probing each state twice and comparing. That is a
    coin flip when a rule has few outcomes: this harness's world is air
    everywhere, `DragonEggBlock.teleport` needs a target whose *support* is
    not air, and the only such square in the whole world is the one directly
    above the egg — so the search either finds it or exhausts its 1,000
    attempts, and two probes agree whenever both succeed or both fail.
    Measured over six dumper runs: **twice out of six the egg came back "not
    random"**, which churns the file and fails the two-run `cmp` with nothing
    changed. It has been like that since the file was written. Every state is
    now probed under eight **fixed** RNG seeds through an overridden
    `getRandom`, so the classification is the same question every run.

    Two wrong fixes are recorded in the source because both looked right.
    Flagging any rule that *touched* the RNG is exact and cannot flake, and is
    far too coarse — `DoorBlock.playSound` draws a sound **pitch**, which no
    table here carries, so 55 blocks including every door, trapdoor and fence
    gate came back random and lost every cell. And seeds `0..5` are not
    independent: `RandomSource.create` is `java.util.Random`'s algorithm,
    whose scramble leaves small consecutive seeds differing in a few low bits,
    and under those six the egg's teleport failed under **all** of them. It
    looked stable and was blind.
  * **A class map that answered nothing.** `class` was added to the use table
    so a rule could ask "is this a `ButtonBlock`" without keying on the
    block's *name* — and the table was not regenerated. Every lookup returned
    `None`, which reads as "not a button", so the un-press silently did not
    happen and the corpus run showed no error anywhere. The map now lives in
    one place, covers the **whole registry** rather than only the blocks with
    a row, and `ci/placed-golden.sh` asserts its size and three specific
    classes. A partial map is the same bug in slower motion: it answers "not
    that class" for everything it omits.

### What it does not cover

One seed's worth of substrate, and **timing is excluded from the strict
comparison** — wall-clock milliseconds between two different servers is not a
behavioural property. Tick-accurate timing is the instrument that would close
that, and it does not exist yet.

**This paragraph said "no inventory, no entities" until 2026-08-04 and both
had been false for a day.** Counted by `./ci/corpus-coverage.py` over the
**63**-check corpus, as distinct checks projecting at least one kind in each
family. (The prose said 62 while the rows beside it said 63; re-derived and
the rows were right.)

```text
  entities         7 of 63   add_entity, remove_entities, set_entity_data,
                             set_entity_motion, entity_position, move_entity,
                             take_item_entity
  inventory        7 of 63   container_set_slot, container_set_content,
                             set_equipment, open_screen
  block entities   9 of 63   block_entity_data, block_event, open_sign_editor
```

Re-run 2026-08-05 on the 70-check corpus, `./ci/corpus-coverage.py` in
`~/rustmc-chain-container`. The denominator had drifted by four before this
branch touched it, which is the ordinary failure of a hand-maintained number
beside an automated one:

```text
  entities         9 of 70   add_entity, entity_event, entity_position,
                             move_entity, remove_entities, set_entity_data,
                             set_entity_motion, take_item_entity
  inventory       10 of 70   container_set_content, container_set_slot,
                             open_screen, set_equipment
  block entities   9 of 70   block_entity_data, block_event, open_sign_editor
```

Inventory moved 7 to 10 on the three container-click checks and entities 7 to
9 on `entity_event` and `remove_entities` entering the family. **`inventory`
is still the family to distrust**: every one of those ten watches a container
*open* or an equipment slot change, and until 2026-08-05 not one of them could
put an item anywhere. Three now can, and all three say the item does not stay.

Refreshed 2026-08-04 by `./ci/corpus-coverage.py`, second time that day.
Entities moved 5 to 6 on `530-walking-away-drops-a-standing-player-from-view`
and 6 to 7 on `540-a-player-that-leaves-the-ground-is-resynced`; the other two
are unchanged in kind and only in denominator.

**The script was lying about one row and it has been fixed in the same pass.**
It reported `move_entity_heartbeat` as `NOT DECODED — projected but the
decoder never emits it`, which says a live projection is dead. `trace.rs`
yields that kind as `HEARTBEAT.into()` and the extraction regex matched only
string *literals*, so a kind behind a named constant was invisible to it.
`520-a-standing-player-is-rebroadcast-on-a-timer` is `match` and watches
exactly that kind. This script exists so that no corpus figure is
hand-counted; a wrong automated figure is worse than a hand-counted one,
because nobody re-checks it.

Thin, and not nothing. The distinction matters because a "not covered at all"
reads as a reason to write the first check, and writing a first check that
already exists is how two of this corpus's duplicates got in.

**The staleness is not this branch's doing, and the same script says so.** Run
against the corpus at `fea2c3a`, before any of the work above, it reports
`entities 5 of 55`, `inventory 6 of 55`, `block entities 9 of 55`. Four of the
five entity checks and all six of the inventory ones were added on 2026-08-03
and this paragraph was not touched. Only the inventory figure moved here, 6 to
7, because adding `open_screen` to 490's projection put that check into the
family — so of the three numbers above, two are entirely the previous day's
and the third is one check different.

**No survival, and that one is structural rather than thin.** The substrate is
creative by construction, and in creative
`ServerPlayerGameMode.handleBlockBreakAction` takes the `instabuild` branch on
`START_DESTROY_BLOCK` and returns *before* `destroyPos` is ever assigned —
that assignment is on the survival path, at the "actual start of destroying"
log line. `STOP_DESTROY_BLOCK` then tests `pos.equals(this.destroyPos)`, never
matches, and falls to a `debugLogging` call that sends nothing. The only
packet on the wire is the unconditional `ackBlockChangesUpTo(seq)` that
`ServerGamePacketListenerImpl.handlePlayerAction` makes after the game-mode
call returns — which is exactly what the action itself forced, so any check
built on `finish_dig` is `inert` by construction and this gate refuses it.
`finish_dig` is therefore at **zero checks by a substrate constraint, not an
oversight**, and closing it needs a survival player rather than another
scenario.

Measured rather than inferred, by `./ci/finish-dig-probe.sh` on 2026-08-04,
with a plain `dig` recorded beside it as the control — silence is only
evidence next to a rig known to be live:

```text
  probe-finish-dig-alone   block_changed_ack seq=3       and nothing else
  probe-dig-only           block_update [1,0,0] -> air, block_changed_ack
```

Three more, added 2026-08-03 because each is a gate that cannot fail and
saying so is cheaper than someone assuming otherwise:

  * **Every workspace sits at absolute `z = 0`**, so section `z` is never
    negative and no check can exercise `SectionPos.asLong`'s 22-bit `z` — the
    field this server had at 20 bits until today. Only the unit test in
    `crate::broadcast` covers it.
  * **Every scenario changes blocks in one chunk**, so nothing here can tell
    our cross-chunk broadcast order from vanilla's. Ours is sorted by
    `(x, z)`; `ServerChunkCache.tick` walks `ChunkMap`'s own iteration order.
  * **No scenario fires two actions inside one tick**, so nothing can see
    that vanilla drains packets at a fixed point in the tick while this
    server handles each on arrival — two clicks 20 ms apart are one
    `section_blocks_update` in vanilla and two packets here.

A fourth, found 2026-08-04 and closed the same day:

  * **Nothing changed anybody's visibility.** Every check that moved anything
    moved the actor, and the only other entity in almost every scenario is
    the other player, which moves too — so `ChunkMap.tick`'s `movedPlayers`
    sweep, whose entire job is the case where the *player* moved and the
    *entity* did not, could be deleted with no check noticing.
    `530-walking-away-drops-a-standing-player-from-view` closes it and is
    written up under "Entities exist" below.

### Two things a new check inherits from the checks before it

Both measured from the gate's own vanilla recording, 2026-08-04, and both
change which item is cheap rather than being defects.

**Container ids and state ids are per-player counters that run for the whole
corpus.** One actor plays every scenario, `ServerPlayer.nextContainerCounter`
is `containerCounter % 100 + 1` on a field that lives as long as the player,
and `AbstractContainerMenu.incrementStateId` is the same shape for the
inventory menu, which is window 0 and never closes:

```text
  open_screen         window=1  chest-opens-a-screen
                      window=2  a-furnace-opens-a-screen-a-chest-does-not
                      window=3  a-crafting-table-opens-a-menu-with-no-block-entity
                      window=4  a-bystander-sees-a-chest-open

  container_set_slot  window=0 state=2  torch-falls-with-its-support
                      window=0 state=3  swapping-to-the-offhand (slot 36)
                      window=0 state=4  swapping-to-the-offhand (slot 45)
```

So the four menu checks are **one indivisible item in corpus order**: a
server that implements only the crafting table sends `window=1` where vanilla
sends `window=3` and diverges on a field that has nothing to do with crafting
tables. `a-crafting-table-opens-a-menu-with-no-block-entity` looks like the
cheapest of the four — no block entity, a one-key title, `stateId` always 1 on
a fresh menu — and it is not separately closable.

> **TRUE OF VANILLA, NO LONGER TRUE OF THE COMPARISON — both counters are now
> rebased.** `compare::rebase_session_counters` rebases `window` (since the
> `worldgen-iceberg-912` repair) and `container_set_slot.state` (since
> 2026-08-11, scoped per `(connection, window)`) onto the scenario, so what is
> compared is each value's distance from the first one in the check's own
> projection. Vanilla's counters still run for the whole corpus exactly as
> described above — the mechanism is unchanged and the table is still the
> right reading of it — but **the inheritance no longer reaches a verdict**,
> the four menu checks are no longer one indivisible item, and three
> `ci/checks/gameplay-unstable.toml` entries that existed only for the state
> id were retired with the decoder. What a later check still inherits is what
> a scenario leaves in the *world* or in the actor's *inventory*, which is why
> `790` must stay last.
>
> **MEASURED 2026-08-12, because everything above is a reading of source.**
> `ci/window-coupling.py` shifts the whole session counter in a real
> 109-scenario vanilla recording and compares it against the unshifted
> original: +1, +2, +7 and +99 each move **zero** verdicts, declared and
> strict, across the 34 scenarios that carry an allocated window id. The
> coupling is not four and not one; it is zero, and menu coverage scales in
> any order. The controls that make that zero quotable, and the one real
> container-id defect the corpus was blind to until `757` was written, are in
> `docs/PARITY-ROADMAP.md` §42.

**Vanilla's NBT compounds are not written in sorted key order — and neither
are ours, as of 2026-08-05.** `CompoundTag` wraps a `java.util.HashMap` and
`CompoundTag.write` iterates `keySet()`, so the wire order is Java's hash
order: bucket-major, a pure function of `String.hashCode` and the table
capacity. It is visible in vanilla's own bytes without decoding anything,
because `trace::ascii_runs` scans the packet in wire order — the reply to
`/list` carries `with` before `translate`, and inside a player's display name
`click_event, insertion, text, hover_event`. Neither is alphabetical.

`rustmc_world::nbt`'s writer now emits that order. The module that reproduces
it, `java_hash_set`, was in `rustmc-worldgen` — which depends on
`rustmc-world`, so `nbt.rs` calling it was a dependency cycle — and has been
lifted down to `rustmc-data`, the bottom of the graph. It is **not** a second
copy: the one that moved is verified against real JVM output by
`ci/hash-order-golden.sh`, and a second implementation would be an unverified
one that agrees with the first until it does not.

The reproduction is exact when a compound's keys occupy distinct buckets, and
falls back to sorted order — legal NBT, which is what it emitted before — when
the table refuses to order itself. Everything this server writes today is in
the first case and the ones that matter are asserted, not assumed.

This was the whole difficulty of `server-answers-a-command`, whose single
`chat` observation is a `commands.list.players` component carrying two display
names with `click_event`, `insertion` and a `show_entity` hover; see
`crate::commands`, which holds vanilla's own 539 bytes as its test vector. It
will matter the same way for `placing-a-sign-opens-its-editor`. It does **not**
bite the menu channel: `container.crafting` is a one-key compound and one key
has no order — measured, not assumed, by 470 and 480 staying `match` across the
change.

## Measuring it end to end

`ci/dig-probe.py` is the only thing here that speaks the whole stack. The
unit tests drive the play loop with a fake world; the integration tests drive
the world with no socket. Neither can catch a wrong packet id, a frame the
server never flushes, or a compression threshold applied in one direction
only.

```sh
./target/release/rustmc-server run --port 25611 --radius 1
./ci/dig-probe.py 127.0.0.1 25611 Digger
```

Measured 2026-07-28 on the Threadripper, radius 1 (9 chunks offered, 49
generated, prepared in 6.4s), run twice against one server so the second run
is a second player in a world the first already changed:

```text
  spawned at (8.0, 75.0, 8.0), standing in block (8,75,8); game type 1
    break:      block_update minecraft:air at (8, 74, 8), ack 1
    place:      block_update minecraft:stone at (8, 74, 8), ack 2
    axis:       block_update minecraft:oak_log[axis=x] at (9, 74, 8), ack 4
    oak_stairs: block_update minecraft:oak_stairs[facing=east,...] at (9, 74, 8), ack 6
    furnace:    block_update minecraft:furnace[facing=west,lit=false] at (9, 74, 8), ack 8
  PROBE OK
```

The last two are one check, not two: they are placed from the **same** yaw
(−90, which `Direction.fromYRot` rounds to east) and come back facing opposite
ways. A server applying any single generic rule to `facing` cannot produce that
pair, so it is the one line that distinguishes "extracted per block" from
"guessed once".

`game type 1` is `GameType.CREATIVE` read off the join packet by walking it,
not by scanning for the `00 ff` of survival's `gameType 0,
previousGameType -1` — which is a byte pattern that only exists when the
answer is the one the probe is there to rule out.

## Running the gate while another agent shares the machine

`ci/t0-remote.sh` rsyncs the working tree to `threadripper:rustmc` with
`--delete`. Two agents running it against the same remote directory overwrite
each other mid-build, and the failure looks like your own edit vanishing —
one run here reported `cannot find value PLAYER_ACTION` for a constant that
was present in both the local and the remote file by the time it was
checked. Set `RUSTMC_REMOTE_DIR` to something of your own:

```sh
RUSTMC_REMOTE_DIR=rustmc-server ./ci/t0-remote.sh
```

A fresh remote directory has no `data/vanilla/` and no `oracle-work/` — both
are rsync-excluded on purpose — so the gates that need the extract skip
silently and report themselves as passing in 0.00s, which is the tell. Give
it both once:

```sh
ssh threadripper 'mkdir -p rustmc-server/data rustmc-server/oracle-work
  cp -a rustmc/data/vanilla rustmc-server/data/vanilla
  ln -sfn /home/sayer/rustmc/oracle-work/vanilla-26.2 \
          rustmc-server/oracle-work/vanilla-26.2'
```

Both live under an rsync `--exclude`, so `--delete` leaves them alone.

## Regenerating the inputs

**Both are needed to reach the play state at all**, and their absence is why
a checkout cannot join-test itself: `serve_one` disconnects during
configuration with the reason, rather than failing ten packets later.

Two files are extracted from a real server and gitignored, like the rest of
`data/vanilla/`:

```sh
ci/dump-registries.sh          # the 29 configuration registries
ci/dump-chunks.sh              # captured chunk packets, for the encoder gate
```

Two more are optional — the server runs without them and says on the console
what it loses:

```sh
ci/dump-items.sh               # 1,537 item ids; without it, dig but not build
ci/placement-golden.sh         # getStateForPlacement per block; without it,
                               # every placement takes the default state
ci/shape-golden.sh             # updateShape per state and direction; without
                               # it nothing cascades and a torch floats
```

Scheduled ticks need no extract of their own: which blocks schedule is
`placement.json`'s `schedulesTickBlocks`, and what they do when they fire is
in `crate::ticks` with its bytecode cited.

Registry *contents* are datapack data, not code — the same category this
project extracts everywhere else. Building them from the datapack JSON would
mean re-deriving NBT types that JSON does not carry, across hundreds of
fields in 29 registries: is `ambient_light` a float or a double, is
`has_skylight` a byte or a bool. Every one is a guess unless read from a
codec.

## Items: what the server knows about them, and what it does with them

Added 2026-09-05 by the `items` lane. Everything above about items is about
*placement* — `use_item_on`, the creative slot, `BlockItem`'s block. This is
the item itself.

### The registry was ids and nothing else, and now it is components

`data/vanilla/items.json` is Mojang's registry report: 1,537 names and their
protocol ids, no columns. So every question about what an item *is* was
answered by a constant. `crate::item::DEFAULT_MAX_STACK_SIZE = 64` said so in
its own doc comment.

`ci/item-props-golden.sh` walks `Item.components()` — the `DataComponentMap`
that `Item.Properties` builds — and writes `data/vanilla/item_props.json`:
**1,537 items, 19,356 components, 59 of the 111 registered component types,
0 refusals**, byte-identical across two runs. In 26.2 that one map is where
`stacksTo`, `durability`, `rarity`, `food`, `sword`/`pickaxe`/`axe`,
`humanoidArmor` and the rest all put their answers, so it is one table rather
than six that can disagree about which items exist.

Three things it needed that a naive dumper does not do:

* **`Item.components()` throws after a bare `Bootstrap.bootStrap()`.** The
  initializers are applied during a datapack load; the dumper runs
  `DATA_COMPONENT_INITIALIZERS` the way `UseGolden` already had to, and
  asserts it applied something.
* **Block tags must be bound or every tool rule is empty.** A `minecraft:tool`
  rule names its blocks as `"#minecraft:mineable/pickaxe"`, and an unbound tag
  resolves to zero blocks and throws nothing: 16 of 17 sets came back empty,
  and the resulting table said every tool mines everything at speed 1.0 and
  drops nothing. The binding is recursive, unlike `SurfaceChunkGolden`'s,
  because `tags/block/mineable/*.json` is a subdirectory.
* **Component order is imposed.** A built `DataComponentMap`'s `keySet` is
  identity-hashed, so components are emitted sorted by their registry id.

The table also carries `item.getClass().getName()` per item. The registry
report names no class, and it is what vanilla dispatches `use`, `useOn` and
`placeBlock` through.

### The census that orders any item work

70 classes over 1,537 items: `BlockItem` 938, plain `Item` 236,
`SpawnEggItem` 88, `DoubleHighBlockItem` 28, `BoatItem` 20,
`StandingAndWallBlockItem` 20, `SmithingTemplateItem` 19, `BundleItem` 17,
`BannerItem`/`BedItem`/`DyeItem` 16 each, `SignItem`/`HangingSignItem` 12
each, `AxeItem`/`HoeItem`/`ShovelItem`/`GameMasterBlockItem`/`MobBucketItem`
7 each. The `BlockItem` family together is 1,051.

**26.2 has no `SwordItem`, `PickaxeItem`, `DiggerItem` or `TieredItem`.**
`minecraft:diamond_sword` and `minecraft:diamond_pickaxe` are plain `Item`;
mining and attack live in the `minecraft:tool` and `minecraft:weapon`
components. Only `AxeItem`, `HoeItem` and `ShovelItem` survive as classes, and
only for their `useOn` block transforms.

Stack sizes: 1,242 items at 64, 49 at 16, 246 at 1. So the old constant was
wrong for **295 of 1,537**, and not evenly: the 1s are every tool, every
weapon, every piece of armour and every filled bucket.

### What now reads the table

* `crate::itemprops` parses it and answers `max_stack_size`, `max_damage`,
  `is_damageable_item`, `class_of`, `mining_speed`,
  `correct_tool_for_drops` and `use_arm`.
* `crate::player::Hotbar` carries it (`with_item_props`, installed at the one
  production site in `crate::status::listen`), so the pickup path and
  `crate::click::Slots::max_stack_size` both cap at the item's own maximum: a
  chest slot took 24 buckets before and takes 16 now.
* `crate::blockitem` uses the class column three ways: `clear_above`
  (`DoubleHighBlockItem.placeBlock`, 28 items), `places_from_use_on`
  (`PlaceOnWaterBlockItem` refuses outright, 2 items;
  `GameMasterBlockItem` needs `canUseGameMasterBlocks`, 7 items) and
  `place_flags` (`BedItem.placeBlock`'s 26, 16 items).

`SessionTables` loads it and the boot line prints
`item props: 1537 items, 295 of them not stacking to 64, 84 damageable, 38 tools`.

### The item side of a right-click: five `useOn` overrides

`crate::itemuse` holds `ItemStack.useOn`, which runs **after** both block
calls. It had two overrides — `HoeItem` and `FlintAndSteelItem` — and now has
six, over 24 items:

| class | items | what it does | flags | refused |
| --- | --- | --- | --- | --- |
| `HoeItem` | 7 | `TILLABLES`, 5 rows | 11 | the `rooted_dirt` drop |
| `FlintAndSteelItem` | 1 | fire against a sturdy face | 11 | the lighting branch, the RNG pitch |
| `ShovelItem` | 7 | `FLATTENABLES`, 6 rows to `dirt_path` | 11 | the campfire dowse |
| `AxeItem` | 7 | `STRIPPABLES`, 23 rows, axis carried | 11 | scrape, wax-off, the offhand guard |
| `ShearsItem` | 1 | `age = 25` on a growing vine head | 3 | `mineBlock` |
| `FireChargeItem` | 1 | the same fire, different flags | 3 | the lighting branch, the RNG pitch |

**The flags column is not decoration.** Three call sites in the jar write with
three different numbers — `setBlock(..., 11)`, `setBlockAndUpdate`'s 3, and
`BedItem.placeBlock`'s 26 — so they are a per-effect field here rather than a
module constant.

Everything else returns `PASS`, which is `Item.useOn`'s own body, so an
unported item is *correct* rather than unhandled.

### Named gaps, with their sizes

* **`minecraft:use_item` (id 67) is decoded nowhere.** Right-click with
  nothing aimed at a block is unhandled, so `Item.use` has no caller. Its
  dispatch is transcribed (`ItemProps::use_arm`) and its arms are 1,411 Pass /
  75 Equippable / 43 Consumable / 7 KineticWeapon / 1 BlocksAttacks.
* **No durability.** `Item.mineBlock`'s `hurtAndBreak` and every `useOn`'s
  are unported; `Tool.damage_per_block` is parsed and unread.
* **`Block.withPropertiesOf`** has no analogue here, which is what refuses
  `AxeItem`'s scrape and wax-off and would refuse `HoneycombItem`.
* **No `canSurvive`.** `StandingAndWallBlockItem.canPlace` (20 items, plus
  `SignItem`'s 12 and `BannerItem`'s 16 by inheritance) is
  `state.canSurvive(level, pos)` and `HangingSignItem.canPlace` adds
  `WallHangingSignBlock.canPlace`. Placement refusal here comes from
  `state_for_placement` returning `None`, not from a survival model, so none
  of the three is portable yet.
* **`doClick` never asks `overrideStackedOnOther` / `overrideOtherStackedOnMe`**
  — 17 items, all `BundleItem`, and unported because a bundle is its
  `minecraft:bundle_contents` component. Named at the call site in
  `crate::click`.
* **`SmithingTemplateItem` overrides nothing** — 19 items, inert by
  construction rather than unported.
* **A new gameplay scenario cannot be registered from a lane worktree.** The
  only recordings on the box are from 2026-08-08; a `--sweep` files a new
  check at `tier = 0, "not-in-recording"` and rewrites the ledger's counters
  against a stale universe. Recordings are the chain's job.

## `BlockItem.canPlace`, and the four other things the item lane refused

Added 2026-09-05 by the `items-2` lane, which is the second half of the
section above: `docs/cold-start/items.md` closed naming five mechanisms it
could not reach, and this is what each came to.

### The gate that was never asked

`BlockItem.canPlace` is two tests (offsets 5-51):

```text
   5-24   mustSurvive() && !state.canSurvive(level, clickedPos)      -> false
  27-43   !level.isUnobstructed(state, clickedPos, placementContext) -> false
     46   true
```

This server's placement arm asked neither. A torch clicked onto air was
placed, a sign was hung on nothing, and a rail sat on a hopper — and none of
it was visible, because the packet a refusal *does not* send looks exactly
like a client that never clicked.

`data/vanilla/survival.json` cannot answer the first test and the reason is
structural rather than a gap: its subjects are read out of the **datapack**,
every state a `would_survive` placement modifier names, which is 51 blocks and
all of them vegetation. The blocks a `BlockItem` can place are a different set
of 1,115.

### The table, and why a neighbourhood rather than a state

`canSurvive` reads the world around a position, so there is no per-state fact
to extract. What extracts is *the answer over a controlled neighbourhood*, and
`ci/item-survive-golden.sh` builds one world per (direction, ground) pair —
exactly one non-air block at a named offset, air everywhere else:

* **1,115 blocks** — the 1,054 a `BlockItem` names plus 61
  `StandingAndWallBlockItem.wallBlock` halves, which `getBlock()` cannot
  reach (a torch item names `minecraft:torch` and nothing else) and which are
  read by reflection rather than by re-deriving a pairing from names;
* **311 of them override `canSurvive`**, over 50 declaring classes; the other
  804 reach `BlockBehaviour`'s `iconst_1; ireturn`. The declaring class is
  found by reflection, so the census is the jar's and not a hand-list;
* **6,600 states x 79 neighbourhoods x 2 light levels = 1,042,800 answers,
  0 refused**, 1.85 MB, 14 s, two runs byte-identical.

Unlike `SurvivalGolden`'s reader, this one does not throw outside one
position: the world is fully specified, so every cell is a fact about the rule
even where the harness cannot say *which* read produced it.

Two light levels because one would have been silently wrong about a whole
family. `CropBlock` needs raw brightness >= 8 and `MushroomBlock` needs <= 12,
so at 15 no mushroom survives anywhere and at 0 no crop does.

### `crates/rustmc-behavior/src/place_survive.rs`

19 families, and `golden_item_survive` replays every cell: **640,532
compared, 0 disagreements**, with 2,546 states in 34 classes answering
`Unported` and ratcheted by name. It is not merged with `crate::survival`,
which answers the *generator's* question over a world its oracle can see one
block of: there the third value means "this harness cannot see far enough",
here it means "this port has not transcribed the rule", and those are
different facts with opposite handling. `vanilla_would_place` answers **true**
for `Unported`, because refusing what cannot be adjudicated would stop a
scaffold or a chorus flower being placed at all.

Two findings a reading of the classes does not produce:

* **The dispatch key is two classes.** `VegetationBlock.canSurvive` is
  `mayPlaceOn(below)` and nothing else, and eleven classes override
  `mayPlaceOn` while inheriting `canSurvive` unchanged. Keyed on one class a
  port answers all eleven with `#supports_vegetation` and is wrong for
  `lily_pad`, `dead_bush` and `cactus_flower` — 144 cells.
* **`getConnectedDirection` points away from the support**, so a floor
  button's is UP. It is a `lookupswitch` over a `$SwitchMap` whose numbering
  is the compiler's, so which arm is FLOOR cannot be read from the listing at
  all; `LanternBlock`'s spelled-out `hanging ? DOWN : UP` settles it, and
  reading it the other way cost 8,784 cells.

### `block_props.json` grew three things, and one of them was a lie

`unstableBottomCenter` was **0 for all 32,366 states** and had been since it
was written: `BlockPropsGolden` called `Bootstrap.bootStrap()` and never bound
block tags, and an unbound tag is not an error here — `HolderSet` resolves
empty and `state.is(tag)` answers `false`. So a column read by `survival.rs`,
`decorate.rs` and `block_predicate.rs` was a constant, while its doc comment
described the refusal `Block.canSupportCenter` applies before it looks at a
shape. It is 384 states now, behind a two-depth positive control.

Beside it: `rigidFaces`, because `SupportType.RIGID` is a **third** question
after FULL and CENTER and derivable from neither — farmland is `100000` /
`101111` / `100000` across the three — and 14 `placeTags` bits plus one
`placeFluidTags` bit, all resolved by vanilla's own `state.is(tag)` because
every one of them nests. The fluid bit exists because
`#minecraft:supports_lily_pad` is a tag in two registries with different
members, and `LilyPadBlock.mayPlaceOn` reads the fluid one first.

### The other four

* **`BonemealableBlock`** — `ci/bonemeal-golden.sh` is the census nothing in
  this repo carried: 63 blocks over 45 classes, 721 states, 303 valid
  targets, 0 refused. Five are `CropBlock` and are ported; the other 58
  answer `Unported`, which is kept distinct from `Pass` because a gap that
  reports as a pass is a gap nobody counts. `TorchflowerCropBlock` **draws no
  RNG at all** and `BeetrootBlock` is `super / 3` on the same single draw, so
  a beetroot fails one bone meal in four and still consumes it.
* **Durability** — `crate::durability` is `Item.mineBlock` ->
  `hurtAndBreak` -> `processDurabilityChange` -> `applyDamage`, and it has no
  caller. `ServerPlayerGameMode.destroyBlock` returns at
  `player.preventsBlockDrops()` (offsets 190-200) **above** the `mineBlock`
  call at 241, so in creative the call is never reached; and
  `Item.mineBlock` gates on `getDestroySpeed`, which is in no table this
  tree has.
* **The component patch** — `crate::patch`. `0, 0` means "exactly this item's
  prototype components". Since `item-components` a stack carries its patch:
  loot-rolled stacks (enchantments, stored enchantments, potion, stew
  effects, instrument, ominous amplifier, damage) and creative stacks (kept
  raw, any component) go out with it in every menu packet, it decides what
  merges (`isSameItemSameComponents`) and a patched `max_stack_size` is the
  stack's maximum. Placing a block applies `custom_name` (containers, banner,
  beacon, enchanting table, skull), `container`, `pot_decorations` and
  `block_state`. There is **no length prefix** around a clientbound payload,
  so a component whose codec a port has not read must not be written at all
  — which is why a nested `container` stack must carry the empty patch.
* **`use_item` (67)** — decoded and dispatched. It carries no
  `BlockHitResult`: four fields, hand, sequence, two floats. Its
  `ackBlockChangesUpTo` is unconditional and above the empty-hand return, so
  a right-click into the air now corrects a client that mispredicted a block.
