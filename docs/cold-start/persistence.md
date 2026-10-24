# COLD START — branch `persistence`

### COLD START — branch `persistence`, 2026-09-05

**The area: the running server saving and loading its own world the way
vanilla 26.2 does.**

## What the brief said and what the tree said

The brief said "docs/SERVER.md records that nothing persists". It does not, and
had not since 2026-08-05: `--world <dir>` already wrote edited chunks and
block entities as real Anvil, on a 300 s timer, and loaded them back before a
tile was served, with `ci/t2-vanilla-loads-ours.sh` proving vanilla can read
one of those region files. The brief also said "no chunk unload exists yet, say
so" — there is one: `Tiles::retain_around` retires a tile after
`RETIRE_AFTER_SECS` and saves it before evicting its block entities. Both
corrections were the first measurement of the run, per the standing rule.

What was genuinely absent was everything *around* the region file, and the
gap was not visible from inside: a directory of `.mca` files with no
`level.dat` is a directory `LevelStorageSource` treats as a world that does
not exist. So "a world vanilla can open" was true of the file and false of the
world, and no gate could see the difference because no gate ever asked vanilla
to open the **directory**.

## The single largest fact

**26.2 split `level.dat`.** It now carries 14 keys under `Data` in 383 bytes
and holds no seed, no game rules, no time of day and no weather. Those live in
nine `data/minecraft/*.dat` files, each a `data` compound plus a `DataVersion`
int. The seed is in `world_gen_settings.dat`. The player directory is
`players/data`, not `playerdata`. All of it read off
`oracle-work/gen-20260722-25801/world`, real vanilla output from 2026-07-27,
with `LevelResource`'s constant pool as the authority for the names.

## What is saved, and the golden that proves each

| what | golden |
|---|---|
| `level.dat` | 40 harvested vanilla files, decoded and re-encoded **byte-identically** |
| the nine `data/minecraft/*.dat` | built from scratch (seed + two booleans) and compared to vanilla's bytes |
| `players/data/<uuid>.dat` | a real 26.2 player file, 48 keys, re-encoded byte-identically |
| the block chunk | **754 real vanilla chunks, five statuses, byte-identical** |
| `entities/r.X.Z.mca` | a real vanilla entity region, record payloads identical + round trip |
| the whole world directory | **a real vanilla server boots on it and reports our seed** |

The last one is `ci/t2-vanilla-opens-our-world.sh`. `server.properties` carries
a decoy seed; vanilla ignores it for an existing world, so `seed` answering
20260722 is proof it read a file we wrote. `--negative-control` deletes that
file and requires the decoy. Measured: GREEN, and CONTROL GREEN at 1234567890.

## Three defects the byte-identity golden found, all shipping

1. **Every heightmap read out of a vanilla chunk was 64 too high.**
   `Heightmap.setHeight` stores `y - getMinY()`; the decoder returned the
   stored value while `pack_heightmap` subtracts `min_y` again on the way out.
   Vanilla trusts a saved heightmap rather than recomputing it.
2. `block_entities` was omitted when empty; vanilla writes `[]` at every status.
3. `LastUpdate`, `InhabitedTime`, `isLightOn`, `block_ticks`, `fluid_ticks`,
   `PostProcessing` and `structures` were written back as **constants**, so a
   loaded chunk lost its tick queues and structure references on the first
   rewrite. `ChunkColumn::extra` is the passthrough that fixes it.

`ci/anvil-byte-identity-falsify.sh` reinserts each one and requires a red:
GREEN / RED / RED / RED / GREEN.

**And the window nearly hid all three.** Bounded to two regions, the golden saw
161 chunks, every one at `minecraft:structure_starts` — empty heightmaps,
air-only single-entry palettes, no `isLightOn` — and passed. It now walks until
it has seen a `minecraft:full` chunk and 20 heightmapped ones, and asserts
both.

## What is REFUSED, by name, and why

