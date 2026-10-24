# Cold start — branch `train-f`

### COLD START — `train-f` off `origin/main` 4033 (`3a74284a5`), 2026-10-01

Landing train for four lanes, gated once: `animals` (`75f4183b7`, 14
commits), `passive-mobs` (`036bd95af`; its first eleven commits are
`animals`' replayed and the twelfth is main's `0ae84efee`, so 12 of its 24
land), `xp-orbs` (`8124718b0`, 15) and `item-pickup` (`9d9dacceb`, 10).
Worktree `~/grit/work-train-f` (detached; push with
`git push origin HEAD:refs/heads/train-f`), remote dir `~/rustmc-train-f`.
Scripts in the session scratchpad `trainf/`.

#### Integration, beyond the cherry-picks

* Conflicts: `mob_in_world.rs` (call order kept as `passive-mobs` ran it
  green), `entities.rs` / `synched.rs` (one `ExperienceOrb` variant, xp-orbs';
  passive-mobs' `Stack` beside it), `tracker.rs` (the player's score, the orb,
  then the mob-or-item dirty read), `world_send.rs` (the furnace take and the
  new `spawn_and_announce_thrown_item` signature).
* The breeding orb is `crate::xporb`'s (`spawn_with_direction`), so it ticks;
  `mob::breed::add_experience_orb` is gone.
* A mob's drops and a dropped lead register in `itementity::ItemEntities`
  (pickup delay 10), so item-pickup's tick and touch reach them.
* `ci/checks/mob-surface.json`: `our_entity_kinds` 6 -> 11, `our_mob_kinds`
  2 -> 6.
* `683` gets `within = 4`: vanilla tells the actor the observer's
  `entity_position` (`[0, 0, 5]`) in the window and this server does not.
  Re-compared offline on `xp-r3`'s pair, declared and strict `match`.

#### Parked, not dropped: `docs/cold-start/train-f-parked/`

Each one cannot be graded as written, by a recording, not by argument.

* `096-two-pigs-in-love-make-a-baby`: `error` on vanilla in both runs of
  `anim-rec8` (`~/rustmc-animals-rec3/branch.log:62`, `selfagree.log`): the
  interact names a pig the client was never told of. `099` (two blocks apart)
  and `101` (four apart, never) grade breeding and stay.
* `641`, `646`, `654`: `vacuous` in `pm-rec3`'s second subset
  (`~/rustmc-passive/oracle-work/gameplay/pm-subset-2.log`, 13/16 match, 3
  vacuous). Each is a silent second click whose setup carries the only
  traffic, so vanilla's projected window is empty. To revive one, move the
  first click into `[[step]]` so the window holds it, and re-record.
* `994`, `998`: `inert` in `pickup-chain3` (`~/pickup-rec/fixed-2/subset.log`):
  a refused placement sends nothing beyond what the test forces on either
  side.

#### Verdicts and ledgers

From the lanes' own subset recordings, compared together on a splice of the
gate pair (`oracle-runner compare`, 210 scenarios): of the 31 live lane checks
29 `match` and 2 `diverge` (993, 996: vanilla sends the pickup's hotbar
`container_set_slot` before `remove_entities`, this server after). All 210
tier 2 in `gameplay-falsifiable.toml`; `FIELD_UNIVERSE` 129 -> 149,
`MAX_UNASKABLE` 14 -> 11 (reasons at the pins); `REQUIRED_FLOOR` stays 23.
The 31 are in `gameplay-baseline.toml` at those verdicts.

Keep-going T0 before the last fix: one red stage,
`ci/mob-tick-census-falsify.sh` case 6 (it added a cow, which passive-mobs
already declares); fixed to an armadillo.

Not done here: a full corpus run (by brief). The gate's own recording is
the first time these 31 run beside main's 179 in one session.
