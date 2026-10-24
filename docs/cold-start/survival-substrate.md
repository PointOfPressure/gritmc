# COLD START — branch `survival-substrate`

Append-only. Newest entry last.

### COLD START — branch `survival-substrate`, 2026-08-18, what creative made ungradeable

Worktree `~/grit/work-surv`. **The audit half is tree-only and reads recordings
that already existed. One recording is taken, at the end, through
`ci/run-detached.sh`.** Read `docs/PARITY-ROADMAP.md` §63 for the argument;
this file is the resumable state.

## THE ONE THING TO TAKE AWAY

**A kind no recording has ever carried is not evidence of a missing port.** It
may be evidence that *vanilla* never sent it either, because the corpus
substrate takes an early return above the send. `damage_event` and `set_health`
sat in the roadmap as unported work items on exactly that confusion.

That is now an instrument rather than an anecdote: `ci/substrate-gate-audit.py`
re-reads seven gates out of the pinned jar and refuses if any has moved.
`ci/substrate-gate-audit-falsify.sh` is the 10-case falsifier; cases 1-4, 6 and
9 were each seen failing before they passed.

## THE SUBSTRATE IS THREE LEVERS AND EVERYBODY NAMES ONE

| lever | set by | what it gates |
|---|---|---|
| `gamemode=creative` | `VANILLA_EXTRA_PROPS` | `Abilities.invulnerable` **and** `instabuild` |
| `difficulty=peaceful` | `VANILLA_EXTRA_PROPS` | difficulty-scaled damage; hunger depletion; **auto-heal** |
| four game rules | console, per corpus run | weather, mob spawning, random ticks, time |

The second lever was named nowhere before this branch and it sits **inside the
same method** as the first. `Player.hurtServer`:

```text
   15: getfield Abilities.invulnerable      ifeq 33   -> 31: iconst_0; 32: ireturn
   52: DamageSource.scalesWithDifficulty()  ifeq 108
   62: getstatic Difficulty.PEACEFUL        if_acmpne 70 -> 68: fconst_0; fstore_3
  108: fload_3; fconst_0; fcmpl             ifne 116  -> 114: iconst_0; 115: ireturn
```

So a lane that flips `gamemode` alone and finds a mechanism still dark has no
way to know why. **It matters less than it looks, and that is measured rather
than assumed**: `scalesWithDifficulty()` is
`DamageScaling.WHEN_CAUSED_BY_LIVING_NON_PLAYER` for 47 of the jar's 51 damage
types, and its causer test excludes players — so for `player_attack`, whose
causer IS a player, the peaceful branch is skipped entirely. Only 4 types are
`always`: `explosion`, `player_explosion`, `bad_respawn_point`, `sonic_boom`.

**`difficulty=peaceful` therefore stays**, and the survival pass changes exactly
one property. What peaceful *would* have broken is the third row above:
`ServerPlayer.tickRegeneration` offsets 7-32 heal a survival player on peaceful
on a `tickCount % 20` timer, which would make every `set_health` a coin toss.
`gamerule natural_health_regeneration false` shuts that **and** the food-based
regen in `FoodData.tick`, which is why one rule buys the whole thing.

## THE MEASUREMENT: 7 OF 36 KINDS, AND ONLY 5 ARE THE SUBSTRATE'S

Over **N = 87 distinct vanilla recordings** on this box, deduped by sha256 from
97 files. The tool prints N every run because the box's recording set is live —
an earlier pass the same morning read 88.

```text
  kinds with a decoder arm                    36
  never carried by any recording               7
    substrate-attributed                       5
    NOT substrate (named, so they are not swept in)   2
```

| kind | class | lever | why |
|---|---|---|---|
| `damage_event` | substrate | creative | `Player.hurtServer` 15 -> 32 |
| `set_health` | substrate | creative | nothing changes health or food; the send in `ServerPlayer.doTick` is an unconditional change-detector with no gate of its own |
| `block_destruction` | substrate | creative | `handleBlockBreakAction` 178 -> 194, above where `destroyPos` is assigned |
| `set_experience` | substrate | creative | `destroyBlock` 193 -> 200, no drops and no orbs; superflat also carries no ore, so this row is **over-determined** |
| `game_event` | both | creative | weather is shut by a game rule; the other reachable producer is the `CHANGE_GAME_MODE` event, which a corpus that never changes mode cannot provoke |
| `player_position` | **unasked** | — | a resync; no scenario provokes one. NOT substrate |
| `set_default_spawn_position` | **unasked** | — | sent during login, before the recorder attaches. NOT substrate |

