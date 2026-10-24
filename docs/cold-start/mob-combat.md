# COLD START — branch `mob-combat`

### COLD START — branch `mob-combat`, based on `origin/mob-in-world` (4e83c9d45), 2026-09-05

**A pig that can be hurt, dies, drops its loot through the loot crate and
yields experience.** Fifteen commits on top of `mob-in-world`, of which the
first five are cherry-picks from `origin/player-systems` and the rest are new.

    REUSED     495f637cb combat_rules + javamath, 5a5e619f3 health,
               e067556c9 hurt, 70ec1903e experience, and the health/hurt half
               of 485ae8450 applied as a patch (that commit also moves
               sleep.rs and weather.rs, neither of which is on this base).
               `LivingEntity.hurtServer` is a LivingEntity method —
               `Player.hurtServer` is the OVERRIDE, adding only the
               creative-invulnerability return at offsets 11..32 before
               calling super — so a mob calls the same transcription rather
               than a second copy. `495f637cb` needed one conflict resolved:
               it edits `food.rs`, which is not on this base, and the hunk was
               dropped.
    NEW        `mob/combat.rs` (hurtServer's level half, knockback, die,
               tickDeath, the damage-type registry lookup),
               `mob/drops.rs` (dropAllDeathLoot, dropExperience, the item and
               orb entities), `xporb.rs` (ExperienceOrb.award and the split
               table), `randomsequences.rs` (the named Xoroshiro sequences),
               `loot.rs` (rustmc-loot's FIRST call site in the server), the
               entity parameter set and four opcodes in `crates/rustmc-loot`,
               the rest of `Player.attack` in `attack.rs`, the `hurtMarked`
               limb in `tracker.rs`, and the mob arm of `handleAttack`.

**What a hit, a death and a drop produce on the wire**, and every line of it
is transcribed rather than inferred:

```text
  a hit that lands   damage_event{entity, source=typeId+1, cause=id+1,
                                  direct=id+1, no position}
                     sound entity.<variant>.hurt      at the mob
                     sound entity.player.attack.strong at the attacker
   next sendChanges  set_entity_data{health @9, FLOAT}
                     set_entity_motion                (hurtMarked, 1202)
  the killing hit    the above minus the hurt sound, then
                     entity_event{entity, 3}          (writeInt, not varint)
                     sound entity.<variant>.death
                     add_entity  the porkchop         (dropFromLootTable)
                     add_entity  the orb(s)           (dropExperience)
  twenty ticks on    entity_event{entity, 60}
                     remove_entities{entity}
```

`setPose(Pose.DYING)` at `die` offset 170 rides the same dirty flush as the
health: the pose is `Entity` accessor 6, `mob::data::MobData` already carries
it with `STANDING`'s zero as its initial value, and `Pose.DYING`'s wire value
is its **`id` field** — its own `bipush 7` — because the `POSE` codec is
`idMapper(BY_ID, Pose::id)`. The orb's value rides `Synched::ExperienceOrb`,
accessor 8 with the `INT` serializer.

**Two packets the brief named are not in that list and cannot be.**
`hurt_animation` is referenced by exactly one gameplay class in the jar,
`ServerPlayer`, so no mob ever produces one; and `entity_event 2` is the
pre-1.19.4 generic hurt event that `broadcastDamageEvent` replaced —
`LivingEntity` broadcasts 3, 35, 46, 60 and 67 and no 2 anywhere. Both are in
`docs/FINDINGS.md`.

**Checked against the jar, bytes on the wire:**

* `crates/rustmc-server/tests/mob_combat.rs` — `mob_in_world`'s rig, three
  parts in one function because the entity list and the mob store are
  process-wide. A hit (both `+ 1` conventions on `damage_event`, the health at
  accessor 9 on the **very next** `sendChanges` rather than on the next
  multiple of the pig's `updateInterval` 3, the `set_entity_motion` beside
  it); the invulnerability cooldown (a second equal hit sends nothing, and a
  **larger** one lands for the DIFFERENCE — `10 - 2 - (5-2) = 5`, which is the
  positive control that stops the pair passing for an implementation that
  refuses every second hit); a kill (`entity_event 3` as a four-byte int,
  every `add_entity` after it, the porkchop's before the orb's, nineteen ticks
  of nothing, then the poof before the removal on the twentieth).
* `crates/rustmc-loot/tests/entity_dispatch.rs` — all 94
  `minecraft:entities/*` tables rolled with a full context, graded by a
  **refusal histogram** rather than a count, with a positive control that the
  histogram must not empty (an empty one means a condition started answering
  `false` instead of refusing, which silently deletes drops). Plus the pig's
  own table to the draw: one `nextInt(3)`, `furnace_smelt`'s two
  `entity_properties` terms drawing nothing, and Looting costing a second draw
  only when it is present.
* Unit tests: `xporb` 8, `mob::combat` 7 + 1 registry, `attack::melee_tests` 9,
  `randomsequences` 5, `mob::drops` 3, `loot` 2.

**Scenario: `083-a-pig-hit-once-reports-the-damage` — PARKED in
`ci/checks/gameplay/open/` on 2026-09-06, and the parking is the finding.**
The corpus run recorded `vanilla obs = 0, error = "Broken pipe (os error 32)"`
against `rustmc obs = 186` with a correct-looking `damage_event`. **Vanilla
disconnected the actor on the attack step.** The break is inside this check:
`081` and `082` recorded 44 and 148 observations with no error, and
`090-chest-opens-a-screen` is `fresh_session = true` in this recording and
`false` in the `vanilla.json` of every other tree on the box (nine checked), so
the harness reconnected after `083` killed the socket.
`ServerGamePacketListenerImpl.handleAttack` has exactly one `disconnect` —
`multiplayer.disconnect.invalid_entity_attacked` at offset 161, four cases —
and **which one fired is not known**, because `ci/t2-gameplay.sh` `rm -rf`'s
`$VAN_DIR` on exit and vanilla's console log went with it. The sweep graded the
check **tier 0 with `killed_by = []`**: nothing can move an outcome whose
reference side is an error, so a baseline row taken from that run would have
pinned the crash. The file's own header lists what is already ruled out (the
verb, the rig, `wait` in setup, the settle cap, and target resolution finding
nothing) so nobody re-runs those; the next move is
`RUSTMC_CHECKS=<one file> T2_GAMEPLAY_KEEP=1 ./ci/t2-gameplay.sh` and one grep
of `oracle-work/gameplay/vanilla/logs/latest.log`.