* **Entities.** The format is done and has a golden; there is nothing to put
  in it. This server has two entity kinds. `EntityTypes.PLAYER` is built with
  `noSave()` — vanilla never writes a player into the entity region either,
  and players go to `players/data`, which is done. `EntityTypes.ITEM` is the
  other, and `crate::entities::Entity` carries **no item stack**, so a saved
  item entity would have no `Item` compound, which vanilla discards on load.
  Writing the region today would produce a file whose only honest content is
  an empty list. **Do not re-open this until an item entity carries a stack.**
* **`fluid_ticks`.** There is no fluid tick queue; `crate::ticks` is one queue
  and holds block ticks. Written empty, which is correct and is not evidence.
* **`InhabitedTime`.** Vanilla accumulates it per chunk and derives local
  difficulty and mob spawning from it. Nothing here tracks it, and a made-up
  value would change behaviour on a world moved to vanilla. Left 0.
* **`isLightOn`.** No light engine. 0 makes vanilla relight the chunk, which is
  the right answer.
* **Player inventory, health, hunger, experience.** No per-player state of any
  of those exists in this server, so a saved file would carry defaults. The
  `extra` passthrough means a player who has played on vanilla keeps theirs.
* **A SIGTERM shutdown save.** Needs a signal dependency this workspace does
  not carry and `Cargo.toml` goes through the orchestrator. The autosave timer
  bounds the loss either way and is what `kill -9` and a power cut get.

## A hole in a gate, found on the way past

`ci/liveness-proofs.py`'s `PROBE` was `\bgrep\s+-q\b`, which matches neither
`-qa` nor `-qE` nor `-qP` nor `-qF` because a letter is a word character. It
found **6 of the 9** wait loops in `ci/*.sh` and missed **both of
`ci/run-detached-falsify.sh`'s** — the waits on the sentinel of the detached
runner every agent is told to use. Widened; all three now registered with
fixtures a not-ready subject produces. 9/9, 0 unsound.

## A fifth defect, found by asking what the save path rebuilds

`World::save_to` does not round-trip a loaded chunk: it rebuilds the column
from the generated canvas and lays the overlay on top, because a saved chunk is
not expressed relative to anything. That is right for the blocks and **wrong
for everything the format crate does not model.** Point this server at a world
vanilla made, break one block in a chunk a village starts in, and the save
writes that chunk back with `generated_extra()`'s empty `structures`: the
blocks stay and vanilla no longer knows a village is there.
`anvil::read_chunk_extra` reads the chunk already on disk and seeds
`column.extra` from it, and the test has the naive rewrite as its own control
— it asserts the loss happens without the repair before asserting it does not
with it.

## And one of the goldens ate our own output

`a_real_vanilla_player_file_re_encodes_byte_for_byte` enumerated every
`oracle-work/*/world/players/data`, and `ci/t2-player-survives-a-restart.sh` —
added the same day — leaves a world there. Ours carries exactly the 24 keys
the struct models, so it round-tripped trivially and the golden was comparing
this server's output to itself for about two hours. **Its own
`extra.len() >= 10` assertion caught it**, first time it could, which is the
argument for putting a population assertion inside every fixture-driven test
rather than beside it.

The population now filters on `level.dat`'s `ServerBrands` — `PrimaryLevelData`
keeps the set of brands that have ever run the world, so one this server has
opened says `rustmc`. That is a property of the **world**, not of the file
being compared, so the filter cannot select for the answer. The same shape as
the `gen-*` filter on the world-gen-settings golden, for the same reason.

## A known limitation, named rather than left to be found

A rejoining player's **chunk set is still the spawn tile's**. `serve_one`
collects the chunks around the spawn and then overrides the position from
`players/data`, so a player who logged out far from spawn rejoins into chunks
the join packet did not carry and sees void until `Tiles` generates the tile
they are standing in — which it does, on the same trigger as walking there.
Fixing it means choosing the tile from the saved position *before* the chunk
sweep, which changes what `serve_one` generates synchronously while a client
waits. `ci/t2-player-survives-a-restart.sh` walks six blocks, so it does not
exercise this.