Reading recordings **without** the projection is deliberate and sound here only
because every count is a presence census of one server at a time, never a
comparison of two streams — the same basis as `ci/entity-reach.py`.

## TWO CHECKS OF 108 CARRY A DEAD DECLARATION

`550` and `551`, each declaring `damage_event` and `set_health` among four
kinds. Half of each projection has been carrying no signal, so a green on
either was never evidence that damage is modelled.

Not merged with §53's reading, which is scenario-scoped and names a third kind:
`entity_event` is emitted 73 times across 73 recordings, so it is dark *for
those two scenarios* for a scenario reason, not a substrate one. Only
`damage_event` and `set_health` are dark everywhere.

A dead declaration is **not automatically a defect** —
`ci/checks/gameplay/README.md` requires `section_blocks_update` beside
`block_update` as insurance — and it is **not a dead check**: both are decided
by the kinds they named that do arrive. Read them as narrower than their
`observe` list claims.

## THE TWO HARNESS VERBS AT ZERO ARE DIFFERENT KINDS OF ZERO

`ci/corpus-coverage.py` reports two scenario actions exercised by nothing, and
they read identically there:

```text
  finish_dig                   0  (declared but exercised by nothing)
  use_item                     0  (declared but exercised by nothing)
```

| verb | class | why |
|---|---|---|
| `finish_dig` | **substrate** | `handleBlockBreakAction` returns on `instabuild` at 194, above where `destroyPos` is assigned at 439, so a `STOP` can never match. Needed a game mode. |
| `use_item` | **coverage** | no **creative-lever** gate in `useItem`'s 95 instructions. Needs a scenario, and it is not this lane's. |

**The first version of that row said "no ability gate anywhere" and was
wrong.** `ServerPlayerGameMode.useItem` opens with

```text
    0: getfield  gameModeForPlayer:GameType
    4: getstatic GameType.SPECTATOR
    7: if_acmpne 14
   10: getstatic InteractionResult.PASS
   13: areturn
```

a game-mode gate on the **spectator** lever. The conclusion survives — the
creative substrate is not what keeps `use_item` at zero — but the stated reason
was false, which is the very defect this audit exists to separate: naming a
lever that does not move the thing being looked at. It is the same lever as
`mayBuild`, and this server refuses to serve either mode.

`use_item`'s row is a **verified negative**, which is the direction that rots
silently: the zero means "coverage gap" only while `useItem` carries no
creative-lever gate. The check was negative against the single string
`Abilities`, so a gate spelled `isCreative()` would not have matched and the
row would have stayed green while becoming false. It is now negative against
**five spellings**, each paired with a method known to contain it, and the
witnesses are verified *before* the rows that depend on them — a needle that
stops matching is `NEEDLE DEAD`, not a silent pass.

`GameType` is deliberately **not** a needle, because `useItem`'s spectator gate
reads it and including it would false-positive. Falsifier case 13 asserts that
false positive really would occur, so the omission is load-bearing rather than
stylistic; cases 11a-c point the row at three methods gating three different
spellings; case 12 kills a needle; case 10 does the opposite for `finish_dig`.

## THE CROSS-CHECK, AND IT DEFLATES THE OBVIOUS HYPOTHESIS

`ci/unported-inventory.py --open` on `origin/server-unported-audit`: 30 open
rows, 20 with `reach = none`. The tempting conclusion is that the substrate is
hiding a large part of that inventory. **It is not.**

* **exactly one** of the 20 is blocked by the game-mode lever: `damage-path`,
  whose own text names `causeFoodExhaustion` and `damageStatsAndHearts`, both
  behind `invulnerable`;
* `abilities-model` and `maybuild-usewithoutitem` are blocked by a **different**
  lever — the absence of a per-player ability model — and `--gamemode survival`
  does not touch either;
* the remaining 17 are ordinary porting or coverage gaps and this branch says
  nothing about them.

**`mayBuild` is the trap.** It reads like a creative flag and is not one:
`GameType.updatePlayerAbilities` assigns it *conditionally* rather than per-arm,
and it is true in survival and creative alike, false only for adventure and
spectator. The tool prints the matrix every run for that reason.

So the substrate's cost is **observability**, not unported surface: it blocks
packet *kinds*, and the two populations overlap in one marker.

## WHAT THE SUBSTRATE COST TO BUILD

Smaller than expected, because whoever wrote the attack path declined to bake in
the constant.

