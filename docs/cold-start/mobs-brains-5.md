# Cold start — branch `mobs-brains-5`

### COLD START — branch `mobs-brains-5` off `origin/mobs-brains-4` (`2c726c777`), 2026-09-05

**The brain surface is closed. `tests/brain_tables.rs::PENDING` went from
1 to 0** — every behaviour class all twenty brain packages name (131 at the
ratcheted floor) is transcribed in `crates/rustmc-entity`, and
`PENDING_SENSORS` has been 0 since the fifth lane. Worktree
`~/grit/work-brains3`, remote dir `~/rustmc-brains5` (inner-loop compile dir
`~/rustmc-brains5-dev`). Not merged — the orchestrator gates.

The one class: **`TransportItemsBetweenContainers`**, 39,537 bytes of class
file, 62 methods (five bridges and eleven lambdas among them), four inner
types and eleven constants, in
`crates/rustmc-entity/src/brain/behaviors/transport_items_between_containers.rs`
(2,036 lines including the module doc's full offset table and 32 tests).
`cargo test -p rustmc-entity`: **484 lib + 26 integration tests green** (452
+ 26 before), `cargo clippy -p rustmc-entity --all-targets -- -D warnings`
clean, `cargo fmt` applied.

**The container host surface, seventeen `MobHost` methods with base
defaults, field-backed on `testing::TestMob`:**

    scan      chunk_block_entity_positions, block_entity_kind,
              block_entity_identity, transport_container_exists
    guards    container_is_locked, chest_is_blocked_at, chest_connected_pos
    geometry  collision_shape_bounds, clip_block_hit
    container container_is_empty, container_items, container_remove_item,
              container_set_item, container_set_changed
    mob       main_hand_item, set_guaranteed_drop_main_hand,
              has_running_panic_goal

plus `phys.rs`: `Aabb::of_block` / `of_size` / `moved_by_block` /
`contains`, and `block_to_section_coord`, `chunk_pos_containing`,
`chunk_pos_range_closed`. `TestMob::place_chest` and
`place_container_with_identity` set a chest up in one call.

**Eleven findings, all in `docs/FINDINGS.md`.** The four worth reading
before touching this class again:

1. **`path == null && !within` is a VALID travelling path** (offsets
   58..73). The obvious reading — no path means unreachable — is the
   opposite of the bytecode, and inverting it would blacklist every target
   on the tick it was chosen.
2. **`tick` is three independent `if`s over the state**, so one tick can
   carry a mob queuing -> travelling -> interacting -> its first
   `ticksSinceReachingTarget++`. `timedOut` is `iconst_0`, so the 60/60
   durations never fire and the `tryStart` `nextInt(1)` draw is spent for
   nothing.
3. **`addItemsToContainer` debits the hand by `room`, not by `moved`**
   (offsets 92..124), so the hand's count goes negative when it holds less
   than the slot's room. Items are conserved because the slot grew by the
   `min`, and `ItemStack.isEmpty()` is `count <= 0` — but `putDownItem` has
   already written that stack to the main hand before testing it.
4. **The chunk scan's tie-break is fastutil hash order.**
   `ChunkAccess.blockEntities` is an `Object2ObjectOpenHashMap`, and
   `getTransportTarget` keeps a candidate on a strict `<`, so two
   equidistant containers in one chunk resolve by that map's internal
   order. `MobHost::chunk_block_entity_positions` carries that requirement
   in its doc; a host that wants the tie to match vanilla has to reproduce
   the map, not just the set of positions.

**Golden-checked or deferred:** deferred, and for the reason every earlier
lane in this stack gave — the class needs a `PathfinderMob` in a
`ServerLevel` with loaded chunks, block entities and containers, so a jar
golden is `mob-in-world`'s to add. Two tests carry a **second independent
transcription of the same algorithm** as their twin and say so in the test
rather than claiming a golden: `ChunkPos$2.tryAdvance`'s x-inner/z-outer
walk, and `Direction.VALUES`' six face-centre offsets.

**Positive control on the ledger, checked rather than assumed:** deleting
the new `TRANSCRIBED` row makes
`every_brain_package_behaviour_is_transcribed_or_named_pending` panic at
`brain_tables.rs:405`, so an empty `PENDING` is not a way of satisfying it.

**Nulls, so nobody re-runs them:** `PASSENGER_MOB_TARGET_SEARCH_DISTANCE`,
`CLOSE_ENOUGH_TO_START_INTERACTING_WITH_TARGET_DISTANCE` and its
`_PATH_END_` twin are declared and then written as bare literals
(`iconst_1`, `ldc2_w 0.5d`, `dconst_1`) at every use; `lambda$onReachedTarget$0`
and `$1` are byte-identical and both ignore their two parameters for the
captured `mob`; the `Map<ContainerInteractionState, OnTargetReachedInteraction>`
is only ever `Map.get`, so its iteration order is never read; `start`
resets nothing, only the below-surface flag. `Aabb::inflate` was writing
the struct directly where the jar goes through the min/max constructor —
fixed here, and no caller in the tree passes a deflation large enough for
the two to differ, so nothing moved.

**Needs hands: nothing in this branch.** The next item is what the fourth
lane already named: the `*Ai` **provider lambdas** themselves (`AxolotlAi`,
`FrogAi`, `WardenAi` with its `AngerManagement`, `SnifferAi`, `BreezeAi`,
`GoatAi`, `ArmadilloAi`, `CamelAi`) as data in the tables the same way the
villager's are — every behaviour those providers construct now exists, so
that lane is assembly rather than transcription. The result that closes it
is a `brain_tables` assertion that each provider's `List.of` reconstructs
from this crate's factories, priority for priority.