## Nothing runs the gates for you

`ci/t2-vanilla-opens-our-world.sh`, `ci/t2-player-survives-a-restart.sh` and
`ci/anvil-byte-identity-falsify.sh` are **hand-run**, exactly like
`ci/t2-vanilla-loads-ours.sh`, which nothing has ever invoked automatically
either. They boot a JVM or two rustmc servers. They are listed in
`docs/CI.md`'s gate-tier section with what each proves and what each control
is. Quote them with that caveat: a gate nobody runs is a gate that has not run.

## The next item, and what closes it

**Save `InhabitedTime` for real.** It is the one remaining chunk field with a
behavioural consequence on a world handed to vanilla, and it needs one thing
this server does not have: a per-chunk counter incremented once per tick per
resident player, as `ServerChunkCache.tickChunks` does. Closed by a scenario
that stands a player in one chunk for N ticks, saves, and reads `InhabitedTime`
back as N — with the control being a *neighbouring* chunk, whose value must
stay 0.

---

### COLD START — branch `persistence-2`, 2026-09-05

**The area: the second persistence lane — wiring the four hand-run gates into
the merge gate, the autosave and shutdown triggers, `InhabitedTime`, and the
entity save format.** Rebased off `persistence` (14 commits) onto `origin/main`
1a31e0a56.

## The headline

    the four hand-run gates       ci/handoff.sh STAGE 7, conditional, t2p=n/a when it skips
    a fifth gate                  ci/t2-sigterm-saves.sh — a clean stop saves a
                                  CONNECTED player; kill -9 is the control
    the autosave                  counts TICKS, saves players FIRST, level.dat LAST
    InhabitedTime                 real: per chunk, by the game-time delta, < 128 blocks
    isLightOn                     refused BY NAME, and the refusal changed shape
    fluid_ticks                   refused, re-checked against origin/redstone
    the entity compound           438 real vanilla entities re-encode byte-identically
    the Entities LIST             still refused — mob-in-world has not merged

## 1. The gates run now, and the skip is a third state

`docs/CI.md` used to say "nothing runs them for you" and stop there.
`ci/t2-vanilla-loads-ours.sh` had been in the tree since August and had never
once been run by a gate.

`ci/t2-persistence.sh` is the stage driver: it takes the corpus lock once
around every gate, runs each gate AND its control, and asserts on each
script's own GREEN sentinel rather than on its exit status — a control arm
exits 0 when it *works*, so the status cannot tell a fired control from a
crashed one.

**It is conditional and that is a cost decision, not a hedge.** The stage
boots a JVM or a pair of rustmc servers once per gate and once per control
against a serial merge gate whose measured mean is 65.2 minutes.
`ci/persistence-applies.py` reads `ci/checks/persistence-paths.toml` (20
prefixes, floor 8) and answers APPLIES / NOT-APPLICABLE / UNDECIDABLE;
UNDECIDABLE runs the stage. The verdict line carries `t2p=n/a` for a skip,
which is a different string from `t2p=skipped` (an earlier stage was red) and
from an exit code. Three states, never interchangeable — the `t1=99`
distinction that cost 113 wrong RED verdicts on 2026-08-18, applied before it
could cost anything here.

A wrong path list can only be as bad as today: before the stage the gates ran
on nothing at all.

`ci/persistence-paths-falsify.sh` is 32 assertions in T0 — every declared
prefix must apply, five branch shapes must not, a mixed branch with one
declared path among four must, and each of `--check`'s four refusals must
fire. `--check` earned its place on its own first run by refusing a prefix
naming `crates/rustmc-world/src/region.rs`, a file that does not exist.

## 2. `ci/t2-vanilla-loads-ours.sh` had no control and now has one