* `status::GAME_TYPE` was a `pub const` with **two** consumers. It is now an
  `AtomicU8` behind `game_type()` / `set_game_type()`, settled in `main` before
  the listener opens.
* `--gamemode <creative|survival>`, **rejected** on a bad name rather than
  ignored like the flags above it — a misspelling that fell back to creative
  would make a survival pass record a creative server and report the whole
  difference as this server's divergence.
* The session already carried `creative` as a *field* and already branched on it
  in both places that matter (`START` vs `STOP` destroy; `Target::invulnerable`).
  `attack.rs`'s header had already written down that `Outcome::Hurt` becomes
  reachable with "the first survival player". It was right.
* `ci/t2-gameplay-survival.sh` + `ci/checks/survival/` — four scenarios.

## WHY A SECOND PASS AND NOT A FLAG ON THE CORPUS

`Step::Hold` sends `set_creative_mode_slot`, and
`handleSetCreativeModeSlot` refuses the whole packet when `!player.isCreative()`
— **silently, no error, nothing in the log**. So a `hold` in a survival scenario
does not fail, it does *nothing*, and the scenario runs with an empty hand and
reports a rig failure as a divergence. Flipping the corpus would not measure
more; it would break all 115 checks in it.

`ci/t2-gameplay-survival.sh` **refuses** a survival scenario containing a `hold`
for that reason. It is the most expensive mistake available in that directory
and the only one invisible in the output.

The same fact is why the game mode is **server-wide and not per-player**. A
per-player mode needs a console, argument nodes in the command tree
(`crate::commands` has literals only) and the ability model the unported
inventory still lists as a GAP — all three before one damage packet could be
compared.

## THE PASS ASSERTS ITS OWN SUBSTRATE

If vanilla emitted none of `damage_event`, `set_health`, `block_destruction`,
`set_experience`, the run **refuses**. A game mode that did not take produces a
recording that looks exactly like a green, and that is the one failure a
substrate change can have that nothing downstream would catch.

## WHAT IS DELIBERATELY NOT DONE

* **No baseline row was written from the first run.**
  `ci/checks/gameplay-baseline.toml` is a ratchet and a row written from one run
  locks a state in at N = 1, which is how a future false regression is
  manufactured. The first run reports; a row goes in with its N beside it and
  only for a verdict a second run reproduced.
* **The corpus floor is untouched, and the survival pass has NO floor of its
  own.** `ci/checks/gameplay-floor.toml` reads `floor = 4` over
  `scenarios = 92`, measured over `ci/checks/gameplay`. These four scenarios
  live in `ci/checks/survival`, so that denominator does not move and this
  branch cannot disturb the standing floor — that is checkable rather than
  asserted: the corpus directory is unchanged by this branch.

  What follows from that is the *opposite* of reassuring. The survival pass has
  never been run vanilla-against-vanilla, so **the reproducibility of its four
  scenarios is unknown**, and `003` is the one to worry about: it spawns an
  item entity, and three of the four names constituting the corpus floor are
  item-entity scenarios (`a-dropped-item-goes-into-your-inventory`,
  `dropping-an-item-throws-it-on-the-floor`, plus `spawn-egg-makes-a-mob` and
  `a-player-that-leaves-the-ground-is-resynced`). The prior is therefore that
  `003` is the LEAST reproducible file here, not that it is free. It must not
  be ratcheted until an ensemble says otherwise.
* **The surface block's destroy time is not asserted.** `004`'s margin is stated
  as a *bound* — what `getDestroyProgress` would have to be for the file to be
  measuring the other branch — rather than as a predicted tick count, because
  the hardness was not read out of the jar on this branch.
  `data/vanilla/flat_preset.json` pins the block itself
  (`minecraft:grass_block` over two dirt and one bedrock) and that is the part
  the scenarios depend on.
* **`hurt_animation`, `player_combat_kill` and `respawn` are not chased.** They
  are zero in 87 recordings like the others, but they have no decoder arm, so
  they are `packet:0x..` digests rather than named kinds and belong to
  `ci/corpus-surface.py`'s population.

## THE `finish_dig` COVERAGE ZERO IS **NOT** CLOSED, AND THE COMMIT SAID IT WAS

`ci/corpus-coverage.py` still prints:

```text
  finish_dig                   0  (declared but exercised by nothing)
```

and this branch does not move it. The zero counts checks in
`ci/checks/gameplay`; `004` is in `ci/checks/survival` and is invisible to that
tool. **The commit that landed `004` claimed it "closes the Step::FinishDig
coverage zero". That was an overclaim and this paragraph is the correction.**

