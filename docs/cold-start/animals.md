# Cold start — branch `animals`

---

### COLD START — branch `animals` off `origin/main` (`d48904cb5`, 3,973), 2026-10-01

Worktree `~/grit/work-animals` (detached; push with
`git push origin HEAD:refs/heads/animals`), remote dirs `~/rustmc-animals`
(recordings) and `~/rustmc-animals2` (builds, T0). Recording worktree
`~/grit/work-animals-rec` is a detached copy the record script syncs from, so
the main worktree can be edited while a subset waits on the corpus lock.

#### What the server could not do, and now does

`ServerboundInteractPacket` (26) had no handler and the harness no verb, so
every animal mechanic behind `handleInteract` was untestable, not untested.
Ported from the 26.2 jar (offsets in each module's header):

* **The path**: `handleInteract` 12-220 (reach, the sneak bit, the
  `SUCCESS_SERVER` swing to trackers and self), `Mob.interact` 0-92,
  `checkAndHandleImportantInteractions` (spawn egg on a mob), `Entity.interact`'s
  lead branches, `Pig.mobInteract`, `Animal.mobInteract`,
  `AgeableMob.mobInteract` — `crate::mob::interact`.
* **Love and breeding**: `setInLove` (600 ticks, entity event 18),
  `canMate`, `spawnChildFromBreeding` / `finalizeSpawnChildFromBreeding`
  (baby at -24000, both parents 6000, `resetLove`, the breeder's hearts, the
  `MOB_DROPS` orb with the breeder's `nextInt(7)+1`), `Pig.getBreedOffspring`
  (one `nextBoolean` on the parent) — `crate::mob::breed`. `BreedGoal` was
  already in `rustmc-entity`; the host now answers it.
* **Babies**: `SpawnEggItem.spawnOffspringFromSpawnEgg` (a bat egg on a bat
  still burns an entity id), feeding a baby (`ageUp(getSpeedUpSecondsWhenFeeding
  (-age), true)`), the golden dandelion (`AGE_LOCKED`, accessor 17, toggled,
  `setAge(-24000)`, the 40-tick cooldown — which counts down on the server,
  contrary to what `crate::mob::living` said), `Pig.playEatingSound`
  (`eat`/`baby_eat` added to `mob_variants.json`). Aging itself
  (`AgeableMob.aiStep`) was already ported.
* **Leads**: `setLeashedTo`, `dropLeash`/`removeLeash` (the lead
  `ItemEntity`), `tickLeash` at the end of `Entity.baseTick` with the 12-block
  snap, the elastic band (`computeElasticInteraction` and friends) and
  `PathfinderMob.closeRangeLeashBehaviour`; `set_entity_link` (100) on the
  wire, in the pairing bundle and in the recorder's decoder —
  `crate::mob::leash`.
* `EXPERIENCE_ORB` is an `EntityKind` (`DATA_VALUE`, accessor 8); the orb is
  spawned, never ticked.

#### The checks, `091`-`099` and `101`

All on `083`'s pinned pit, far out on z (±240, ±320, ±400), observing only
what vanilla cannot vary: no `sound` (sound variants and ambient sounds are
unseeded), no `add_entity` (`LivingEntity.<init>`'s `yHeadRot`), and the
breeding checks grade the baby's `set_entity_data` alone, because which
parent breeds is the `(tickCount + id) % 2` goal phase's coin.

VERDICTS: see the entry below this one once recorded.

#### Refused, by name — do not re-open without a new instrument

* **Taming (wolf, cat, parrot, horse family).** None of the kinds exists on
  main, and every taming roll is drawn from the animal's unseeded random:
  `Wolf.tryToTame` 0-10 `random.nextInt(3)`, then entity event 7 or 6
  (offsets 40, 53); `Cat`, `Parrot` and `AbstractHorse` the same shape. And a
  wolf's every `set_entity_data` carries its sound variant, which
  `Wolf.finalizeSpawn` 86-94 draws from `level.getRandom()`. So no corpus
  observation of taming can agree with itself, and porting the four kinds
  would land behaviour nothing could grade. This is a refusal on
  verifiability, not on size.
* **The breeding orb on the wire.** Its `add_entity` yaw and its pairing
  `DATA_VALUE` are both unseeded draws, and its tick (gravity,
  `followNearbyPlayer`, pickup and the experience it awards) is not ported.
  Every breeding check turns `mob_drops` off. Grading it needs a projection
  that drops `add_entity.y_rot` for `minecraft:experience_orb` the way
  `compare::exempt_item_spawn_yaw` does for items — a recorder change that
  moves other lanes' checks, so not this branch's.
* Saddles and riding, shears on a lead, leash knots, named name tags.

### STATE at the HALT, 2026-10-01

Code, tests and 10 checks pushed; unit + mob_in_world green in `~/rustmc-animals2`. Keep-going T0 at 64b63b508: only reds are the new checks' ledgers (corpus-falsifiable, corpus-grades, mob-blind, order-blind, entity-id-allocator; mob-surface since re-pinned) plus a t1-par timing flake.
Next: the subset recording of 091-101 (orphaned run pid 2006241 in `~/rustmc-animals`, copied to `~/rustmc-animals-rec2/run1` by job `anim-follow`), then splice it into the gate's pair (`~/rustmc-stack/oracle-work/gameplay`, read-only copy) and sweep/register those five ledgers, then T0 in `~/rustmc-animals`.
No red-before recording exists yet (the base run timed out in the lock queue); main has no interact handler, so the red-before verdict is `absent`.

### STATE at the PAUSE, 2026-10-01 (tip after this commit)

Rebased onto main 4024. `101` re-rigged (player between the pits; its far pit was out of vanilla's block reach — FINDINGS). Allocator re-pinned for the orb.
Detached jobs left running (they finish on their own; logs in `~/.rustmc-detached/anim-rec8.log`, `anim-chain.log`):
* `anim-rec8` — subset recording of 091-099/101 on this tree (`~/grit/work-animals-rec`, synced to `~/rustmc-animals`), then red-before on main's `crates/rustmc-server`, then vanilla-vs-vanilla. Output: `~/rustmc-animals-rec3/{branch,base}`, `branch.log`, `base.log`, `selfagree.log`.
* `anim-chain` — waits on `anim-rec8`, then `ledger.sh` on a splice in `~/rustmc-animals2` (falsifiable sweep, history+grades, order-blind population +this lane's vanilla recordings, mob-blind rows for the ten, `repin.py` sets FIELD_UNIVERSE 129->140 and MAX_UNASKABLE 14->15 with reasons), baseline rows, a verdicts entry here, WORKLIST index row, commit+push, then keep-going T0 in `~/rustmc-animals2` (log `chain-t0.log`). Scripts: `/tmp/claude-1000/-/befd9b5b-5163-468a-9d93-210a7ac9ac95/scratchpad/animals/`. Dry run over the old run1 recordings: all six ledger checks green.
Next on restart: `./ci/run-detached.sh --status anim-chain`. If DONE rc=0: read `T0-EXIT` and the reds in that log. If it stopped early: its last line says why (no recording / broken verdict / ledger check red); re-run `chain.sh` (edit the wait line if `anim-rec8` was killed and relaunch `record.sh`).
