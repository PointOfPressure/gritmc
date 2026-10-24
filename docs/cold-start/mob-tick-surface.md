# COLD START — branch `mobs-tick-surface`

Append-only. Newest entry last.

### COLD START — branch `mobs-tick-surface` off `d48c46ee`, 2026-08-19

Worktree `~/grit/work-mobs`, remote dir `~/rustmc-mobs`. **Nothing was ported.**
Everything below is derived from the pinned jar, from the tree, or from
recordings that already existed on the box. `docs/PARITY-ROADMAP.md` §94 is the
claim; this file is the resumable state.

One full corpus run was taken at the end — `T0 GREEN`, `T2-GAMEPLAY GREEN`,
**97/108 declared and 93/108 strict**, which is `main`'s number to the
scenario, as it must be for a branch that touches no crate.

## THE RECORDING ADDED NOTHING TO N, AND THAT IS THE POINT

The run went out with `RUSTMC_REFERENCE_CACHE=1` and logged
`reference-cache: HIT ref:9fd5a82c...`. So the reference was served from the
cache, `vanilla.json` came back byte-identical to one already on the box, and:

```text
  vanilla.json files carrying the scenario   21 -> 22
  DISTINCT recordings                        17 -> 17
  reports                                    19 -> 21
```

**With the reference cache on, taking a recording adds nothing to the
reference-side N.** A lane that runs a corpus pass to raise N for a
vanilla-side measurement gets a new file and no new sample. The subject side
does move — `rustmc.json` is built and run per tree every time — which is why
the verdict count went up by two and the recording count did not.

This branch's own de-duplication finding, confirmed first-hand by the one run
it was allowed to take.

## THE ONE-GAP FIGURE IS AN ARTEFACT, AND THE ARTEFACT IS MEASURABLE

`ci/unported-inventory.py --open` reports **one** entity-behaviour gap in the
whole server, `entities.rs:19`. That is a true statement about the crate's
refusal markers and it is not a statement about how much mob is missing.

A marker needs a host line — it is written inside the module implementing the
surrounding mechanic, by whoever declined to go further. So the inventory's
resolution on a subsystem is bounded by how much of it is **written**.

`ci/marker-density.py` measures that bound rather than asserting it:
**r = 0.760 between lines and marker count over the audit's own 58 files, with
a permutation p = 0.0001 over 10,000 re-pairings.** The objection "bigger files
have more of everything" is the finding restated: an instrument whose output is
predicted by how much code exists is measuring how much code exists.

`entities.rs:19` is not even the sentence. It is the heading
`# What is here and what is deliberately not`, matched on "deliberately not";
the gap is a clause three lines below.

## THE SIZE OF WHAT THAT ROW STANDS FOR

`ci/mob-tick-census.py`, pinned in `ci/checks/mob-surface.json`, gated by
`--check`. All jar-derived:

| | |
|---|---|
| entity types | 158 — 72 MISC, 86 in a spawning category |
| subtypes of `Mob` | 114 |
| classes in the `Mob` hierarchy | 115 |
| **method declarations** | **3,870** |
| distinct signatures | 1,548 |
| overridden by 2+ classes | 371 |
| a mob tick enters `ai/**` | 1,754 methods, 240 classes |
| a mob tick enters `pathfinder/` | 164 methods, 14 classes |

Per-kind hooks: **54** classes declare `registerGoals`, **39** `aiStep`, **39**
`tick`, **38** `customServerAiStep`, **59** `finalizeSpawn`, **41**
`mobInteract`, **18** `createNavigation`.

### `Mob.serverAiStep`, transcribed — the thing a porter needs and should not re-read

Eight subsystems in one method, in this order, with vanilla's own profiler
section names because the order is observable and the names are how it is
navigated:

```java
this.noActionTime++;                                            // offset 2
push("sensing");  this.sensing.tick();                pop();    // 10-31
if ((this.tickCount + this.getId()) % 2 == 0 || this.tickCount <= 1) {
    push("targetSelector"); this.targetSelector.tick();     pop();   // 61-77
    push("goalSelector");   this.goalSelector.tick();       pop();   // 83-99
} else {
    push("targetSelector"); this.targetSelector.tickRunningGoals(false); pop();
    push("goalSelector");   this.goalSelector.tickRunningGoals(false);   pop();
}
push("navigation"); this.navigation.tick();           pop();    // 153-170
push("mob tick");   this.customServerAiStep(level);   pop();    // 175-196
push("controls");
  push("move");     this.moveControl.tick();
  popPush("look");  this.lookControl.tick();
  popPush("jump");  this.jumpControl.tick();
pop(); pop();                                                   // 201-264
```

**THE GOAL PHASE IS A FUNCTION OF THE ENTITY ID.** `(tickCount + getId()) % 2`
staggers mobs so they do not all re-evaluate their goal lists on the same tick,
and the consequence for parity is not small: **entity id allocation order is
behaviour-visible**, one tick at a time, through which mobs re-pick goals on
which tick. A port that allocated ids in a different order would diverge in a
way no single packet shows, and `rustmc-entity/src/lib.rs`'s note that "vanilla's
tick order is observable" is understating it — the id itself is, too.

The `tickCount <= 1` clause is the other half: a mob's first two ticks always
re-evaluate, whatever its id.

`Mob.customServerAiStep(ServerLevel)`'s base body is `0: return` — a genuine
no-op, and 38 classes override it.

The workspace contains **zero** occurrences of `GoalSelector`,
`NaturalSpawner`, `PathNavigation`, `registerGoals`, `serverAiStep`,
`SpawnEggItem` or `spawn_egg`, re-checked against `main` at 2,232 commits.

**Ours is two `EntityKind` constants, `PLAYER` and `ITEM`.** The intersection
with the jar's 86 spawning types is empty. No mob can exist here, so every mob
mechanic is unreachable **by construction** — which is stronger and more
checkable than "not ported", and is what the census gates.

**Do not quote the unrestricted closure (24,251 of 53,685 methods).**
Devirtualisation makes it most of the jar; it measures the approximation.

## THE CLEANEST INSTANCE OF THE BOUND IS IN THE AUDIT'S OWN SCAN ROOT

`crates/rustmc-server/src/tick.rs` has a doc block headed *"And what does **not**
tick, which is the more useful half"*, lines 37-53. It names, in four bullets:

1. random ticks; 2. entity motion of any kind (gravity, drag, collision,
`Entity.move`); 3. `EntityTickList` order; 4. the
`tickNonPassenger`/`tickPassenger` body; 5. mob AI; 6. mob spawning;
7. `checkDespawn`; 8. item merging and pickup; 9. fluid ticks; 10. the
block-entity tickers.

**Ten absent mechanics.** That count is a hand count and is labelled as one —
the audit's own lesson is that hand counts pick a convention silently. The
mechanical half is the one that matters and it is this: **the audit's regex
matches exactly one line in that range**, and one in the whole file. The match
is bullet 1, on the words "deliberately not". Bullets 2, 3 and 4 name eight
more mechanics and use no refusal keyword at any point, so the inventory sees
`tick.rs` as a file with a single random-tick gap.

This is not the audit being careless. It is the audit's own predicted failure
— its "THE MARKER SET IS A LOWER BOUND" section, with `item.rs:32` as the
example — landing inside its scan root rather than outside it, which is the
harder place to notice. The file also records, in that same block, that
`rustmc_entity::tick_list` and `rustmc_entity::tick` exist and **nothing calls
either**: "Derived is not wired." Confirmed against the tree —
`crates/rustmc-server` imports `EntityId` from that crate and nothing else.

## THE SECOND ENTITY GAP HAS NO MARKER AT ALL

`SpawnEggItem` is unported and `crates/rustmc-server/src` contains no
occurrence of `spawn_egg` or `SpawnEggItem`. There is no line declining to
implement it because there is no code where such a line would go. The audit's
own "THE MARKER SET IS A LOWER BOUND" section predicted this shape with
`item.rs:32`; this is the harder instance, because `item.rs:32` at least had
prose.

