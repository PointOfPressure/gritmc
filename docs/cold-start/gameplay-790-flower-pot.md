# Cold start — branch `gameplay-790-flower-pot`

Append-only. A new `### COLD START` entry per turn on this branch; nothing
above is edited.

---

### COLD START — branch `gameplay-790-flower-pot`, off `golden-useitemon-axis` (`d7f6b89e`), 2026-08-12

**The item: convert `790-a-flower-pot-empties-when-right-clicked`, and say how
much of the 39-block class the port actually covers.** Worktree
`~/grit/work-pot`, remote dir `~/rustmc-pot`. Not merged — the orchestrator
gates.

**This branch is stacked, not based on `main`.** `golden-useitemon-axis` is on
`golden-probe-audit` and both were in a gate chain landing on `main` when this
started. Nothing here duplicates either: the table is theirs, the consumer is
this branch's. `gameplay-item-use-group`'s `crates/rustmc-server/src/itemuse.rs`
is **not** in this base and is not depended on — checked, `origin/main..origin/gameplay-item-use-group`
is two commits and neither is an ancestor here. The dispatch this branch adds is
inside `handle_use_item_on` and is `BlockState.useItemOn` (offset 145), not
`ItemStack.useOn` (offset 275), so the two are different layers of the same
method and do not collide.

### The brief held, and the one correction is about scope rather than fact

`gameplay-block-interactions` sized `790` as "a new golden axis plus two ports
plus `Player.addItem`". The golden axis arrived on the base branch, so what was
left was two ports and `addItem` — and **one of the two ports turned out to be
already in the tree**. `golden-useitemon-axis`'s `UseGolden.bindItemComponents`
fix means `use_without_item.json` now carries all 38 `potted_*` rows
(`writes: [{state: minecraft:flower_pot, flags: 3}]`, verified for all 38 in
`useitemon::tests::every_pairing_agrees_with_the_use_without_item_table`), where
they were `NullPointerException: Components not bound yet` before. So the
emptying **block write** needed no code at all; what needed code was the
filling, the item handed back, and where the packet for it goes on the wire.

### What was built — `crate::useitemon`

`data/vanilla/use_item_on.json`, restricted at load time to
`PORTED_CLASSES`, which is `FlowerPotBlock` and nothing else. The table
describes **163 blocks**; applying all of them would turn 162 unmeasured blocks
on in one commit, which is the opposite of the standing rule that new behaviour
lands opt-in until it is measured.

The loader **refuses** rather than skipping on: a refused state or cell inside a
kept row, a second write, a write away from `[0,0,0]`, a scheduled tick, a
sound, a `probeAxes` entry other than `game_master`, a non-empty `probeUnknown`,
`random`/`faceSensitive`/`handSensitive`, a `stackAfter` other than `empty`, and
a pairing that is not a bijection. Each of those is a way a regenerated table
could quietly half-serve a block.

Call order, from `ServerPlayerGameMode.useItemOn`:

```text
  145  r = state.useItemOn(stack, ...)   -> consumesAction? return
  196  r instanceof TryEmptyHandInteraction && MAIN_HAND -> useWithoutItem
  223  the placement path
```

`crate::useblock`'s step is unchanged and now lives in
`world_send::use_without_item_step`; the body was moved, not rewritten.

### How many of the 39 the port covers, and how that is known

**All 39, and the evidence is the loader plus four unit tests, not the corpus.**

* `UseItemOnTable::blocks()` is **39** and `len()` is **39** — one state each —
  asserted against the real generated table. Those 39 are `minecraft:flower_pot`
  and the 38 `minecraft:potted_*`; the loader keeps a row **by class**, so
  coverage is not a list anyone maintains.
* `pairings()` is **38**: every potted block but the empty pot, which hands
  nothing back.