What `docs/CI.md` called its control — "three arms, A and C are the
discriminating pair" — is an argument about the arms' *structure*. It is
sound and it is not evidence that the gate has been seen going red.

`--negative-control` flips **one byte at the midpoint of chunk (64,64)'s
compressed payload**, after our writer produced it and before vanilla boots.
Deleting the file would test whether vanilla regenerates a missing world;
truncating it would fail at `region-differ scan` before vanilla ran and record
a REFUSED as a fired control. A byte inside the deflate stream leaves the
header, the sector table and every other chunk intact.

It requires **three** things, not one: arm A red, arm C still green, and a
region-file complaint in vanilla's own words. "The gate went red" alone is
also satisfied by vanilla never having started.

## 3. The autosave had three defects and one of them lost data

`MinecraftServer.saveEverything` opens with `getPlayerList().saveAll()`, before
it touches a chunk. **This server saved a player only on disconnect**, so a
`kill -9`, a crash or a power cut put every connected player back to wherever
they last logged out — hours and a world away — in a world whose chunks were
saved five minutes ago. The autosave bounded the loss for the terrain and not
for the people standing on it.

The other two: the trigger was a wall clock and vanilla counts ticks (the two
agree on an idle server and diverge on a saturated one, which is when the save
matters); and `level.dat` was written FIRST, justified by a comment saying a
world whose chunks are newer than its `level.dat` opens at the wrong time of
day — which is the condition writing it first creates.

**6000 is not a constant to copy.** `AUTOSAVE_INTERVAL = 6000` is the
constructor's initial `ticksUntilAutosave`; every later interval is
`max(100, (int)(tickrate * 300.0f))`, 6000 only at the default rate.
`onTickRateChanged` clamps in one direction only, and that clamp is what makes
`--autosave-seconds 2` mean what an operator reading the flag expects.

## 4. SIGTERM saves, and the gate that shows kill -9 does not

`libc 0.2` is the one dependency added — no transitive deps, already in this
box's registry cache. The handler is **one relaxed atomic store and nothing
else**: a handler may not allocate, print, or take a lock the interrupted
thread might hold, and saving a world does all three. The tick thread notices
the flag at the end of its next tick and runs `stopServer`'s body, which is
where vanilla runs it.

**The watchdog is not belt-and-braces.** Installing a handler changes
SIGTERM's disposition from "terminate" to "whatever this program does", so a
wedged tick thread would start ignoring `kill` — a regression this would
otherwise have introduced silently.

`ci/t2-sigterm-saves.sh` measures what no other gate can. Three paths write
`players/data` — the disconnect handler, the autosave, the hook — and
`ci/t2-player-survives-a-restart.sh` cannot separate them because its probe
closes its socket. This one keeps the player **connected** (`ci/rejoin-probe.py
--stay`), pushes the autosave to 3600 s, asserts no player file exists before
the signal, and asserts from the server's own log that the autosave did not
fire. The control is `kill -9`: uncatchable, differing in exactly one respect,
holding everything else.

## 5. What was refused, by name, and what was checked before refusing

* **`fluid_ticks`.** The brief said to serialise it "now that
  `origin/redstone` has the `LevelTicks` queue". **It does not have a fluid
  one.** That branch adds 84 lines to `crate::ticks` and its `on_used` still
  refuses a fluid tick with "no fluid queue", carrying the same
  `a_fluid_tick_is_not_queued_as_a_block_tick` test. Merging redstone will not
  make the empty list evidence. Do not re-open this on that argument.
* **`isLightOn`.** `origin/light-engine-2` is 417 commits unlanded. The
  refusal moved from `Byte(0)` to **no key at all**, which is what vanilla
  writes when `lightCorrect` is false. And what the chunk NBT will want when
  the engine lands is recorded, because it is not the flag: the payload is the
  per-section `BlockLight` and `SkyLight` byte arrays, 2048 bytes each, each
  written only when its `DataLayer` is non-null. **Setting the flag true
  without the arrays is worse than the 0 it replaced** — vanilla would trust
  an empty layer instead of relighting.