## `080` CANNOT GRADE THE GAP IT IS FILED AGAINST — AND CANNOT PASS EITHER

`080-spawn-egg-makes-a-mob` declares `observe = ["add_entity",
"set_entity_data"]`. Measured over **N = 17 distinct** vanilla recordings, from 22 files
(`ci/mob-observability.py --measure`, ledger `ci/checks/mob-blind.toml`):

| | |
|---|---|
| vanilla emitted | 29..150 observations, **15 distinct counts in 17 runs** |
| the check compares | **6..6** |
| behaviour channels emitted | 10..104 |
| behaviour channels compared | **0** |

**COUNT RECORDINGS, NOT FILES.** 22 `vanilla.json` files on this box carry the
scenario and they are 17 recordings. The reference cache serves one reference
to several trees — that is the cache working — and copied trees carry older
ones. File-counting overcounts N by 29%, systematically and in one direction,
and it will get worse while the cache is on. De-duplicate on the sha256 of the
scenario's own recorded block. The *verdict* count is the opposite case and is
deliberately not de-duplicated: the cache serves `vanilla.json` and never
`rustmc.json`, so two reports sharing a reference are still two independent
readings of the subject.

**The check is stable precisely because it cannot see the mob.** The emitted
range is one server disagreeing with itself and is NOT a divergence — the
distinction `551` was retracted on.

And over **N = 19** `report.json` files: declared `absent` 19/19, strict
`diverge` 19/19, `reference_obs` 6, **`subject_obs` 0**, missing kinds
`add_entity, set_entity_data` in all 19. So the audit's note *the entity exists
and is tracked; it does nothing* is wrong in both halves — the entity does not
exist. What is red is the **announcement**, a different gap.

Worse than `710`: `080` **can never go green** either. It is in
`ci/checks/gameplay-floor.toml` at 15 flaps of 15 pairs over 6 runs — vanilla
disagreeing with itself about the mob's spawn yaw — and the yaw rides on
`add_entity`, *inside* the projection. It is not a signal in either direction.

Derived from the tree: **exactly one** of 108 loaded checks can put a mob in
the world; **four** observe a behaviour channel; the intersection is empty. The
four are all about players or items, the only two entity kinds this server has.

## THE SUBSTRATE, WHICH DECIDES WHAT ANY MOB CHECK CAN MEAN

`ci/t2-gameplay.sh` boots `minecraft:flat` with `gamemode=creative`,
`difficulty=peaceful`, `spawn-monsters=false`, `spawn-radius=0`,
`generate-structures=false`, and game rules `advance_time=false`,
`spawn_mobs=false`, `random_tick_speed=0`, `advance_weather=false`.
`ci/mob-substrate.py --check` refuses if any of that moves, because every
finding here is conditional on it.

* **38 of 158 registrations are `.notInPeaceful()`**, all `MobCategory.MONSTER`
  — 38 of the 86 spawning types. Of 45 MONSTER types, **7 survive peaceful**:
  `camel_husk`, `ender_dragon`, `piglin`, `shulker`, `sulfur_cube`,
  `zombie_horse`, `zombie_nautilus`.
* They cannot enter **at all**, and this rests on three call sites read one at
  a time rather than on the call graph, which over-approximates:
  `SpawnEggItem.useOn` and `SpawnEggItem.use` call `EntityType.canSpawn`
  (`isAllowedInPeaceful() || difficulty != PEACEFUL`);
  **`SummonCommand.createEntity` does not use `canSpawn` at all** — it reads
  the difficulty and `isAllowedInPeaceful()` itself and throws, so `/summon` is
  refused with a message; and `Mob.checkDespawn` discards such a mob *before*
  the entity-ticking range gate.
* Natural spawning is off **three times over**: peaceful, `spawn-monsters=false`
  (feeding `ServerChunkCache.spawnEnemies`, read by
  `NaturalSpawner.getFilteredSpawningCategories`), and the `spawn_mobs` game
  rule (read by `NaturalSpawner` and `ServerChunkCache` both).

