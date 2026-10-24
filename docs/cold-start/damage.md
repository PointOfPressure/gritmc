# Cold start — branch `damage`

---

### COLD START — branch `damage` off `origin/main` 3745 (`c52d52e7b`), 2026-09-30

Worktree `~/grit/work-damage`, remote dir `~/rustmc-damage`. **The branch
merges `origin/player-systems-2` and `origin/scenario-verbs`** — the damage
pipeline (`crate::hurt`, `crate::health`, `crate::damage_types`, fall damage)
lives on the first and the console step on the second, and neither was on
main. Landing this branch lands both; see "Merge repairs" below for what the
merge itself needed.

#### The verb

`do = "console"`, `text = "damage oracle <amount> [<type> [by <entity>]]"` —
`DamageCommand`, run by both servers' consoles. `/damage` is permission 2 and
the harness players are level 0, so only the console reaches it
(`MinecraftServer.createCommandSourceStack()` is full permission). Our side:
`crate::damagecmd` parses the five `DamageCommand` branches, resolves names
against the connection list, and queues the hit to the target's own loop,
tagged with the tick it was read in; the loop applies it after that tick's
`doTick` (`DedicatedServer.tickConnection` order). `kill <player>` and
`effect give` ride the same queue.

The restore for a damage check is `console_restore = ["kill oracle"]`: a
respawned `ServerPlayer` is a new one (20.0, no cooldown, `lastHurt` 0, no
effects), and `record::respawn_if_dead` answers a reported death with the
client's `perform_respawn`. That makes our death and respawn load-bearing for
every later survival check, deliberately.

`Step::Console` gained `settle_ms` (a default console step is ~800 ms, 16
ticks; the hurt cooldown arm lives 10).

#### What is ported, from the jar

* `ServerPlayer.hurtServer` / `Player.hurtServer` above `crate::hurt`: the
  four damage game rules, `pvp`, abilities, difficulty scaling
  (`DamageSource.scalesWithDifficulty`), the zero return,
  `Player.actuallyHurt`'s exhaustion, Resistance and Fire Resistance from the
  live effects.
* The level half: `damage_event` (bare registry id from the captured
  `registry_data`), `markHurt` -> `set_entity_motion` via `sendChanges`'
  `hurtMarked` tail, `needsSync`, `knockback` + `indicateDamage`
  (`hurt_animation`), the hurt sound by `DamageEffects`, `set_entity_data` for
  the health (`packDirty`), kill credit (`resolvePlayerResponsibleForDamage`).
* `ServerPlayer.die`, `tickDeath`'s removal, `PlayerList.respawn`.
* **The server's own player travel.** `Player.canSimulateMovement` is true on
  the server; `PlayerMovement::server_travel` ports aiStep's velocity prelude
  and `travelInAir` with no input.
* A fall now takes the same Player layers and level half, after
  `causeFallDamage`'s two sounds.

#### Refused, by name

`hurt_animation` is sent but not decoded by the recorder (a new decoded kind
would be dead in the creative corpus and `ci/decoded-kind-reach.py` ratchets
that); armour (no `/item`, no equipped-item attributes); `fall_variants` and
`intentional_game_design` death messages; death loot; `change_difficulty` in
the respawn (no packet id on this server); `travelInFluid` and the
trapdoor-ladder arm of `onClimbable`; `Player.actuallyHurt`'s single
absorption subtraction (this server subtracts twice — unreachable, nothing
grants absorption).

#### Merge repairs

`tests/fall_damage_wire.rs` and `tests/move_correction.rs` did not compile
(main's `Session` gained two fields); `data/vanilla/villager_trades.json` was
written by two generators in two shapes (main's `villager-trade-golden.sh`
and `scenario-verbs`' `dump-villager-trades.sh`) — the latter's is
`villager_trades_datapack.json` now.

#### Measured, 2026-09-30 16:21 (`~/rustmc-damage`, oracle-runner and server at `6328a25cc`)

Two survival passes, 15 scenarios each, vanilla 26.2:

* **vanilla against itself: 13/15 declared match** — every damage check
  (006..016) matches; the two that do not are `002` (inert) and `003`
  (vacuous), both older than this branch.
* **ours against vanilla: 10/15 declared, all ten damage checks match**, in
  both passes. The other five: `001`/`005` `absent` (player-on-player attack
  deals no damage here — `Player.attack` is not ported; see below), `002`
  inert, `003` vacuous, `004` diverge (mining, not this lane's).
* **Red before green, measured**: the first pair on this branch
  (`c6f8764a5`, 2026-09-30 00:36) read `diverge` on all seven damage checks
  then present, and every fix above was taken from a recording of that red.
* Falsifiability sweep: 15/15 tier 2, 0 dead fields of 71, 34 unaskable of
  105 (pins in `ci/corpus-falsifiable.py`).

#### Killed, so nobody re-runs them

* **`kill` + respawn as the between-check restore** — strands the actor at
  the world spawn; the walk to the next rig starves the observer's
  keep-alive (vanilla timed it out). Instant Health IV instead.
* **a trailing `perform_respawn` after every `damage`** — the measured step
  is the last one, so every check read `mute` vanilla-against-vanilla.
* **100 ms console gaps** — the next-tick packets fall either side of the
  step boundary between two vanilla runs.
* **observing `set_entity_motion` on a fall** (vanilla disagrees with itself:
  -0.649 vs -0.715) and **`set_entity_data` across an effect's end** or near
  other entities.

#### The single next item

**`Player.attack` against a player** — `001` and `005` stay `absent` until
it is ported. `origin/mob-combat` already carries the attack-strength
arithmetic (`crate::attack`), so it belongs after that lands. The hard part
is ordering, not arithmetic: vanilla hurts the target synchronously inside
`attack` (the target's `damage_event` and hurt sound precede the attacker's
`player.attack.*` sound on the wire), and here the target's `LivePlayer` lives
on the target's own loop. Closing result: `001` and `005` read `match`,
`damage_event.source` 34 (`player_attack`) with the attacker as cause and
direct, health `19.792` (attack strength 0.1 right after the swing).

#### T0

**GREEN** at `430d42802` (`~/rustmc-damage`, rsync -> touch -> `gen-gameplay-tables.sh` x2 -> `ci/t0.sh`, 2026-09-30 17:14). Two reds on the way, both fixed: `ci/entity-id-allocator.py` (respawn must burn an id — it does now) and `ci/order-blind-falsify.sh` (box-state; main's `b6061e72b` cherry-picked and the ledger re-taken).
