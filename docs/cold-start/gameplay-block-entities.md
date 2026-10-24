# Cold starts — branch `gameplay-block-entities`

Append-only. Newest entry last.

### COLD START — branch `gameplay-block-entities`, 2026-08-12

**The item was §21.5's row 4: "48 block entity types, only `sign` has ever been
observed". The first measurement refuted the row's framing, and everything
below follows from that.**

#### What the number actually is

`ci/vanilla-reach-census.py` reads the block-entity registry through one
channel: a `block_entity_data` observation naming a type. There is no second
channel — a block entity's other route to a client is inside
`LEVEL_CHUNK_WITH_LIGHT`, and `trace::is_housekeeping` drops that packet before
any decoder sees it.

`block_entity_data` has exactly one emit site, `ChunkHolder.broadcastBlockEntity`,
and it returns at `getUpdatePacket(); ifnull 31` without sending anything.
`BlockEntity.getUpdatePacket()` is `aconst_null; areturn`. **29 of the 49
registered types inherit it unchanged.**

So the row's denominator is **20, not 49**, and `1/49` was `1/20` of what is
reachable. `ci/block-entity-probe.sh` is the measurement rather than the
reading: it boots its own vanilla, places one of each candidate, prints the
payload under `RUSTMC_TRACE_RAW`, and places **chest, barrel, hopper and
lectern under the same rig in the same run** so that the silence of the 29 is
evidence about the type and not about the harness. All four placed their block.
None of them said anything about it.

#### What was ported

Eleven types — the twenty that can be seen, less the nine that cannot be
reached — into `crates/rustmc-server/src/blockentity.rs`:

`hanging_sign` (8), `creaking_heart` (10), `beacon` (15), `skull` (16),
`banner` (20), `conduit` (25), `campfire` (32), `shelf` (39),
`brushable_block` (40), `decorated_pot` (41), `copper_golem_statue` (47).

The port is small because the lifecycle was already general: `for_state`
decides what a block makes, `on_block_changed` creates and removes, and
`broadcast_block_entity_if_needed` already declined to send for a null
`getUpdatePacket()`. What was missing was the types and their update tags.

Thirteen checks at `760`-`772`.

#### Nulls — do not re-run these

* **`structure_block`, `jigsaw`, `test_block`, `test_instance_block` cannot be
  placed by this harness.** `GameMasterBlockItem.getPlacementState` returns
  **null** unless `Player.canUseGameMasterBlocks()`, and in 26.2 that is
  `abilities.instabuild && permissions().hasPermission(
  Permissions.COMMANDS_GAMEMASTER)` — **a `PermissionSet`, not an op level**,
  which is what it was in earlier versions and is the thing to get right
  before spending a turn on it. The corpus player is creative, so it passes
  the first half and fails the second. Measured, not read:
  `ci/block-entity-probe.sh` records `grass_block,air` for all four — the
  click resynced the ground and put nothing down. Closing them is a
  **substrate** change, not a scenario. It is the cheapest of the remaining
  blockers and it buys four types.
* **`mob_spawner` and `vault` can never be `match` for anybody.** A spawner
  re-broadcasts a second `block_entity_data` about a second later whose
  `Delay` is `minSpawnDelay + nextInt(max - min)` — 20 then 592 in the probe
  run. A vault re-broadcasts three times with `display_item` rolled from its
  loot table (`golden_apple`, `wind_charge`, `honey_bottle` in one run) and a
  `connected_players` list holding a per-run uuid. Do not write a check for
  either. Closing them needs a projection that can drop a *named NBT key*
  inside `rest`, which does not exist and would be a real instrument item.
* **`end_gateway` has no `BlockItem` at all** and the corpus world is a
  superflat overworld. Not a scenario.
* **The 29 with a null `getUpdatePacket()` are a RECORDER item, not a corpus
  item.** `ClientboundLevelChunkPacketData$BlockEntityInfo` carries
  `{packedXZ, y, type, tag}` for every block entity in a chunk, and
  `is_housekeeping` drops the packet that carries it. **Decoding just that one
  list would take the ceiling from 20 to 49** without un-dropping the chunk
  payload. That is one decoder arm against 29 types; no number of scenarios
  can do it. This is the single highest-leverage next move on this item and it
  is not a block-entity item at all.

