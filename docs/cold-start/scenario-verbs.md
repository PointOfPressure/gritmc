# Cold start — branch `scenario-verbs`

---

### COLD START — branch `scenario-verbs` off `941dfd27d` (= `origin/world-systems-2`), 2026-09-06

**Five lanes had built working behaviour and could register no check, because
the harness had no verb that makes the subject do the thing. Two of the five
gaps were real verbs, two were already reachable through `Step::Console` and
needed the evidence rather than the code, and one is another lane's branch.**
T0 green; remote dir `~/rustmc-verbs` on the Threadripper.

#### What was added

* **`Step::ClientCommand { action }`** — `client_command`, packet 12, the
  packet nothing in this tree had ever constructed. Body is one varint
  (`FriendlyByteBuf.writeEnum` is `writeVarInt(ordinal())`, offsets 0..8).
  26.2 has **three** actions, not two: `PERFORM_RESPAWN` 0, `REQUEST_STATS` 1,
  `REQUEST_GAMERULE_VALUES` 2.
* **`Step::ReleaseUse`** — `RELEASE_USE_ITEM`, action 5, which had been in the
  ordinal table for a month with no arm sending it. `Step::UseItem` was a press
  with no release, so a bow could be drawn and never loosed.

#### The doors, which is most of what this branch is

Every one of these sends **nothing** and reads as agreement, and each is
handled by the cheapest instrument that can see it:

| door | bytecode | how it is refused |
|---|---|---|
| `perform_respawn` on a living player | `handleClientCommand` 127..139, `getHealth() > 0` -> `return` | not decidable from the file (console kill *or* a walk below `getMinY() - 64`); the runner's **vacuous** verdict is what refuses it |
| `request_gamerule_values` unopped | `sendGameRuleValues` 7..39, `COMMANDS_GAMEMASTER` -> `LOGGER.warn`, `return` | refused at load, unconditionally |
| `release_use` with no open use | `releaseUsingItem` 13..16, `useItem.isEmpty()` | refused at load |
| `release_use` after the hand changed | `releaseUsingItem` 24..27, `isSameItem` | refused at load |
| a use left open at the end | nothing clears `useItem`; a bow is 72000 ticks | refused at load — 552/553 in a second shape |
| a console `kill` with no respawn | `LivingEntity.kill` = `hurtServer(genericKill(), MAX_VALUE)` | refused at load; the respawn IS the death's `console_restore` |

`request_stats` is the one action with no door — `ServerStatsCounter.sendStats`
sends a `ClientboundAwardStatsPacket` at offsets 56..71 with no branch in front
of the send — so it is the anchor a scenario can use to prove the arm fires at
all, independently of whether anything died.

#### Killed as nulls — do not re-run these

* **No verb damages the subject and none should be added.** `Step::Attack`
  resolves through `newest_of_type(&self.entities, ..)` and `self.entities` is
  fed only by `ADD_ENTITY` (`client.rs:684`, `703`), which a player is never
  sent for themselves — so the actor can only hit `oracle_eye`, and that does no
  damage: `docs/PARITY-ROADMAP.md:784` records `damage_event`, `set_health` and
  `entity_event` in both 550 and 551's `observe` lists with vanilla emitting
  none of the three. `Step::Console` + `kill` is the route, because
  `minecraft:generic_kill` is one of the two entries in the
  `bypasses_invulnerability` damage-type tag.
* **Do not op the harness players.** Measured over three independent corpus
  pairs a week apart (`rustmc-combat`, `rustmc-misc2`, `rustmc-stack`, all 111
  scenarios of each): vanilla declares 10 roots, we declare 6, identically in
  every scenario. `weather`, `kill`, `gamerule` and `damage` are in neither
  tree. `Step::Console` is the route and it needs no permission change.
* **Do not add a third verb for hold-and-release.** `Hold` selects a stack and
  `UseItem` is the press; the missing piece was one packet, not a verb pair.
* **Menus are not a verb gap.** `origin/menus-2` is 316 commits over main by
  `git cherry`. `Step::ContainerClick` already clicks a slot; what it has
  nothing to click is a menu this server opened.

#### Two guards that could not go red, both in the permission path

* `ci/checks/command-tree-baseline.json` shipped `provenance: "derived"` with
  `rustmc: ["list"]` — a prediction no run ever confirmed. A derived baseline
  keeps the exit code at 0 by design, so every corpus run since printed
  `this run DISAGREES with it` and passed, ratcheting at 1 against a real 6.
  Re-baselined as `recorded`.
* A root in **our** tree that vanilla's level-0 tree does not carry was printed
  as `ours only N` and never scored. So op-ing our players and not vanilla's —
  the one state in which every `Step::Command` runs at a different permission
  level on the two servers — exited 0. It is a refusal now.
  `ci/command-tree-parity-falsify.py` 8 cases -> 11, shown red on the
  unrepaired script first.

#### The next item

**No corpus run was taken and one is owed.** Nothing here needed a `--sweep`
(no scenario added, no check's `steps` or `observe` edited) and all 111
scenarios were shown to still load through the new validation with the verdict
table unmoved — `oracle-runner compare ci/checks/gameplay` against the
`rustmc-combat` pair gives 99/111 declared and 94/111 strict before and after.
What that does **not** cover is the two new `Client::act` arms on a live pair,
because no scenario uses them yet: the first scenario each lane writes is the
first time either arm meets a server. `death-respawn` and `projectiles` should
expect to be the ones who find out, and the anchor to write first is
`request_stats`, whose reference side cannot be silent.
