# Cold start — branch `entity-ports-pvp`

---

### COLD START — branch `entity-ports-pvp` off `origin/damage` (`7910a942e`), 2026-09-30

Worktree `~/grit/work-entity-pvp`, remote dir `~/rustmc-entpvp`. The
player-attacks-player half of `entity-ports`. Off `damage` because the
target's hurt path (`LivePlayer`, the level half, the survival rig) is only
there; it carries train-b's `crates/rustmc-server/src/attack.rs`
byte-identical (plus the one fix `entity-ports` 2a8187bef also makes) so it
merges with train-b without touching that file.

#### Measured — `ci/t2-gameplay-survival.sh`, `~/rustmc-entpvp`, vanilla 26.2

| pass | tip | declared | strict | 001 | 005 |
|---|---|---|---|---|---|
| `damage`'s own, 2026-09-30 16:21 | `6328a25cc` | 10/15 | 8/15 | absent | absent |
| p1, 21:09 | `2bd095ad4` | 12/15 | 8/15 | match / strict diverge | match / strict diverge |
| p3, 23:05 | `f3fdc4139` | **12/15** | **10/15** | **match / match** | **match / match** |

No check moved backwards against `damage`'s own pass; the three non-matches
(002 inert, 003 vacuous, 004 diverge) are mining and older than both branches.

#### What is ported, from the jar

* `crate::pvp`: the attacker keeps `Player.attackStrengthTicker`
  (`Player.tick` 230-288, `ServerPlayer.swing` 2-6, `onAttack` at `attack`
  76), advanced to the game clock before a swing or attack is handled — a
  packet read at `T` runs after tick `T` in vanilla. The attacker computes
  `crate::attack::melee` and hands the rest of `Player.attack` to the target
  as a `PendingAttack` due at `T + 1`, which the target's loop runs before
  its `doTick`: the sprint-knockback sound, `ServerPlayer.hurtServer` with
  `player_attack` through the level half, `causeExtraKnockback` including the
  `ServerPlayer` arm (own velocity packet, mark cleared, velocity restored),
  the strong/weak/crit sounds, and the attacker's 0.1 exhaustion sent back.
* `crate::combat_tracker`: `recordDamage` from `actuallyHurt` 111-124,
  `recheckStatus`'s 300/100-tick limits every 20 ticks,
  `player_combat_enter` / `player_combat_end` (ids 67 / 66). This also
  reaches `/damage ... by <entity>`.

#### Measured causes, each closed

* p1's 001 health 19.728 against 19.792: our ticker read 1, not 0 — tick
  `T`'s increment landed between the swing's reset and the attack.
* p1's missing `packet:0x43` to the target: `player_combat_enter`.

#### Not ported, by name

Sprinting (no `ServerboundPlayerCommand` handling on this base), the sweep
(no sword attributes), `DAMAGE_INDICATOR` particles (unreachable under 2.0
dealt), enchantment hooks, `LivingEntity.die`'s `recheckStatus` (a death
inside combat sends no `player_combat_end` here).

#### The single next item

Rebase onto main once train-b and `damage` land (`attack.rs` identical on
both). Closing result: 001 and 005 strict match on main.

#### T0

**GREEN** at `d66e58d2c` (`~/rustmc-entpvp`, `rsync --checksum` -> touch ->
`ci/gen-gameplay-tables.sh` x2 -> plain `ci/t0.sh`, 2026-09-30). One red on
the way: two clippy lints in the first commit.
