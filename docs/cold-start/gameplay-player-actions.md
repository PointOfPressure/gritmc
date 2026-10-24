# Cold start — branch `gameplay-player-actions`

Append-only. Newest entry last.

---

### COLD START — branch `gameplay-player-actions` off `53cf6424`, 2026-08-11

**The item: the three gameplay checks whose verdict was `absent` and whose
cause was a serverbound packet with no handler.** Not wrong answers — no
answers. Remote dir `~/rustmc-actions` on the Threadripper, worktree
`/home/sayer/grit/work-actions`.

    550-attacking-another-player                        absent
    551-attacking-from-out-of-range                     absent
    510-swapping-to-the-offhand-shows-on-both-channels  absent

**MEASUREMENT PENDING** — this entry was written while the subset run was
queued behind three other agents on the box-wide corpus lock, and is committed
now rather than held, because an uncommitted write-up is the thing this
project loses. The verdicts above are the *baseline*, not a result. The
measured table replaces this paragraph in the same file.

## What the brief got wrong, corrected by the tree on the first check

**The two attacks are not `ServerboundInteractPacket`'s `ATTACK` action.** In
26.2 `minecraft:attack` is its own packet at **protocol id 1** and
`minecraft:interact` is a separate one at **26** — confirmed against Mojang's
`reports/packets.json` in the pinned jar, which is also what
`crates/rustmc-proto/src/ids.rs` already carried and what
`docs/PARITY-ROADMAP.md` §20.5 already says ("`client.rs` has `ATTACK` and not
`INTERACT`; they are separate packets in 26.2"). Nothing was built on the
wrong id because the id was read rather than remembered, which is the rule
that exists in `ids.rs` for exactly this.

The offhand swap *is* `ServerboundPlayerActionPacket`, action ordinal **6**,
as briefed.

## What was ported, from `javap -p -c -constants` on the pinned jar

**`crates/rustmc-server/src/attack.rs`** is new and holds the pure decision;
`world_send.rs` holds the two handlers.

* **`handleAttack`'s refusal ladder.** Nine tests in the handler's own order,
  of which five are modelled and four are recorded as constant-true in this
  server's world (`hasClientLoaded`, the world border, `PIERCING_WEAPON` /
  `isItemEnabled` / `cannotAttackWithItem`, whose data components
  `crate::item::ItemStack` does not carry).
* **The reach test, which is the whole of `551`.** `handleAttack`'s literal
  `3.0d` at offset 96 is **not the reach** and this file says so because the
  project has already paid for reading it as one. It is the `d` argument of
  `AttackRange.isInRange(LivingEntity, ToDoubleFunction<Vec3>, double)`:

      dist = sqrt(aabb.distanceToSqr(living.getEyePosition()))
      lo   = (effectiveMinRange(living) - hitboxMargin) - d
      hi   = (effectiveMaxRange(living) + hitboxMargin) + d
      return dist >= lo && dist <= hi

  With `AttackRange.defaultFor`'s `(0, r, 0, r, 0, 1)` and
  `Attributes.ENTITY_INTERACTION_RANGE` = **3.0** (its own `RangedAttribute`
  default — `Player.createAttributes` does not add it, `createLivingAttributes`
  does), a creative attacker reaches `0 + 3.0 + 3.0` = **6.0 blocks, eye to
  bounding box**.
* **`Player.attack` as far as `hurtOrSimulate` returning false**, which is
  `playServerSideSound(PLAYER_ATTACK_NODAMAGE)` — sound **1304**, at the
  attacker's *feet*, `SoundSource.PLAYERS` (7), volume 1.0, and broadcast to
  **everyone including the attacker**: `playServerSideSound` puts `aconst_null`
  in `Level.playSound`'s entity slot, the opposite of `Player.playSound`'s
  `aload_0` that keeps a step sound off the walker's own stream. Both checks
  are `observe_from = "actor"`, so an implementation that excluded the
  attacker would produce silence on the only stream they read.
* **`handlePlayerAction`'s `SWAP_ITEM_WITH_OFFHAND` arm**, offsets 160-221 —
  read the offhand, write the main hand into it, write the saved offhand into
  the main hand, `stopUsingItem`, return **without** an ack. Plus an offhand
  slot on `Hotbar` (it is not an `Inventory` index: `Inventory.items` is 36
  long and the offhand lives on `EntityEquipment`), menu slot 45 in
  `apply_hand`, and the window-0 `container_set_slot` pair
  `AbstractContainerMenu.broadcastChanges` sends — selected hotbar slot first,
  then 45, one `incrementStateId` each.

`crate::menu::MenuState::inventory_state_id`'s own doc named the offhand swap
as one of the two window-0 corrections this server never emitted. It emits one
now. The other, picking a dropped item up, is still open.

## The finding: `510`'s name is refuted by `510`'s own reference

The scenario is called `shows-on-both-channels` and its `about` says the
change goes out as `container_set_slot` on the acting connection **and**
`set_equipment` broadcast to everyone tracking the player. Its `observe` list
is `["set_equipment", "container_set_slot", "container_set_content"]`.

**Vanilla sends no `set_equipment` for this action.** Measured before it was
derived, across five independent full-corpus recordings already on the box —
`rustmc-batch`, `-batch2`, `-cov3`, `-iceberg`, `-joint` — the reference trace
is, in every one: two `container_set_slot` on window 0 to the actor, and one
`entity_event` to the observer. `set_equipment` is a kind the recorder decodes
and does report on the observer in four other scenarios of the *same* runs
(`stairs-and-furnace-face-apart`, `a-repeater-delays-the-signal-it-passes-on`,
`dropping-an-item-throws-it-on-the-floor`,
`walking-away-drops-a-standing-player-from-view`), so its absence here is a
fact about the action and not about the instrument.

