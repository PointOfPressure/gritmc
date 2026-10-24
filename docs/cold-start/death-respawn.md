# COLD START — branch `death-respawn`

### COLD START — branch `death-respawn`, off `main` (`1d109abcf`, 2,788), 2026-09-06

**The whole 26.2 player death and respawn path is transcribed from the pinned
jar, with 66 unit tests and one byte-exact control against a real server's
capture. Nothing is wired to a live death and nothing can be: the corpus
cannot kill the harness player and cannot send `PERFORM_RESPAWN`, both
measured. One live behaviour change landed — the join packet's
`showDeathScreen`, which was a literal.** Worktree `~/grit/work-death`, remote
dir `~/rustmc-death`. Not merged; the orchestrator gates.

## WHAT IS HERE

`crates/rustmc-server/src/death.rs` and `crates/rustmc-server/src/respawn.rs`,
plus `PLAYER_COMBAT_KILL` (68) and `RESPAWN` (82) in
`crates/rustmc-proto/src/ids.rs`. Every function is a decision the bytecode
makes, written over plain values so a test can take each branch; every doc
comment carries the offsets it was read from.

Encoders that produce real bytes: `player_combat_kill`, `respawn`,
`set_experience`, `set_health`, `set_default_spawn_position`, `game_event`,
and `CommonSpawnInfo` (the block shared by `login` and `respawn`).

## THE MEASUREMENT THAT DECIDED THE SHAPE OF THIS BRANCH

The brief proposed a gameplay check: a console `/kill`, then the client's
`PERFORM_RESPAWN`, grading `combat_death`, the death message and the respawn
packets. **It is not reachable on this tree, for two independent reasons, and
fixing either leaves the other:**

1. **Nothing can kill the harness player.** `tools/oracle-runner`'s `Step`
   enum has eighteen variants — `hold`, `look`, `walk`, `dig`, `finish_dig`,
   `drop`, `swap_offhand`, `place`, `use_item`, `attack`, `sneak`,
   `container_click`, `close_container`, `sign_update`, `swing`, `command`,
   `say`, `wait` — and none of them damages anybody. The only console route on
   `main` is the per-scenario `[gamerule]` block, whose `GameRule::well_formed`
   refuses anything but `gamerule <rule> <value>`. `Step::Command` runs as the
   acting player, and `ci/t2-gameplay.sh` lines 676-677 say in its own words
   that the harness's players are **not operators** — which is why it sets
   rules through the console at all. `/kill` is permission level 2, so vanilla
   refuses it from a scenario exactly as it refuses `/gamerule`.
2. **Nothing can send `PERFORM_RESPAWN`.** `minecraft:client_command` is id 12
   and has been a named constant in `rustmc_proto::ids::play::sb` for some
   time. No `Step` variant constructs one, `Client::act` has no arm that could,
   and nothing in `crates/rustmc-server` reads one. Vanilla would sit on its
   death screen for the rest of the recording.

**So no check is registered and no `gameplay-baseline.toml` row is added.** A
scenario written today would compare two silences and no edit to its own
measured step could redden it — this project's named defect class. The order
to fix it in: `harness` adds a `client_command` verb, `player-systems-2` lands
a damage path, and then the first check to write is the one whose *measured
step* is the `PERFORM_RESPAWN`, with
`observe = ["player_combat_kill", "system_chat", "respawn",
"set_default_spawn_position", "set_experience"]` — which is the order
`respawn_packet_order` gives, taken from `PlayerList.respawn`'s own send sites.

## WHAT MOVED ON THE WIRE

One thing, and it is squarely this area's: `ClientboundLoginPacket`'s
`showDeathScreen` is the **negation** of the `immediate_respawn` game rule
(`PlayerList.placeNewPlayer` offsets 297..307), and `status.rs` hardcoded it
`true` — along with `reducedDebugInfo` and `doLimitedCrafting`, which are the
same defect. `crate::gamerules` has carried all 59 rules and a live
`/gamerule` since it landed, so setting `immediate_respawn` changed a value
nothing read. All three now come off one `GameRules::shared()`, read before the
literal as vanilla reads one local at offset 209.

`JoinInfo::default()` deliberately keeps the literals: the 109-byte captured
vanilla join packet `play.rs` is gated against was taken at defaults, and that
gate must keep passing.

## WHAT THE BYTECODE SAYS THAT A FIELD LIST DOES NOT

Thirteen lines of it are in `docs/FINDINGS.md` under 2026-09-06. The five that
will cost another lane most:

* **There is no `combat_death`.** The packet is `minecraft:player_combat_kill`,
  id **68**; 66 and 67 are `player_combat_end` and `player_combat_enter` and
  neither carries a message.
* **A respawn sends `respawn` (82), never `login` (49)** — even though it
  builds a whole new `ServerPlayer` and swaps it onto the same connection,
  which is exactly what makes a re-login look plausible.