#### The trap this surface is full of, and what was done about it

Ten of the eleven new types can **only** be observed at the instant the block
entity is created. All ten would pass on a server that emitted the right
constant and stored nothing — this project's signature defect, and the reason
the item was worth a branch. Two things answer it:

* `772-typing-on-a-hanging-sign-you-placed-sets-its-text` edits a block entity
  *after* it exists and re-reads it. No constant survives it.
* `761-a-decorated-pot-sends-block-entity-data-and-a-lectern-does-not` places
  a silent type beside a speaking one, so a server that broadcast every block
  entity fails. Without it, all ten placement checks would pass such a server
  while it was wrong about more types than it was right about.

And every payload is a **golden vector against bytes vanilla put on a socket**,
not a shape: the projection digests every byte after the type id, so
`every_new_update_tag_is_byte_identical_to_what_vanilla_sent` compares against
the probe's own `RUSTMC_TRACE_RAW` output. It catches a sorted NBT writer
(`Items` would precede `align_items_to_bottom` on a shelf), a suppressed empty
list (`{}` where vanilla sends `{Items: []}` on a campfire) and a missing
`components` map on the banner — the one new type whose `getUpdateTag` goes
through `saveWithoutMetadata` rather than `saveCustomOnly`.

#### Two smaller findings worth not rediscovering

* **`getContainerSize()` and the length of a block entity's item list are
  different questions.** A campfire holds four stacks and is *not* a
  `Container` — `implements Clearable`, no `getContainerSize` anywhere in its
  hierarchy — while a shelf holds three and is one (`ListBackedContainer`).
  `Kind::container_size` and `Kind::item_slots` are now separate for that
  reason; a single number cannot say both, and the campfire is where a single
  number goes wrong.
* **A hanging sign is byte-identical to a sign except for the type id.**
  `HangingSignBlockEntity extends SignBlockEntity` and inherits `getUpdateTag`
  whole; both come back `rest d010602cd96118eb` freshly placed. So a server
  that resolved a hanging sign to `minecraft:sign` puts a packet on the wire
  whose every other byte is right, which decodes cleanly on a client and
  renders the wrong block entity. `771` is the check that separates them and
  it is the sharpest one in the set.

#### The reading, and the two caveats that travel with it

```text
  block_entity_type      1 / 49    2.04%   ->   12 / 49   24.49%
                         1 / 20                 12 / 20   of the reachable set
  corpus                92 scenarios           105 scenarios
  declared              72 / 92                 86 / 105     T2-GAMEPLAY GREEN
  strict                68 / 92                 82 / 105
```

Threadripper, vanilla 26.2, superflat creative, seed 20260803, 2026-08-12.
All thirteen new checks `match` declared and strict; 0 vacuous, 0 mute,
0 inert, 0 error.

**The two corpus scores are different denominators and are not a rate to
compare.** They do reconcile: 13 added, all 13 `match`, so `86 - 13 = 73` and
`82 - 13 = 69` of the old 92, against §21's 72 and 68. The `+1` on each is the
run's `BETTER` on `a-fence-gate-opened-from-behind-turns-to-face-you`, which
is **not this branch's work and was deliberately not locked in** — `main`'s
cold start says one gate had seen it match, and this is the second sighting.
That is for whoever ratchets it, with this run as evidence.

* **The 24.49% is N = 1.** The `1/49` was six pooled recordings; the `12/49` is
  one recording of the 105-check corpus, because no ensemble of the new corpus
  exists. The census's own `per-run 12..12` is therefore a single run stating
  no stability. What is measured is narrower: the eleven payloads were
  byte-identical across three cold vanilla boots that day.
* **The floor is now stale in its denominator and the gate says so** —
  `4/92` measured over 92 scenarios, quoted beside a score over 105. Not
  re-stamped, deliberately: repairing it means six cold vanilla boots of the
  new corpus, not a `--update`.

#### Tools left behind

* `ci/block-entity-probe.sh` — re-runnable, no corpus lock, prints the exact
  NBT vanilla sends for any block you add to its list.
* `ci/block-entity-subset.sh` — records a *glob* of checks against both
  servers and compares, in about a minute instead of a corpus run. **Its
  numbers are not corpus scores and its header says so**; §20 is why.
