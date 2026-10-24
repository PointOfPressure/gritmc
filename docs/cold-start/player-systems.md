# Cold start — branch `player-systems`

### COLD START — branch `player-systems` off `main` (`bd5be4a36`), 2026-09-05

**Vanilla 26.2's per-player state machines are in
`crates/rustmc-server/src/`, transcribed from the jar's bytecode: food,
experience, health, the two absorption stages, the whole of `hurtServer`,
mob effects, fall damage, sleeping and the weather cycle — plus three new
`data/vanilla/` tables (mob effects, potions, damage types) with their
Goldens and readers.** Worktree `~/grit/work-player`, remote dir
`~/rustmc-player` on the Threadripper. Not merged — the orchestrator gates.

**Nothing here is wired to the tick loop or to a packet handler, and that is
deliberate rather than unfinished.** Every module is a state machine that
takes what vanilla reads and returns what vanilla does; the wiring is one
commit that a later branch makes once there is a scenario able to grade it.
See **WHAT CANNOT BE MEASURED** below, which is the reason there is not one.

    TRANSCRIBED   food.rs         FoodData + all of FoodConstants
                  experience.rs   the level curve and both normalisation loops
                  combat_rules.rs CombatRules, both absorb formulas
                  health.rs       LivingEntity health/absorption/actuallyHurt
                  hurt.rs         LivingEntity.hurtServer, offsets 0..613
                  effects.rs      MobEffectInstance + the five TickRules
                  falldamage.rs   causeFallDamage/calculateFallDamage/-Power
                  sleep.rs        the bed ladder, SleepStatus, the sleep counter
                  weather.rs      advanceWeatherCycle, both draws, the levels
                  peaceful.rs     ServerPlayer.tickRegeneration
                  breath.rs       air, drowning, and the two baseTick counters
                  playersync.rs   doTick's set_health / set_experience triggers
                  javamath.rs     Mth.clamp / Math.min / Math.max, written out

    TABLES        mob_effects.json    40 effects   ci/mob-effects-golden.sh
                  potions.json        46 potions   ci/potions-golden.sh
                  damage_types.json   51 types     ci/damage-types-golden.sh
                  (21 -> 24 required tables; each registered in
                   ci/gen-gameplay-tables.sh with the literal path the
                   deriving grep matches)

    READERS       potions.rs, damage_types.rs — both refuse a malformed row
                  rather than dropping it, and both have a unit test that
                  removes a field to watch the refusal.

**WHAT MAIN HAD.** Nothing of any of it. `docs/SERVER.md` says "The clock
ticks; nothing else does" and describes the player as "a position, a hotbar,
a creative slot, swing, the click surface"; `crate::player::PlayerState` is a
position, a rotation, two collision flags, the sneak key and a teleport
acknowledgement. There was no health field, no food, no experience, no
effect, no `set_health` or `set_experience` on the wire, and `advance_weather`
sat in `crate::gamerules` read by nothing. The recorder had decoded
`set_health`, `set_experience`, `damage_event` and `game_event` for longer
than that — the instruments existed and the things they measure did not.

**WHAT THE TABLES MEASURED** (Threadripper, `~/rustmc-player`, each generator
identical across two runs):

    40 effects, 3 instantaneous, 20 beneficial, 12 with attribute modifiers,
       10 ever reach applyEffectTick
    46 potions, 4 with instant effects, 4 with none, 45 effect instances
    51 damage types (51 keys declared in DamageTypes), 34 tag files,
       35 tags declared in DamageTypeTags, 41 distinct tag-set signatures,
       13 distinct DamageTags, per-field counts [19,1,2,1,0,8,1,3,1,30]

**THE ONE THAT IS A REAL CHECK.** `crates/rustmc-server/tests/
mob_effects_table.rs` compares `crate::effects::TickRule` against a **probe**
of `MobEffect.shouldApplyEffectTickThisTick` — every effect, durations
0..=120, amplifiers 0..=6 plus 32. **38,720 cells compared, 6,920 firing, 7
distinct rules exercised.** Shown able to fail rather than asserted to be:
with `PoisonMobEffect`'s period changed 25 -> 24 it reds naming the cell
(`duration=24, amplifier=0` is false in the jar and true here). Three vacuity
guards stop it passing against an empty or uniform table. This is the only
place in this branch where the Rust is checked against vanilla's own answers
rather than against a second reading of the same `javap` dump by the same
pair of eyes, and it is the pattern the rest should grow toward.

**WHAT I KILLED AS A NULL — do not re-run these.**

* **The second `setAbsorptionAmount(absorption - damage)` at `actuallyHurt`
  offsets 137..144 is not observable through the absorption value.** Probed
  on four `(damage, shield)` pairs: the clamp to `[0, maxAbsorption]` lands
  on the same number as a single-subtraction implementation every time. It is
  transcribed anyway and `ActuallyHurt::absorption_double_subtracted` records
  that it ran, because "unobservable through this field today" is not
  "unobservable".