* **`set_default_spawn_position` (97) has four fields, not two**: a dimension
  identifier, a packed position, a yaw *and* a pitch, through
  `LevelData$RespawnData` -> `GlobalPos.STREAM_CODEC` + `FLOAT` + `FLOAT`.
* **`ServerPlayer.die` does not call `super.die`.** Both superclass versions
  also drop loot, so a port that chained them drops the inventory twice.
* **The respawn's invulnerability is not a tick counter.** It is
  `!connection.hasClientLoaded()`, and `hasClientLoaded()` is
  `!waitingForRespawn && clientLoadedTimeoutTimer <= 0`. The 60 is a *ceiling*,
  ended early by `minecraft:player_loaded` (44).

## WHAT WAS KILLED AS A NULL — DO NOT RE-RUN THESE

* **A `client_command` dispatch arm.** Id 12 currently falls through every
  branch of `world_send`'s frame loop to `crate::player::apply`, whose
  `match id` ends `_ => Ok(Update::Ignored)`. An arm added today changes
  nothing any client or check can see, and the branch would then hold a decoder
  feeding an output nothing produces — the `ACCEPT_TELEPORTATION` shape.
  `client_command_blocker` names a *different* reason per action so the three
  do not collapse into one "not implemented".
* **Serving `REQUEST_GAMERULE_VALUES`.** It looked shippable — the permission
  half is exact (`sendGameRuleValues` offsets 0..39,
  `Permissions.COMMANDS_GAMEMASTER`), `crate::gamerules` holds all 59 rules and
  `GameRuleValue::serialize` is already vanilla's own string form, and
  `ClientboundGameRuleValuesPacket` is id **39**. It is not: the codec is
  `ByteBufCodecs.map(HashMap::new, ResourceKey.streamCodec(GAME_RULE),
  STRING_UTF8)` over a `java.util.HashMap` filled at offset 51, so the wire
  order is that map's bucket order over `ResourceKey` hashes. Registration
  order is a packet a client accepts and a byte comparison rejects. **Derive
  the order first; do not send it in the meantime.**
* **Turning on a client-loaded gate.** `markClientUnloadedAfterDeath` is one
  `putfield waitingForRespawn = true` and eight `ServerGamePacketListenerImpl`
  handlers read `hasClientLoaded()`. This server has no such gate — nothing
  reads `minecraft:player_loaded` (44) — so the death half has nothing to set,
  and switching it on would start dropping movement packets the corpus relies
  on. That is `crate::play`'s call.

## REFUSED AND NAMED, BY OWNER

* **`mob-combat`**: mob death, and the death loot of anything that is not a
  player. `LivingEntity.dropAllDeathLoot`'s loot-table arms are transcribed
  only far enough to say that a player's are empty.
* **`advancements`**: every trigger on death, and the `CHANGED_DIMENSION`
  trigger on the End-portal return arm of `handleClientCommand`.
* **`dimensions`**: the End's respawn special case. `wonGame` is checked
  *before* health at offset 60, so that arm is reached first, and it is
  modelled as an outcome and not implemented.
* **`enchantments`**: `EnchantmentHelper.processMobExperience`, which wraps the
  base experience reward, and `EnchantmentEffectComponents
  .PREVENT_EQUIPMENT_DROP`, which decides which stacks
  `destroyVanishingCursedItems` removes. Both are parameters here.
* **`player-systems-2`**: the damage path, `Player.isInvulnerableTo`'s
  damage-type-tag ladder, and the `doTick` that turns
  `restoreFrom`'s `-1` sentinels into the `set_health` and `set_experience` a
  respawned player receives. `PlayerList.respawn` sends **neither** of those
  itself; it calls `setHealth(getHealth())` at offsets 446..453 and lets the
  next tick notice.
* **`persistence-2`**: `RespawnConfig` on disk. `respawn_outcome` takes
  `has_config` and `forced` as inputs rather than reading a save.
* **`harness`**: a `client_command` step verb.
* **`CombatTracker`'s `FALL_VARIANTS` and `INTENTIONAL_GAME_DESIGN` arms**:
  both need a damage-entry history this server has no damage to fill.

## THE SINGLE NEXT ITEM

**Derive `ResourceKey`'s `hashCode` and Java's `HashMap` bucket order well
enough to emit `ClientboundGameRuleValuesPacket` (39) byte-exactly**, then
serve `REQUEST_GAMERULE_VALUES` behind the `COMMANDS_GAMEMASTER` check. That
is the one arm of this area whose substrate is already entirely present, and
it closes with a golden vector: 59 rules at their defaults, compared against
bytes a real server sends. Everything else here waits on a damage path.

## HOW IT WAS GATED

`cargo fmt`, `cargo clippy --all-targets -- -D warnings`, and
`cargo test --release -p rustmc-server --lib` on the Threadripper in
`~/rustmc-death` — **0 failed**, of which **66** are this branch's (`-- death respawn`). `ci/t0-remote.sh` with `RUSTMC_REMOTE_DIR=rustmc-death`.
