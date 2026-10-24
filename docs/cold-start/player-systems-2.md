# Cold start — branch `player-systems-2`

### COLD START — branch `player-systems-2` off `origin/player-movement` (`20af533ba`), 2026-09-05

Worktree `~/grit/work-ps2`, remote dir `~/rustmc-ps2`. The branch is
`origin/player-movement`'s nine commits with `origin/player-systems`' 23
cherry-picked on top — the two sets are **disjoint** (`git cherry` finds no
shared commit either way), so `git cherry origin/main player-systems-2`
dedupes cleanly at main — and then the joins that make the ported state live.

## What now changes a player's health, food, effects and sleep on a real tick

| join | where | checked against |
|---|---|---|
| `ServerPlayer.doTick` runs once per elapsed tick for **every** session, not only ones with collision tables | `crate::playertick::LivePlayer::do_tick`, called from `world_send::keep_alive_loop` | 12 module tests; `tests/fall_damage_wire.rs` on the wire |
| a fall applies damage through `fall.rs` -> `hurt_server` -> `Health` -> `set_health` | `world_send::apply_landing`, on **both** `Outcome::Moved` and `Outcome::Corrected` | `tests/fall_damage_wire.rs`: 4/5/10 blocks leave 19/18/13 health, 3 blocks sends nothing |
| `set_health` and `set_experience` leave this server for the first time | `world_send::{set_health_body, set_experience_body}` | the same test decodes both off the socket |
| `checkMovementStatistics`' exhaustion, and `jumpFromGround`'s | `crate::movestats`, called from the accepted-move arm | 9 module tests, each with a positive control |
| `FoodData.tick`, natural regeneration and starvation on a real tick | `LivePlayer::do_tick` offset-42 arm | `starvation_stops_at_ten_health_below_hard_and_kills_on_hard` |
| `LivingEntity.tickEffects`, and the two effect packets | `LivePlayer::tick_effects`, `world_send::{update_mob_effect_body, remove_mob_effect_body}` | 4 module tests over `data/vanilla/mob_effects.json`'s 40 rows |
| `BedBlock.useWithoutItem`'s four arms | `crate::sleep::bed_use_without_item` | 2 module tests; **not wired** — see below |
| `java.lang.Math.round`, which every exhaustion charge is gated on | `crate::javamath::{round_f, round_d}` | 3 cells the naive form gets wrong, and a 4,000-value control |

`data/vanilla/damage_types.json` supplies the tags for `minecraft:fall`,
`:drown` and `:starve`; without it the whole damage path is inert and
`LivePlayer::hurts_refused()` counts each refusal, because a refusal that is
not counted is indistinguishable from a player who was never hurt.

## The scenario, and the ledger that had never existed

`ci/checks/survival/006-a-five-block-fall-costs-two-health.toml` is the first
scenario on either substrate that can grade `set_health`.
`ci/checks/survival-falsifiable.toml` is the first falsifiability ledger for
that directory, and `ci/t0.sh` now runs `ci/corpus-falsifiable.py --check`
against it — which was `docs/cold-start/player-systems.md`'s single next item.

## What the tree refuted

* **`FoodData.tick`'s `difficulty != PEACEFUL` guard is on the food-level
  decrement, not on the starve arm.** This branch's own commit message said
  the opposite until the test refuted it. On the corpus substrate a player at
  food 0 *does* starve, stopping at exactly 10.0 health; what peaceful
  prevents is the food level ever *reaching* 0 from exhaustion.
* **A fall CAN be scenarioed in survival**, which refutes
  `docs/FINDINGS.md`'s own 2026-09-05 entry. `Step::Lurch` is one packet at
  any distance; `Step::Walk` cannot do it because `hop_to` caps at four
  blocks and applies one `on_ground` flag to all of them.
* **241 one-block sprints drop a food level, not 40.** Each 4.0 of exhaustion
  is spent on saturation while there is any, and `START_SATURATION` is 5.0.
* **`move_correction` was a test target in no bucket**, which is a T0 red on
  `ci/check-test-targets.py` by itself. Registered here with the two this
  branch adds.

## What I refused, and why — do not re-run these

* **Effects reached through the item-use ladder.** `Item.use`'s `Consumable`
  arm lives on `origin/items-2` and needs that branch's `item_props` extract
  and its `ci/item-props-golden.sh` generator; cherry-picking it pulls 31
  commits and their `data/vanilla` generators onto this branch, and the gate
  would then be measuring items-2's work under this branch's name. Nothing
  else on this server calls `addEffect`: `/effect` is permission level 2 and
  the corpus players are not operators, there is no beacon and no hostile mob.
  So `LivePlayer::add_effect` is called by `do_tick`'s own resend and removal
  paths and by tests, which is written into its doc comment rather than left
  for a reader to find.
* **`damage_event`.** `ClientboundDamageEventPacket.write` offset 9 encodes
  the type through `DamageType.STREAM_CODEC`, a varint into the *dynamic*
  `minecraft:damage_type` registry, and `data/vanilla/damage_types.json`
  carries no registry id. Writing the packet means inventing a number. `006`
  therefore projects `["set_health"]` alone and says so.
* **Sleeping, wired.** `BedBlock.useWithoutItem` is transcribed and tested;
  reaching it needs `LivePlayer` off the play-loop stack and into shared
  state (the click handler has a `Session`, not the tick's locals), plus
  `bedInRange`/`bedBlocked` world reads and a monster scan over mobs this
  server does not spawn — and its one observable, the `set_time` skip through
  `ServerLevel.updateSleepingPlayerList`, belongs to `world-systems-2`. The
  ladder is the half that could be written from bytecode and graded; the
  wiring is a plumbing change that would touch every `Session` construction
  site in the crate.
* **Sprint-driven hunger.** `Entity.setSprinting` is
  `setSharedFlag(3, …)` on `DATA_SHARED_FLAGS_ID` and this server has no
  `packDirty`/`ServerEntity.sendChanges` path, which
  `origin/player-movement` already refused. `isSprinting()` is therefore
  always false and the ground arm charges `0.0f`; the only non-zero
  exhaustion this server can drive is `jumpFromGround`'s `0.05f`.
* **The fluid predicates.** `isSwimming`, `isEyeInFluid(WATER)` and
  `isInWater` need an eye-position fluid query; `crate::playermove` tracks a
  fluid state for the body and exposes none. So the air never falls and no
  player drowns — the drowning arm of `do_tick` is reachable and unit-tested
  and what is missing is the predicate that opens it.

## The single next item

**Add a `registryId` column to `ci/damage-types-golden.sh` and emit
`damage_event`.** It is the one thing standing between this branch's fall
damage and a packet a scenario could grade beside `set_health`, and it makes
`ci/checks/survival/001` and `005` — the two attack checks — able to say
something about damage rather than only about a sound. **The result that
closes it:** `006` projects `["set_health", "damage_event"]` and both match,
and the ledger's tier for it does not fall.