* **Fall damage cannot be scenarioed on either substrate.** Creative sets
  `abilities.mayfly`, so `Player.causeFallDamage` returns `false` at offset
  11. In survival the harness cannot lift the player: `Step::Walk`'s teleport
  is accepted only because `handleMovePlayer` skips its 0.0625 disagreement
  check for a creative player (offsets 785..837), and the superflat substrate
  has no edge to walk off.
* **Hunger cannot move the wire.** The `causeFoodExhaustion` gate opens in
  survival, but `ServerPlayer.doTick` resends `set_health` only when health,
  the food level, or the saturation-IS-ZERO flag changes — saturation falling
  5.0 -> 4.0 changes none of the three, and reaching a food-level drop needs
  4.0 exhaustion, which is 40 attacks or 800 mined blocks.
* **Effects cannot be applied at all.** `/effect` is permission level 2 and
  the harness players are not operators (`ci/t2-gameplay.sh`); a potion needs
  a `hold`, and `handleSetCreativeModeSlot` refuses the packet outright for a
  non-creative player.

**WHAT CANNOT BE MEASURED, AND THE GAP THAT MAKES IT WORSE.**
`ci/checks/survival` has **no falsifiability ledger and no sweep**:
`ci/corpus-falsifiable.py`'s `DEFAULT_CHECKS` is `ci/checks/gameplay` and its
`DEFAULT_LEDGER` is `ci/checks/gameplay-falsifiable.toml`, and nothing in
`ci/t0.sh` reads a survival one. So none of the five checks in that directory
has ever been shown able to go red, and the one this branch adds
(`005-two-attacks-inside-the-hurt-cooldown-land-once.toml`) **carries no
tier and must not be quoted as tier 2**. The script accepts `--checks` and
`--ledger`, so what is missing is the ledger and the T0 hook, not the tool.

That check is still worth having: `001` grades "an attack on a hurtable
target produces damage", which a server answering *every* attack would pass;
`005` grades the refusal — two identical attacks eight ticks apart, and
vanilla answers one `damage_event` and one `set_health`. It was verified to
load, and verified that the loader can refuse it (`do = "attack"` mistyped
reds at line 82 naming the verb).

**FINDINGS.** Thirteen lines appended to `docs/FINDINGS.md`. The four worth
repeating here:

* `DamageTypeTags` declares 35 `TagKey`s and the jar ships 34 tag files.
  `minecraft:bypasses_cooldown` has none, a `TagKey` with no file is an
  **empty tag**, so `DamageSource.is(BYPASSES_COOLDOWN)` is false for all 51
  types and the conjunct at `hurtServer` offset 196 is dead in 26.2.
  `tests/damage_types_table.rs` pins it so a later version shipping the file
  reds and somebody re-reads the branch.
* The hurt cooldown is a **difference**, not a skip: 4 then 6 within ten
  ticks deals 4 then 2, and the `fullHit` flag clears so the second is silent
  on the wire.
* The `raining` flag leads `isRaining()` by **20** ticks, not the 21 exact
  arithmetic gives, because twenty `f32` additions of 0.01 land on
  0.20000001788139343. A port that counts ticks and multiplies announces rain
  one tick late, every time.
* **`invulnerableTime` is decremented in two places and a `ServerPlayer` gets
  exactly one of them.** `LivingEntity.baseTick` offset 475 skips it for a
  player (`instanceof ServerPlayer; ifne 491`) and `ServerPlayer.tick`
  offsets 21..38 does it instead. Copy `baseTick` alone and a player is
  permanently invulnerable after one hit; copy both and the hurt cooldown is
  five ticks instead of ten. `hurtTime` has no such exclusion.
* `ClientboundSetExperiencePacket`'s constructor is `(progress, total, level)`
  and its **wire** order is progress, **level**, total. Verified against the
  packet's own `write`, not assumed.

**THE SINGLE NEXT ITEM.** *Give `ci/checks/survival` a falsifiability ledger
and a T0 hook.* Run `ci/t2-gameplay-survival.sh` to get a survival reference
and subject, then
`ci/corpus-falsifiable.py --sweep --checks ci/checks/survival --ledger
ci/checks/survival-falsifiable.toml <vanilla.json> <rustmc.json>`, and add
the matching `--check` to `ci/t0.sh` beside the corpus one. **The result that
closes it:** every check in `ci/checks/survival` carries a measured tier, and
a tier-1 or tier-0 survival check reds T0 exactly as a corpus one does. Until
that exists, no damage, health or hunger work on this server can be graded,
and this branch's nine state machines stay unwired because there is nothing
that could tell a correct wiring from a wrong one.