Its design, for whoever un-parks it — it observed
`["damage_event"]` and nothing else. `080` can never `match` because the spawn
yaw and the sound variant are unseeded draws, so any projection naming
`add_entity` or `set_entity_data` carries those along; `damage_event`'s five
fields are three entity ids the recorder normalises, one registry index and an
absent position, and nothing in it is drawn. **The creative substrate is
enough**, unlike `ci/checks/survival/001`: all four gates
`ci/substrate-gate-audit.py` reads are on the PLAYER being hurt, and
`abilities.invulnerable` is a property of the target.

**So this branch adds no corpus scenario**, and every claim it makes about the
wire is graded by `tests/mob_combat.rs` and `tests/entity_dispatch.rs`
instead. That is a weaker instrument than the corpus in one specific way and
no other: it compares this server against the **jar** rather than against a
running vanilla, so a misreading shared by the port and the test survives it.
Said plainly rather than left as a green number.

**Refused, and why** — two scenarios this lane wanted and could not write:

* **a mob DEATH scenario.** Killing a pig takes ten landed hits: base
  `ATTACK_DAMAGE` is 1.0, a pig has 10 health, and `hurtServer` sets
  `invulnerableTime` to 20 and refuses any equal hit while it is above 10 — so
  the hits must be spaced at least ten ticks apart, which is about seven
  seconds of scenario. **A pig strolls** (`WaterAvoidingRandomStrollGoal`), and
  over seven seconds it can leave the `0 + 3.0 + 3.0` reach `handleAttack`
  computes. The check would therefore flap for a reason with nothing to do with
  death. The missing instrument is a way to pin a mob: `/summon` and
  `/data merge entity` are not in this server's command tree (literal nodes
  only — `docs/cold-start/mob-in-world.md`), and a 2-high block pen is six
  `place` steps that a pig's own `jumpIfStuck` may clear. **Not written rather
  than written and left flapping**, because an unstable scenario in the corpus
  costs a baseline row and a floor entry and grades nothing.