Then the mechanism, so it is a derivation rather than a coincidence.
`LivingEntity.detectEquipmentUpdates`:

    Map changed = collectEquipmentChanges(lastEquipmentItems);
    if (changed != null) {
        handleHandSwap(changed);
        if (!changed.isEmpty()) handleEquipmentChanges(changed);
    }

`handleHandSwap`, offsets 0-156, fires when both hands are in the changed map
**and** the new main hand `ItemStack.matches` the last known offhand and the
new offhand matches the last known main hand — an exchange, not two
independent writes. It broadcasts `ClientboundEntityEventPacket` with
`EntityEvent.SWAP_HANDS` (**byte 55**, from that class's own constant) through
`ServerChunkCache.sendToTrackingPlayers`, then `remove`s both slots from the
map and copies both into `lastEquipmentItems`. The map handed to
`handleEquipmentChanges` is empty, so the `set_equipment` never happens.

**So vanilla substitutes one entity event for two equipment slots and lets the
client do the exchange itself.** The equipment channel exists; it is simply
not the packet the check names. Both channels are now reproduced — the
inventory one, which is the whole of what `510` projects, and the entity-event
one, which only the strict projection can see. `set_equipment` for this action
is a packet **neither** server sends, and `510` cannot observe the difference
between a server that answers the equipment channel correctly and one that
does not answer it at all.

That is worth acting on and is left for the next agent rather than done here:
`510`'s `observe` list should gain `entity_event`, or a sibling check should
name it, or the `about` should be corrected. Changing a check's projection
moves its verdict, and doing that in the same branch that implements the
mechanism is how a check that cannot fail gets manufactured.

## WHERE THIS DELIBERATELY STOPS SHORT OF VANILLA'S DAMAGE MACHINERY

`Player.attack` continues past the branch this ports into an entire combat
system. **None of it is implemented, and `crate::attack::Outcome::Hurt` is a
variant that produces no packet at all rather than one that plays the
no-damage sound anyway.** The boundary, named so the next agent starts from it:

| vanilla, past `hurtOrSimulate` returning true | state here |
|---|---|
| `getKnockback` + `causeExtraKnockback` | absent |
| `doSweepAttack` and `isSweepAttack`'s five-way condition | absent |
| `attackVisualEffects` — the crit / strong / weak / sweep sounds and the damage tilt | absent; only `PLAYER_ATTACK_NODAMAGE` is produced |
| `damage_event` and `set_health` | absent, and **occur 0 times in 17 recordings** — both checks project them and neither reference has ever carried one |
| `itemAttackInteraction`, `damageStatsAndHearts`, `causeFoodExhaustion(0.1f)` | absent |
| `getAttackStrengthScale` / `attackStrengthTicker` / the cooldown | absent — it decides only the knockback sound and the crit, both of which are past the boundary |
| death, respawn, invulnerability frames | absent |
| offset 118's kick, `multiplayer.disconnect.invalid_entity_attacked` | **refused instead of kicked.** Attacking a dropped stack, an experience orb or yourself produces nothing here where vanilla ends the session. No corpus scenario reaches it |

**Why the boundary is where it is, and why it is not a fudge.** The gate is
`Player.hurtServer` offsets 11-32: `abilities.invulnerable &&
!source.is(BYPASSES_INVULNERABILITY) -> return false`. `Abilities.invulnerable`
is what `GameType.CREATIVE` sets, and `crate::status::GAME_TYPE` is creative
for every connection. So every target this server can produce takes the
no-damage branch **as a consequence of the target's abilities**, which
`crate::attack::Target::invulnerable` is a field for. `550` is not passing
because we always play that sound; it is passing because the target cannot be
hurt, and `attack.rs`'s
`a_vulnerable_target_takes_the_unported_branch` test is what keeps those two
apart. The first survival player makes `Outcome::Hurt` reachable and it
answers with silence until somebody builds the rest.

`docs/PARITY-ROADMAP.md` §20.5 already ranks the substrate item this needs:
**a non-creative target**, shared with survival mining. That is the next move
on damage and it is not a check, it is a substrate change.

## Nulls — do not re-run these

* **`set_equipment` for the offhand swap is not a missing packet.** Vanilla
  does not send one. Five recordings and the `handleHandSwap` bytecode above.
  Do not "fix" `510` by emitting one; it would put a packet on the wire that
  vanilla does not.
* **A `PIERCING_WEAPON` / `isItemEnabled` / `cannotAttackWithItem` gate is not
  worth building yet.** All three read data components off the held stack and
  `ItemStack` here carries an item name and a count. `cannotAttackWithItem` is
  `minCharge > 0 && charge < minCharge` with `minCharge` defaulting to `0.0f`,
  so it is false for every stack that has no components — which is all of them.
* **The attack cooldown does not decide anything observable in this corpus.**
  `getAttackStrengthScale` reaches only `strongAttack` (the sprint knockback
  sound) and the crit, both past the damage boundary. Building an
  `attackStrengthTicker` would move no verdict.
* **`AttackRange`'s mob arm (`minReach * mobFactor`) is unreachable.** Only a
  player can send `handleAttack`, and a mob attacking is an AI this server does
  not have. Modelled as a game-mode boolean rather than an entity so the dead
  arm is absent instead of present and untested.

## The single next item

**A non-creative target**, per `docs/PARITY-ROADMAP.md` §20.5 — the substrate
that makes `damage_event` and `set_health` reachable at all. The result that
closes it: `550`'s reference trace carries a `damage_event` and a `set_health`
where today it carries neither, and `crate::attack::Outcome::Hurt` stops being
a variant that produces nothing. It is the same substrate item as survival
mining (`Step::FinishDig`, used by zero checks), and doing one does the other.