**No spawn-selection mechanic is measurable by this corpus as it stands.** A
check written against the default substrate would be vacuous by construction.
This is not an argument for changing the substrate — peaceful is load-bearing,
`ci/t2-gameplay.sh` records what 32 stray slime observations did to one
scenario, and `origin/survival-substrate` is a whole branch on the cost of
moving one of these.

## AND THE SUBSTRATE IS NOT THE BINDING CONSTRAINT — READ `corpus-dark-systems` FIRST

Two things the paragraph above gets half right, both already on `main` from
`origin/corpus-dark-systems`, 2026-08-12. **Do not re-derive them.**

* **Peaceful does not empty the world.** 86 spawning types minus 38
  `.notInPeaceful()` leaves **48 that spawn fine in a peaceful superflat**, and
  that branch measured them: **8 to 21 `add_entity` per eight-second window**,
  113..531 observations, different every run.
* **The suppression is liftable per scenario.** `[[gamerule]]` applies a rule
  down the reference's console fifo after the rig is quiet and restores it
  after the settle window. The door exists and it was tried.

`spawn_mobs` through that door came back **NULL — 15/15 pairs over 6 runs, 6
distinct projections.** Not hard: unmeasurable.

**One field ties three separately-recorded facts together, and joining them up
is this lane's addition.** `EntityType.create` draws the spawn yaw at **offset
103** — `ServerLevel.getRandom().nextFloat()`, `* 360.0f`, into `Entity.snapTo`
fifteen instructions later, from an unseeded generator. The same draw is why
`spawn-egg-makes-a-mob` flaps 15/15 and can never `match`, why natural spawning
is a null with numbers, and why **the whole standing 4-of-92 corpus floor is
entity scenarios**.

**But it is not the only unseeded draw on the announcement — see the projection
search below.**

What that changes for the next lane: **lifting the substrate buys nothing.**
What stands between this project and spawn parity is that vanilla does not
agree with itself about where a mob faces, on the very packet a check must
compare. A spawn check needs a projection that drops `y_rot`/`y_head_rot` and
still says something — and `origin/entity-parity` has twice ruled out a
type-keyed exemption for this, because the wire does not carry which path
spawned the mob.

## THE PROJECTION SEARCH — WHAT A MOB CHECK COULD COMPARE

`ci/mob-projection-search.py`, pinned in `ci/checks/mob-projection.json`. Each
candidate projection applied to every recording on disk, runs compared
**pairwise**, so it measures vanilla against itself and boots nothing.
**Sixteen** distinct recordings on one recorder schema — the seventeenth uses
an older `add_entity` decoder and mixing it in makes every row read ~74% stable
when the truth is 0% or 100% — giving **120 pairs**:

| projection | pairs identical | |
|---|---|---|
| `observe` as declared | **0/120** | 0.0% |
| `observe` minus `y_rot` + `y_head_rot` | 34/120 | 28.3% |
| `observe` minus `y_rot` + `y_head_rot` + `rest` | **120/120** | 100.0% |
| `{add_entity}` alone | 1/120 | 0.8% |
| **`{add_entity}` minus `y_rot` + `y_head_rot`** | **120/120** | **100.0%** |
| `{set_entity_data}` alone | 34/120 | 28.3% |
| `{update_attributes}` alone | **120/120** | 100.0% |

`080` as declared is **0 of 120** — never, not rarely. The announcement minus
the two yaw fields is **exact across every pair**: position, kind id, `data`,
`x_rot` and the encoded remainder already agree across sixteen cold servers.

**Two unseeded draws stand in the way, not one.** The second is
`set_entity_data`, independent of the yaw, and the bytecode names it:

    Pig.finalizeSpawn -> PigSoundVariants.pickRandomSoundVariant(
                             registryAccess(), level.getRandom())
                      -> Registry.getRandom(RandomSource) -> setSoundVariant

uniform over `minecraft:pig_sound_variant` — `big`, `classic`, `mini`.

**Every unseeded draw a spawned pig makes, and which reach a client.** Read
from the jar, then checked against the recordings in that order — because the
last inference was wrong, which is the useful part:

| draw | call site | packet | on the wire? |
|---|---|---|---|
| spawn yaw | `EntityType.create` @103, `nextFloat() * 360.0f` | `add_entity` | **yes** |
| sound variant | `Pig.finalizeSpawn` -> `PigSoundVariants.pickRandomSoundVariant` -> `Registry.getRandom` | `set_entity_data` | **yes** |
| left-handed | `Mob.finalizeSpawn` -> `nextFloat()` -> `setLeftHanded` | `set_entity_data` | **yes** |
| age | `AgeableMob.finalizeSpawn` -> `nextFloat()` -> `setAge` | `set_entity_data` | **yes** |
| follow-range bonus | `Mob.finalizeSpawn` -> `triangle(0.0, 0.11228)` on `FOLLOW_RANGE` | `update_attributes` | **NO** |

**The last row is the one worth the space.** A continuous `triangle` draw into
an attribute modifier looks like the worst case there is — a real-valued
divergence on a packet the corpus already records — and it is not a case at
all: `{update_attributes}` alone is **120/120 identical**. The jar says why.
`Attributes`' static initialiser registers `FLYING_SPEED` through
`RangedAttribute.setSyncable(true)` at offsets 420-424 and registers
`FOLLOW_RANGE` at 430-451 **with no `setSyncable` call**, so it keeps the
default and `isClientSyncable()` is false. The draw happens on every spawn and
no client is ever told.

A divergence in the server is not a divergence on the wire, and only the
measurement could say which this was. The bytecode said "unseeded draw"; the
recordings said "exact in every pair"; the bytecode then said why.

The three that do reach `set_entity_data` are consistent with what is measured
there — 5 distinct payloads over 16 runs at 8/3/3/1/1 reads as three sound
variants with two rarer left-handed ones beside them, `setLeftHanded` being a
5% draw whose expectation over 16 runs is 0.8. **Consistent with, not proven**:
the recorded field is a digest, so the decomposition is inference and is
labelled as one.

This does **not** argue for a field exemption — `origin/entity-parity` ruled a
type-keyed yaw exemption out twice and the reason survives. It supplies the
cost side of that decision, measured.

## KILLED AS NULLS — DO NOT RE-RUN THESE

* **`ci/jarcallgraph.py` cannot answer "what does a mob tick send".** Asked for
  the `Clientbound*Packet` classes constructible from the mob tick roots it
  returns **77**, which is most of the server's packet surface, because
  devirtualisation resolves every `invokevirtual` to all subtypes. The header
  is right that only the *dark* direction is sound. The empirical answer is
  better and already measured: the 14 kinds vanilla emitted for `080` over
  N = 17, in `ci/checks/mob-blind.toml`.
* **`crates/rustmc-entity/src/tick.rs`'s transcription is correct.** Re-read
  against the jar: the classes overriding `Entity.checkDespawn` are exactly
  `Mob`, `ShulkerBullet`, `WitherBoss`, `EnderDragon`; `getDespawnDistance` is
  128 for every `MobCategory` except `WATER_AMBIENT` at 64; `getNoDespawnDistance`
  is `bipush 32; ireturn`. It is now a pinned figure
  (`transcription_disagreements`) with a falsifier assertion, so it stays
  correct rather than merely being correct today.
* **The wider scan root adds no mob gap.** Under the audit's own regex outside
  `crates/rustmc-server/src`: `rustmc-behavior` 9, `rustmc-entity` 4,
  `rustmc-data` 3, `tools/` 31, `rustmc-worldgen` 94 (owned). None of the
  entity-crate four is a mob-behaviour gap; they are design notes. The scan
  root matters for a different reason — `rustmc-entity/src/lib.rs` writes the
  same gap out at length and the inventory cannot see it.

## WHAT WAS DELIBERATELY NOT DONE

* **No reach widening.** `origin/entity-parity` established the 88 types one
  `hold` away all land on `EntityType.create` offsets 103-115, an unseeded yaw
  draw: unstable rows, zero parity.