* **The `Entities` list.** `origin/mob-in-world` (16 commits) has not merged
  and `crate::entities::Entity` on main still carries no item stack. The
  FORMAT now exists with a golden against real vanilla output, so the refusal
  is a refusal with something behind it rather than an empty one.
* **Brain memories.** Refused, and the refusal is a ledger now — see below.

## 6. Two defects 438 real vanilla entities found

`crates/rustmc-world/src/entitydat.rs` transcribes `Entity.saveWithoutId`,
`LivingEntity.addAdditionalSaveData` and `Mob.addAdditionalSaveData`, with
each conditional key marked by the offset of the `ifeq` that skips it. Twelve
keys unconditional, seven only when set. The golden is every entity in every
harvested vanilla world on this box — 438 across 11 species — decoded and
re-encoded and compared as BYTES.

**`Health` is two fields with one name.** `LivingEntity` writes it Float;
`ItemEntity` writes it Short, from an int despawn counter
(`DEFAULT_HEALTH = 5`). 109 items Short, 165 living entities Float.
`health: Option<f32>` parses every mob and refuses every item, which is
exactly what happened on the golden's first run.

**"Every vanilla `Brain` is empty" was false and was written from one pig.**
Of 181 brain-carrying entities, two `minecraft:zombie_nautilus` hold a live
`minecraft:attack_target_cooldown` memory. The distinction that survives:
CARRYING a memory is byte-exact through `extra` and needs no understanding;
ACTING on one is refused. `BRAIN_MEMORY_MODULES` is the measured set and the
test reddens on a module outside it **and on a module listed but never seen**,
which is how the previous claim survived being wrong.

## 7. `InhabitedTime` is real, and the previous cold start's rule was wrong

That file said "once per tick per resident player". It is **once per tick per
CHUNK**, by the game-time delta, for a chunk at least one non-spectator player
is within 128 blocks of. A chunk with three players gains one tick per tick —
and a per-player counter is indistinguishable from a correct one in every
single-player test, which is why that is the assertion the module leads with.

Three more things from the same bytecode: the increment is `delta` and not 1
(a stalled server credits the whole gap on resuming); the distance is **not
flat** (the player's y is a term, so a player 200 blocks above a chunk is out
of range); and the comparison is `<`, so exactly 128 blocks is out.

Two divergences stated rather than hidden: vanilla's set is "spawn-candidate
chunk with a ticking `LevelChunk`" and this server has no ticket system, so it
uses "resident and within 128 blocks" — an over-count in exactly the case it
cannot distinguish; and the spectator filter is written and can never fire,
because omitting it makes the rule silently wrong the day spectator lands.

## 8. Two gate instruments that could not see what they claim to

Neither is this lane's to fix and both are recorded in `docs/FINDINGS.md`.

* **`ci/check-figures.py` rule 18 cannot fire in a merge gate.** It asks git
  whether a provenance sha resolves — the half its own text says earns its
  keep — but `RESOLVE_SHAS` is `git rev-parse --git-dir` in the tree being
  checked, and stage 2 ships the branch with `--exclude .git`. In every gate
  directory it falls back to shape, and an orphaned sha is hex-shaped and is
  not a date. Found by this rebase: `ci/figures.toml`'s `[[figure]]` body is a
  declared merge region, so the rebase took this lane's side of three entries
  and restored three shas that resolve to nothing. A gate would have called
  that green.
* **`ci/run-detached.sh <name> '<command>'` runs nothing and reports success.**
  The payload comes from stdin; an argument is ignored and an empty payload is
  not refused, so the log reads `DETACHED-DONE <name> rc=0` — indistinguishable
  from a sequence that ran and passed.