* **a drops scenario.** Same problem, plus a second: the drop's own
  `add_entity` carries the `ItemEntity` constructor's unseeded yaw, which
  `ci/checks/gameplay-unstable.toml` already records at **25 flaps in 25
  comparisons** for `dropping-an-item-throws-it-on-the-floor`. An observation
  naming `add_entity` for a drop cannot match whatever else is right.

Both are graded by `tests/mob_combat.rs` instead, which controls the tick and
therefore has neither problem. That is the honest split: the corpus grades what
vanilla and this server can be made to do identically, and the fake client
grades what only one of them can be driven to.

**Join points:**

* `origin/player-systems` — this branch cherry-picks five of its commits. When
  it lands, `git rebase` will find them already applied; the two that could
  conflict are `crates/rustmc-server/src/lib.rs`'s module list (append-only in
  practice) and `Cargo.toml`. `damage_types.rs` was deliberately **not**
  taken: it needs `ci/damage-types-golden.sh`, a `gen-gameplay-tables.sh` row
  and a test-target registration, and this branch resolves the one damage type
  it needs from `registries.bin` instead. When `player-systems` lands, the
  `DamageTags` for `minecraft:player_attack` should come from its table rather
  than from `DamageTags::default()` — `mob::attack_mob` names the line.
* `origin/packets` — the attack packet's decoder is on main already
  (`crate::attack::decode`); nothing here waits on it.
* `useblock` — `crate::loot` is wired for **entities only**. Breaking a block
  on this server still produces no item, and `Loot::block_drops` still has no
  call site.

**Killed as nulls, do not re-run:**

* *"`rustmc-loot` is wired into the server and just needs an entity path"* — it
  was not wired at all. `rustmc-server` did not depend on the crate; the
  17,943-of-18,003 figure is about the interpreter and was reachable from
  nothing a client could do.
* *"an entity loot table draws from `rustmc_loot::roll`'s legacy source"* — it
  draws from a named `XoroshiroRandomSource`. See `randomsequences.rs` and
  `docs/FINDINGS.md`.
* *"`Animal.getExperienceReward` is the `nextInt(3)`"* — it is
  `Animal.getBaseExperienceReward`, and `Mob.getBaseExperienceReward` has a
  **different** `nextInt(3)` that belongs to equipment and never fires for a
  pig.

**Needs hands:**

* **`Player.attackStrengthTicker` is not per-connection**, so every hit from a
  packet is fully charged and the weak/crit/sweep branches are unreachable from
  the wire. `crate::attack`'s unit tests reach all of them.
* **There is no enchantment-tag extract.** `ci/loot-golden.sh` dumps
  `data/minecraft/tags/item/` and the entity loot family also names
  `#minecraft:smelts_loot`, which is in `tags/enchantment/`. It costs nothing
  today — `EnchantmentPredicate` never reaches a tag lookup for a stack with no
  enchantment component, and `crate::item::ItemStack` carries none — and it
  will cost a refusal the day a stack does. `ServerLoot::enchantment_tags`
  supplies an empty map and says so.
* **The first shield changes `Player.attack`'s sound branch.** `hurtOrSimulate`
  returns `!wasBlocked || damage > 0.0`, so a hit blocked entirely down to zero
  plays `PLAYER_ATTACK_NODAMAGE` even though it was not refused. The join tests
  the return value alone, which is right for every input today and is written
  that way so it stays right.
* The difficulty mismatch from `mob-in-world` stands: `ci/t2-gameplay.sh` boots
  this server at `easy` while the corpus's vanilla runs `peaceful`.
