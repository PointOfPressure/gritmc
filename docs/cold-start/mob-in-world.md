# Cold start — branch `mob-in-world`

### COLD START — branch `mob-in-world` off `bd5be4a36`, 2026-09-05

**The first living mobs of vanilla 26.2 stand, move, are seen and despawn
in this server: the pig (goals) and the bat (custom AI), transcribed from
the jar into `crates/rustmc-server/src/mob/` on top of the goal, control,
sensing and navigation system `crates/rustmc-entity` already held.**
Worktree `~/grit/work-mobs`, branch `mob-in-world` on origin, remote dir
`~/rustmc-mobs` on the Threadripper. Not merged — the orchestrator gates.

    HOST       mob/host.rs     ServerMob: Entity/LivingEntity/Mob/AgeableMob/Animal/Bat fields,
                               MobHost for the AI; attributes.rs (AttributeMap, update_attributes),
                               data.rs (SynchedEntityData per chain: pig 21 accessors, bat 17)
    LEVEL      mob/level.rs    MobLevel over Tiles: PathLevel from path_facts.json, BlockGetter.clip
                               (traverseBlocks + VoxelShape/AABB.clip) for line of sight, noCollision,
                               sky darken (the OLD cosine formula; 26.2's is an environment attribute)
    TICK       mob/living.rs   Mob.tick -> LivingEntity.tick -> baseTick chain -> aiStep chain ->
                               serverAiStep -> travel/travelInAir -> Entity.move (+checkFallDamage,
                               step sounds), EntityFluidInteraction.update, pushEntities/Entity.push
    SPAWN      mob/spawn.rs    SpawnEggItem.useOn, EntityType.spawn/create/getYOffset, finalizeSpawn
                               chain (Pig variant + sound variant, AgeableMob, Mob), SummonCommand
    DESPAWN    mob/mod.rs      Mob.checkDespawn before the ticking-range gate; the entity tick loop
                               body for mobs (tick_all), the store, the dirty flush the tracker drains
    KINDS      mob/pig.rs      the nine goals from the table, VariantUtils.selectVariantToSpawn,
               mob/bat.rs      Bat.tick/customServerAiStep/resting/ambient
    TABLES     path_facts.json (PathFactsGolden.java, JDK), mob_variants.json (unzip + python),
               protocol_ids.json + `attributes`, attributes.json's `syncable` now read

**Checked against the jar, bytes on the wire:**

* `crates/rustmc-server/tests/mob_in_world.rs` — a flat tile, a fake
  client: pairing bundle = `add_entity` + `set_entity_data{health 10 @9}`
  and no `update_attributes`; `update_attributes{movement_speed 0.25, id
  26}` on the tick after; the standing pig at the surface with resting dy
  `(0 - 0.08) * 0.98f`; a bat 150 blocks from the only player discarded on
  its first tick, the pig beside it kept; a summoned pig keeps the
  constructor yaw in `[0, 2π)`. Positive control: attribute id 26→25 reds
  it. 3.7 s on the box.
* Scenarios: `081-a-spawned-pig-syncs-its-movement-speed-a-tick-later`
  (`update_attributes`), `082-a-bat-left-far-away-despawns-and-a-pig-does-not`
  (`add_entity`/`remove_entities` on the return walk). Tier and verdict:
  see the `MEASURED` block below once the corpus run has been taken.

**Corrected from the tree** (`docs/FINDINGS.md`, 2026-09-05): no
`AgeableMob` age draw on the egg path (group size 0); the follow-range
bonus is `triangle(0, 0.11485)` not `0.11228`; `updateSkyBrightness` is
`15 - SKY_LIGHT_LEVEL` from `timeline/day.json`, not a cosine of the time.

**Deferred, named at the line** (`mob/living.rs` module docs): damage
(`hurtServer` from suffocation/drowning/freezing/fall/cramming —
`player-systems`' join point; the air-supply decrement and its synced
value ARE applied), `applyEffectsFromBlocks`, fluid currents
(`FlowingFluid.getFlow` — `travelInFluid`, `travelInWater`/`InLava`,
`jumpOutOfFluid`, `isFree` and `doWaterSplashEffect`'s sound and draws
ARE ported), the light engine (`canSeeSky` and the pathfinding light cost
from `WORLD_SURFACE`), landing particles. `/summon` as a chat command: `commands.rs`'s tree
has literal nodes only; `mob::summon` is the call and is tested. Player
push: a pushed player's server-side delta accumulates here where vanilla
decays it by 0.98 a tick in `aiStep` — unobservable (players do not
`trackDeltas` and `add_entity.movement` is exempt).

**Refused:** the villager. 40 of 72 brain behaviours are pending
(`tests/brain_tables.rs::PENDING`); a villager here would be a guess at
its idle. It waits for `mobs-brains-2`, then a `Brain::make` from the
tables over this host.

**Join points:** `items` — `SpawnEggItem.getType` reads the egg's
`ENTITY_DATA` component; this branch maps the item name
(`<type>_spawn_egg`) and `mob::kind_for_spawn_egg` is where
`item_props.json` plugs in. `player-systems` — every `hurtServer` site in
`living.rs` is a comment naming the offsets.

**Nulls:** none killed by measurement on this branch yet.

**Needs hands:** `ci/t2-gameplay.sh` boots this server with vanilla's
default difficulty (`easy`) while the corpus's vanilla runs `peaceful`;
`--difficulty` exists and the launch line is `stack-gameplay`'s. Nothing
this lane grades depends on it (both kinds spawn in peaceful).

### COLD START — branch `mob-in-world` rebased onto `58f1fa5c6`, 2026-09-05

**Landing pass, not a porting pass.** Nothing about the server moved. What
moved is the branch's base, the two ratcheted constants its own corpus run
falsified, and one baseline row the gate asked for by name.

    REBASE     8 mob commits replayed onto main at 58f1fa5c6 (2,743). The only
               conflict is `docs/FINDINGS.md`, add/add, both sides kept — main's
               stack-extract appends above, this branch's mob appends below.
               `git range-diff` over the two ranges shows one content change,
               in `ci/gen-gameplay-tables.sh`, where main renamed the command
               generator underneath the two rows this branch adds.
    EVIDENCE   `ci/corpus-falsifiable.py --sweep` RE-RUN on the rebased tree
               with `oracle-runner` rebuilt (proj:1277e43a7e157183), against
               the mobs-corpus recording pair in `~/rustmc-mobs`. 1759
               comparator runs. It reproduces the committed
               `ci/checks/gameplay-falsifiable.toml` BYTE FOR BYTE, so the
               ledger the mobs-corpus chain wrote pre-rebase is valid post-.
    TIERS      081 and 082 are both **tier 2**, localised, 110/110 swept, 0 not
               in the recording. Neither needed a refusal row.
                 081 killed_by drop-measured, retype, reorder, drop-all,
                     perturb:entity, perturb:rest
                 082 killed_by drop-measured, retype, inject, drop-all,
                     perturb:entities, perturb:rest
    CONSTANTS  `DEAD_FIELDS` 4 -> 0 and `FIELD_UNIVERSE` 90 -> 95 in
               `ci/corpus-falsifiable.py` — the whole of the T0 red that held
               this branch. **Read the commit before reading the constant**:
               the dead set emptied because the sweep lost resolving power over
               those four fields, not because a check gained it.
    BASELINE   `spawn-egg-makes-a-mob` absent -> diverge, which the corpus run
               printed as `BETTER ... update gameplay-baseline.toml to lock
               these in`. 6 reference observations against 6, no missing or
               extra kinds, and the entire difference is the spawn yaw
               (`y_rot`/`y_head_rot` -17 against -78). It is in
               `gameplay-unstable.toml`, so `diverge` is its ceiling.

**Verdicts as measured** (corpus 110, declared 100/110 match, 9 diverge,
1 absent, 0 vacuous, 0 mute, 0 inert, 0 error; `~/grit/logs/mobs-world-t2.log`):
`081` **match**, `082` **match**, both strict-diverge, both already baselined at
`match` by the porting pass and both confirmed rather than corrected. `080`
diverge, as above.

**These rows survive `origin/corpus-instruments` (d055a470) landing.** Its
`--check` re-derives `frozen`/`unresolved` from `history_n` in both directions,
so 081 and 082 at `frozen, history_n = 1` reclassify to `unresolved` with no
re-take; an `unresolved` row must carry tier 2 in the falsifiable ledger and
both do; 2 is under `HISTORY_UNRESOLVED_CEILING` = 14, and the frozen count
falls 60 -> 58 under a ceiling of 61. Its `Corpus` refactor moves
`DEAD_FIELDS`/`FIELD_UNIVERSE` into a record built at import time — which is
named in the selftest comment, because the repin there will stop taking and
the case is written to go RED when it does.

**Killed as nulls, do not re-run:**
* *"the sweep evidence for 081/082 is missing"* — it is not, and was not. The
  T0 red named `DEAD-FIELDS` and `FIELD-UNIVERSE` only; both scenarios were
  already in the ledger at tier 2. Read the three refusal lines, not the
  remedy paragraph underneath them, which is printed once for every refusal.
* *"main's oracle-runner change invalidates the recordings"* — it does not.
  `declared_commands` is `#[serde(default, skip_serializing_if)]`, the decoder
  is recorder-only, and the re-sweep is byte-identical. It DOES move the
  projection identity; see `docs/FINDINGS.md`.

**Needs hands** (neither is this branch's to fix):
* `ci/check-figures.py`'s `PROVENANCE-NO-COMMIT` rule cannot fire in the gate —
  `.git` is excluded from the rsync. Run locally, `main` is red on three
  figures. `docs/FINDINGS.md`, 2026-09-05.
* The difficulty mismatch from the previous entry stands: `ci/t2-gameplay.sh`
  boots this server at `easy` while the corpus's vanilla runs `peaceful`.

### COLD START — branch `mob-in-world` rebased onto `1d109abcf`, 2026-09-05

**A second landing pass, and nothing about the server moved on this one
either.** 16 commits replayed onto main at 1d109abcf (2,788). What moved is
two instruments on main firing on this branch's own content — one of them a
positive control this branch wrote for exactly this event.

    REBASE     16/16 replayed. ONE textual conflict, `docs/FINDINGS.md`, add/add,
               both sides kept in the file's own order (main's appends above,
               this branch's below). `git range-diff` over the two ranges shows
               that commit as the only one whose content changed, and the change
               is the union. Everything the brief expected to conflict —
               `docs/WORKLIST.md`, `ci/checks/test-targets.toml`,
               `ci/checks/required-checks.toml`, `ci/liveness-proofs.py`,
               `ci/figures.toml` — merged without a hunk.
    CONTROL    `ci/corpus-falsifiable.py --selftest` went RED on
               `the repinned dead set is the one `check` reads`. That case's own
               comment names `origin/corpus-instruments` and says the control
               "is what will say so" when the pins move into a `Corpus` record
               built at import time. They have; `check` reads
               `corpus_for(...).dead_fields`; repinning the module global alone
               left `check` comparing the ledger against the real (empty) set.
               Repaired by repinning `GAMEPLAY.dead_fields` — the record the
               shadowed `check` in that scope is passed — alongside the global,
               both restored in `finally`, plus a cheaper first `want` that
               fails on a repin that never reaches the record. Shown red again
               with the injection removed: two cases fail.
    LEDGER     `ci/mob-observability.py --check` (main, added after this branch
               was written) REFUSES a mob-bearing scenario with no row in
               `ci/checks/mob-blind.toml`, and `081`/`082` are two. `--ledger`
               re-measured over the box's recordings: 3 mob-bearing checks.
               `081` runs=2 compared=[2,2] emitted=[193,234];
               `082` runs=2 compared=[5,6] emitted=[154,270].
               `080-spawn-egg-makes-a-mob` moves runs 17 -> 25, `subject_obs`
               [0] -> [0, 6] and `declared_verdicts` absent=21 -> absent=28,
               diverge=2. **That movement is this branch**: the instrument was
               built to say the check cannot see a mob, and it now records the
               subject announcing one.

**Killed as nulls, do not re-run:**
* *"the branch's own ledgers went stale under main"* — they did not.
  `ci/corpus-falsifiable.py --check` 110/110, `ci/corpus-grades.py --check`
  110 checks / 58 frozen / 2 unresolved over 29 recordings,
  `ci/liveness-proofs.py --check` 10/10, `ci/check-checks-present.py` 15/15,
  `ci/check-figures.py` GREEN, all in this worktree post-rebase.
* *"`figure-provenance` is red because of the rebase"* — it is not. See
  **Needs hands**.

**Needs hands** (neither is this branch's):
* `ci/figure-provenance.py --check` is red on **56 of 56** figures in a
  worktree of `origin/main`, and identically red in an untouched worktree of
  `origin/redstone`, so it is a property of the repository. The previous entry
  in this file recorded it as three. `ci/t0.sh:138` exits 1 on it, and the
  only reason no gate is red is that the rsync excludes `.git` and the check
  prints "not inside a git repository" and exits 0.
* The difficulty mismatch from the two entries above still stands.