* **No mob port.** Every mob mechanic is downstream of a `Mob` existing, and
  under this substrate the only measurable mob is a peaceful-legal one whose
  behaviour no check observes. A goal, a navigator or a spawn rule written now
  would be verified by nothing, and `docs/AGENT-BRIEF.md` names an approximate
  pathfinder as the one-way door this project must not walk through. Defer
  verification, never fidelity — here the verification does not exist yet.
* **No touch of `add_entity.movement`** — a sibling lane owns it.

## THE SINGLE NEXT ITEM

**`SpawnEggItem`.** It is the only route by which a mob can enter this world
and it is the gap with no marker. It does not need the AI: `EntityType.canSpawn`,
the peaceful gate, and the `add_entity` / `set_entity_data` announcement are the
whole of it. It moves `080` from `absent` to `diverge` and **not** to `match`,
because the yaw floor is unmovable — so it must not be sold as closing the
check, and `ci/checks/gameplay-baseline.toml`'s row moves with it.

After that, in order: a behaviour channel in some mob check's `observe`, or mob
behaviour stays ungraded however much is ported — and that check has to survive
a projection whose vanilla side spans 29..150 observations, which is a
scenario-design problem before it is a porting one. Then the entity tick loop
body in `crates/rustmc-entity/src/tick.rs`, whose every gate is already a trait
method waiting for an answer, `Mob.checkDespawn` first.

## THE INSTRUMENTS, AND WHAT EACH REFUSES

| tool | question | gates? |
|---|---|---|
| `ci/mob-tick-census.py` | how big is the mob surface, against ours | **yes**, T0, 8-assertion falsifier |
| `ci/mob-observability.py` | can any mob check see behaviour | **yes**, T0, 7-assertion falsifier |
| `ci/mob-substrate.py` | what world does the corpus run, what does it hide | **yes**, T0, 6-assertion falsifier |
| `ci/marker-density.py` | does a marker census measure absence or effort | **yes**, T0, 5-assertion falsifier |
| `ci/mob-projection-search.py` | which projection does vanilla agree with itself under | **yes**, T0, 6-assertion falsifier |

Every one of them has a **zero or an absence** as its headline, which is the
reading a broken instrument and a correct one produce identically. So every one
refuses before printing if its own reader is not live: the census must find
`PLAYER` and `ITEM` and match both to jar registrations; the substrate reader
must find at least three properties, three game rules, 100 registrations and a
non-zero `.notInPeaceful()` count; the density census must find at least 30
files and 40 markers. Each falsifier also carries a **negative** control — a
mobless check must not be flagged, an unrelated comment must not redden the
substrate gate — because a detector that fires on everything passes its
positive assertions for free.

### CORRECTION — 2026-09-05, from `stack-gameplay`'s rebase onto main `f4fa28ff8`

Two claims above are no longer true of the tree, and both are corrected in
`docs/PARITY-ROADMAP.md` §94, which is where `ci/mob-tick-census.py` points.
Appended rather than edited, per this file's own shape.

* **"Ours is two `EntityKind` constants, `PLAYER` and `ITEM`" is three.**
  `2313ff73c` on `stack-gameplay` added `FALLING_BLOCK` with the block half of
  a gravity block and did not re-pin, which is the census reddening exactly as
  its `_expected_to_move` note says it should. `our_mob_kinds` is still **0**,
  so the intersection with the jar's 86 spawning-category types is still empty
  and §94's conclusion is unchanged.
* **"The workspace contains zero occurrences of `GoalSelector`,
  `NaturalSpawner`, `PathNavigation`, `registerGoals`, `serverAiStep`,
  `SpawnEggItem` or `spawn_egg`" is now five-of-seven false.** Main
  `f4fa28ff8` (`mobs-goals`) landed the goal system and the pathfinder in
  `crates/rustmc-entity/src/ai`: `GoalSelector`, `PathNavigation`,
  `registerGoals` and `serverAiStep` are all present, the last of them in
  `crates/rustmc-server/src/entities.rs` as well. **`NaturalSpawner`,
  `SpawnEggItem` and `spawn_egg` are still zero**, and those three are now the
  whole of the gap: the mob code exists and nothing can be spawned into it.
