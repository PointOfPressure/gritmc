### COLD START — branch `advancements`, 2026-09-05

**The area.** Vanilla 26.2's 1,688 advancement documents as data, the criterion
triggers this server can already fire, per-player progress, and the
`update_advancements` path. Census before this branch: 1,688 in the jar, 0 here.

**What landed.**

* `tools/goldengen/vanilla/AdvancementGolden.java` + `ci/advancements-golden.sh`
  → `data/vanilla/advancements.json`, registered in
  `ci/gen-gameplay-tables.sh` (28 → 29 derived tables) with its literal in
  `crates/rustmc-server/src/status.rs`. 1,688 documents, 6 roots, 126 with a
  display, 1,561 granting recipes, 23 granting experience, 0 loot, 0 function,
  54 distinct triggers.
* `crates/rustmc-server/src/advancements.rs` — the document table, the
  requirements AND-of-ORs, `AdvancementProgress`, the visibility evaluator, the
  tree.
* `advancements/jhash.rs` — `java.util.HashMap`'s iteration order, which is
  what decides the sequence of ids in the packet's three containers. Vectors
  are vanilla's own, emitted per tree under two insertion orders by the same
  Golden.
* `advancements/player.rs` — `PlayerAdvancements`: `award`, `revoke`,
  `markForVisibilityUpdate`, `updateTreeVisibility`, `flushDirty`, and the
  packet body.
* `advancements/triggers.rs` — `minecraft:inventory_changed`, fired from
  `crate::world_send`'s existing creative-slot event.
* `ci/checks/gameplay/560-a-crafting-table-awards-story-root.toml`.
* `tools/oracle-runner/src/trace.rs` — `update_advancements` out of the
  housekeeping list, with a decoder arm that records `reset`, the `added` count
  and the body length and digests nothing.

**The finding that shaped everything: this packet is not byte-comparable, and
its `added` payload is not even GENERATABLE.** `AdvancementNode.children` is an
identity-hashed `ReferenceOpenHashSet` and `TreeNodePosition.run` walks it to
write `DisplayInfo.x/y` onto the wire. Permuting only the parse order moved 122
of 126 `y` values and 120 of 126 `holder_wire` strings; two *identical* JVM
runs then disagreed on the fourth attempt. The dumper pins
`setLocation(0, 0)`. `CriterionProgress.grant()` writes `Instant.now()` onto
the wire independently of that. `docs/PARITY-ROADMAP.md` §109.

**Nulls — do not re-run these.**

* **Do not try to make `holder_wire` reproduce vanilla's `x`/`y`.** It is not a
  function of the datapack and vanilla does not reproduce it either. Nine JVM
  runs of the pinned version, one md5, only because the layout is pinned to
  zero.
* **Do not digest this packet's body in a corpus check.** Two independent
  reasons, either sufficient; a digest check would be red while the server is
  right. The length is the comparable quantity and it is exact.
* **Do not port `minecraft:recipe_unlocked`.** Its 1,561 criteria all sit on
  display-less advancements, so completing one changes nothing on any wire. A
  trigger whose only effect is invisible is a call site no check can turn red.
* `Advancement.CODEC.encodeStart` is not available here: 55 biome holders
  decoded through `VanillaRegistries.createLookup()` come back `not valid in
  current registry set` from `HolderSetCodec`'s owner check. The Golden emits
  the datapack's own JSON instead, and vanilla's parsed `requirements` beside
  it for the one derived value that would have been lost.
* The two banner-pattern icons cannot be encoded without a `ResourceManager`
  over the datapack. They are counted, not faked.

**The single next item, and the result that closes it.**
`minecraft:placed_block` — 15 criteria, all of them
`ItemUsedOnLocationTrigger` evaluating a `LootContext` through
`block_state_property` / `location_check` (with x/y/z offsets) / `any_of` /
`all_of`. `crates/rustmc-loot` already holds most of that interpreter. It
closes when a scenario placing a waterlogged `minecraft:dried_ghast` — the
simplest of the fifteen, one `block_state_property` term — produces an
`update_advancements` whose `added` count and `len` match vanilla's, and the
sweep puts it at tier 2.