What `004` does establish is that such a check is now *possible*.
`ci/finish-dig-probe.sh`'s conclusion on 2026-08-04 was that closing the zero
"needs a survival player rather than another scenario", and the survival player
now exists. Closing the zero itself means a scenario **inside the corpus**,
which needs a per-player game mode so a creative rig can be built with `hold`
and then flipped for the measured window — and that is deliberately not built
(see the ability-model reasoning above).

So the honest status is: **the blocker moved from "impossible" to "needs a
per-player mode"**, which is a smaller and differently-shaped piece of work
than it was this morning.

## THE PREDICTION, WRITTEN BEFORE THE RECORDING

Recorded here so the measurement can **refute** it rather than be rationalised
after. Same discipline as `ci/finish-dig-probe.sh`, whose header states its
prediction from the bytecode and then reports what happened.

All four are predicted `diverge`, and **that is this lane's success condition,
not its failure**: the deliverable is a mechanic that can be graded, not one
that is green. Each prediction is read off `world_send.rs`'s own dispatch:

```rust
    Action::StartDestroyBlock => s.creative,     // survival: FALSE -> we do nothing
    Action::StopDestroyBlock  => !s.creative,    // survival: TRUE  -> we destroy at once
```

| file | vanilla should send | we should send | predicted |
|---|---|---|---|
| `001` | `sound`, `damage_event`, `set_health` | **nothing** | `diverge` |
| `002` | `block_destruction` stages, then `block_update` | ack only | `diverge`, `missing_kinds` both |
| `003` | `add_entity`(item), `block_update` | ack only | `diverge` |
| `004` | `block_destruction`, deferred `block_update` | `block_update` **immediately** | `diverge`, and ours is EARLY rather than absent |

**`001`'s row is the one worth reading.** `attack::resolve` returns
`Outcome::Hurt` once `invulnerable` is false, and `handle_attack` returns
without playing a sound on anything but `NoDamageSound`. So this server goes
**silent** in survival where it was audible in creative — which means `550`'s
standing `match` rested entirely on the *no-damage* branch. If that is what the
recording shows, the combat row's green was never about combat.

`004` is the only row predicting a divergence in the other direction: we send
something vanilla does not send *yet*. An early packet and a missing packet are
different defects and the baseline must not record them as one.

## THE RECORDING WAS NEVER TAKEN, AND EVERYTHING ABOVE IS A JAR READING

**No survival recording exists yet.** The pass was queued through
`ci/run-detached.sh` as `survpass2` and spent the whole session behind the
corpus lock: two corpus runs had been waiting 167 and 161 minutes when it
joined, a third 139, and a `RUSTMC_CORPUS_LOCK_ROLE=gate` run jumps the queue
ahead of all of them. It was still waiting when this file was written.

So every number in this document is **a reading of the pinned jar plus a census
of recordings that already existed**. Not one of them depends on the survival
pass having run, which was deliberate: the audit was built first precisely so
that it would survive the substrate proving expensive.

### How to pick it up

```sh
    ci/run-detached.sh --status survpass2      # what happened
    ci/run-detached.sh --log    survpass2      # the log
    # the artefacts, if it got that far:
    oracle-work/gameplay/survival/vanilla.json
    oracle-work/gameplay/survival/rustmc.json
    oracle-work/gameplay/survival/compare.json
```

It is chained: `ci/t2-gameplay-survival.sh` then `ci/substrate-gate-audit.py`.
If the pass refused, read the refusal before rerunning — the substrate
assertion (`ci/survival-substrate-assert.py`) failing means the game mode did
not take, and **that failure looks exactly like a green** everywhere else.

**Check the prediction table above against it first.** It was written before
the run for the express purpose of being refuted, and a prediction that
survives unread is worth nothing.

## THE SINGLE NEXT ITEM

**Do not ratchet anything off the first run.** Whenever it lands, it is N = 1.
The scenarios that matter — `001`'s `damage_event` and `set_health`, `002`'s
`block_destruction` — are exactly the ones with no prior distribution on this
box, and `551` was retracted on 2026-08-18 for treating a distribution
difference as a divergence (vanilla 4 in 32 runs and 5 in 39; ours 5 in 66 of
66). **The count of observations per scenario is the number to watch, not the
verdict.**

Then run it a second time and compare the two. `003` is the file to distrust:
it spawns an item entity, and three of the four scenarios constituting the
corpus floor are item-entity scenarios.