And one this lane did fix, because its own gate exposed it:
`ci/liveness-proofs.py`'s `PROBE` had the same word-boundary hole in its FILE
half as it had had in its flag half. `\b(log|LOG)\b` matches `$log` and `$LOG`
and matches neither `$PLOG` nor `$srvlog`. A gate with waits on `$LOG` and
`$PLOG` had the first refused as unregistered and the second invisible.
Discovery 10 -> 12.

## The next item, and what closes it

**Put a mob in the `Entities` list.** The format is done and byte-identical
against 438 real vanilla entities; the content is still absent, and it is
absent for one reason: `origin/mob-in-world` has not merged. When it does, the
work is `EntityData::from` a `crate::mob` entity and a chunk's list assembled
in `World::save_to` beside `block_entities`. **Closed by a scenario that spawns
a pig, saves, and boots real vanilla on the directory with the pig still
standing there** — which is `ci/t2-vanilla-opens-our-world.sh` with one more
arm, and it is now a gate that runs rather than one somebody remembers.

### ADDENDUM — 2026-09-05, the T0 red that was not a registry change

`persist2-final` came back `T0-RC=101` on one test:

    level::saved_goldens::the_saved_game_rules_are_the_registry_minus_the_gated_one
    panicked: read: Os { code: 2, kind: NotFound }        48 passed; 1 failed

**The 26.2 game-rule registry has not changed.** `rules.len() == 58` and the
absence of `minecraft:max_minecart_speed` still hold over every real vanilla
world in the harvest; the panic was a MISSING FILE, not a disagreeing count.

`worlds()` took every directory under `oracle-work` with a
`world/data/minecraft` in it, whoever wrote it. Three gates in `ci/` leave a
world at exactly that depth, and `ci/t2-sigterm-saves.sh` — added by this lane
— has a negative-control arm that `kill -9`s the server, so its world holds a
`data/minecraft` created at open with **zero files in it** and no `level.dat`
at all. 28 candidate worlds on the box, one of them that.

**The crash is the lucky half of this and the reason to read further.** The
goldens either side of that one call `vanilla(&dir, name)`, which returns
`Option` and `continue`s on a missing file. For a world this server wrote
*completely* — `ci/t2-player-survives-a-restart.sh` leaves one, with all nine
files — they found the file and compared **this crate's encoder against this
crate's own output**, silently, while `checked >= 80` stayed green. That is
the same defect the player golden was caught in on the same day by its own
`extra.len() >= 10` assertion; this is the third instance, in a second file,
and it had no assertion that could catch it.

`worlds()` now filters on `level.dat`'s `ServerBrands` — a property of the
world rather than of the file being compared, so it cannot select for the
answer — with the identical predicate `crate::playerdat` uses.
`the_world_sweep_excludes_worlds_this_server_wrote` asserts **both**
directions: that our worlds are gone, and that the filter has not emptied the
sweep, because every population assertion in the module is guarded by
`|| worlds().is_empty()` and an over-eager filter turns the whole module green
and silent. It also asserts every survivor has the `game_rules.dat` the
goldens read unconditionally — a half-written *vanilla* world is a different
failure with the same panic, and that panic is what sent this investigation at
the registry instead of at the sweep.

50 of 50 in `-p rustmc-world --lib`.

**One thing found on the way and NOT fixed, deliberately, because it is a boot
path change and this turn was scoped to the red.** `LevelStore::open` writes
`session.lock` and mkdirs `data/minecraft`, `players/data` and `datapacks`,
and writes no `level.dat` and none of the nine until the first autosave or
shutdown save. A server started with `--world` on a fresh directory and killed
before either leaves a directory `LevelStorageSource` treats as a world that
does not exist — which is the failure this whole area opened by fixing,
reachable again through the boot path. Closed by writing the initial
`level.dat` at open, plus a gate that `kill -9`s a server within a second of
boot and requires vanilla to open the directory.