* `all_thirty_eight_pots_fill_and_empty` exercises every one of the 38 in both
  directions off the real table — `flower_pot` + item writes the matching
  `potted_*` with flags 3 and consumes, and `given_back` returns that item.
* `every_pairing_agrees_with_the_use_without_item_table` pins the other half
  against a **different generated file**, so a port that got the hand-back right
  and the block write wrong could not pass both.

**What the corpus measures is one of the 39.** `790` clicks
`minecraft:flower_pot` with `minecraft:dandelion` and then `potted_dandelion`
with a stick; nothing in `ci/checks/gameplay/` names any other pot — checked, a
grep of the whole directory for `flower_pot|potted_|dandelion|poppy|sapling|
cactus|bamboo|fern|azalea|mushroom` matches `790` and one *prose* mention in
`335`'s `about` line. So the honest statement is: **the implementation is one
table lookup shared by 39 blocks, all 39 are fully measured in the table with no
refusals, 39 are covered by unit tests over that table, and 1 is measured
end-to-end against real vanilla.** A claim of 39 from the corpus alone would be
the misattribution this project keeps refusing; a claim of 1 would understate a
port that cannot treat the other 38 differently.

### The item handed back, and which of the two candidate maps was used

`FlowerPotBlock.useWithoutItem` offset 11 is `new ItemStack(this.potted)`, and
`potted` is a private field with no accessor. Two routes recover it and
`gameplay-block-interactions` asked that whichever is used be named:

* **used — the code map.** `POTTED_BY_CONTENT` is the same field's map read in
  the *potting* direction, and `minecraft:flower_pot`'s row in
  `use_item_on.json` has one `except` group per pottable item naming the
  `potted_*` it writes. Inverting those 38 groups is the map. The loader
  refuses if the inversion is not injective.
* **corroboration — the loot tables.** `data/vanilla/loot_blocks.json` gives
  every `potted_*` exactly two item pools, `minecraft:flower_pot` then the
  content. Checked against the inversion on 2026-08-12 in `~/rustmc-pot`:
  **38 of 38, zero exceptions.** It is *not* a unit test, deliberately — a test
  would put a `"data/vanilla/loot_blocks.json"` literal in `crates/` and so make
  a **second** table required of every tree, for a corroboration already taken.

### Three things that are the port rather than decoration

1. **The creative branch is the consumer's, not the table's.**
   `ItemStack.consume(n, entity)` is
   `if (entity == null || !entity.hasInfiniteMaterials()) shrink(n)`, which is
   why `use_item_on.json` marks `minecraft:flower_pot` probe-dependent on
   `game_master`. The table says *that* a consume happens;
   `world_send::item_use_consumed` decides whether it shrinks, from
   `Session::creative`. The corpus actor is creative, so the pot fills and the
   flower stays in hand — and the block write is the same either way, which is
   what makes that axis answerable at all.
2. **`Inventory.add`, not `Inventory.placeItemBackInInventory`.** The two walk
   the same slots in the same order; they differ in that the second sends a
   `set_player_inventory` per touched slot and the first sends nothing.
   Vanilla's recording of `790` carries **no** `set_player_inventory` beside its
   `container_set_slot`, which is how they were told apart.
3. **The slot packet is deferred a tick, and that is the whole of it.**
   `Connection::pending_menu` + `Players::flush_menu_sweeps`, called in the tick
   between the block broadcast and the acknowledgements — which is where
   `ServerPlayer.doTick`'s `containerMenu.broadcastChanges()` sits in
   `ServerLevel.tick`. Vanilla's actor stream for `790` is
   `block_update, block_update, block_update, container_set_slot,
   block_changed_ack`; a handler that sent the slot itself would put it second.
   `world_send::tests::emptying_a_pot_hands_the_flower_back_after_the_block_update`
   is the fast witness — the corpus is a fifty-minute instrument and this
   ordering had no other signal.

### `use_item_on.json` is now a required table, and it regenerates identically

