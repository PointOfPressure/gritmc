# Cold start — branch `mining`

---

### COLD START — branch `mining` off `origin/main` 3953 (`606551b54`), 2026-09-30 .. 2026-10-01

Worktree `~/grit/work-mining` (detached), remote dirs `~/rustmc-mining`
(recordings) and `~/rustmc-mining-t0` (survival passes, T0). Recordings and
logs in `~/mining-rec/`. The branch was rebased onto main by the chain once
mid-run; fetch before every push.

#### What the area is now

* **`crate::mining`** — `ServerPlayerGameMode`'s dig state machine
  (`handleBlockBreakAction` START/STOP/ABORT, `tick()`,
  `incrementDestroyProgress`), `BlockBehaviour.getDestroyProgress` and
  `Player.getDestroySpeed` in `f32` in the listing's order, and the
  interaction-range test. Pure: the session hands it a `World`.
* **`crate::dig`** — the session glue: digger facts (tool speed, correct
  tool, Efficiency's `L*L + 1`, eye-in-water, on-ground), the three
  `BlockState.attack` overrides (note block plays, redstone ore lights; the
  dragon egg's teleport is NOT ported), `destroyBlock` (canDestroyBlock,
  game-master refusal, plugin veto, 2001 to everyone but the digger,
  removeBlock, then durability and the drop), console `give`/`clear`
  delivery.
* **`crate::givecmd`** — `give <player> <item>[damage=N,enchantments={...}]
  [count]` and `clear <player>` on the console. The fake item and pickup
  sound of `GiveCommand` are not sent (setup-only).
* **`ci/mining-golden.sh`** -> `data/vanilla/block_mining.json`: destroy
  time, requires-correct-tool, game-master, and the class declaring
  `playerWillDestroy`/`playerDestroy`/`spawnAfterBreak`, per block, from the
  live registry. 1196 blocks, 15 unbreakable, 181 instant, 438 need a tool.
* Drops roll through `rustmc-loot` (`Loot::block_drops_enchanted`), spawn
  an `ItemEntity` with `DATA_ITEM` (`Synched::Item`, accessor 8, serializer
  7), sent in the pairing bundle and once more dirty on the first tracker
  tick — byte-identical to vanilla (003 matches).
* `ItemStack` carries `minecraft:damage` and `minecraft:enchantments`;
  `crate::patch` writes the enchantments component.
* **Placement add-on:** the ten look-facing blocks (pistons, dispensers,
  droppers, observer, barrel, crafter, command blocks) face by
  `getNearestLookingDirection()` (`crate::placement::look_facing`), not by
  their default.
* A survival block placement consumes the item; `Hotbar::add` keeps the
  arriving stack's components.

#### Measured

Survival, three passes on vanilla (`~/mining-rec/s3-1001-0206`, tip
`06b113347`): run 2 **15/16 declared match** — every mining check; the one
`absent` is `001` (combat, not this area). Run 1 13/16: 047/050 met a vanilla
item PICKUP (slot 37) that this server cannot produce; those rigs (and 046,
049) now dig three blocks out. Creative subset: `010`, `990` (sword), `991`
(reach) match. `992` (piston looking west) not yet recorded at its fixed
position.

#### Scenarios

Survival: 002 (reworked: early STOP, observer stages 0..10), 003 (reworked:
on-time STOP, the drop's DATA_ITEM), 004 (about corrected), 040, 041, 042,
044, 045, 046, 047, 048, 049, 050, 051, 052. Creative: 990, 991, 992.

#### Killed, so nobody re-runs them

* **The harness cannot make a survival digger airborne.** A `walk` with
  `on_ground = false` left vanilla's digger on the ground (overlay at the
  ground rate, break at STOP). Scenario 043 withdrawn; `/5` is unit-tested
  only.
* **The `set_entity_data` brute force.** Before the first recording a search
  over every item id, count and patch shape found no match for vanilla's
  `dff031b033b1b979`; the first real recording matched our bytes exactly.
  The search was wrong, not the encoding — record before deriving.
* **Digging next to the player.** Vanilla's drops get picked up at random;
  keep projected rigs three blocks out until pickup is ported.

#### Not ported, named

Haste / Conduit Power / Mining Fatigue (no effect store on main — the damage
branch has one; `Digger` takes the amplifiers), Unbreaking (a binomial draw),
item pickup, `playerDestroy` overrides (ice, beehive, turtle egg, double
plants), `spawnAfterBreak` experience, the dragon egg, spawn protection,
adventure mode, the loot table's level `RandomSequences` (fresh seed; no
corpus table draws), a crafter's `triggered` on placement, survival-placement
`isUnobstructed`.