**This is the cost this branch imposes on everyone else and it is stated
plainly.** `ci/gen-gameplay-tables.sh` derives its list from the
`"data/vanilla/…"` literals under `crates`, `tools` and `plugins`.
`crate::useitemon` is the first thing in the tree to name this one, so the
required set went **19 tables to 20** and every tree from this commit pays
`ci/use-item-on-golden.sh`'s ~95 s. The dormant arm's comment in the generator
said "this arm does not fire" and has been corrected in the same commit.

**Byte-identical on regeneration, measured rather than assumed** — sayer-desktop,
`~/rustmc-pot`, 2026-08-12. The table `gen-gameplay-tables.sh` produced and a
second, independent run of `ci/use-item-on-golden.sh` over the same tree are
`cmp`-clean: **1,748,342 bytes, md5 `33bf3128688a105248fab730bad4e2bf` both
times.** The generator additionally dumps twice internally and refuses if its
own two runs disagree, so this is two independent statements of the same
property. A table that were not reproducible would poison every measurement
taken after this commit, which from here is all of them.

### What was killed as a null — do not re-run these

* **"The 38 potted blocks need a hand port for their block write."** They do
  not, and have not since `golden-useitemon-axis`. `use_without_item.json`
  carries all 38 with `flags: 3`; only the *item* is missing from it. Do not add
  a `hand_ported` arm for `FlowerPotBlock`.
* **"A loot-table route is needed to recover `POTTED_BY_CONTENT`."** It is not.
  The potting direction is in `use_item_on.json` and inverts. The loot route was
  run once as corroboration (38/38) and deliberately left out of the crate so it
  does not make a second table required.
* **A hand-written list of the 38 pottable items.** Not needed and not present.
  The loader keys on the row's `class`, so the count comes out of the data.
* **Consuming the flower on a creative pot.** `hasInfiniteMaterials()` is true
  for the corpus actor, so the stack does not shrink. A test that asserted an
  empty hand after potting would be asserting the survival branch against a
  creative fixture.

### Stated blind spots

* **38 of the 39 are covered by unit test only.** Nothing in the corpus clicks a
  cactus in a pot.
* **`Stats.POT_FLOWER` and `Level.gameEvent`** are read out of the bytecode in
  the module header and not run: no statistics and no sculk listener anywhere in
  this server.
* **A full inventory drops the flower with a thrown item's velocity**, not a
  dropped one's, and carries no `set_entity_data` — the same gap
  `handle_container_click` already names. Unreachable from the corpus, whose
  `hold` verb sends `count = 1`.
* **The other 162 blocks in `use_item_on.json` are still unported**, including
  the 17 `CandleBlock` rows that `ci/checks/use-item-on-probe-dependent.toml`
  carries as a real debt. Widening `PORTED_CLASSES` is a corpus run each time,
  not a one-line change.

### Provenance

`ci/t0-remote.sh` on the committed tree, `~/rustmc-pot`, sayer-desktop
2026-08-12: **T0 GREEN**, `grep -c "skipping: no vanilla"` = **0**, 787
lib+bins tests, zero `test result: FAILED`. `cargo clippy --lib --tests
-- -D warnings` clean (it caught one `iter_over_hash_type` on the pairing map,
fixed in its own commit).

### COLD START — branch `gameplay-790-flower-pot`, 2026-08-16 — rebased onto `911c2e04`, and where the `mayBuild` gate goes

`main` moved a long way under this branch. Seven of its ten commits had already
landed under different SHAs — the probe audit, the `openMenu` correction,
`probe-reads`, the goldengen item bindings, `use_item_on.json` itself and its
T0 provenance. `git rebase --onto origin/main d7f6b89e` replays the three that
are actually this branch's: the port, the pairing-map ordering, and this
document.

**The one real composition, and it is in `handle_use_item_on`.** Both sides
restructured the same dispatch function and neither had seen the other:

* this branch split the empty-hand path out into `use_without_item_step` and
  put a `crate::useitemon::use_item_on` step in front of it;
* `main` added the `mayBuild` / `canUseGameMasterBlocks` gate and made
  `hand_ported` a second arm beside the table, all inline in the handler.

**The order is not in doubt and neither side had to guess it — the pinned jar
settles it, and the two sides' own comments already agreed.** From
`javap -p -c -constants` on `server-26.2.jar`:

```text
  ServerPlayerGameMode.useItemOn
    126  if (secondary) goto 223            the sneaking skip
    145  BlockState.useItemOn(...)          the item step
    176  instanceof TryEmptyHandInteraction
    196  BlockState.useWithoutItem(...)     the empty-hand step
    223  the placement path

  ComparatorBlock.useWithoutItem
      0  Player.getAbilities
      5  Abilities.mayBuild
      8  ifne 15
     11  InteractionResult.PASS
```

The gate is offset **0 of the block's `useWithoutItem`**, not of
`ServerPlayerGameMode.useItemOn`. So it is reached only *after* 145 has been
asked and has handed back a `TryEmptyHandInteraction` at 176. `main`'s comment
"answers first and wins over everything below" is true **within
`useWithoutItem`** and was written when nothing ran in front of it. Composed:
the gate moves into `use_without_item_step`, at its head, ahead of
`hand_ported` and the table, with `flag_override` intact. Hoisting it into the
caller would gate a call vanilla makes unconditionally.

**DOES THE COMPOSITION CHANGE WHAT `790` MEASURES? No, and the reason is
sharper than the corpus run that confirms it.** `use_gate` returns `Some` on
exactly two conditions: `!may_build` on one of the five `MAY_BUILD_GATED`
classes, or `game_master` on `minecraft:light`. Both flags are **hardcoded** —
`may_build = true`, `game_master = false` — because this server has no
per-player ability model, so `use_gate` returns `None` on every call in the
tree today. `main`'s own doc comment says so. And `minecraft:potted_dandelion`
is not one of the six blocks either flag covers, so the gate would decline for
a second, independent reason. `790`'s measured step therefore reaches
`hand_ported` (`None` for pots) and then the table, exactly as it did before
the gate existed.

**Say the uncomfortable half too: that makes the gate's POSITION currently
unobservable.** No check in the corpus can distinguish the composed order from
the wrong one while both flags are constants — `790` included. The order is
placed from the bytecode because that is the only instrument that can see it,
and it is written down here so that whoever makes `may_build` a real
per-player bit knows there is an ordering decision under them that no green
number is currently defending. That is the honest statement, not a hedge:
`ci/checks/use-item-on-probe-dependent.toml` already carries `game_master` as
a real probe axis on `minecraft:flower_pot`, so the axis is known to matter to
the *stack* even where it does not move the *block write*.

**Verified on the rebased tree**, Threadripper `~/rustmc-pot2`, with
`data/vanilla` copied and `ci/gen-gameplay-tables.sh` re-run under this tree's
own stamps (it reports **20 tables current**, which is this branch's own
19 -> 20 claim reproduced): `cargo fmt --check` clean;
`cargo clippy -p rustmc-server --all-targets -- -D warnings` clean;
`cargo test -p rustmc-server --lib --bins` **601 passed, 0 failed**, including
all eight `useitemon::tests` and `all_thirty_eight_pots_fill_and_empty`.
`ci/check-doc-claims.py` 71 claims, 0 collisions, 0 drift.
`ci/check-figures.py` GREEN.

**KILLED AS A NULL — do not re-run.** Running the tests before provisioning
`data/vanilla` and `ci/gen-gameplay-tables.sh` gives **15 failures**, every one
of them a `useitemon` or `useblock` test reading a table that is not there. It
reads exactly like a broken composition and is not one. Provision first; the
AGENT-BRIEF's HARD PRECONDITION block is the recipe.