#### Next

Item pickup (`ItemEntity.playerTouch`) — it is what keeps every drop rig
three blocks away, and vanilla does it within ~450 ms of a break next to the
player.

#### State at HALT (2026-10-01)

T0 NOT green: survival/gameplay falsifiability rows, grades and baseline rows for 990-992 are unregistered. `mining-c4` (detached) writes them to `~/mining-rec/c4-*`: copy `survival-falsifiable.toml`, `gameplay-falsifiable.toml`, `gameplay-grades.toml` into `ci/checks/`, add baseline rows for 990/991/992 from `subset-2.log`, then T0.
992 (piston) is not yet recorded at its two-blocks-out position; everything else measured is listed above.

### COLD START — branch `mining` rebased onto `origin/main` 4024, 2026-10-01 (second session)

Worktree `~/grit/work-mining2` (local branch `mining-local`, pushes to
`mining`), chain worktree `~/grit/work-mining3` (detached), remote dirs
`~/rustmc-mining-t0` (recordings, sweeps, T0 — chains only) and
`~/rustmc-mining-dev` (unit tests, clippy). Recordings in `~/mining-rec/c7-*`
and `c8-*`.

#### Ported this session (all unit-tested; corpus checks in flight)

* **The four `playerDestroy` overrides**, dispatched on the declaring class
  `block_mining.json` names (`crate::mining::player_destroy`): ice melts to a
  water source over anything that `blocksMotion()` or is `liquid()` (water,
  lava, bubble_column), unless the tool has a `#prevents_ice_melting`
  enchantment; a survival double plant drops in `playerWillDestroy` (before
  its 2001, ungated by the correct-tool test) and its `playerDestroy` rolls
  air; a turtle-egg clutch loses one egg (sound, flags-2 write, 3-arg 2001);
  a beehive takes the default drop.
* **Cascade drops**: `Level.destroyBlock(pos, true)` rolls
  `dropResources(state, .., null, EMPTY)` between its 2001 and the write.
  `shapes::Effects::drops` carries the request with its level-event index;
  `shapes::each_event_and_drop` replays both in vanilla's order at the
  session, scheduled-tick and piston drains (`dig::cascade_drop`). Torches,
  double plants' other halves and piston bases now drop, creative included.
* **Unbreaking** (`crate::item_damage`): `processDurabilityChange` through the
  `minecraft:item_damage` conditional effects — LevelBasedValue
  constant/linear/fraction, `RemoveBinomial` with both arms, requirements via
  the new `rustmc_loot::test_condition`.
* **The dragon egg** teleports on the first hit (the third `attack` override).
* **A placed crafter's `triggered`** is `hasNeighborSignal(clickedPos)`.

#### In flight (detached, chained)

* `mining-c7`: survival ×2 + creative subset 010/990-992 ×2 on the rebased
  tip with new survival 053 (ice melts), 054 (silk touch keeps ice), 055
  (sunflower drops before its 2001); survival + gameplay ledgers, grades,
  baseline 990-992; commit, push, T0 keep-going.
* `mining-c8` (after c7): creative subset 010/990-994 ×2 with new gameplay
  993 (crafter triggered) and 994 (cascade torch drop); ledger, grades,
  baseline; commit, push, T0.

Both keep a new scenario only if it reaches tier 2 and is live; a dropped one
is saved as `dropped-*.toml` in the chain's rec dir.

#### Killed, so nobody re-runs them

* **`EnchantmentHelper.onHitBlock` at START needs no port** — the datapack's
  one `hit_block` effect is Channeling's, which requires `this = trident`.
* **Spawn protection cannot be ported from this area**: it needs the level's
  respawn position (a placeholder here, `levelstore.rs`) and
  `spawn-protection` from `server.properties` (not read). The harness never
  reaches it: `DedicatedServer.isUnderSpawnProtection` offsets 21-34 return
  false on an empty op list.
* **`isUnobstructed` on placement** is explicitly the entity owner's
  (`world_send.rs`, the `cannot_survive` comment); not taken here.

#### Not ported, named

Haste / Conduit Power / Mining Fatigue (no effect store on main),
`spawnAfterBreak` experience (needs the `xp-orbs` lane's `ExperienceOrb`),
item pickup (`item-pickup` lane), the loot table's level `RandomSequences`,
the loot context's block entity on a cascade drop, adventure mode.

#### Next

`spawnAfterBreak` once `xp-orbs` lands: `DropExperienceBlock` (17 ores),
`RedStoneOreBlock`, the sculk family and the spawner — `block_mining.json`
already names the class per block.
